use super::*;

#[test]
fn entering_previews_direction_without_adjusting_counts() {
    for density in [1.0, 1.25, 2.0] {
        let mut drag = ReadingDrag::new(ReadingSettings::default(), false, density);
        for (delta, expected) in [
            ((2.0, 0.0), None),
            ((-26.0, 0.0), Some(true)),
            ((72.0, 0.0), Some(false)),
            ((0.0, 100.0), None),
            ((0.0, -100.0), Some(false)),
        ] {
            drag.motion((delta.0 * density, delta.1 * density));
            assert_eq!(drag.direction, expected);
            assert_eq!(
                (drag.settings.page_count, drag.settings.first_page_count),
                (2, 2)
            );
            assert_eq!(drag.settings.reversed, expected.unwrap_or(false));
            assert_eq!(drag.before, ReadingSettings::default());
        }
    }
}

#[test]
fn a_single_adjustment_switches_both_axes_without_residual_jumps() {
    for density in [1.0, 1.25, 2.0] {
        let mut drag = ReadingDrag::new(ReadingSettings::default(), true, density);
        let move_by = |drag: &mut ReadingDrag, x, y| drag.motion((x * density, y * density));
        move_by(&mut drag, 0.0, -47.0);
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (3, 3)
        );
        // A small orthogonal wobble cannot select another axis.
        for x in [3.0, -3.0, 3.0, -3.0] {
            move_by(&mut drag, x, 0.0);
            assert_eq!(drag.axis, Some(true));
        }
        move_by(&mut drag, -48.0, 0.0);
        assert_eq!(drag.axis, Some(false));
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (3, 3)
        );
        move_by(&mut drag, -23.0, 0.0);
        assert_eq!(drag.settings.first_page_count, 3);
        move_by(&mut drag, -1.0, 0.0);
        assert_eq!(drag.settings.first_page_count, 2);
        move_by(&mut drag, 0.0, -48.0);
        assert_eq!(
            drag.axis,
            Some(false),
            "recent horizontal motion still dominates"
        );
        move_by(&mut drag, 0.0, -48.0);
        assert_eq!(drag.axis, Some(true));
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (3, 2)
        );
        move_by(&mut drag, 0.0, -24.0);
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (4, 2)
        );
        assert_eq!(drag.before, ReadingSettings::default());
    }
}

#[test]
fn limits_and_partial_first_spreads_survive_reversal_in_the_same_axis() {
    for full in [false, true] {
        let settings = ReadingSettings {
            page_count: 5,
            first_page_count: if full { 5 } else { 2 },
            ..ReadingSettings::default()
        };
        let mut drag = ReadingDrag::new(settings, true, 1.0);
        drag.motion((0.0, 24_000.0));
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (2, 2)
        );
        drag.motion((0.0, -24.0));
        assert_eq!(
            (drag.settings.page_count, drag.settings.first_page_count),
            (3, if full { 3 } else { 2 })
        );
        drag.motion((0.0, -24_000.0));
        assert_eq!(drag.settings.page_count, 10);
        drag.motion((0.0, -23.0));
        drag.motion((0.0, -24.0));
        drag.motion((0.0, 24.0));
        assert_eq!(drag.settings.page_count, 9);
    }
}

#[test]
fn initial_jitter_diagonal_motion_and_switch_sign_changes_do_not_steal_an_axis() {
    let mut drag = ReadingDrag::new(ReadingSettings::default(), true, 1.0);
    drag.motion((1.0, 0.0));
    assert_eq!(drag.axis, None);
    drag.motion((0.0, -24.0));
    assert_eq!(drag.axis, Some(true));
    for _ in 0..10 {
        drag.motion((3.0, -3.0));
        assert_eq!(drag.axis, Some(true));
    }
    for x in [7.0, -7.0, 7.0, -7.0] {
        drag.motion((x, 0.0));
        assert_eq!(drag.axis, Some(true));
    }
    for _ in 0..8 {
        drag.motion((2.0, 0.0));
    }
    assert_eq!(drag.axis, Some(false), "sustained small events also switch");
}

