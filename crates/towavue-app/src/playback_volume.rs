use super::*;

#[derive(Clone, Copy)]
pub(super) struct PlaybackVolume {
    level: f32,
    unmuted: f32,
}

impl Default for PlaybackVolume {
    fn default() -> Self {
        Self {
            level: 1.0,
            unmuted: 1.0,
        }
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn tab_audio_indicator(&self, tab: &towavue_core::Tab) -> Option<bool> {
        let id = tab.id;
        let (state, drained, session) = if self.tabs.active().is_some_and(|tab| tab.id == id) {
            if self.path.as_deref() != Some(tab.target.current_path()) {
                return None;
            }
            (self.state, self.audio_drained, self.session.as_ref())
        } else {
            let saved = self.retained_playback.get(&id)?;
            if saved.path != tab.target.current_path() {
                return None;
            }
            (saved.state, saved.audio_drained, saved.session.as_ref())
        };
        (state == PlaybackState::Playing
            && !drained
            && session.is_some_and(PlaybackSession::has_audio))
        .then(|| {
            self.playback_volumes
                .get(&id)
                .is_some_and(|volume| volume.level == 0.0)
        })
    }

    pub(super) fn playback_volume(&self) -> f32 {
        self.tabs
            .active()
            .and_then(|tab| self.playback_volumes.get(&tab.id))
            .copied()
            .unwrap_or_default()
            .level
    }

    pub(super) fn toggle_tab_mute(&mut self, id: TabId) {
        let Some(tab) = self.tabs.tabs().iter().find(|tab| tab.id == id) else {
            return;
        };
        if self.tab_audio_indicator(tab).is_none() {
            return;
        }
        if self.tabs.active().is_some_and(|tab| tab.id == id) {
            self.toggle_playback_mute();
            return;
        }
        let volume = self.playback_volumes.entry(id).or_default();
        volume.level = if volume.level == 0.0 {
            volume.unmuted
        } else {
            0.0
        };
        let gain = self
            .edits
            .get(&id)
            .map(EditHistory::state)
            .unwrap_or_default()
            .volume;
        self.retained_playback
            .get_mut(&id)
            .expect("playing tab")
            .session
            .as_mut()
            .expect("audio output")
            .set_volume(gain * volume.level);
        self.request_redraw();
    }

    pub(super) fn set_playback_volume(&mut self, level: f32) {
        if !level.is_finite()
            || !matches!(self.media_kind, Some(MediaKind::Audio | MediaKind::Video))
        {
            return;
        }
        let Some(id) = self.tabs.active().map(|tab| tab.id) else {
            return;
        };
        let level = level.clamp(0.0, towavue_core::MAX_VOLUME);
        if level == self.playback_volume() {
            return;
        }
        let volume = self.playback_volumes.entry(id).or_default();
        volume.level = level;
        if level > 0.0 {
            volume.unmuted = level;
        }
        // Existing saved gain remains independent; changing the listening level
        // never re-decodes the timeline, modifies history, or changes export.
        let gain = self.edit_state().volume * level;
        if let Some(session) = &mut self.session {
            session.set_volume(gain);
        }
        self.volume_hud
            .changed(id, self.media_generation, Instant::now());
        self.request_redraw();
    }

    pub(super) fn toggle_playback_mute(&mut self) {
        let volume = self
            .tabs
            .active()
            .and_then(|tab| self.playback_volumes.get(&tab.id))
            .copied()
            .unwrap_or_default();
        self.set_playback_volume(if volume.level == 0.0 {
            volume.unmuted
        } else {
            0.0
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires D3D11 and a live shared WASAPI endpoint; generated silence only"]
    fn native_playback_volume_survives_reprime_tab_switch_and_device_recovery() {
        use winit::platform::windows::EventLoopBuilderExtWindows;

        let Some(root) = crate::tests::isolated_test_root(
            "playback_volume::tests::native_playback_volume_survives_reprime_tab_switch_and_device_recovery",
        ) else {
            return;
        };
        let path = root.join("silence.wav");
        let ffmpeg =
            PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
        assert!(
            std::process::Command::new(ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=r=48000:cl=stereo",
                    "-t",
                    "10",
                    "-c:a",
                    "pcm_s16le"
                ])
                .arg(&path)
                .status()
                .expect("silent fixture")
                .success()
        );
        let next = root.join("next.wav");
        std::fs::copy(&path, &next).expect("next file");

        fn check<N: Fn(AppEvent) + Send + Sync + 'static>(app: &Application<N>, level: f32) {
            let session = app.session.as_ref().expect("live audio session");
            assert_eq!(session.verification_volume(), (level, Some(level)));
            assert_eq!(app.playback_volume(), level);
            let tab = app.tabs.active().expect("active tab");
            assert_eq!(app.tab_audio_indicator(tab), Some(level == 0.0));
            assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
        }

        fn tab_frame<N: Fn(AppEvent) + Send + Sync + 'static>(
            app: &mut Application<N>,
            context: &egui::Context,
            events: Vec<egui::Event>,
        ) -> (egui::FullOutput, Vec<UiAction>) {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_top_bar(ui, &mut actions),
            );
            (output, actions)
        }

        struct Trial {
            path: PathBuf,
            next: PathBuf,
            completed: bool,
        }
        impl ApplicationHandler for Trial {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let window = Arc::new(
                    event_loop
                        .create_window(
                            Window::default_attributes()
                                .with_visible(false)
                                .with_inner_size(LogicalSize::new(320, 240)),
                        )
                        .expect("hidden window"),
                );
                let renderer = FrameRenderer::new(&window).expect("D3D11 renderer");
                let mut app = Application::new(None, |_| {}).expect("app");
                app.window = Some(window);
                app.renderer = Some(renderer);
                app.ui_context = Some(fonts::test_context());
                let first = app.tabs.open_new(self.path.clone(), MediaKind::Audio);
                app.playback_volumes.insert(
                    first,
                    PlaybackVolume {
                        level: 0.3,
                        unmuted: 0.3,
                    },
                );
                app.load_path(self.path.clone(), MediaKind::Audio);
                app.media_duration = Some(Duration::from_secs(10));
                check(&app, 0.3);
                for state in [
                    PlaybackState::Paused,
                    PlaybackState::Ended,
                    PlaybackState::Faulted,
                ] {
                    let old = std::mem::replace(&mut app.state, state);
                    assert_eq!(
                        app.tab_audio_indicator(app.tabs.active().expect("tab")),
                        None
                    );
                    app.state = old;
                }
                app.audio_drained = true;
                assert_eq!(
                    app.tab_audio_indicator(app.tabs.active().expect("tab")),
                    None
                );
                app.audio_drained = false;
                let session = app.session.take();
                assert_eq!(
                    app.tab_audio_indicator(app.tabs.active().expect("tab")),
                    None
                );
                app.session = session;
                let generation = app.generation;
                app.handle_ui_action(UiAction::Volume(first, 3.0));
                check(&app, 3.0);
                assert_eq!(
                    app.generation, generation,
                    "volume does not restart decoding"
                );
                app.dispatch(CommandId::ToggleMute);
                check(&app, 0.0);
                app.push_edit(EditOperation::SetRate(1.25));
                check(&app, 0.0);
                assert_ne!(app.generation, generation, "exercise a real rate re-prime");
                app.dispatch(CommandId::Undo);
                check(&app, 0.0);
                let range = towavue_core::TimeRange::new(
                    MediaTime::ZERO,
                    media_time(Duration::from_secs(1)),
                )
                .expect("gain range");
                app.push_edit(EditOperation::Timeline(
                    towavue_core::TimelineEdit::SetVolume(range, 0.5),
                ));
                check(&app, 0.0);
                assert!(app.session.as_ref().expect("session").timeline().is_some());
                app.dispatch(CommandId::ToggleMute);
                check(&app, 3.0);
                app.push_edit(EditOperation::SetVolume(3.0));
                assert_eq!(app.playback_volume(), 3.0);
                assert_eq!(
                    app.session
                        .as_ref()
                        .expect("combined output")
                        .verification_volume(),
                    (9.0, Some(9.0))
                );
                app.dispatch(CommandId::Undo);
                check(&app, 3.0);
                let history = app.edits[&first].clone();

                let second = app.tabs.open_new(self.path.clone(), MediaKind::Audio);
                app.playback_volumes.insert(
                    second,
                    PlaybackVolume {
                        level: 0.8,
                        unmuted: 0.8,
                    },
                );
                app.load_path(self.path.clone(), MediaKind::Audio);
                app.media_duration = Some(Duration::from_secs(10));
                check(&app, 0.8);
                app.dispatch(CommandId::ToggleMute);
                check(&app, 0.0);
                assert_eq!(
                    app.retained_playback[&first]
                        .session
                        .as_ref()
                        .expect("background audio")
                        .verification_volume(),
                    (3.0, Some(3.0))
                );
                let first_tab = app
                    .tabs
                    .tabs()
                    .iter()
                    .find(|tab| tab.id == first)
                    .expect("background tab");
                assert_eq!(app.tab_audio_indicator(first_tab), Some(false));
                app.retained_playback
                    .get_mut(&first)
                    .expect("background")
                    .state = PlaybackState::Paused;
                assert_eq!(app.tab_audio_indicator(first_tab), None);
                app.retained_playback
                    .get_mut(&first)
                    .expect("background")
                    .state = PlaybackState::Playing;
                let context = app.ui_context.clone().expect("context");
                context.enable_accesskit();
                for _ in 0..3 {
                    let mut actions = Vec::new();
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(960.0, 576.0),
                            )),
                            ..Default::default()
                        },
                        |ui| app.draw_top_bar(ui, &mut actions),
                    );
                    assert!(actions.is_empty());
                    let tree = output
                        .platform_output
                        .accesskit_update
                        .expect("tab accessibility");
                    for description in ["Playing audio", "Playing audio — Muted"] {
                        let nodes: Vec<_> = tree
                            .nodes
                            .iter()
                            .filter(|(_, node)| node.description() == Some(description))
                            .collect();
                        assert_eq!(nodes.len(), 1);
                        assert_eq!(
                            nodes[0].1.label(),
                            Some("silence.wav"),
                            "icons do not pollute the accessible filename"
                        );
                    }
                }
                for density in [1.0, 1.25, 2.0] {
                    context.set_pixels_per_point(density);
                    for _ in 0..3 {
                        tab_frame(&mut app, &context, vec![]);
                    }
                    let histories = app.edits.clone();
                    let generation = app.generation;
                    for (id, label, expected) in [
                        (first, "Mute tab: silence.wav", 0.0),
                        (second, "Unmute tab: silence.wav", 0.8),
                    ] {
                        let (output, _) = tab_frame(&mut app, &context, vec![]);
                        let tree = output.platform_output.accesskit_update.expect("tree");
                        let (node_id, node) = tree
                            .nodes
                            .iter()
                            .find(|(_, node)| node.label() == Some(label))
                            .expect("audio button");
                        let bounds = node.bounds().expect("bounds");
                        let icon = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Text(text)
                                    if matches!(text.galley.text(), "\u{eb75}" | "\u{eb24}") =>
                                {
                                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                                    (rect.center().x >= bounds.x0 as f32
                                        && rect.center().x <= bounds.x1 as f32)
                                        .then_some(rect)
                                }
                                _ => None,
                            })
                            .expect("audio icon in its own hit region");
                        let title_left = output
                            .shapes
                            .iter()
                            .filter_map(|shape| match &shape.shape {
                                egui::Shape::Text(text)
                                    if text.galley.text() == "silence.wav"
                                        && text.pos.x >= bounds.x1 as f32 =>
                                {
                                    Some(text.pos.x)
                                }
                                _ => None,
                            })
                            .min_by(f32::total_cmp)
                            .expect("following tab title");
                        assert!(
                            ((icon.left() - bounds.x0 as f32) - (title_left - icon.right())).abs()
                                <= 1.0 / density,
                            "tab audio icon must have equal leading and trailing spacing"
                        );
                        let point = egui::pos2(
                            ((bounds.x0 + bounds.x1) / 2.0) as f32,
                            ((bounds.y0 + bounds.y1) / 2.0) as f32,
                        );
                        let button = |pos, pressed| egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        };
                        // Dragging out of this control must not become a tab move or activation.
                        tab_frame(
                            &mut app,
                            &context,
                            vec![egui::Event::PointerMoved(point), button(point, true)],
                        );
                        let outside = point + egui::vec2(60.0, 80.0);
                        let (_, actions) =
                            tab_frame(&mut app, &context, vec![egui::Event::PointerMoved(outside)]);
                        assert!(actions.is_empty());
                        assert!(
                            crate::tab_drag::active_pointer(
                                &context,
                                (Some(second), app.media_generation, app.graphics_epoch)
                            )
                            .is_none()
                        );
                        let (_, actions) =
                            tab_frame(&mut app, &context, vec![button(outside, false)]);
                        assert!(actions.is_empty());
                        let (output, actions) =
                            tab_frame(&mut app, &context, vec![egui::Event::PointerMoved(point)]);
                        assert!(actions.is_empty());
                        assert_eq!(
                            output.platform_output.cursor_icon,
                            egui::CursorIcon::PointingHand
                        );
                        let (_, actions) = tab_frame(&mut app, &context, vec![button(point, true)]);
                        assert!(actions.is_empty(), "toggle waits for release");
                        let (_, actions) =
                            tab_frame(&mut app, &context, vec![button(point, false)]);
                        assert!(
                            matches!(actions.as_slice(), [UiAction::ToggleTabMute(tab)] if *tab == id)
                        );
                        for action in actions {
                            app.handle_ui_action(action);
                        }
                        assert_eq!(app.playback_volumes[&id].level, expected);
                        let session = if id == first {
                            app.retained_playback[&id].session.as_ref()
                        } else {
                            app.session.as_ref()
                        }
                        .expect("session");
                        assert_eq!(session.verification_volume(), (expected, Some(expected)));
                        let (_, actions) = tab_frame(
                            &mut app,
                            &context,
                            vec![egui::Event::AccessKitActionRequest(
                                egui::accesskit::ActionRequest {
                                    action: egui::accesskit::Action::Click,
                                    target_tree: egui::accesskit::TreeId::ROOT,
                                    target_node: *node_id,
                                    data: None,
                                },
                            )],
                        );
                        assert!(
                            matches!(actions.as_slice(), [UiAction::ToggleTabMute(tab)] if *tab == id)
                        );
                        for action in actions {
                            app.handle_ui_action(action);
                        }
                        assert_eq!(
                            app.playback_volumes[&id].level,
                            if id == first { 3.0 } else { 0.0 }
                        );
                        assert_eq!(app.tabs.active().expect("active").id, second);
                        assert_eq!(app.edits, histories);
                        assert_eq!(app.generation, generation);
                        assert!(
                            crate::tab_drag::active_pointer(
                                &context,
                                (Some(second), app.media_generation, app.graphics_epoch)
                            )
                            .is_none()
                        );
                    }
                }
                let saved = &app.retained_playback[&first];
                let instance = saved.instance;
                let generation = saved.session.as_ref().expect("background").generation();
                app.handle_background_playback(
                    instance,
                    PlaybackEvent::Failed(generation, "Injected background failure".into()),
                );
                assert_eq!(app.retained_playback[&first].state, PlaybackState::Faulted);
                let first_tab = app
                    .tabs
                    .tabs()
                    .iter()
                    .find(|tab| tab.id == first)
                    .expect("tab");
                assert_eq!(app.tab_audio_indicator(first_tab), None);
                app.retained_playback
                    .get_mut(&first)
                    .expect("background")
                    .restart();
                assert_eq!(app.tab_audio_indicator(first_tab), Some(false));
                // The injected fault is not part of the subsequent recovery trial.
                app.retained_playback
                    .get_mut(&first)
                    .expect("background")
                    .error = None;
                app.activate_tab(first);
                check(&app, 3.0);
                assert_eq!(app.edits[&first], history);
                let epoch = app.graphics_epoch;
                let position = app.current_position();
                app.recover_graphics_device(position);
                assert!(app.graphics_epoch > epoch);
                check(&app, 3.0);
                assert_eq!(
                    app.retained_playback[&second]
                        .session
                        .as_ref()
                        .expect("recovered background audio")
                        .verification_volume(),
                    (0.0, Some(0.0))
                );
                assert_eq!(app.edits[&first], history);

                app.tabs
                    .get_mut(first)
                    .expect("tab")
                    .target
                    .set_current_path(self.next.clone(), MediaKind::Audio);
                app.load_path(self.next.clone(), MediaKind::Audio);
                check(&app, 3.0);
                app.dispatch(CommandId::ToggleMute);
                check(&app, 0.0);
                app.dispatch(CommandId::ToggleMute);
                check(&app, 3.0);
                let moved_window = Arc::new(
                    event_loop
                        .create_window(
                            Window::default_attributes()
                                .with_visible(false)
                                .with_inner_size(LogicalSize::new(320, 240)),
                        )
                        .expect("destination window"),
                );
                let renderer = FrameRenderer::with_graphics_device(
                    &moved_window,
                    app.renderer.as_ref().expect("renderer").graphics_device(),
                )
                .expect("shared-device destination");
                let mut destination = Application::new(None, |_| {}).expect("destination");
                destination.window = Some(moved_window);
                destination.renderer = Some(renderer);
                destination.ui_context = Some(fonts::test_context());
                app.dispatch(CommandId::ToggleMute);
                check(&app, 0.0);
                let generation = app.session.as_ref().expect("moving session").generation();
                let request = app
                    .tab_detach_request(first)
                    .expect("live transfer request");
                let packet = app.take_tab_transfer(&request, None);
                let moved = destination.accept_tab_transfer(packet, 0);
                check(&destination, 0.0);
                assert_eq!(
                    destination
                        .session
                        .as_ref()
                        .expect("moved session")
                        .generation(),
                    generation
                );
                destination.dispatch(CommandId::ToggleMute);
                check(&destination, 3.0);
                assert!(!app.playback_volumes.contains_key(&first));
                destination.remove_tab(moved, false);
                assert!(destination.session.is_none());
                assert!(destination.playback_volumes.is_empty());
                check(&app, 0.0);
                app.dispatch(CommandId::ToggleMute);
                check(&app, 0.8);
                self.completed = true;
                event_loop.exit();
            }

            fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        }
        let mut trial = Trial {
            path,
            next,
            completed: false,
        };
        let mut event_loop = EventLoop::builder();
        event_loop.with_any_thread(true);
        event_loop
            .build()
            .expect("event loop")
            .run_app(&mut trial)
            .expect("native trial");
        assert!(
            trial.completed,
            "native volume path must execute, not silently skip"
        );
        eprintln!(
            "PASS native volume commands: initial/live/retained/re-primed/recovered WASAPI mixer levels; generated silence, no physical input or acoustic measurement"
        );
    }

    #[test]
    fn playback_volume_commands_do_not_change_export_history() {
        let Some(root) = crate::tests::isolated_test_root(
            "playback_volume::tests::playback_volume_commands_do_not_change_export_history",
        ) else {
            return;
        };
        for kind in [MediaKind::Audio, MediaKind::Video] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let source = root.join(if kind == MediaKind::Audio {
                "audio.wav"
            } else {
                "video.mp4"
            });
            let tab = app.tabs.open_new(source.clone(), kind);
            app.path = Some(source.clone());
            app.media_kind = Some(kind);
            app.state = PlaybackState::Paused;
            app.media_duration = Some(Duration::from_secs(2));
            app.set_playback_volume(2.9);
            app.dispatch(CommandId::VolumeUp);
            assert_eq!(app.playback_volume(), 3.0);
            app.dispatch(CommandId::VolumeUp);
            assert_eq!(app.playback_volume(), 3.0);
            app.toggle_playback_mute();
            app.toggle_playback_mute();
            assert_eq!(app.playback_volume(), 3.0);
            app.handle_ui_action(UiAction::Volume(tab, 0.4));
            assert_eq!(
                app.edit_state().volume,
                1.0,
                "wheel must not change export gain"
            );
            assert!(!app.command_context().has_unsaved_edits);
            app.dispatch(CommandId::VolumeUp);
            app.dispatch(CommandId::VolumeDown);
            app.dispatch(CommandId::ToggleMute);
            assert_eq!(app.edit_state().volume, 1.0, "mute must not mute export");
            assert!(!app.command_context().has_unsaved_edits);
            assert_eq!(app.playback_volume(), 0.0);
            let selection =
                towavue_core::TimeRange::new(MediaTime::ZERO, media_time(Duration::from_secs(1)))
                    .expect("range");
            app.push_edit(EditOperation::Timeline(
                towavue_core::TimelineEdit::SetVolume(selection, 3.0),
            ));
            let history = app.edits[&tab].clone();
            app.dispatch(CommandId::VolumeUp);
            app.dispatch(CommandId::ToggleMute);
            assert_eq!(app.edits[&tab], history, "saved gain remains independent");
            // Same-source rejection prevents this request-capture job writing a file.
            for output in [ExportOutput::Media, ExportOutput::AudioOnly] {
                assert!(app.start_export(tab, source.clone(), kind, source.clone(), None, output));
                app.set_playback_volume(3.0);
                app.toggle_playback_mute();
                let export = app.active_export.as_ref().expect("export request");
                assert_eq!(export.request.operations, history.operations());
                assert_eq!(
                    export.options,
                    ExportOptions {
                        output,
                        ..ExportOptions::default()
                    }
                );
                app.active_export.take();
            }
        }
    }
}
