use super::*;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(crate) fn tab_volume_wheel(&mut self, id: TabId, path: &Path, deltas: &[f32]) {
        if self.modal_input_blocked()
            || self.tab_mute_state(id).is_none()
            || !self
                .tabs
                .tabs()
                .iter()
                .any(|tab| tab.id == id && tab.target.current_path() == Some(path))
        {
            return;
        }
        self.seed_playback_volume(id);
        let before = self.playback_volume_for(id);
        let level = deltas
            .iter()
            .copied()
            .filter(|delta| delta.is_finite())
            .fold(before, |level, delta| {
                self.stepped_playback_volume(level, delta)
            });
        if before == level {
            return;
        }
        if self.tabs.active_id() == Some(id) && self.path.as_deref() == Some(path) {
            self.set_playback_volume(level);
            self.tab_volume_huds
                .entry(id)
                .or_default()
                .changed(id, 0, Instant::now());
            return;
        }
        let volume = self.playback_volumes.entry(id).or_default();
        volume.level = level;
        if level > 0.0 {
            volume.unmuted = level;
        }
        let volume = *volume;
        self.remember_playback_volume(volume);
        let gain = self
            .edits
            .get(&id)
            .map(EditHistory::state)
            .unwrap_or_default()
            .volume;
        if let Some(session) = self
            .retained_playback
            .get_mut(&id)
            .filter(|saved| saved.path == path)
            .and_then(|saved| saved.session.as_mut())
        {
            session.set_volume(gain * level);
        }
        self.tab_volume_huds
            .entry(id)
            .or_default()
            .changed(id, 0, Instant::now());
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_tab_title_volume_requires_tab_or_card_wheel_input() {
        let Some(root) = crate::tests::isolated_test_root(
            "playback_volume::tab_wheel::tests::active_tab_title_volume_requires_tab_or_card_wheel_input",
        ) else {
            return;
        };
        for kind in [MediaKind::Audio, MediaKind::Video] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let context = fonts::test_context();
            context.global_style_mut(chrome::style);
            app.ui_context = Some(context.clone());
            let path = root.join(if kind == MediaKind::Audio {
                "audio.wav"
            } else {
                "video.mp4"
            });
            let id = app.tabs.open_new(path.clone(), kind);
            app.tabs.close_gallery(app.tabs.gallery().expect("gallery"));
            app.path = Some(path.clone());
            app.media_kind = Some(kind);
            let percentages = |app: &mut Application<_>| {
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(960.0, 576.0),
                        )),
                        ..Default::default()
                    },
                    |ui| app.draw_top_bar(ui, &mut Vec::new()),
                );
                output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text().ends_with('%') => {
                            Some(text.galley.text().to_owned())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            for _ in 0..3 {
                percentages(&mut app);
            }
            // This action is shared by media-wheel and media-HUD gestures.
            app.handle_ui_action(UiAction::Volume(id, 0.7));
            assert!(
                app.volume_hud
                    .title_opacity(&context, (id, app.media_generation), Instant::now())
                    .is_some()
            );
            assert!(percentages(&mut app).is_empty());
            app.dispatch(CommandId::VolumeUp);
            assert!(percentages(&mut app).is_empty());
            app.dispatch(CommandId::ToggleMute);
            assert!(percentages(&mut app).is_empty());
            app.toggle_tab_mute(id);
            assert!(percentages(&mut app).is_empty());
            assert!(app.tab_volume_huds.is_empty());

            app.tab_volume_wheel(id, &path, &[1.0]);
            assert_eq!(percentages(&mut app), vec!["74%"]);
            let mut expired = volume_hud::Hud::default();
            expired.changed(id, 0, Instant::now() - Duration::from_secs(2));
            app.tab_volume_huds.insert(id, expired);
            app.handle_ui_action(UiAction::Volume(id, 0.6));
            app.toggle_tab_mute(id);
            assert!(
                percentages(&mut app).is_empty(),
                "other controls must not renew the title timer"
            );
            app.tab_volume_wheel(id, &path, &[1.0]);
            assert_eq!(percentages(&mut app), vec!["2%"]);
        }
    }

    #[test]
    fn background_wheel_preserves_activation_edits_and_rejects_stale_targets() {
        let Some(root) = crate::tests::isolated_test_root(
            "playback_volume::tab_wheel::tests::background_wheel_preserves_activation_edits_and_rejects_stale_targets",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        let path = root.join("audio.wav");
        let audio = app.tabs.open_new(path.clone(), MediaKind::Audio);
        app.edits
            .entry(audio)
            .or_default()
            .push(EditOperation::SetVolume(0.4), MediaKind::Audio);
        let image_path = root.join("image.png");
        let image = app.tabs.open_new(image_path.clone(), MediaKind::Image);
        app.path = Some(image_path.clone());
        app.media_kind = Some(MediaKind::Image);
        let tabs = app.tabs.clone();
        let edits = app.edits.clone();
        app.tab_volume_wheel(audio, &path, &[1000.0, -1.0]);
        assert_eq!(app.playback_volume_for(audio), 1.98);
        assert_eq!(app.tabs, tabs);
        assert_eq!(app.edits, edits);
        assert!(app.session.is_none());
        app.tab_volume_wheel(audio, &root.join("stale.wav"), &[-1000.0]);
        app.tab_volume_wheel(image, &image_path, &[10.0]);
        assert_eq!(app.playback_volume_for(audio), 1.98);
        assert!(!app.tab_volume_huds.contains_key(&image));
        app.pending_guard = Some(GuardedAction::Exit);
        app.tab_volume_wheel(audio, &path, &[-1000.0]);
        assert_eq!(app.playback_volume_for(audio), 1.98);
        app.pending_guard = None;
        app.tab_volume_wheel(audio, &path, &[-1000.0]);
        assert_eq!(app.playback_volume_for(audio), 0.0);
        app.toggle_tab_mute(audio);
        assert_eq!(app.playback_volume_for(audio), 1.98);
    }

    #[test]
    fn tab_and_card_wheels_change_only_the_hovered_tab_and_keep_plain_accessibility_names() {
        let Some(root) = crate::tests::isolated_test_root(
            "playback_volume::tab_wheel::tests::tab_and_card_wheels_change_only_the_hovered_tab_and_keep_plain_accessibility_names",
        ) else {
            return;
        };
        for density in [1.0, 1.25, 2.0] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let context = fonts::test_context();
            context.global_style_mut(chrome::style);
            context.set_pixels_per_point(density);
            context.enable_accesskit();
            app.ui_context = Some(context.clone());
            let path = root.join("audio.wav");
            let audio = app.tabs.open_new(path.clone(), MediaKind::Audio);
            app.playback_volumes.insert(audio, Default::default());
            let foreground = app
                .tabs
                .open_new(root.join("foreground.png"), MediaKind::Image);
            app.tabs.close_gallery(app.tabs.gallery().expect("gallery"));
            app.path = Some(root.join("foreground.png"));
            app.media_kind = Some(MediaKind::Image);
            let frame = |app: &mut Application<_>, events, time| {
                let mut actions = Vec::new();
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                };
                wheel_input::prepare_native_input(&context, &mut input);
                let output = context.run_ui(input, |ui| {
                    wheel_input::begin_frame(&context);
                    app.draw_top_bar(ui, &mut actions);
                });
                // Match the host: discarded passes can own consumed input.
                let mut unique = Vec::new();
                for action in actions {
                    if !unique.contains(&action) {
                        unique.push(action);
                    }
                }
                (output, unique)
            };
            for i in 0..3 {
                frame(&mut app, vec![], i as f64 * 0.1);
            }
            let point = crate::tab_drag::tests::label_center(&app, audio);
            let wheel = |modifiers| egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, 1.0),
                modifiers,
                phase: egui::TouchPhase::Move,
            };
            let (_, actions) = frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(point),
                    wheel(egui::Modifiers::NONE),
                ],
                0.4,
            );
            assert!(
                matches!(actions.as_slice(), [UiAction::TabVolume(id, _, steps)] if *id == audio && steps == &[1.0]),
                "density={density}, point={point:?}, actions={}, volumes={:?}, events={:?}, layer={:?}, mute={:?}",
                actions.len(),
                actions
                    .iter()
                    .filter_map(|action| match action {
                        UiAction::TabVolume(id, _, steps) => Some((id, steps)),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                context.input(|input| input.events.clone()),
                context.layer_id_at(point),
                app.tab_mute_state(audio)
            );
            for action in actions {
                app.handle_ui_action(action);
            }
            assert_eq!(app.playback_volume_for(audio), 0.52);
            assert_eq!(app.tabs.active_id(), Some(foreground));
            let mut output = egui::FullOutput::default();
            for i in 5..9 {
                output = frame(
                    &mut app,
                    vec![egui::Event::PointerMoved(point)],
                    i as f64 * 0.1,
                )
                .0;
            }
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == "52%")));
            assert!(
                output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree")
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some("audio.wav"))
            );
            let card = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.text() == "audio.wav" && text.pos.y > point.y + 20.0 =>
                    {
                        Some(text.pos + egui::vec2(3.0, 3.0))
                    }
                    _ => None,
                })
                .expect("card filename");
            frame(&mut app, vec![egui::Event::PointerMoved(card)], 0.9);
            let (_, actions) = frame(&mut app, vec![wheel(egui::Modifiers::NONE)], 1.0);
            assert!(
                matches!(actions.as_slice(), [UiAction::TabVolume(id, _, steps)] if *id == audio && steps == &[1.0])
            );
            for action in actions {
                app.handle_ui_action(action);
            }
            assert_eq!(app.playback_volume_for(audio), 0.54);
            assert_eq!(app.tabs.active_id(), Some(foreground));
            assert!(
                frame(&mut app, vec![wheel(egui::Modifiers::CTRL)], 1.1)
                    .1
                    .is_empty()
            );
            assert!(app.edits.is_empty());
            for index in 0..14 {
                app.tabs
                    .open_new(root.join(format!("overflow-{index}.png")), MediaKind::Image);
            }
            app.tabs.activate(foreground);
            for index in 0..30 {
                frame(&mut app, vec![], 2.0 + index as f64 / 60.0);
            }
            let before = crate::tab_drag::tests::label_center(&app, audio);
            let (_, actions) = frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(before),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: egui::vec2(0.0, -1.0),
                        modifiers: egui::Modifiers::NONE,
                        phase: egui::TouchPhase::Move,
                    },
                ],
                3.0,
            );
            assert!(
                matches!(actions.as_slice(), [UiAction::TabVolume(id, _, steps)] if *id == audio && steps == &[-1.0])
            );
            for index in 0..30 {
                frame(&mut app, vec![], 3.1 + index as f64 / 60.0);
            }
            assert_eq!(
                crate::tab_drag::tests::label_center(&app, audio),
                before,
                "volume wheel must not leave a scrolling tail in an overflowed tab strip"
            );
        }
    }
}
