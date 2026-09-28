use super::*;

fn media_app(
    root: &Path,
    kind: MediaKind,
) -> Application<impl Fn(AppEvent) + Send + Sync + 'static> {
    let mut app = Application::new(None, |_| {}).expect("app");
    let path = root.join(if kind == MediaKind::Audio {
        "owned.wav"
    } else {
        "owned.mp4"
    });
    app.tabs.open_new(path.clone(), kind);
    app.path = Some(path);
    app.media_kind = Some(kind);
    app.media_duration = Some(Duration::from_secs(10));
    app.state = PlaybackState::Paused;
    app.timeline_open = true;
    app
}

fn range() -> towavue_core::TimeRange {
    towavue_core::TimeRange::new(
        media_time(Duration::from_secs(2)),
        media_time(Duration::from_secs(6)),
    )
    .expect("range")
}

fn access(target: egui::accesskit::NodeId, action: egui::accesskit::Action) -> egui::Event {
    egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: target,
        data: None,
    })
}

#[test]
fn timeline_context_covers_children_without_retargeting_selection_or_seeking() {
    timeline_context_covers_children_without_retargeting_selection_or_seeking_in_language(
        crate::localization::Language::English,
    );
}

#[test]
fn japanese_timeline_context_covers_children_without_retargeting_selection_or_seeking() {
    timeline_context_covers_children_without_retargeting_selection_or_seeking_in_language(
        crate::localization::Language::Japanese,
    );
}

