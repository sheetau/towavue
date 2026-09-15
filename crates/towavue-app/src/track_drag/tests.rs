use super::*;

fn drag(media: u64, density: f64) -> Drag {
    Drag {
        media,
        density,
        distance: 0.0,
        cursor: None,
    }
}

#[test]
fn track_drag_threshold_reverses_and_release_uses_shared_navigation_guards() {
    let Some(root) = crate::tests::isolated_test_root(
        "track_drag::tests::track_drag_threshold_reverses_and_release_uses_shared_navigation_guards",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let paths = [root.join("a.mkv"), root.join("b.mkv"), root.join("c.mkv")];
    let tab = app.tabs.open_new(paths[1].clone(), MediaKind::Video);
    app.path = Some(paths[1].clone());
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![0]),
        folder_path: root,
        items: paths
            .iter()
            .map(|path| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(
                    path.to_string_lossy().as_bytes().to_vec(),
                ),
                path: path.clone(),
                kind: MediaKind::Video,
            })
            .collect(),
        sort_columns: vec![],
        source: FolderSnapshotSource::PersistedShellView,
        generation: 1,
        captured_at: std::time::SystemTime::now(),
    });
    for kind in [MediaKind::Audio, MediaKind::Video] {
        app.media_kind = Some(kind);
        app.state = PlaybackState::Paused;
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::SetRate(1.25), kind);
        let history = app.edits[&tab].clone();
        for density in [1.0, 1.25, 1.5, 2.0] {
            for (distance, target) in [
                (-24.0, Some(0)),
                (-23.0, None),
                (0.0, None),
                (23.0, None),
                (24.0, Some(2)),
            ] {
                app.track_drag = Some(drag(app.media_generation, density));
                app.move_track_drag((80.0 * density, 0.0));
                assert!(
                    app.track_drag
                        .as_ref()
                        .expect("drag")
                        .direction()
                        .expect("next")
                );
                app.move_track_drag(((distance - 80.0) * density, 9999.0));
                assert!(app.track_window_event(&WindowEvent::MouseInput {
                    device_id: winit::event::DeviceId::dummy(),
                    state: ElementState::Released,
                    button: winit::event::MouseButton::Left,
                }));
                assert!(app.track_drag.is_none());
                match target {
                    Some(index) => {
                        assert!(
                            matches!(&app.pending_guard, Some(GuardedAction::Navigate(path)) if path == &paths[index])
                        );
                        app.resolve_guard(GuardDecision::Cancel);
                    }
                    None => assert!(app.pending_guard.is_none()),
                }
                assert_eq!(app.edits[&tab], history);
                assert_eq!(app.path.as_ref(), Some(&paths[1]));
                assert_eq!(app.state, PlaybackState::Paused);
                assert!(
                    !app.finish_track_drag(false),
                    "a release cannot commit twice"
                );
            }
        }
        for cancel in 0..6 {
            app.track_drag = Some(drag(app.media_generation, 1.0));
            app.move_track_drag((40.0, 0.0));
            match cancel {
                0 => {
                    app.track_window_event(&WindowEvent::Focused(false));
                }
                1 => {
                    app.track_window_event(&WindowEvent::Resized(winit::dpi::PhysicalSize::new(
                        400, 300,
                    )));
                }
                2 => {
                    app.media_generation += 1;
                }
                3 => {
                    app.export_error = Some("modal".into());
                }
                4 => {
                    app.palette_open = true;
                }
                _ => {
                    app.cancel_view_drag();
                }
            }
            app.finish_track_drag(false);
            assert!(app.pending_guard.is_none());
            app.export_error = None;
            app.palette_open = false;
            assert_eq!(app.edits[&tab], history);
        }
        app.ui_context = Some(fonts::test_context());
        for (delta, target) in [(-40.0, &paths[0]), (40.0, &paths[2])] {
            app.begin_track_drag(
                app.media_generation,
                egui::pos2(20.0, 20.0),
                egui::vec2(delta, 0.0),
                true,
            );
            assert!(
                app.track_drag.is_none(),
                "already released input cannot retain a lock"
            );
            assert!(
                matches!(&app.pending_guard, Some(GuardedAction::Navigate(path)) if path == target)
            );
            app.resolve_guard(GuardDecision::Cancel);
            assert_eq!(app.edits[&tab], history);
        }
    }
}

#[test]
fn play_button_horizontal_drag_is_not_a_click_and_vertical_motion_is_not_navigation() {
    let Some(_root) = crate::tests::isolated_test_root(
        "track_drag::tests::play_button_horizontal_drag_is_not_a_click_and_vertical_motion_is_not_navigation",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    for kind in [MediaKind::Audio, MediaKind::Video] {
        app.media_kind = Some(kind);
        app.state = PlaybackState::Paused;
        for density in [1.0, 1.5, 2.0] {
            for horizontal in [false, true] {
                let context = fonts::test_context();
                context.set_pixels_per_point(density);
                app.ui_context = Some(context.clone());
                let origin = egui::pos2(20.0, 284.0);
                let target = origin
                    + if horizontal {
                        egui::vec2(35.0, 0.0)
                    } else {
                        egui::vec2(0.0, -35.0)
                    };
                let button = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                };
                for (index, events) in [
                    vec![],
                    vec![egui::Event::PointerMoved(origin)],
                    vec![button(origin, true)],
                    vec![egui::Event::PointerMoved(target)],
                    vec![button(target, false)],
                ]
                .into_iter()
                .enumerate()
                {
                    let mut actions = Vec::new();
                    let _ = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(480.0, 300.0),
                            )),
                            time: Some(index as f64 * 0.1),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            app.draw_status_bar(ui, &mut actions, &mut Vec::new());
                        },
                    );
                    assert!(!actions.contains(&UiAction::Command(CommandId::TogglePause)));
                    assert_eq!(
                        actions
                            .iter()
                            .any(|action| matches!(action, UiAction::BeginTrackDrag(..))),
                        index == 3 && horizontal,
                        "kind={kind:?} density={density} horizontal={horizontal} frame={index}"
                    );
                }
                // Entire gestures may be queued before the next draw. Only an
                // interruption before release cancels that completed gesture.
                for mode in 0..5 {
                    let mut events = vec![button(origin, true), egui::Event::PointerMoved(target)];
                    if mode == 1 {
                        events.push(egui::Event::WindowFocused(false));
                    }
                    if mode == 3 {
                        events.push(egui::Event::PointerGone);
                    }
                    events.push(button(if mode == 4 { origin } else { target }, false));
                    if mode == 2 {
                        events.push(egui::Event::WindowFocused(false));
                    }
                    let mut actions = Vec::new();
                    let _ = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(480.0, 300.0),
                            )),
                            time: Some(2.0 + f64::from(mode)),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            app.draw_status_bar(ui, &mut actions, &mut Vec::new());
                        },
                    );
                    assert_eq!(
                        actions.iter().any(|action| matches!(
                            action,
                            UiAction::BeginTrackDrag(_, _, _, true)
                        )),
                        horizontal && matches!(mode, 0 | 2),
                        "batched {kind:?} {density} mode {mode}"
                    );
                    assert!(!actions.contains(&UiAction::Command(CommandId::TogglePause)));
                }
            }
        }
    }
}