#[test]
fn preview_commit_cancel_and_layout_changes_keep_the_leading_image_and_source_history() {
    use crate::*;
    let Some(root) = tests::isolated_test_root(
        "reading_input::tests::preview_commit_cancel_and_layout_changes_keep_the_leading_image_and_source_history",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for reversed in [false, true] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let paths: Vec<_> = (0..11)
                .rev()
                .map(|i| root.join(format!("{i}.png")))
                .collect();
            let path = paths[5].clone();
            let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
            app.edits.insert(tab, EditHistory::default());
            let history = app.edits.clone();
            app.path = Some(path.clone());
            app.media_kind = Some(MediaKind::Image);
            app.folder_snapshot = Some(FolderSnapshot {
                folder_identity: towavue_core::ShellIdentity::new(vec![]),
                folder_path: root.clone(),
                items: paths
                    .iter()
                    .map(|path| towavue_core::FolderMediaItem {
                        identity: towavue_core::ShellIdentity::new(vec![]),
                        path: path.clone(),
                        kind: MediaKind::Image,
                    })
                    .collect(),
                sort_columns: vec![],
                source: FolderSnapshotSource::LiveExplorerView,
                generation: 1,
                captured_at: std::time::SystemTime::now(),
            });
            for cancel in [true, false] {
                let before = app.reading_settings;
                app.reading_drag = Some(ReadingDrag::new(before, false, density));
                app.move_reading_drag((if reversed { -24.0 } else { 24.0 } * density, 0.0));
                assert!(!app.reading_mode, "entry is a preview until release");
                assert_eq!(app.reading_settings, before);
                assert_eq!(
                    app.status_notice().as_deref(),
                    Some(if reversed {
                        "(\u{2194}) Reading left: release to enable"
                    } else {
                        "(\u{2194}) Reading right: release to enable"
                    })
                );
                assert!(app.finish_reading_drag(cancel));
                assert_eq!(app.reading_mode, !cancel);
                if cancel {
                    assert_eq!(app.reading_settings, before);
                }
            }
            assert_eq!(app.reading_settings.reversed, reversed);
            let before = app.reading_settings;
            for cancel in [true, false] {
                app.reading_drag = Some(ReadingDrag::new(app.reading_settings, true, density));
                for delta in [
                    (0.0, -48.0),
                    (-48.0, 0.0),
                    (-24.0, 0.0),
                    (0.0, -48.0),
                    (0.0, -48.0),
                    (0.0, -24.0),
                ] {
                    app.move_reading_drag((delta.0 * density, delta.1 * density));
                    let range = app.reading_settings.spread(4, paths.len());
                    let mut expected = vec![path.clone()];
                    expected.extend(paths[range].iter().filter(|p| **p != path).cloned());
                    assert_eq!(app.reading_request_paths(), expected);
                    assert_eq!(app.path.as_ref(), Some(&path));
                    assert_eq!(app.tabs.active_id(), Some(tab));
                    assert_eq!(app.edits, history);
                }
                app.finish_reading_drag(cancel);
                if cancel {
                    assert_eq!(app.reading_settings, before);
                } else {
                    assert_eq!(
                        (
                            app.reading_settings.page_count,
                            app.reading_settings.first_page_count
                        ),
                        (5, 3)
                    );
                }
                assert!(app.reading_mode && app.reading_cursor.is_none());
            }
            app.set_reading_layout(false, before);
            app.reading_drag = Some(ReadingDrag::new(before, false, density));
            app.move_reading_drag((0.0, -48.0 * density));
            app.finish_reading_drag(false);
            assert!(!app.reading_mode, "a vertical entry drag has no direction");
            assert_eq!(app.path.as_ref(), Some(&path));
        }
    }
}

