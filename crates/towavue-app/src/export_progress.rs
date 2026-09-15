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
    pub(super) fn sync_taskbar_progress(&mut self) {
        let progress = taskbar_progress(self.active_export.as_ref());
        if let Some(taskbar) = &mut self.native_taskbar
            && let Err(error) = taskbar.set_progress(progress)
        {
            eprintln!("Could not update taskbar progress: {error}");
        }
    }

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

fn taskbar_progress(export: Option<&ActiveExport>) -> towavue_runtime_windows::TaskbarProgress {
    use towavue_runtime_windows::TaskbarProgress;
    let Some(export) = export else {
        return TaskbarProgress::Hidden;
    };
    if export.cancelling {
        return TaskbarProgress::Indeterminate;
    }
    export
        .progress
        .fraction(export.encoded, export.analyzing_audio)
        .map_or(TaskbarProgress::Indeterminate, |fraction| {
            TaskbarProgress::Fraction((fraction * 1000.0).round() as u16)
        })
}

#[cfg(test)]
mod taskbar_tests {
    use super::*;
    use towavue_runtime_windows::{ExportOutcome, TaskbarProgress};

    #[test]
    fn taskbar_progress_tracks_export_phases_and_clears_without_drawing() {
        let Some(root) = crate::tests::isolated_test_root(
            "export_progress::taskbar_tests::taskbar_progress_tracks_export_phases_and_clears_without_drawing",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        let path = root.join("not-written.mp4");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Video);
        assert_eq!(taskbar_progress(None), TaskbarProgress::Hidden);
        for outcome in [
            Ok(ExportOutcome {
                used_hardware_encoder: false,
            }),
            Err(ExportError::Cancelled),
            Err(ExportError::Failed("fixture failure".into())),
        ] {
            let request = ExportRequest {
                source: path.clone(),
                target: path.clone(),
                kind: MediaKind::Video,
                operations: Vec::new(),
                hardware_encode: false,
            };
            let options = ExportOptions::default();
            app.active_export = Some(ActiveExport {
                progress: ExportProgress::new(&request, &options, Some(Duration::from_secs(100))),
                // Same-source validation rejects this job without reading/writing media.
                job: ExportJob::start(request.clone(), |_| {}).expect("fixture worker"),
                tab,
                request,
                options,
                encoded: Duration::ZERO,
                analyzing_audio: false,
                cancelling: false,
                continuation: None,
            });
            app.handle_app_event(AppEvent::Export(ExportEvent::Progress(
                Duration::from_secs(25),
            )));
            assert_eq!(
                taskbar_progress(app.active_export.as_ref()),
                TaskbarProgress::Fraction(250)
            );
            let other = app.tabs.open_new(root.join("other.png"), MediaKind::Image);
            assert_ne!(other, tab);
            assert_eq!(
                taskbar_progress(app.active_export.as_ref()),
                TaskbarProgress::Fraction(250)
            );
            app.active_export
                .as_mut()
                .expect("export")
                .progress
                .normalized = true;
            app.handle_app_event(AppEvent::Export(ExportEvent::AnalyzingAudio(
                Duration::from_secs(50),
            )));
            assert_eq!(
                taskbar_progress(app.active_export.as_ref()),
                TaskbarProgress::Fraction(250)
            );
            app.handle_app_event(AppEvent::Export(ExportEvent::Progress(
                Duration::from_secs(50),
            )));
            assert_eq!(
                taskbar_progress(app.active_export.as_ref()),
                TaskbarProgress::Fraction(750)
            );
            app.handle_app_event(AppEvent::Export(ExportEvent::Progress(
                Duration::from_secs(500),
            )));
            assert_eq!(
                taskbar_progress(app.active_export.as_ref()),
                TaskbarProgress::Fraction(990)
            );
            app.active_export.as_mut().expect("export").cancelling = true;
            app.handle_app_event(AppEvent::Export(ExportEvent::Progress(Duration::ZERO)));
            assert_eq!(
                taskbar_progress(app.active_export.as_ref()),
                TaskbarProgress::Indeterminate
            );
            app.handle_app_event(AppEvent::Export(ExportEvent::Finished(outcome)));
            assert_eq!(
                taskbar_progress(app.active_export.as_ref()),
                TaskbarProgress::Hidden
            );
            app.handle_app_event(AppEvent::TaskbarReady);
            assert!(
                app.ui_context.is_none(),
                "no render or UI initialization required"
            );
        }
        assert!(!path.exists());
    }

    #[test]
    fn taskbar_fraction_uses_edited_duration_and_unknown_work_is_indeterminate() {
        let mut request = ExportRequest {
            source: "source.mp4".into(),
            target: "target.mp4".into(),
            kind: MediaKind::Video,
            operations: vec![EditOperation::SetRate(2.0)],
            hardware_encode: false,
        };
        let options = ExportOptions::default();
        let progress = ExportProgress::new(&request, &options, Some(Duration::from_secs(100)));
        assert_eq!(progress.fraction(Duration::from_secs(25), false), Some(0.5));
        assert_eq!(progress.fraction(Duration::from_secs(25), true), None);
        for duration in [None, Some(Duration::ZERO)] {
            assert_eq!(
                ExportProgress::new(&request, &options, duration).fraction(Duration::ZERO, false),
                None
            );
        }
        request.kind = MediaKind::Image;
        assert_eq!(
            ExportProgress::new(&request, &options, Some(Duration::from_secs(100)))
                .fraction(Duration::ZERO, false),
            None
        );
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