fn timeline_context_covers_children_without_retargeting_selection_or_seeking_in_language(
    language: crate::localization::Language,
) {
    let test = match language {
        crate::localization::Language::English => {
            "timeline_menu::tests::timeline_context_covers_children_without_retargeting_selection_or_seeking"
        }
        crate::localization::Language::Japanese => {
            "timeline_menu::tests::japanese_timeline_context_covers_children_without_retargeting_selection_or_seeking"
        }
    };
    let Some(root) = crate::tests::isolated_test_root(test) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        let mut app = media_app(&root, kind);
        let context = fonts::test_context();
        if language == crate::localization::Language::Japanese {
            crate::localization::test_ui::configure_japanese(&context, 1.0);
        }
        context.enable_accesskit();
        context.global_style_mut(chrome::style);
        app.ui_context = Some(context.clone());
        let frame = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(660.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_timeline(ui, &mut actions),
            );
            (
                actions,
                output.platform_output.accesskit_update.expect("tree"),
            )
        };
        for scale in [1.0, 1.25, 2.0] {
            context.set_pixels_per_point(scale);
            for selection in [None, Some(range())] {
                app.time_selection = selection;
                for _ in 0..3 {
                    frame(&mut app, vec![]);
                }
                // Exercise both selection interiors and exteriors, then each overlapping
                // numeric child by its actual accessible bounds at this density.
                let (_, tree) = frame(&mut app, vec![]);
                let mut points = vec![egui::pos2(260.0, 350.0), egui::pos2(620.0, 350.0)];
                for label in [
                    Text::SelectionStartSeconds.in_language(language),
                    Text::SelectionEndSeconds.in_language(language),
                    Text::RelativeVolumeDb.in_language(language),
                    Text::SelectedDurationSeconds.in_language(language),
                ] {
                    let bounds = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| node.label() == Some(label))
                        .expect("timeline child")
                        .1
                        .bounds()
                        .expect("bounds");
                    points.push(egui::pos2(
                        ((bounds.x0 + bounds.x1) * 0.5) as f32,
                        ((bounds.y0 + bounds.y1) * 0.5) as f32,
                    ));
                }
                for point in points {
                    for pressed in [true, false] {
                        let (actions, _) = frame(
                            &mut app,
                            vec![
                                egui::Event::PointerMoved(point),
                                egui::Event::PointerButton {
                                    pos: point,
                                    button: egui::PointerButton::Secondary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                        );
                        assert!(
                            actions.is_empty(),
                            "right click must not select, seek or edit"
                        );
                    }
                    let (_, tree) = frame(&mut app, vec![]);
                    assert!(
                        egui::Popup::is_any_open(&context),
                        "point {point:?}, scale {scale}, actual {}, selection {selection:?}, nodes {:?}",
                        context.pixels_per_point(),
                        tree.nodes
                            .iter()
                            .filter_map(|(_, node)| node
                                .label()
                                .map(|label| (label, node.bounds())))
                            .collect::<Vec<_>>()
                    );
                    for label in [
                        Text::CommandSelectAll.in_language(language),
                        Text::CommandClearSelection.in_language(language),
                        Text::TimelineDelete.in_language(language),
                        Text::TimelineCrop.in_language(language),
                        Text::TimelineSilence.in_language(language),
                        Text::TimelinePlay.in_language(language),
                    ] {
                        let (_, node) = tree
                            .nodes
                            .iter()
                            .find(|(_, node)| {
                                node.label().is_some_and(|text| text.starts_with(label))
                            })
                            .unwrap_or_else(|| {
                                panic!(
                                    "missing {label} at {point:?}, scale {scale}: {:?}",
                                    tree.nodes
                                        .iter()
                                        .filter_map(|(_, node)| node.label())
                                        .collect::<Vec<_>>()
                                )
                            });
                        assert_eq!(
                            node.is_disabled(),
                            label != Text::CommandSelectAll.in_language(language)
                                && label != Text::TimelineSilence.in_language(language)
                                && selection.is_none(),
                            "{label}"
                        );
                    }
                    assert_eq!(app.time_selection, selection);
                    egui::Popup::close_all(&context);
                    frame(&mut app, vec![]);
                }
            }
        }
        // Accessible invocation uses the same stamped action, and changing selection
        // while a menu is open retires the popup instead of changing its target.
        let point = egui::pos2(260.0, 350.0);
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let (_, tree) = frame(&mut app, vec![]);
        let target = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.label().is_some_and(|text| {
                    text.starts_with(Text::TimelineSilence.in_language(language))
                })
            })
            .expect("silence")
            .0;
        let actions = frame(
            &mut app,
            vec![access(target, egui::accesskit::Action::Click)],
        )
        .0;
        assert!(
            actions
                == [UiAction::TimelineMenu(Intent {
                    owner: app.timeline_menu_owner().expect("owner"),
                    action: Action::Silence
                })]
        );
        assert!(!egui::Popup::is_any_open(&context));
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(egui::Popup::is_any_open(&context));
        let (_, tree) = frame(&mut app, vec![]);
        let bounds = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.label().is_some_and(|label| {
                    label.starts_with(Text::TimelineSilence.in_language(language))
                })
            })
            .expect("silence item")
            .1
            .bounds()
            .expect("item bounds");
        let inside = egui::pos2(
            ((bounds.x0 + bounds.x1) * 0.5) as f32,
            ((bounds.y0 + bounds.y1) * 0.5) as f32,
        );
        for pressed in [true, false] {
            let actions = frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(inside),
                    egui::Event::PointerButton {
                        pos: inside,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            )
            .0;
            if pressed {
                assert!(actions.is_empty());
                assert!(
                    egui::Popup::is_any_open(&context),
                    "inside press keeps the item available"
                );
            } else {
                assert!(
                    actions
                        == [UiAction::TimelineMenu(Intent {
                            owner: app.timeline_menu_owner().expect("owner"),
                            action: Action::Silence
                        })]
                );
                assert!(!egui::Popup::is_any_open(&context));
            }
        }
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(egui::Popup::is_any_open(&context));
        app.time_selection = None;
        assert!(frame(&mut app, vec![]).0.is_empty());
        assert!(!egui::Popup::is_any_open(&context));
        let (_, tree) = frame(&mut app, vec![]);
        let timeline = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.label() == Some(Text::PlaybackPositionSeconds.in_language(language))
            })
            .expect("timeline access node")
            .0;
        frame(
            &mut app,
            vec![access(timeline, egui::accesskit::Action::Focus)],
        );
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::F10,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            }],
        );
        assert!(egui::Popup::is_any_open(&context));
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!egui::Popup::is_any_open(&context));
        assert_eq!(frame(&mut app, vec![]).1.focus, timeline);
    }
}