#[test]
fn reading_arrows_keep_fixed_shell_spreads_and_ordinary_commands_keep_their_meaning() {
    use crate::*;
    let Some(root) = tests::isolated_test_root(
        "reading_input::tests::reading_arrows_keep_fixed_shell_spreads_and_ordinary_commands_keep_their_meaning",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("application");
    let paths: Vec<_> = [
        "z.png", "c.png", "b.png", "a.png", "g.png", "d.png", "e.png",
    ]
    .iter()
    .map(|name| root.join(name))
    .collect();
    let tab = app.tabs.open_new(paths[0].clone(), MediaKind::Image);
    app.media_kind = Some(MediaKind::Image);
    app.reading_mode = true;
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    let history = app.edits.clone();
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![]),
        folder_path: root.clone(),
        items: paths
            .iter()
            .map(|path| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![]),
                path: path.clone(),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: vec![],
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 1,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    });

    let paths = paths.clone();
    for first in [1, 2] {
        app.reading_settings.first_page_count = first;
        for reversed in [false, true] {
            app.reading_settings.reversed = reversed;
            for index in 0..paths.len() {
                app.path = Some(paths[index].clone());
                app.tabs
                    .active_mut()
                    .expect("active tab")
                    .target
                    .set_current_path(paths[index].clone(), MediaKind::Image);
                for (key, forward) in [
                    ("Left", reversed),
                    ("Right", !reversed),
                    ("Space", true),
                    ("Backspace", false),
                ] {
                    let target = app
                        .reading_settings
                        .adjacent_spread(index, paths.len(), forward);
                    app.process_shortcut(key.parse().expect("valid shortcut"));
                    match target {
                        Some(target) => assert!(
                            matches!(&app.pending_guard, Some(GuardedAction::Navigate(path)) if path == &paths[target]),
                            "{key} at {index}, reversed={reversed}"
                        ),
                        None => {
                            assert!(app.pending_guard.is_none(), "stop at the reading boundary")
                        }
                    }
                    app.resolve_guard(GuardDecision::Cancel);
                    assert_eq!(app.path.as_ref(), Some(&paths[index]));
                    assert_eq!(app.edits, history);
                }
            }
        }
    }

    app.path = Some(paths[2].clone());
    app.reading_settings.reversed = false;
    app.process_shortcut("H".parse().expect("valid shortcut"));
    assert!(app.reading_settings.reversed);
    assert_eq!(app.path.as_ref(), Some(&paths[2]));
    app.shortcuts
        .set(CommandId::NextImage, "N".parse().expect("valid shortcut"));
    app.process_shortcut("N".parse().expect("valid shortcut"));
    assert!(matches!(&app.pending_guard, Some(GuardedAction::Navigate(path)) if path == &paths[4]));
    assert_eq!(app.edits, history);
}

