use crate::*;

pub(super) struct ExportProgress {
    duration: Option<Duration>,
    normalized: bool,
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
            animation_start: None,
            stopped_at: None,
        }
    }

    fn fraction(&self, time: Duration, analyzing: bool) -> Option<f32> {
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

pub(super) fn draw(ui: &egui::Ui, panel: egui::Rect, export: Option<&mut ActiveExport>) {
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
        let offset = ((elapsed / 1.6).fract() * 0.75) as f32;
        (offset, offset + 0.25)
    };
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(egui::lerp(track.x_range(), start), track.top()),
            egui::pos2(egui::lerp(track.x_range(), end), track.bottom()),
        ),
        0.0,
        chrome::FOREGROUND,
    );
    ui.ctx()
        .accesskit_node_builder(egui::Id::new("toolbar-export-progress"), |node| {
            node.set_role(egui::accesskit::Role::ProgressIndicator);
            node.set_label("Export progress");
            node.set_value(if export.cancelling {
                "Cancelling export"
            } else if export.analyzing_audio {
                "Analyzing audio (estimated progress)"
            } else {
                "Encoding (estimated progress)"
            });
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