#[test]
fn timeline_context_edits_current_range_and_undo_restores_history() {
    let Some(root) = crate::tests::isolated_test_root(
        "timeline_menu::tests::timeline_context_edits_current_range_and_undo_restores_history",
    ) else {
        return;
    };
    let mut app = media_app(&root, MediaKind::Audio);
    for (action, edit) in [
        (
            Action::Silence,
            towavue_core::TimelineEdit::ScaleVolume(range(), 0.0),
        ),
        (Action::Crop, towavue_core::TimelineEdit::Keep(range())),
        (Action::Delete, towavue_core::TimelineEdit::Delete(range())),
    ] {
        app.time_selection = Some(range());
        let tab = app.tabs.active_id().expect("tab");
        let volume = app.playback_volume();
        app.handle_timeline_menu(Intent {
            owner: app.timeline_menu_owner().expect("owner"),
            action,
        });
        assert_eq!(
            app.edits[&tab].operations(),
            &[EditOperation::Timeline(edit)]
        );
        assert_eq!(
            app.playback_volume(),
            volume,
            "range edit leaves listening volume alone"
        );
        app.dispatch(CommandId::Undo);
        assert!(app.edits[&tab].operations().is_empty());
    }
    app.handle_timeline_menu(Intent {
        owner: app.timeline_menu_owner().expect("owner"),
        action: Action::SelectAll,
    });
    assert_eq!(
        app.time_selection.expect("all").duration(),
        media_time(Duration::from_secs(10))
    );
    app.handle_timeline_menu(Intent {
        owner: app.timeline_menu_owner().expect("owner"),
        action: Action::DeselectAll,
    });
    assert!(app.time_selection.is_none());
}

#[test]
fn timeline_context_rejects_changed_owners_selection_and_blocked_views() {
    let Some(root) = crate::tests::isolated_test_root(
        "timeline_menu::tests::timeline_context_rejects_changed_owners_selection_and_blocked_views",
    ) else {
        return;
    };
    let mut app = media_app(&root, MediaKind::Audio);
    app.time_selection = Some(range());
    let owner = app.timeline_menu_owner().expect("owner");
    let other = app.tabs.open_new(root.join("other.wav"), MediaKind::Audio);
    app.tabs.activate(owner.tab);
    for stale in [
        Owner {
            tab: other,
            ..owner
        },
        Owner {
            generation: owner.generation.next(),
            ..owner
        },
        Owner {
            instance: owner.instance + 1,
            ..owner
        },
        Owner {
            selection: None,
            ..owner
        },
    ] {
        app.handle_timeline_menu(Intent {
            owner: stale,
            action: Action::Silence,
        });
        assert!(app.edits.is_empty());
    }
    for gate in (0..7).filter(|&gate| gate != 3) {
        app.timeline_open = gate != 0;
        app.fullscreen = gate == 1;
        app.palette_open = gate == 2;
        app.filmstrip_open = gate == 4;
        app.about_open = gate == 5;
        app.state = if gate == 6 {
            PlaybackState::Loading
        } else {
            PlaybackState::Paused
        };
        app.handle_timeline_menu(Intent {
            owner,
            action: Action::Silence,
        });
        assert!(app.edits.is_empty(), "blocked view {gate}");
    }
}

