use crate::*;

const LOADING_DELAY: Duration = Duration::from_millis(200);

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
    pub(super) fn export_gesture_hint(&self) -> Option<String> {
        if let Some(message) = self.ui_context.as_ref().and_then(seekbar::precision_status) {
            Some(message.into())
        } else if self.reading_drag.is_some() {
            Some(self.reading_status())
        } else if self.held_speed.is_some() || self.track_drag.is_some() {
            self.status_notice()
        } else if self.view_drag.is_some() {
            self.visual_selection_status()
        } else {
            None
        }
    }

    pub(super) fn folder_notice_delay(&self, now: Instant) -> Option<Duration> {
        if !matches!(self.pending_folder, Some((_, FolderIntent::Refresh(_)))) {
            return None;
        }
        LOADING_DELAY
            .checked_sub(now.saturating_duration_since(self.folder_refresh_started))
            .filter(|remaining| !remaining.is_zero())
    }

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
        let delay = LOADING_DELAY.as_secs_f64();
        if elapsed < delay {
            context.request_repaint_after(Duration::from_secs_f64(delay - elapsed));
            None
        } else {
            Some((label, elapsed - delay))
        }
    }
}

fn taskbar_progress(export: Option<&ActiveExport>) -> towavue_runtime_windows::TaskbarProgress {
    use towavue_runtime_windows::TaskbarProgress;
    let Some(export) = export else {
        return TaskbarProgress::Hidden;
    };
    if !export.job.cancellable() || export.cancelling {
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
                job: ExportJob::start(request.clone(), |_| {})
                    .expect("fixture worker")
                    .into(),
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
    started: Instant,
    duration: Option<Duration>,
    normalized: bool,
    verifies_loudness: bool,
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
            started: Instant::now(),
            duration,
            normalized: options.audio.normalization.is_enabled(),
            verifies_loudness: matches!(
                options.audio.normalization,
                towavue_runtime_windows::AudioNormalization::Loudness(_)
            ),
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
        // No output timestamp yet: source probing, preroll and encoder startup
        // have no measurable fraction, even when the output duration is known.
        // Loudness may re-encode after measuring the compressed candidate; the
        // number of passes is unknown until its constraints are met.
        if self.verifies_loudness || time.is_zero() {
            return None;
        }
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

fn preparation_label(analyzing: bool) -> &'static str {
    if analyzing {
        "Preparing audio analysis"
    } else {
        "Preparing output"
    }
}

/// The separate nonmodal area keeps Cancel usable while a save continuation
/// disables the main UI. Other dialogs still disable it and native modal layers
/// retain their normal input priority.
pub(super) fn show_status(
    context: &egui::Context,
    rect: egui::Rect,
    parent: Option<egui::LayerId>,
    export: &ActiveExport,
    gesture: Option<String>,
    enabled: bool,
    actions: &mut Vec<UiAction>,
) {
    let operation = if export.job.is_save() {
        "Saving"
    } else {
        "Exporting"
    };
    let heading = if export.continuation.is_some() {
        format!("{operation} before continuing")
    } else {
        operation.into()
    };
    let mut parts = vec![heading, status(export, Instant::now())];
    if export.options.audio != AudioExportOptions::default() {
        parts.push(audio_export::summary(export.options.audio));
    }
    if !export.options.metadata.is_empty() {
        parts.push("Metadata changes: verified before replacing the target".into());
    }
    if let Some(gesture) = gesture {
        parts.insert(0, gesture);
    }
    let message = parts.join(" · ");
    let tooltip = format!("{}\n{}", export.request.target.display(), parts.join("\n"));
    if !export.cancelling && export.encoded.is_zero() {
        context.request_repaint_after(Duration::from_secs(1));
    }
    let area = egui::Area::new("export-status".into());
    if let Some(parent) = parent {
        context.set_sublayer(parent, area.layer());
    }
    area.order(egui::Order::Middle)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .movable(false)
        .constrain(false)
        .enabled(enabled)
        .show(context, |ui| {
            ui.set_width(rect.width());
            ui.set_height(rect.height());
            ui.set_clip_rect(rect.intersect(context.content_rect()));
            ui.interact(
                rect,
                ui.id().with("padding"),
                egui::Sense::CLICK | egui::Sense::DRAG,
            );
            chrome::flat_buttons(ui);
            ui.spacing_mut().item_spacing.x = chrome::STATUS_BUTTON_GAP;
            ui.spacing_mut().button_padding = egui::Vec2::ZERO;
            ui.spacing_mut().interact_size = egui::Vec2::splat(chrome::STATUS_BUTTON_SIZE);
            ui.horizontal_centered(|ui| {
                let label = if export.job.is_save() {
                    "Cancel save"
                } else {
                    "Cancel export"
                };
                let cancel = ui
                    .add_enabled_ui(!export.cancelling && export.job.cancellable(), |ui| {
                        chrome::status_button(
                            ui,
                            egui::vec2(54.0, chrome::STATUS_BUTTON_SIZE),
                            egui::Button::new(egui::RichText::new("Cancel").size(12.0))
                                .fill(chrome::HOVER)
                                .stroke(egui::Stroke::NONE),
                        )
                    })
                    .inner;
                cancel.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, cancel.enabled(), label)
                });
                let cancel = cancel
                    .help_text(label)
                    .disabled_help_text(if export.cancelling {
                        "Cancellation requested"
                    } else {
                        "Publishing the saved file"
                    });
                if cancel.clicked() {
                    actions.push(UiAction::CancelExport);
                }
                ui.add(
                    egui::Label::new(fonts::reading_hint(&message, 12.0, chrome::FOREGROUND))
                        .truncate()
                        .show_tooltip_when_elided(false),
                )
                .help_ui_above(|ui| {
                    ui.label(fonts::reading_hint(&tooltip, 14.0, chrome::FOREGROUND));
                });
            });
        });
}

pub(super) fn status(export: &ActiveExport, now: Instant) -> String {
    if !export.job.cancellable() {
        return "Publishing the saved file…".into();
    }
    if export.cancelling {
        if export.job.is_save() {
            "Cancelling save…".to_owned()
        } else {
            "Cancelling export…".to_owned()
        }
    } else if export.encoded.is_zero() {
        format!(
            "{} · elapsed {}",
            preparation_label(export.analyzing_audio),
            format_time(media_time(
                now.saturating_duration_since(export.progress.started)
            ))
        )
    } else {
        format!(
            "{} {}",
            if export.analyzing_audio {
                "Analyzing audio"
            } else {
                "Encoded"
            },
            format_time(media_time(export.encoded))
        )
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
        } else if export.encoded.is_zero() {
            preparation_label(export.analyzing_audio)
        } else if progress.verifies_loudness {
            if export.analyzing_audio {
                "Analyzing and verifying audio"
            } else {
                "Encoding audio to meet loudness targets"
            }
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
