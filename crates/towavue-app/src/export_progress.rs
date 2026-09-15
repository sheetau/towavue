use crate::*;

#[derive(Clone, Copy, PartialEq)]
enum LoadingOwner {
    Media(Option<TabId>, u64),
    Folder(u64),
}

#[derive(Default)]
pub(super) struct LoadingProgress {
    owner: Option<LoadingOwner>,
    started: f64,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn toolbar_loading(
        &mut self,
        context: &egui::Context,
    ) -> Option<(&'static str, f64)> {
        let label = if self.media_kind == Some(MediaKind::Image) && self.image_loading {
            Some("Loading images")
        } else if matches!(self.media_kind, Some(MediaKind::Audio | MediaKind::Video))
            && self.state == PlaybackState::Loading
        {
            Some("Loading media")
        } else {
            None
        };
        let pending = label
            .map(|label| {
                (
                    LoadingOwner::Media(
                        self.tabs.active().map(|tab| tab.id),
                        self.media_generation,
                    ),
                    label,
                )
            })
            .or_else(|| {
                self.pending_folder.as_ref().map(|(generation, intent)| {
                    (
                        LoadingOwner::Folder(*generation),
                        match intent {
                            FolderIntent::Open | FolderIntent::OpenReplacing(_, _) => {
                                "Opening folder"
                            }
                            FolderIntent::Refresh(_) => "Loading folder order",
                        },
                    )
                })
            });
        let progress = &mut self.loading_progress;
        let Some((owner, label)) = pending.filter(|_| self.active_export.is_none()) else {
            progress.owner = None;
            return None;
        };
        let now = context.input(|input| input.time);
        if progress.owner != Some(owner) {
            progress.owner = Some(owner);
            progress.started = now;
        }
        // Do not flash a completed cached navigation, inherit another tab's
        // animation, or pretend an unknown amount of decode work is a percentage.
        let elapsed = (now - progress.started).max(0.0);
        const DELAY: f64 = 0.2;
        if elapsed < DELAY {
            context.request_repaint_after(Duration::from_secs_f64(DELAY - elapsed));
            None
        } else {
            Some((label, elapsed - DELAY))
        }
    }
}

pub(super) struct ExportProgress {
    duration: Option<Duration>,
    normalized: bool,
    counts_audio_samples: bool,
    animation_start: Option<f64>,
    stopped_at: Option<f64>,
}

impl ExportProgress {
    pub(super) fn new(
        request: &ExportRequest,
        options: &ExportOptions,
        source_duration: Option<Duration>,
    ) -> Self {
        let duration = source_duration
            .filter(|duration| request.kind != MediaKind::Image && !duration.is_zero())
            .and_then(|duration| {
                towavue_core::EditTimeline::from_operations(
                    media_time(duration),
                    &request.operations,
                )
            })
            .and_then(|timeline| {
                Duration::try_from_secs_f64(
                    timeline.duration().as_seconds_f64()
                        / f64::from(
                            towavue_core::EditState::from_operations(&request.operations).rate,
                        ),
                )
                .ok()
            })
            .filter(|duration| !duration.is_zero());
        Self {
            duration,
            normalized: options.audio.normalize_peak,
            counts_audio_samples: request.kind != MediaKind::Image
                && towavue_core::EditState::from_operations(&request.operations).rate != 1.0
                && !request
                    .operations
                    .iter()
                    .any(|operation| matches!(operation, EditOperation::Timeline(_))),
            animation_start: None,
            stopped_at: None,
        }
    }

    fn fraction(&self, time: Duration, analyzing: bool) -> Option<f32> {
        // Counting precedes tempo; normalization, if enabled, then restarts on the
        // final output time axis. Do not present both passes as one percentage.
        if analyzing && self.counts_audio_samples {
            return None;
        }
        self.duration.map(|duration| {
            let phase = (time.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0);
            let fraction = if self.normalized {
                (phase + if analyzing { 0.0 } else { 1.0 }) * 0.5
            } else {
                phase
            };
            fraction.min(0.99) as f32
        })
    }
}

pub(super) fn draw(
    ui: &egui::Ui,
    panel: egui::Rect,
    export: Option<&mut ActiveExport>,
    loading: Option<(&'static str, f64)>,
) {
    let density = ui.ctx().pixels_per_point();
    let bottom = (panel.bottom() * density).round() / density;
    let track = egui::Rect::from_min_max(
        egui::pos2(panel.left(), bottom - 1.0 / density),
        egui::pos2(panel.right(), bottom),
    );
    ui.painter().hline(
        track.x_range(),
        track.center().y,
        egui::Stroke::new(1.0 / density, chrome::BORDER),
    );
    let Some(export) = export else {
        if let Some((label, elapsed)) = loading {
            let (start, end) = indeterminate_span(elapsed);
            paint_span(ui, track, start, end);
            ui.ctx().request_repaint_after(Duration::from_millis(60));
            accessibility(
                ui,
                track,
                "toolbar-media-loading",
                "Media loading",
                label,
                None,
            );
        }
        return;
    };
    let progress = &mut export.progress;
    let fraction = progress.fraction(export.encoded, export.analyzing_audio);
    let (start, end) = if let Some(fraction) = fraction {
        (0.0, fraction)
    } else {
        let now = ui.input(|input| input.time);
        let started = *progress.animation_start.get_or_insert(now);
        let elapsed = (now - started).max(0.0);
        let elapsed = if export.cancelling {
            *progress.stopped_at.get_or_insert(elapsed)
        } else {
            ui.ctx().request_repaint_after(Duration::from_millis(60));
            elapsed
        };
        indeterminate_span(elapsed)
    };
    paint_span(ui, track, start, end);
    accessibility(
        ui,
        track,
        "toolbar-export-progress",
        "Export progress",
        if export.cancelling {
            "Cancelling export"
        } else if export.analyzing_audio {
            "Analyzing audio (estimated progress)"
        } else {
            "Encoding (estimated progress)"
        },
        fraction,
    );
}

fn indeterminate_span(elapsed: f64) -> (f32, f32) {
    let offset = ((elapsed / 1.6).fract() * 0.75) as f32;
    (offset, offset + 0.25)
}

fn paint_span(ui: &egui::Ui, track: egui::Rect, start: f32, end: f32) {
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(egui::lerp(track.x_range(), start), track.top()),
            egui::pos2(egui::lerp(track.x_range(), end), track.bottom()),
        ),
        0.0,
        chrome::FOREGROUND,
    );
}

fn accessibility(
    ui: &egui::Ui,
    track: egui::Rect,
    id: &'static str,
    label: &str,
    value: &str,
    fraction: Option<f32>,
) {
    ui.ctx().accesskit_node_builder(egui::Id::new(id), |node| {
        node.set_role(egui::accesskit::Role::ProgressIndicator);
        node.set_label(label);
        node.set_value(value);
        node.set_bounds(egui::accesskit::Rect::new(
            track.left().into(),
            track.top().into(),
            track.right().into(),
            track.bottom().into(),
        ));
        if let Some(fraction) = fraction {
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value(100.0);
            node.set_numeric_value(f64::from(fraction) * 100.0);
        }
    });
}

#[cfg(test)]
pub(crate) mod tests;
