use super::*;
use towavue_core::EditTimeline;

const SETTLE_TIME: Duration = Duration::from_millis(150);

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Key {
    path: PathBuf,
    plan: EditTimeline,
    rate: f32,
    volume: f32,
    columns: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_core::TimelineEdit;

    #[test]
    fn refined_waveform_renders_native_samples_and_rejects_stale_edits_and_sizes() {
        use std::os::windows::process::CommandExt;
        let Some(root) = crate::tests::isolated_test_root(
            "waveform_detail::tests::refined_waveform_renders_native_samples_and_rejects_stale_edits_and_sizes",
        ) else {
            return;
        };
        let path = root.join("tone.wav");
        let ffmpeg =
            PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
        assert!(
            std::process::Command::new(ffmpeg)
                .creation_flags(0x0800_0000)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000:duration=1",
                    "-c:a",
                    "pcm_s16le"
                ])
                .arg(&path)
                .status()
                .expect("owned fixture")
                .success()
        );
        for density in [1.0, 1.25, 2.0] {
            let (send, receive) = std::sync::mpsc::channel();
            let mut app = Application::new(None, move |event| {
                let _ = send.send(event);
            })
            .expect("app");
            let context = fonts::test_context();
            context.enable_accesskit();
            context.set_pixels_per_point(density);
            app.ui_context = Some(context.clone());
            let tab = app.tabs.open_new(path.clone(), MediaKind::Audio);
            app.path = Some(path.clone());
            app.media_kind = Some(MediaKind::Audio);
            app.timeline_open = true;
            app.media_duration = Some(Duration::from_secs(1));
            app.state = PlaybackState::Paused;
            let waveform = context.load_texture(
                "overview",
                egui::ColorImage::filled([16, 4], Color32::WHITE),
                TextureOptions::LINEAR,
            );
            let texture = waveform.id();
            app.waveform = Some(waveform);
            let frame = |app: &mut Application<_>| {
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(640.0, 300.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        let mut actions = Vec::new();
                        app.draw_timeline(ui, &mut actions);
                        assert!(actions.is_empty());
                    },
                )
            };
            frame(&mut app);
            let before = frame(&mut app);
            let rect = before
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                        Some(mesh.calc_bounds())
                    }
                    _ => None,
                })
                .expect("coarse overview during settle time");
            assert!(!app.waveform_detail.started);
            app.waveform_detail.changed = Some(Instant::now() - SETTLE_TIME);
            assert!(app.detailed_waveform(&context, rect, true).is_none());
            assert!(
                !app.waveform_detail.started,
                "release preview must not refine the pre-edit plan"
            );
            let started = frame(&mut app);
            assert!(
                started
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree")
                    .nodes
                    .iter()
                    .any(|(_, node)| node.value() == Some("Refining waveform…")),
                "newly submitted refinement is labeled in the same timeline frame"
            );
            assert!(app.toolbar_loading(&context).is_none());
            assert!(app.waveform_detail.started);
            assert!(app.waveform_detail.is_pending());
            app.load_waveform();
            assert!(
                !app.waveform_loading && app.waveform_detail.is_pending(),
                "a retained source overview must not replace pending refinement"
            );
            let event = receive
                .recv_timeout(Duration::from_secs(10))
                .expect("native envelope worker completion");
            let AppEvent::DetailedWaveform(generation, mut key, result) = event else {
                panic!("waveform event")
            };
            let mut values = result.expect("native samples");
            assert_eq!(values.len(), key.columns as usize);
            assert_eq!(key.columns, (rect.width() * density).round() as u32);
            assert!(values.iter().all(|value| *value > 0.02 && *value < 0.2));
            for restart in [false, true] {
                if restart {
                    app.waveform_detail.restart_pending();
                } else {
                    let wider =
                        egui::Rect::from_min_size(rect.min, rect.size() + egui::vec2(20.0, 0.0));
                    app.detailed_waveform(&context, wider, false);
                    app.detailed_waveform(&context, rect, false);
                }
                app.waveform_detail.changed = Some(Instant::now() - SETTLE_TIME);
                app.detailed_waveform(&context, rect, false);
                assert!(app.waveform_detail.is_pending());
                let status = app.status_message.clone();
                // Hold the completed old response until the same plan/width is pending again.
                app.install_detailed_waveform(generation, key.clone(), Ok(values.clone()));
                assert!(
                    app.waveform_detail.values.is_none(),
                    "obsolete success must not finish a newer request; restart={restart}"
                );
                app.install_detailed_waveform(
                    generation,
                    key.clone(),
                    Err("obsolete failure".into()),
                );
                assert!(
                    app.waveform_detail.is_pending(),
                    "obsolete failure must not clear current progress; restart={restart}"
                );
                assert_eq!(
                    app.status_message, status,
                    "obsolete errors must not replace current status"
                );
                let AppEvent::DetailedWaveform(current_generation, current_key, result) = receive
                    .recv_timeout(Duration::from_secs(10))
                    .expect("replacement refinement")
                else {
                    panic!("replacement waveform event")
                };
                assert_eq!(current_generation, generation);
                let current_values = result.expect("replacement samples");
                assert_eq!(
                    current_values, values,
                    "same plan still produces the same waveform"
                );
                key = current_key;
                values = current_values;
            }
            app.install_detailed_waveform(
                generation.wrapping_add(1),
                key.clone(),
                Ok(values.clone()),
            );
            assert!(app.waveform_detail.values.is_none());
            app.install_detailed_waveform(generation, key.clone(), Ok(values.clone()));
            let retained = app
                .waveform_detail
                .values
                .clone()
                .expect("installed envelope");
            assert!(!app.waveform_detail.is_pending());
            let expected = app.waveform_detail.mesh(rect, density).expect("sharp mesh");
            let output = frame(&mut app);
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.vertices == expected.vertices)));
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture)));
            assert!(
                output.textures_delta.set.is_empty(),
                "refinement uploads no bitmap"
            );
            for vertex in &expected.vertices {
                assert!((vertex.pos.x * density - (vertex.pos.x * density).round()).abs() < 0.0001);
                assert!((vertex.pos.y * density - (vertex.pos.y * density).round()).abs() < 0.0001);
            }
            app.set_playback_volume(3.0);
            assert!(
                app.detailed_waveform(&context, rect, true).is_none(),
                "dragging uses the lightweight source overview"
            );
            assert!(app.detailed_waveform(&context, rect, false).is_some());
            assert!(Arc::ptr_eq(
                &retained,
                app.waveform_detail
                    .values
                    .as_ref()
                    .expect("same listening-independent data")
            ));
            assert!(app.edits.is_empty());
            let taller =
                egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), rect.height() * 2.0));
            assert!(app.detailed_waveform(&context, taller, false).is_some());
            assert!(Arc::ptr_eq(
                &retained,
                app.waveform_detail
                    .values
                    .as_ref()
                    .expect("no height-dependent decode")
            ));
            let saved = app.waveform_detail.take_retained();
            assert!(app.waveform_detail.values.is_none());
            app.waveform_detail = saved;
            app.graphics_epoch += 1;
            assert!(
                app.detailed_waveform(&context, rect, false).is_some(),
                "CPU amplitudes survive graphics recovery"
            );
            assert!(Arc::ptr_eq(
                &retained,
                app.waveform_detail.values.as_ref().expect("retained data")
            ));

            let wider = egui::Rect::from_min_size(rect.min, rect.size() + egui::vec2(20.0, 0.0));
            assert!(app.detailed_waveform(&context, wider, false).is_none());
            app.install_detailed_waveform(generation, key.clone(), Ok(values.clone()));
            assert!(
                app.waveform_detail.values.is_none(),
                "old-width completion cannot install"
            );
            let history = app.edits.entry(tab).or_default();
            history.push(
                EditOperation::Timeline(TimelineEdit::SetVolume(
                    towavue_core::TimeRange::new(
                        MediaTime::ZERO,
                        media_time(Duration::from_millis(500)),
                    )
                    .expect("range"),
                    0.0,
                )),
                MediaKind::Audio,
            );
            app.detailed_waveform(&context, rect, false);
            assert_ne!(
                app.waveform_detail
                    .key
                    .as_ref()
                    .expect("new saved edit")
                    .plan,
                key.plan
            );
            app.install_detailed_waveform(generation, key, Ok(values));
            assert!(
                app.waveform_detail.values.is_none(),
                "old-edit completion cannot install"
            );
            app.waveform_detail.started = true;
            let failed_key = app.waveform_detail.key.clone().expect("pending key");
            app.install_detailed_waveform(
                generation,
                failed_key,
                Err("injected refinement failure".into()),
            );
            assert!(
                !app.waveform_detail.is_pending(),
                "failed detail must not leave a loading indicator running"
            );
            let pending = app.waveform_detail.take_retained();
            assert!(
                !pending.started,
                "cancelled background work is eligible on activation"
            );
            app.waveform_detail = pending;
            app.media_duration = Some(Duration::ZERO);
            frame(&mut app);
            assert!(
                app.waveform_detail.key.is_none(),
                "empty timelines discard pending refinement"
            );
            app.load_path(root.join("next.wav"), MediaKind::Audio);
            assert!(
                app.waveform_detail.key.is_none(),
                "navigation discards the former envelope"
            );
        }
    }

    #[test]
    fn trim_reuses_source_waveform_without_starting_an_overview_job() {
        let Some(root) = crate::tests::isolated_test_root(
            "waveform_detail::tests::trim_reuses_source_waveform_without_starting_an_overview_job",
        ) else {
            return;
        };
        for kind in [MediaKind::Audio, MediaKind::Video] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let context = fonts::test_context();
            app.ui_context = Some(context.clone());
            let path = root.join("retained-source.wav");
            app.tabs.open_new(path.clone(), kind);
            app.path = Some(path);
            app.media_kind = Some(kind);
            app.media_duration = Some(Duration::from_secs(10));
            app.state = PlaybackState::Paused;
            app.waveform = Some(context.load_texture(
                "retained overview",
                egui::ColorImage::filled([16, 4], Color32::WHITE),
                TextureOptions::LINEAR,
            ));
            let texture = app.waveform.as_ref().expect("overview").id();
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 96.0));
            app.detailed_waveform(&context, rect, false);
            for operation in [
                EditOperation::SetTrimStart(media_time(Duration::from_secs(2))),
                EditOperation::SetTrimEnd(media_time(Duration::from_secs(8))),
            ] {
                let previous = app.waveform_detail.key.clone().expect("original plan");
                app.push_edit(operation);
                assert!(app.timeline_open);
                assert!(
                    !app.waveform_loading,
                    "trim must reuse the original overview"
                );
                assert_eq!(app.waveform.as_ref().expect("same overview").id(), texture);
                app.detailed_waveform(&context, rect, false);
                assert_ne!(app.waveform_detail.key.as_ref(), Some(&previous));
                assert!(
                    !app.waveform_detail.started,
                    "the edited plan must settle before decoding"
                );
            }
            assert_eq!(
                app.edit_state().trim_start,
                Some(media_time(Duration::from_secs(2)))
            );
            assert_eq!(
                app.edit_state().trim_end,
                Some(media_time(Duration::from_secs(8)))
            );
            // Navigation/recovery removes this texture; an absent overview must still load.
            app.waveform = None;
            app.load_waveform();
            assert!(app.waveform_loading);
            app.waveform_worker.clear();
        }
    }
}