#[test]
fn timeline_menu_outside_press_hands_off_selection_and_secondary_retargeting() {
    let Some(root) = crate::tests::isolated_test_root(
        "timeline_menu::tests::timeline_menu_outside_press_hands_off_selection_and_secondary_retargeting",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        for density in [1.0, 1.25, 2.0] {
            let mut app = media_app(&root, kind);
            let context = fonts::test_context();
            context.global_style_mut(chrome::style);
            context.set_pixels_per_point(density);
            app.ui_context = Some(context.clone());
            let frame = |app: &mut Application<_>, events| {
                let mut actions = Vec::new();
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(660.0, 400.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        overlay_input::dismiss_menu_on_outside_press(&context);
                        app.draw_timeline(ui, &mut actions);
                    },
                );
                actions
            };
            let button = |pos, button, pressed| {
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            };
            for _ in 0..3 {
                frame(&mut app, vec![]);
            }
            let source = egui::pos2(100.0, 350.0);
            let target = egui::pos2(560.0, 350.0);
            for pressed in [true, false] {
                assert!(
                    frame(
                        &mut app,
                        button(source, egui::PointerButton::Secondary, pressed)
                    )
                    .is_empty()
                );
            }
            frame(&mut app, vec![]);
            assert!(egui::Popup::is_any_open(&context));
            let actions = frame(&mut app, button(target, egui::PointerButton::Primary, true));
            assert!(
                !egui::Popup::is_any_open(&context),
                "press inside the context owner but outside its menu dismisses immediately"
            );
            assert!(
                timeline_input::is_active(&context),
                "the same press owns the timeline gesture"
            );
            assert!(
                actions
                    .iter()
                    .any(|action| matches!(action, UiAction::Seek(_)))
            );
            let end = target - egui::vec2(100.0, 0.0);
            frame(&mut app, vec![egui::Event::PointerMoved(end)]);
            let actions = frame(&mut app, button(end, egui::PointerButton::Primary, false));
            assert!(
                actions
                    .iter()
                    .any(|action| matches!(action, UiAction::TimeSelection(_, _, Some(_)))),
                "one uninterrupted drag creates a selection"
            );
            for pressed in [true, false] {
                frame(
                    &mut app,
                    button(source, egui::PointerButton::Secondary, pressed),
                );
            }
            frame(&mut app, vec![]);
            assert!(egui::Popup::is_any_open(&context));
            let mut events = button(target, egui::PointerButton::Primary, true);
            events.push(egui::Event::PointerMoved(end));
            events.extend(button(end, egui::PointerButton::Primary, false));
            let actions = frame(&mut app, events);
            assert!(!egui::Popup::is_any_open(&context));
            assert_eq!(
                actions
                    .iter()
                    .filter(|action| matches!(action, UiAction::TimeSelection(_, _, Some(_))))
                    .count(),
                1,
                "batched outside press/move/release keeps one complete selection gesture"
            );
            for pressed in [true, false] {
                frame(
                    &mut app,
                    button(source, egui::PointerButton::Secondary, pressed),
                );
            }
            frame(&mut app, vec![]);
            assert!(egui::Popup::is_any_open(&context));
            assert!(
                frame(
                    &mut app,
                    button(target, egui::PointerButton::Secondary, true)
                )
                .is_empty()
            );
            assert!(
                egui::Popup::is_any_open(&context),
                "secondary press opens the replacement menu immediately"
            );
            let layers = context.memory(|memory| memory.layer_ids().collect::<Vec<_>>());
            let popup = layers
                .into_iter()
                .find(|layer| egui::Popup::is_id_open(&context, layer.id));
            let popup = popup.expect("replacement menu");
            let rect = context
                .memory(|memory| memory.area_rect(popup.id))
                .expect("menu bounds");
            assert!(
                rect.left() > 300.0,
                "menu moves to the new pointer: {rect:?}"
            );
            frame(
                &mut app,
                button(target, egui::PointerButton::Secondary, false),
            );
            egui::Popup::close_all(&context);
        }
    }
}

#[test]
fn silence_without_selection_edits_the_whole_timeline_and_undo_restores_gain() {
    let Some(root) = crate::tests::isolated_test_root(
        "timeline_menu::tests::silence_without_selection_edits_the_whole_timeline_and_undo_restores_gain",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        let mut app = media_app(&root, kind);
        let tab = app.tabs.active_id().expect("tab");
        let duration = media_time(Duration::from_secs(10));
        app.push_edit(EditOperation::Timeline(
            towavue_core::TimelineEdit::ScaleVolume(range(), 0.5),
        ));
        let before = app.edits[&tab].clone();
        let volume = app.playback_volume();
        app.time_selection = None;
        assert!(app.timeline_menu_enabled(Action::Silence));
        app.handle_timeline_menu(Intent {
            owner: app.timeline_menu_owner().expect("owner"),
            action: Action::Silence,
        });
        let history = &app.edits[&tab];
        assert_eq!(
            history.operations().last(),
            Some(&EditOperation::Timeline(
                towavue_core::TimelineEdit::ScaleVolume(
                    towavue_core::TimeRange::new(MediaTime::ZERO, duration).expect("whole range"),
                    0.0
                )
            ))
        );
        assert!(
            history
                .timeline(duration)
                .expect("plan")
                .spans()
                .iter()
                .all(|span| span.volume() == 0.0)
        );
        assert_eq!(app.playback_volume(), volume);
        assert!(app.time_selection.is_none());
        app.dispatch(CommandId::Undo);
        assert_eq!(app.edits[&tab].operations(), before.operations());
        app.dispatch(CommandId::Redo);
        assert!(
            app.edits[&tab]
                .timeline(duration)
                .expect("redo")
                .spans()
                .iter()
                .all(|span| span.volume() == 0.0)
        );
        app.media_duration = Some(Duration::ZERO);
        assert!(!app.timeline_menu_enabled(Action::Silence));
    }
}