#[test]
fn reversed_seek_mirrors_paint_hover_drag_and_focused_keys_but_not_numeric_indices() {
    use crate::*;
    use egui::accesskit::{Action, ActionData, ActionRequest, TreeId};
    let Some(root) = tests::isolated_test_root(
        "reading_input::tests::reversed_seek_mirrors_paint_hover_drag_and_focused_keys_but_not_numeric_indices",
    ) else {
        return;
    };

    for density in [1.0, 1.25, 2.0] {
        for reading in [false, true] {
            for reversed in [false, true] {
                let mirror = reading && reversed;
                let mut app = Application::new(None, |_| {}).expect("application");
                let paths: Vec<_> = (0..7)
                    .rev()
                    .map(|i| root.join(format!("{i}.png")))
                    .collect();
                app.path = Some(paths[1].clone());
                app.tabs.open_new(paths[1].clone(), MediaKind::Image);
                app.media_kind = Some(MediaKind::Image);
                app.reading_mode = reading;
                app.reading_settings.reversed = reversed;
                let shell_paths = paths.clone();
                app.folder_snapshot = Some(FolderSnapshot {
                    folder_identity: towavue_core::ShellIdentity::new(vec![]),
                    folder_path: root.clone(),
                    items: shell_paths
                        .iter()
                        .map(|path| towavue_core::FolderMediaItem {
                            identity: towavue_core::ShellIdentity::new(vec![]),
                            path: path.clone(),
                            kind: MediaKind::Image,
                        })
                        .collect(),
                    sort_columns: vec![],
                    source: FolderSnapshotSource::LiveExplorerView,
                    generation: 1,
                    captured_at: std::time::SystemTime::UNIX_EPOCH,
                });
                let context = fonts::test_context();
                context.enable_accesskit();
                context.set_pixels_per_point(density);
                context.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
                app.ui_context = Some(context.clone());
                let mut time = 0.0;
                let mut frame = |app: &mut Application<_>, events| {
                    time += 0.1;
                    let mut actions = Vec::new();
                    let output = context.run_ui(
                        egui::RawInput {
                            time: Some(time),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(960.0, 400.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |_| {
                            app.draw_seek_bar(
                                &context,
                                egui::Rect::from_min_size(
                                    egui::pos2(0.0, 376.0),
                                    egui::vec2(960.0, 24.0),
                                ),
                                None,
                                &mut actions,
                            )
                        },
                    );
                    (output, actions)
                };
                for _ in 0..3 {
                    frame(&mut app, vec![]);
                }
                let (output, _) = frame(&mut app, vec![]);
                let fill = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) if rect.fill == chrome::FOREGROUND => {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .expect("progress fill");
                if mirror {
                    assert_eq!(fill.right(), 960.0);
                    assert!((fill.left() - 800.0).abs() < 0.1);
                } else {
                    assert_eq!(fill.left(), 0.0);
                    assert!((fill.right() - 160.0).abs() < 0.1);
                }
                let tree = output
                    .platform_output
                    .accesskit_update
                    .expect("accessibility tree");
                let (id, node) = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("Image position"))
                    .expect("image position slider");
                assert_eq!(node.numeric_value(), Some(2.0));
                let bounds = node.bounds().expect("slider bounds");
                let rect = egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                );
                let point = egui::pos2(egui::lerp(rect.x_range(), 0.2), rect.center().y);
                for _ in 0..12 {
                    frame(&mut app, vec![egui::Event::PointerMoved(point)]);
                }
                let target = if mirror { 5 } else { 1 };
                let (hover, _) = frame(&mut app, vec![]);
                assert!(hover.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.text().contains(&display_name(&paths[target])))),
                    "hover caption follows the mirrored target");
                assert_eq!(app.path.as_ref(), Some(&paths[1]), "hover is read-only");
                let button = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                frame(&mut app, vec![button(point, true)]);
                let (_, actions) = frame(&mut app, vec![button(point, false)]);
                let target = if mirror { 5 } else { 1 };
                if target != 1 {
                    assert!(
                        matches!(actions.as_slice(), [UiAction::OpenMedia(path, false)] if path == &paths[target])
                    );
                } else {
                    assert!(actions.is_empty());
                }
                let start = rect.center();
                frame(
                    &mut app,
                    vec![egui::Event::PointerMoved(start), button(start, true)],
                );
                let (_, actions) = frame(&mut app, vec![egui::Event::PointerMoved(point)]);
                if target != 1 {
                    assert!(
                        matches!(actions.as_slice(), [UiAction::ScrubImage(path, _, _)] if path == &paths[target])
                    );
                }
                frame(
                    &mut app,
                    vec![button(point, false), egui::Event::PointerGone],
                );
                let access = |action, data| {
                    egui::Event::AccessKitActionRequest(ActionRequest {
                        action,
                        target_tree: TreeId::ROOT,
                        target_node: *id,
                        data,
                    })
                };
                frame(&mut app, vec![access(Action::Focus, None)]);
                for (key, target) in [
                    (egui::Key::ArrowLeft, if mirror { 2 } else { 0 }),
                    (egui::Key::ArrowRight, if mirror { 0 } else { 2 }),
                ] {
                    let (_, actions) = frame(
                        &mut app,
                        vec![egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                    );
                    assert!(
                        matches!(actions.as_slice(), [UiAction::OpenMedia(path, false)] if path == &paths[target])
                    );
                }
                for (action, data, target) in [
                    (Action::Increment, None, 2),
                    (Action::SetValue, Some(ActionData::NumericValue(7.0)), 6),
                ] {
                    let (_, actions) = frame(&mut app, vec![access(action, data)]);
                    assert!(
                        matches!(actions.as_slice(), [UiAction::OpenMedia(path, false)] if path == &paths[target])
                    );
                }
            }
        }
    }
}