#[derive(Default)]
pub(super) struct Detail {
    key: Option<Arc<Key>>,
    changed: Option<Instant>,
    started: bool,
    finished: bool,
    values: Option<Arc<[f32]>>,
}

impl Detail {
    pub(super) fn is_pending(&self) -> bool {
        self.started && !self.finished
    }
    pub(super) fn restart_pending(&mut self) {
        if self.values.is_none() {
            self.started = false;
            self.changed = Some(Instant::now());
        }
    }

    pub(super) fn take_retained(&mut self) -> Self {
        let mut saved = std::mem::take(self);
        // Navigation cancels the old worker. A completed CPU envelope survives
        // transfer/recovery, but unfinished work must be eligible on activation.
        saved.restart_pending();
        saved
    }

    fn mesh(&self, rect: egui::Rect, density: f32) -> Option<egui::Mesh> {
        let values = self.values.as_ref()?;
        let snap = |value: f32| (value * density).round() / density;
        let mut mesh = egui::Mesh::default();
        for (column, value) in values.iter().enumerate() {
            let half_height = rect.height() * value.min(1.0) * 0.5;
            let bar = egui::Rect::from_min_max(
                egui::pos2(
                    snap(rect.left() + rect.width() * column as f32 / values.len() as f32),
                    snap(rect.center().y - half_height),
                ),
                egui::pos2(
                    snap(rect.left() + rect.width() * (column + 1) as f32 / values.len() as f32),
                    snap(rect.center().y + half_height),
                ),
            );
            if bar.is_positive() {
                mesh.add_colored_rect(bar, Color32::from_white_alpha(150));
            }
        }
        Some(mesh)
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn draw_waveform_activity(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let label = if self.waveform_loading {
            "Loading waveform…"
        } else if self.waveform_detail.is_pending() {
            "Refining waveform…"
        } else {
            return;
        };
        ui.put(
            egui::Rect::from_center_size(
                rect.center(),
                egui::vec2((rect.width() - 48.0).max(0.0), 20.0_f32.min(rect.height())),
            ),
            egui::Label::new(
                egui::RichText::new(label)
                    .size(time_selection::LABEL_SIZE)
                    .color(chrome::FOREGROUND)
                    .background_color(chrome::BORDER),
            )
            .truncate()
            .show_tooltip_when_elided(false)
            .selectable(false),
        );
    }

    pub(super) fn clear_detailed_waveform(&mut self) {
        if self.waveform_detail.started && !self.waveform_loading {
            self.waveform_worker.clear();
        }
        self.waveform_detail = Detail::default();
    }

    pub(super) fn detailed_waveform(
        &mut self,
        context: &egui::Context,
        rect: egui::Rect,
        gain_preview: bool,
    ) -> Option<egui::Mesh> {
        let path = self.path.as_ref()?;
        let state = self.edit_state();
        let fallback;
        let plan = if let Some(plan) = self.session.as_ref().and_then(PlaybackSession::timeline) {
            plan
        } else {
            fallback = self.history_timeline().ok()?.or_else(|| {
                EditTimeline::new(media_time(self.media_duration?), state.playback_range())
            })?;
            &fallback
        };
        if plan.spans().is_empty() {
            self.clear_detailed_waveform();
            return None;
        }
        let columns = (rect.width() * context.pixels_per_point())
            .round()
            .clamp(1.0, 8192.0) as u32;
        let matches = self.waveform_detail.key.as_ref().is_some_and(|key| {
            key.path == *path
                && key.plan == *plan
                && key.rate == state.rate
                && key.volume == state.volume
                && key.columns == columns
        });
        if !matches {
            if self.waveform_detail.started && !self.waveform_loading {
                self.waveform_worker.clear();
            }
            self.waveform_detail = Detail {
                key: Some(Arc::new(Key {
                    path: path.clone(),
                    plan: plan.clone(),
                    rate: state.rate,
                    volume: state.volume,
                    columns,
                })),
                changed: Some(Instant::now()),
                ..Default::default()
            };
        }
        let detail = &mut self.waveform_detail;
        if !detail.started
            && !self.waveform_loading
            && !gain_preview
            && !crate::timeline_input::is_active(context)
        {
            let remaining =
                SETTLE_TIME.saturating_sub(detail.changed.expect("key timestamp").elapsed());
            if remaining.is_zero() {
                // Give every submission a distinct identity, including retries of
                // identical parameters: an already queued result cannot finish it.
                let key = Arc::new(detail.key.as_deref().expect("requested key").clone());
                detail.key = Some(Arc::clone(&key));
                let notify = Arc::clone(&self.notify);
                let generation = self.media_generation;
                detail.started = true;
                detail.finished = false;
                self.waveform_worker.submit(move |cancellation| {
                    let result = towavue_runtime_windows::timeline_waveform(
                        &key.path,
                        &key.plan,
                        key.rate,
                        key.volume,
                        key.columns,
                        &cancellation,
                    )
                    .map_err(|error| error.to_string());
                    notify(AppEvent::DetailedWaveform(generation, key, result));
                });
            } else {
                context.request_repaint_after(remaining);
            }
        }
        if gain_preview {
            None
        } else {
            detail.mesh(rect, context.pixels_per_point())
        }
    }

    pub(super) fn install_detailed_waveform(
        &mut self,
        generation: u64,
        key: Arc<Key>,
        result: Result<Vec<f32>, String>,
    ) {
        if generation != self.media_generation
            || self.path.as_ref() != Some(&key.path)
            || !self
                .waveform_detail
                .key
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &key))
            || !self.waveform_detail.started
        {
            return;
        }
        self.waveform_detail.finished = true;
        match result {
            Ok(values) => self.waveform_detail.values = Some(values.into()),
            Err(error) => self.set_status(format!("Detailed waveform unavailable: {error}")),
        }
        self.request_redraw();
    }
}
