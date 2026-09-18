use crate::audio_export::tests::frame;
use crate::video_rotation::tests::node;
use crate::*;

type App = Application<Box<dyn Fn(AppEvent) + Send + Sync>>;

fn focus(target: egui::accesskit::NodeId) -> egui::Event {
    egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::Focus,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: target,
        data: None,
    })
}

#[test]
fn middle_pointer_focus_respects_disabled_clipped_and_covered_labels() {
    for blocked in 0..4 {
        let context = fonts::test_context();
        let mut tabs = TabSet::default();
        let tab = tabs.open_new("active.png".into(), MediaKind::Image);
        let numeric = egui::Id::new("late-numeric-control");
        let origin = egui::pos2(30.0, 40.0);
        let end = egui::pos2(200.0, 40.0);
        let draw = |events| {
            let _ = context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    super::begin(&context, Some(tab), true);
                    if blocked == 3 {
                        egui::Area::new("label-cover".into())
                            .order(egui::Order::Foreground)
                            .fixed_pos(egui::pos2(20.0, 20.0))
                            .show(&context, |ui| {
                                ui.allocate_exact_size(
                                    egui::vec2(80.0, 40.0),
                                    egui::Sense::click(),
                                );
                            });
                    }
                    ui.scope(|ui| {
                        if blocked == 1 {
                            ui.disable();
                        }
                        if blocked == 2 {
                            ui.set_clip_rect(egui::Rect::from_min_max(
                                egui::pos2(50.0, 20.0),
                                egui::pos2(100.0, 60.0),
                            ));
                        }
                        let response = ui.interact(
                            egui::Rect::from_min_max(
                                egui::pos2(20.0, 20.0),
                                egui::pos2(100.0, 60.0),
                            ),
                            egui::Id::new("tab-label"),
                            egui::Sense::click_and_drag(),
                        );
                        super::release_pointer_button_focus(&response, egui::PointerButton::Middle);
                    });
                    let response = ui.interact(
                        egui::Rect::from_min_size(egui::pos2(20.0, 150.0), egui::vec2(80.0, 20.0)),
                        numeric,
                        egui::Sense::focusable_noninteractive(),
                    );
                    super::observe(&response, "numeric-role");
                    super::finish(&context, false, true);
                },
            );
        };
        for _ in 0..3 {
            draw(vec![]);
        }
        context.memory_mut(|memory| memory.request_focus(numeric));
        draw(vec![egui::Event::PointerMoved(origin)]);
        assert!(context.memory(|memory| memory.has_focus(numeric)));
        draw(vec![
            egui::Event::PointerButton {
                pos: origin,
                pressed: true,
                button: egui::PointerButton::Middle,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerMoved(end),
            egui::Event::PointerButton {
                pos: end,
                pressed: false,
                button: egui::PointerButton::Middle,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        assert_eq!(
            context.memory(|memory| memory.has_focus(numeric)),
            blocked != 0,
            "blocked={blocked}"
        );
        assert_eq!(super::take(&context, tab).is_some(), blocked != 0);
    }
}

#[test]
fn pointer_panel_resize_releases_numeric_focus_on_press() {
    for density in [1.0, 1.25, 2.0] {
        let context = fonts::test_context();
        context.enable_accesskit();
        let mut tabs = TabSet::default();
        let active = tabs.open_new("active.wav".into(), MediaKind::Audio);
        let other = tabs.open_new("other.wav".into(), MediaKind::Audio);
        let panel = egui::Id::new("focus-resize-panel");
        let value_id = panel.with("value");
        let draw = |tab, events| {
            let mut raw = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 300.0),
                )),
                ..Default::default()
            };
            raw.viewports
                .get_mut(&egui::ViewportId::ROOT)
                .expect("viewport")
                .native_pixels_per_point = Some(density);
            let mut value = None;
            let _ = context.run_ui(raw, |ui| {
                super::begin(&context, Some(tab), true);
                let resizable = timeline_edit::panel_resize_enabled(ui, panel);
                egui::Panel::bottom(panel)
                    .default_size(96.0)
                    .size_range(64.0..=240.0)
                    .resizable(resizable)
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        let response = ui.interact(
                            ui.available_rect_before_wrap(),
                            value_id,
                            egui::Sense::focusable_noninteractive(),
                        );
                        value = seekbar::value_input(
                            &response,
                            "Playback position (seconds)",
                            25.0,
                            0.0..=100.0,
                            5.0,
                            true,
                        );
                    });
                super::finish(&context, false, true);
            });
            assert_eq!(value, None, "resize must not change the numeric value");
            egui::containers::panel::PanelState::load(&context, panel)
                .expect("panel")
                .outer_rect
        };
        draw(active, vec![]);
        draw(active, vec![]);
        let before = draw(active, vec![focus(value_id.accesskit_id())]);
        assert!(context.memory(|memory| memory.has_focus(value_id)));
        let start = before.center_top();
        let end = start - egui::vec2(0.0, 60.0);
        let pointer = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        draw(
            active,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        assert_eq!(
            context.memory(|memory| memory.focused()),
            None,
            "panel resize must release numeric focus on press at {density}x"
        );
        let right = egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        draw(active, vec![right]);
        assert!(context.input(|input| input.key_pressed(egui::Key::ArrowRight)));
        draw(
            active,
            vec![egui::Event::PointerMoved(start - egui::vec2(0.0, 10.0))],
        );
        draw(active, vec![egui::Event::PointerMoved(end)]);
        draw(active, vec![pointer(end, false)]);
        let after = draw(active, vec![]);
        assert!(
            (after.height() - before.height() - 60.0).abs() <= 1.0,
            "resize must commit at {density}x: {before:?} -> {after:?}"
        );
        draw(other, vec![]);
        draw(active, vec![]);
        assert_eq!(
            context.memory(|memory| memory.focused()),
            None,
            "resize must not restore the old numeric role on tab return"
        );
        draw(active, vec![focus(value_id.accesskit_id())]);
        assert!(
            context.memory(|memory| memory.has_focus(value_id)),
            "explicit accessibility focus remains available"
        );
    }
}

fn setup() -> (App, std::sync::mpsc::Receiver<AppEvent>) {
    let (sent, events) = std::sync::mpsc::channel();
    let callback: Box<dyn Fn(AppEvent) + Send + Sync> = Box::new(move |event| {
        let _ = sent.send(event);
    });
    let mut app = Application::new(None, callback).expect("app");
    let context = fonts::test_context();
    context.enable_accesskit();
    context.global_style_mut(chrome::style);
    app.ui_context = Some(context);
    (app, events)
}

fn wait_image(app: &mut App, events: &std::sync::mpsc::Receiver<AppEvent>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.image_loading {
        app.handle_app_event(
            events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("image event"),
        );
    }
    assert!(app.image.is_some(), "{:?}", app.image_error);
}

fn tree(app: &mut App, events: Vec<egui::Event>) -> egui::accesskit::TreeUpdate {
    let output = frame(app, egui::vec2(640.0, 480.0), events);
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.stroke.color == Color32::RED)), "tab focus transitions must not paint red diagnostic rectangles");
    output.platform_output.accesskit_update.expect("tree")
}

fn settle(app: &mut App) -> egui::accesskit::TreeUpdate {
    tree(app, vec![]);
    tree(app, vec![]);
    tree(app, vec![])
}

#[test]
fn tab_focus_image_controls_restore_by_role_without_reloading_or_editing() {
    let Some(root) = tests::isolated_test_root(
        "tab_focus::tests::tab_focus_image_controls_restore_by_role_without_reloading_or_editing",
    ) else {
        return;
    };
    let (mut app, events) = setup();
    let mut tabs = Vec::new();
    let mut bitmap = vec![0_u8; 62];
    bitmap[..2].copy_from_slice(b"BM");
    bitmap[2..6].copy_from_slice(&62_u32.to_le_bytes());
    bitmap[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bitmap[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bitmap[18..22].copy_from_slice(&2_u32.to_le_bytes());
    bitmap[22..26].copy_from_slice(&1_u32.to_le_bytes());
    bitmap[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bitmap[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bitmap[34..38].copy_from_slice(&8_u32.to_le_bytes());
    bitmap[54..].copy_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]);
    for name in ["first.bmp", "second.bmp"] {
        let path = root.join(name);
        std::fs::write(&path, &bitmap).expect("fixture");
        app.open_external(path, true);
        wait_image(&mut app, &events);
        tabs.push(app.tabs.active().expect("tab").id);
        app.image_view.selection = Some(UnitRect {
            min: UnitPoint { x: 0.0, y: 0.0 },
            max: UnitPoint { x: 0.5, y: 1.0 },
        });
        let target = node(
            &settle(&mut app),
            if name == "first.bmp" {
                "Selection right (pixels)"
            } else {
                "Selection bottom (pixels)"
            },
        );
        tree(&mut app, vec![focus(target)]);
        settle(&mut app);
    }
    let histories = app.edits.clone();
    std::fs::remove_file(root.join("first.bmp"))
        .expect("remove owned source to prove retained return");
    std::fs::remove_file(root.join("second.bmp"))
        .expect("remove owned source to prove retained return");
    for _ in 0..3 {
        for (tab, label) in [
            (tabs[0], "Selection right (pixels)"),
            (tabs[1], "Selection bottom (pixels)"),
        ] {
            app.activate_tab(tab);
            assert!(!app.image_loading);
            let returned = settle(&mut app);
            assert_eq!(returned.focus, node(&returned, label), "{label}");
            assert_eq!(app.edits, histories);
        }
    }
    let context = app.ui_context.clone().expect("context");
    for density in [1.0, 1.25, 2.0] {
        context.set_pixels_per_point(density);
        for (tab, name) in [(tabs[1], "second.bmp"), (tabs[0], "first.bmp")] {
            let before = settle(&mut app);
            let target = node(&before, name);
            let focused = if name == "second.bmp" {
                tree(&mut app, vec![focus(target)]);
                let focused = settle(&mut app);
                assert_eq!(
                    focused.focus, target,
                    "explicit accessibility focus is retained"
                );
                focused
            } else {
                before
            };
            let bounds = focused
                .nodes
                .iter()
                .find(|(id, _)| *id == target)
                .expect("tab node")
                .1
                .bounds()
                .expect("tab bounds");
            let point = egui::pos2(
                ((bounds.x0 + bounds.x1) * 0.5) as f32,
                ((bounds.y0 + bounds.y1) * 0.5) as f32,
            );
            let button = |pressed| egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            if density == 1.25 {
                tree(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(point),
                        button(true),
                        button(false),
                    ],
                );
            } else {
                tree(
                    &mut app,
                    vec![egui::Event::PointerMoved(point), button(true)],
                );
                tree(&mut app, vec![button(false)]);
            }
            settle(&mut app);
            assert_eq!(app.tabs.active().expect("active tab").id, tab);
            assert_eq!(
                context.memory(|memory| memory.focused()),
                None,
                "tab clicks must return shortcut ownership to the media"
            );
            assert!(!context.egui_wants_keyboard_input());
            assert!(
                !app.image_loading,
                "pointer return reuses the retained image"
            );
            assert_eq!(app.edits, histories);
        }
    }
    // Explicit focus can be re-established after pointer use; retain the cleanup checks below.
    for (tab, label) in [
        (tabs[0], "Selection right (pixels)"),
        (tabs[1], "Selection bottom (pixels)"),
    ] {
        app.activate_tab(tab);
        let target = node(&settle(&mut app), label);
        tree(&mut app, vec![focus(target)]);
        assert_eq!(settle(&mut app).focus, target);
    }
    app.load_path(root.join("second.bmp"), MediaKind::Image);
    assert!(!context.data(|data| {
        data.get_temp::<super::State>(super::state_id())
            .expect("state")
            .saved
            .contains_key(&tabs[1])
    }));
    app.remove_tab(tabs[0], false);
    assert!(!context.data(|data| {
        data.get_temp::<super::State>(super::state_id())
            .expect("state")
            .saved
            .contains_key(&tabs[0])
    }));
}

#[test]
fn batched_cancelled_media_button_press_releases_prior_numeric_focus() {
    for density in [1.0, 1.25, 2.0] {
        for control in 0..3 {
            let context = fonts::test_context();
            let mut tabs = TabSet::default();
            let active = tabs.open_new("active.png".into(), MediaKind::Image);
            let other = tabs.open_new("other.png".into(), MediaKind::Image);
            let numeric = egui::Id::new("cancelled-button-numeric");
            let draw = |tab, events| {
                let mut raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                };
                raw.viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .expect("viewport")
                    .native_pixels_per_point = Some(density);
                let mut response = None;
                let _ = context.run_ui(raw, |ui| {
                    super::begin(&context, Some(tab), true);
                    response = Some(match control {
                        0 => chrome::button(ui, chrome::Icon::Play, "Play"),
                        1 => chrome::audio_button(ui, chrome::AudioIcon::Shuffle, false, "Shuffle"),
                        _ => chrome::reading_button(ui, true, false, None),
                    });
                    let value = ui.interact(
                        egui::Rect::from_min_size(
                            egui::pos2(100.0, 100.0),
                            egui::vec2(100.0, 20.0),
                        ),
                        numeric,
                        egui::Sense::focusable_noninteractive(),
                    );
                    assert_eq!(
                        seekbar::value_input(
                            &value,
                            "Playback position (seconds)",
                            25.0,
                            0.0..=100.0,
                            5.0,
                            true
                        ),
                        None
                    );
                    super::finish(&context, false, true);
                });
                response.expect("button")
            };
            let response = draw(active, vec![]);
            context.memory_mut(|memory| memory.request_focus(numeric));
            draw(
                active,
                vec![egui::Event::PointerMoved(response.rect.center())],
            );
            assert!(
                context.memory(|memory| memory.has_focus(numeric)),
                "hover retains explicit focus"
            );
            let background = egui::Id::new("background-role");
            super::adopt(&context, other, background);
            let outside = egui::pos2(400.0, 250.0);
            let pointer = |pos, pressed| egui::Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            };
            let result = draw(
                active,
                vec![
                    pointer(response.rect.center(), true),
                    egui::Event::PointerMoved(outside),
                    pointer(outside, false),
                ],
            );
            assert!(!result.clicked(), "outside release cancels activation");
            assert_eq!(
                context.memory(|memory| memory.focused()),
                None,
                "control={control}, density={density}"
            );
            assert!(
                super::take(&context, active).is_none(),
                "cancel must clear the saved numeric role"
            );
            assert_eq!(super::take(&context, other), Some(background));
        }
    }
}

#[test]
fn pointer_media_buttons_release_focus_without_removing_keyboard_activation() {
    for density in [1.0, 1.25, 2.0] {
        for control in 0..7 {
            for batched in [false, true] {
                let context = fonts::test_context();
                context.enable_accesskit();
                let mut tabs = TabSet::default();
                let active = tabs.open_new("active.png".into(), MediaKind::Image);
                let other = tabs.open_new("other.png".into(), MediaKind::Image);
                let time = std::cell::Cell::new(0.0);
                let draw = |tab, events| {
                    time.set(time.get() + 0.1);
                    let mut raw = egui::RawInput {
                        events,
                        time: Some(time.get()),
                        ..Default::default()
                    };
                    raw.viewports
                        .get_mut(&egui::ViewportId::ROOT)
                        .expect("viewport")
                        .native_pixels_per_point = Some(density);
                    let mut response = None;
                    let _ = context.run_ui(raw, |ui| {
                        super::begin(&context, Some(tab), true);
                        response = Some(match control {
                            0 => chrome::button(ui, chrome::Icon::Play, "Play"),
                            1 => chrome::button(ui, chrome::Icon::Pause, "Pause"),
                            2 => chrome::button(ui, chrome::Icon::OpenFolder, "Open folder"),
                            3 => {
                                chrome::audio_button(ui, chrome::AudioIcon::Repeat, false, "Repeat")
                            }
                            4 => chrome::audio_button(
                                ui,
                                chrome::AudioIcon::RepeatOne,
                                true,
                                "Repeat one",
                            ),
                            5 => chrome::audio_button(
                                ui,
                                chrome::AudioIcon::Shuffle,
                                true,
                                "Shuffle",
                            ),
                            _ => chrome::reading_button(ui, true, false, None),
                        });
                        super::finish(&context, false, true);
                    });
                    response.expect("button")
                };
                let response = draw(active, vec![]);
                draw(active, vec![focus(response.id.accesskit_id())]);
                assert!(draw(active, vec![]).has_focus());
                let pos = response.rect.center();
                let pointer = |pressed| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                let mut events = vec![egui::Event::PointerMoved(pos), pointer(true)];
                if !batched {
                    assert!(!draw(active, events).clicked());
                    events = vec![];
                }
                events.push(pointer(false));
                assert!(
                    draw(active, events).clicked(),
                    "pointer still activates control {control}"
                );
                assert_eq!(
                    context.memory(|memory| memory.focused()),
                    None,
                    "pointer control {control} must return keys to media; density={density}, batched={batched}"
                );
                assert!(!context.egui_wants_keyboard_input());
                draw(other, vec![]);
                assert!(
                    !draw(active, vec![]).has_focus(),
                    "do not restore the clicked role on tab return"
                );
                for (moved, focused) in [(false, false), (true, false), (false, true), (true, true)]
                {
                    draw(active, vec![focus(response.id.accesskit_id())]);
                    if !focused {
                        let outside = pos + egui::vec2(50.0, 50.0);
                        draw(
                            active,
                            vec![
                                egui::Event::PointerMoved(outside),
                                egui::Event::PointerButton {
                                    pos: outside,
                                    button: egui::PointerButton::Primary,
                                    pressed: true,
                                    modifiers: egui::Modifiers::NONE,
                                },
                                egui::Event::PointerButton {
                                    pos: outside,
                                    button: egui::PointerButton::Primary,
                                    pressed: false,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                        );
                        assert_eq!(context.memory(|memory| memory.focused()), None);
                    }
                    assert!(context.data(|data| {
                        data.get_temp::<super::State>(super::state_id())
                            .expect("state")
                            .saved
                            .contains_key(&active)
                    }));
                    let background_role = egui::Id::new("background control");
                    super::adopt(&context, other, background_role);
                    draw(active, vec![egui::Event::PointerMoved(pos), pointer(true)]);
                    let release_pos = if moved {
                        pos + egui::vec2(50.0, 50.0)
                    } else {
                        pos
                    };
                    for _ in 0..10 {
                        draw(active, vec![egui::Event::PointerMoved(release_pos)]);
                    }
                    assert!(
                        !draw(
                            active,
                            vec![egui::Event::PointerButton {
                                pos: release_pos,
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            }]
                        )
                        .clicked(),
                        "long hold or drag must not become a click"
                    );
                    assert_eq!(
                        context.memory(|memory| memory.focused()),
                        None,
                        "hold/drag release must return keys to media: control={control}, moved={moved}, focused={focused}"
                    );
                    let saved = context.data(|data| {
                        let state = data
                            .get_temp::<super::State>(super::state_id())
                            .expect("state");
                        (
                            state.saved.get(&active).copied(),
                            state.saved.get(&other).copied(),
                        )
                    });
                    assert_eq!(
                        saved,
                        (None, Some(background_role)),
                        "hold/drag clears only the active role: control={control}, moved={moved}, focused={focused}"
                    );
                    draw(other, vec![]);
                    assert!(
                        !draw(active, vec![]).has_focus(),
                        "do not restore a cancelled pointer gesture's prior role"
                    );
                }
                draw(active, vec![focus(response.id.accesskit_id())]);
                assert!(draw(active, vec![]).has_focus());
                let mut clicks = 0;
                for pressed in [true, false] {
                    clicks += usize::from(
                        draw(
                            active,
                            vec![egui::Event::Key {
                                key: egui::Key::Space,
                                physical_key: None,
                                pressed,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            }],
                        )
                        .clicked(),
                    );
                }
                assert_eq!(clicks, 1, "explicit keyboard activation remains available");
                assert!(draw(active, vec![]).has_focus());
            }
        }
    }
}

#[test]
fn pointer_seek_returns_arrow_keys_to_media_and_keeps_explicit_value_focus() {
    for density in [1.0, 1.25, 2.0] {
        for focused in [false, true] {
            for dragged in [false, true] {
                let context = fonts::test_context();
                context.enable_accesskit();
                let mut tabs = TabSet::default();
                let active = tabs.open_new("active.wav".into(), MediaKind::Audio);
                let other = tabs.open_new("other.wav".into(), MediaKind::Audio);
                let draw = |tab, events| {
                    let mut input = egui::RawInput {
                        events,
                        time: Some(context.cumulative_frame_nr() as f64 * 0.1),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(500.0, 300.0),
                        )),
                        ..Default::default()
                    };
                    input
                        .viewports
                        .get_mut(&egui::ViewportId::ROOT)
                        .expect("viewport")
                        .native_pixels_per_point = Some(density);
                    let mut result = None;
                    let _ = context.run_ui(input, |_| {
                        super::begin(&context, Some(tab), true);
                        let (response, drag) = seekbar::show_drag(
                            &context,
                            egui::Rect::from_min_size(
                                egui::pos2(0.0, 270.0),
                                egui::vec2(500.0, 30.0),
                            ),
                            0.25,
                            None,
                            true,
                            false,
                        );
                        let value = seekbar::value_input(
                            &response,
                            "Playback position (seconds)",
                            25.0,
                            0.0..=100.0,
                            5.0,
                            true,
                        );
                        super::finish(&context, false, true);
                        result = Some((response, drag, value));
                    });
                    result.expect("seek control")
                };
                draw(active, vec![]);
                let (response, _, _) = draw(active, vec![]);
                if focused {
                    draw(active, vec![focus(response.id.accesskit_id())]);
                    assert!(draw(active, vec![]).0.has_focus());
                }
                let start = egui::pos2(100.0, 270.0);
                let end = if dragged {
                    egui::pos2(400.0, 270.0)
                } else {
                    start
                };
                let pointer = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                };
                draw(
                    active,
                    vec![egui::Event::PointerMoved(start), pointer(start, true)],
                );
                assert_eq!(
                    context.memory(|memory| memory.focused()),
                    None,
                    "seek must release numeric focus on press, not only release"
                );
                if dragged {
                    draw(active, vec![egui::Event::PointerMoved(end)]);
                }
                let (_, drag, value) = draw(active, vec![pointer(end, false)]);
                assert!(drag.released);
                assert_eq!(drag.position, Some(end), "pointer seek still commits");
                assert_eq!(value, None);
                assert_eq!(
                    context.memory(|memory| memory.focused()),
                    None,
                    "pointer seek must return keys to media: {density}x, focused={focused}, dragged={dragged}"
                );
                let right = || egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                };
                assert_eq!(
                    draw(active, vec![right()]).2,
                    None,
                    "no slider key interception"
                );
                assert!(
                    context.input(|input| input.events.iter().any(|event| matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::ArrowRight,
                            pressed: true,
                            ..
                        }
                    )))
                );
                draw(other, vec![]);
                assert!(
                    !draw(active, vec![]).0.has_focus(),
                    "no pointer role restored on return"
                );
                draw(active, vec![focus(response.id.accesskit_id())]);
                assert!(draw(active, vec![]).0.has_focus());
                assert_eq!(
                    draw(active, vec![right()]).2,
                    Some(30.0),
                    "explicit value focus still adjusts"
                );
            }
        }
    }
}

#[test]
fn pointer_timeline_gestures_release_focus_and_preserve_explicit_value_input() {
    for density in [1.0, 1.25, 2.0] {
        for focused in [false, true] {
            for dragged in [false, true] {
                let context = fonts::test_context();
                context.enable_accesskit();
                let mut tabs = TabSet::default();
                let active = tabs.open_new("active.wav".into(), MediaKind::Audio);
                let other = tabs.open_new("other.wav".into(), MediaKind::Audio);
                let id = egui::Id::new("pointer-timeline");
                let value_id = id.with(("selection-value", true));
                let selection = towavue_core::TimeRange::new(
                    media_time(Duration::from_secs(2)),
                    media_time(Duration::from_secs(8)),
                )
                .expect("range");
                let draw = |tab, events| {
                    let mut raw = egui::RawInput {
                        events,
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(500.0, 300.0),
                        )),
                        ..Default::default()
                    };
                    raw.viewports
                        .get_mut(&egui::ViewportId::ROOT)
                        .expect("viewport")
                        .native_pixels_per_point = Some(density);
                    let mut result = None;
                    let _ = context.run_ui(raw, |ui| {
                        super::begin(&context, Some(tab), true);
                        let response = ui.interact(
                            egui::Rect::from_min_size(
                                egui::pos2(20.0, 60.0),
                                egui::vec2(460.0, 100.0),
                            ),
                            id,
                            egui::Sense::click_and_drag(),
                        );
                        result = Some(time_selection::show(
                            ui,
                            &response,
                            media_time(Duration::from_secs(10)),
                            media_time(Duration::from_secs(5)),
                            Some(selection),
                            None,
                            true,
                        ));
                        super::finish(&context, false, true);
                    });
                    result.expect("timeline")
                };
                draw(active, vec![]);
                draw(active, vec![]);
                if focused {
                    draw(active, vec![focus(value_id.accesskit_id())]);
                    assert!(context.memory(|memory| memory.has_focus(value_id)));
                }
                let right = || egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                };
                let start = egui::pos2(140.0, 135.0);
                let end = if dragged {
                    egui::pos2(320.0, 135.0)
                } else {
                    start
                };
                let pointer = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                };
                draw(
                    active,
                    vec![egui::Event::PointerMoved(start), pointer(start, true)],
                );
                assert_eq!(
                    context.memory(|memory| memory.focused()),
                    None,
                    "timeline must release numeric focus on press, not only release"
                );
                let held = draw(active, vec![right()]);
                assert!(
                    held.selection.is_none() && held.edit.is_none() && held.seek.is_none(),
                    "held pointer must not leave numeric arrow handling active"
                );
                assert!(context.input(|input| input.key_pressed(egui::Key::ArrowRight)));
                let mut key_release = right();
                if let egui::Event::Key { pressed, .. } = &mut key_release {
                    *pressed = false;
                }
                draw(active, vec![key_release]);
                if dragged {
                    draw(active, vec![egui::Event::PointerMoved(end)]);
                }
                let output = draw(active, vec![pointer(end, false)]);
                assert!(
                    output.selection.is_some() || output.seek.is_some(),
                    "pointer operation must still commit"
                );
                assert_eq!(
                    context.memory(|memory| memory.focused()),
                    None,
                    "timeline pointer focus: density={density}, focused={focused}, dragged={dragged}"
                );
                assert!(!context.egui_wants_keyboard_input());
                let output = draw(active, vec![right()]);
                assert!(
                    output.selection.is_none() && output.edit.is_none() && output.seek.is_none()
                );
                assert!(
                    context.input(|input| input.key_pressed(egui::Key::ArrowRight)),
                    "arrow remains available to media"
                );
                draw(other, vec![]);
                draw(active, vec![]);
                assert_eq!(
                    context.memory(|memory| memory.focused()),
                    None,
                    "no saved pointer role on tab return"
                );
                draw(active, vec![focus(value_id.accesskit_id())]);
                assert!(context.memory(|memory| memory.has_focus(value_id)));
                assert!(
                    draw(active, vec![right()]).selection.is_some(),
                    "explicit numeric focus still adjusts time"
                );
            }
        }
    }
}

#[test]
fn tab_focus_semantic_roles_ignore_widget_ids_and_wait_only_until_ready_or_new_input() {
    let context = fonts::test_context();
    let mut tabs = TabSet::default();
    let a = tabs.open_new("a.png".into(), MediaKind::Image);
    let b = tabs.open_new("b.wav".into(), MediaKind::Audio);
    let draw = |active, salt, enabled, loading, events: Vec<egui::Event>, request| {
        let mut ids = Vec::new();
        let _ = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                super::begin(&context, Some(active), enabled);
                ui.push_id(salt, |ui| {
                    for role in ["play", "seek"] {
                        let response = ui.button(role);
                        if request == Some(role) {
                            response.request_focus();
                        }
                        super::observe(&response, role);
                        ids.push(response.id);
                    }
                });
                super::finish(&context, loading, true);
            },
        );
        ids
    };
    draw(a, 1, true, false, vec![], Some("seek"));
    draw(b, 1, true, false, vec![], Some("play"));
    let ids = draw(a, 4, true, false, vec![], None);
    assert_eq!(context.memory(|m| m.focused()), Some(ids[1]));
    let ids = draw(b, 7, true, false, vec![], None);
    assert_eq!(context.memory(|m| m.focused()), Some(ids[0]));
    draw(a, 9, false, true, vec![], None);
    assert!(context.data(|d| {
        d.get_temp::<super::State>(super::state_id())
            .expect("state")
            .pending
            .is_some()
    }));
    let ids = draw(a, 9, true, false, vec![], None);
    assert_eq!(context.memory(|m| m.focused()), Some(ids[1]));
    draw(b, 1, true, false, vec![], None);
    draw(a, 2, false, true, vec![], None);
    let ids = draw(
        a,
        2,
        true,
        false,
        vec![egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        Some("play"),
    );
    assert_eq!(context.memory(|m| m.focused()), Some(ids[0]));
    super::forget(&context, a);
    assert!(!context.data(|d| {
        d.get_temp::<super::State>(super::state_id())
            .expect("state")
            .saved
            .contains_key(&a)
    }));
}

#[test]
fn tab_focus_registers_image_video_audio_values_and_path_rows_without_dispatch() {
    let Some(root) = tests::isolated_test_root(
        "tab_focus::tests::tab_focus_registers_image_video_audio_values_and_path_rows_without_dispatch",
    ) else {
        return;
    };
    for (kind, extension, labels) in [
        (
            MediaKind::Image,
            "png",
            vec![
                "Reading mode",
                "Selection left (pixels)",
                "Selection right (pixels)",
                "Selection top (pixels)",
                "Selection bottom (pixels)",
                "Image position",
                "second.png",
            ],
        ),
        (
            MediaKind::Video,
            "mp4",
            vec![
                "Play / replay",
                "Playback position (seconds)",
                "Time selection start (seconds)",
                "Time selection end (seconds)",
                "Relative volume (%)",
                "Selected duration (seconds)",
            ],
        ),
        (
            MediaKind::Audio,
            "wav",
            vec![
                "Play / replay",
                "Repeat off",
                "Shuffle off",
                "2. second.wav",
                "Time selection start (seconds)",
                "Time selection end (seconds)",
                "Relative volume (%)",
                "Selected duration (seconds)",
            ],
        ),
    ] {
        let (mut app, _) = setup();
        let context = app.ui_context.clone().expect("context");
        let paths =
            ["first", "second", "third"].map(|name| root.join(format!("{name}.{extension}")));
        let active = app.tabs.open_new(paths[0].clone(), kind);
        let other = app
            .tabs
            .open_new(root.join(format!("other.{extension}")), kind);
        app.tabs.activate(active);
        app.path = Some(paths[0].clone());
        app.media_kind = Some(kind);
        app.state = PlaybackState::Paused;
        app.media_duration = Some(Duration::from_secs(10));
        app.timeline_open = kind != MediaKind::Image;
        app.time_selection = towavue_core::TimeRange::new(
            media_time(Duration::from_secs(2)),
            media_time(Duration::from_secs(8)),
        );
        app.image_view.selection = Some(UnitRect::FULL);
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: paths
                .iter()
                .map(|path| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: path.clone(),
                    kind,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        if kind == MediaKind::Image {
            app.image = Some(
                ImagePresentation::from_decoded(
                    &context,
                    &paths[0],
                    Arc::new(DecodedImage {
                        animation_plays: 0,
                        format: "test",
                        frames: vec![towavue_runtime_windows::DecodedImageFrame {
                            width: 2,
                            height: 2,
                            rgba: vec![255; 16],
                            delay: Duration::ZERO,
                        }],
                    }),
                )
                .expect("in-memory image"),
            );
        }
        for label in labels {
            app.filmstrip_open = label == "second.png";
            let initial = settle(&mut app);
            let target = initial
                .nodes
                .iter()
                .find(|(_, n)| n.label().is_some_and(|l| l.starts_with(label)))
                .expect(label)
                .0;
            tree(&mut app, vec![focus(target)]);
            assert_eq!(settle(&mut app).focus, target, "initial {label}");
            // Keep the widget layout identical to expose accidental cross-tab ID reuse.
            app.tabs.activate(other);
            settle(&mut app);
            app.tabs.activate(active);
            let returned = settle(&mut app);
            assert_eq!(returned.focus, target, "restored {label}");
            assert!(app.edits.is_empty() && app.pending_guard.is_none());
            assert_eq!(app.state, PlaybackState::Paused);
            assert_eq!(app.path.as_ref(), Some(&paths[0]));
        }
    }
}

#[test]
fn tab_focus_fullscreen_waits_for_hover_and_missing_controls_fall_back() {
    let Some(root) = tests::isolated_test_root(
        "tab_focus::tests::tab_focus_fullscreen_waits_for_hover_and_missing_controls_fall_back",
    ) else {
        return;
    };
    let (mut app, _) = setup();
    let context = app.ui_context.clone().expect("context");
    let active = app.tabs.open_new(root.join("video.mp4"), MediaKind::Video);
    let other = app.tabs.open_new(root.join("other.mp4"), MediaKind::Video);
    app.tabs.activate(active);
    app.path = Some(root.join("video.mp4"));
    app.media_kind = Some(MediaKind::Video);
    app.state = PlaybackState::Playing;
    let initial = settle(&mut app);
    let pause = initial
        .nodes
        .iter()
        .find(|(_, n)| n.label().is_some_and(|l| l.starts_with("Pause")))
        .expect("pause")
        .0;
    tree(&mut app, vec![focus(pause)]);
    settle(&mut app);
    app.tabs.activate(other);
    settle(&mut app);
    app.fullscreen = true;
    app.fullscreen_controls_visible = false;
    app.tabs.activate(active);
    app.state = PlaybackState::Paused;
    let returned = settle(&mut app);
    assert!(!app.fullscreen_controls_visible);
    assert!(!returned.nodes.iter().any(|(_, node)| {
        node.label()
            .is_some_and(|label| label.starts_with("Play / replay"))
    }));
    tree(
        &mut app,
        vec![egui::Event::PointerMoved(egui::pos2(600.0, 465.0))],
    );
    let returned = settle(&mut app);
    assert!(app.fullscreen_controls_visible);
    let play = returned
        .nodes
        .iter()
        .find(|(_, n)| n.label().is_some_and(|l| l.starts_with("Play / replay")))
        .expect("play")
        .0;
    tree(&mut app, vec![focus(play)]);
    assert_eq!(settle(&mut app).focus, play);
    tree(
        &mut app,
        vec![egui::Event::PointerMoved(egui::pos2(320.0, 200.0))],
    );
    assert!(!app.fullscreen_controls_visible);
    app.tabs.activate(other);
    settle(&mut app);
    app.tabs.activate(active);
    app.fullscreen = false;
    app.media_kind = Some(MediaKind::Image);
    let returned = settle(&mut app);
    assert_eq!(returned.focus, node(&returned, "video.mp4"));
    super::forget(&context, active);
    assert!(!context.data(|data| {
        data.get_temp::<super::State>(super::state_id())
            .expect("state")
            .saved
            .contains_key(&active)
    }));
}

pub(crate) fn hardware_focus<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    label: &str,
    set: bool,
) {
    let context = app.ui_context.clone().expect("context");
    context.enable_accesskit();
    let mut target = None;
    for pass in 0..4 {
        let mut actions = Vec::new();
        let events = if set && pass == 2 {
            vec![focus(target.expect("target"))]
        } else {
            vec![]
        };
        // Hosted probes share the live redraw clock; renderer-only fixtures keep
        // egui's simulated clock because they have no native input adapter.
        let time = app.ui_state.as_mut().and_then(|state| {
            state
                .take_egui_input(app.window.as_ref().expect("window"))
                .time
        });
        let mut output = context.run_ui(
            egui::RawInput {
                time,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.draw_ui(ui, &mut actions),
        );
        assert!(actions.is_empty(), "focus must not dispatch media actions");
        let tree = output
            .platform_output
            .accesskit_update
            .take()
            .expect("tree");
        target = Some(
            tree.nodes
                .iter()
                .find(|(_, node)| node.label().is_some_and(|value| value.starts_with(label)))
                .expect("media control")
                .0,
        );
        if pass == 3 {
            assert_eq!(tree.focus, target.expect("live target"), "{label}");
        }
        let renderer = app.renderer.as_mut().expect("renderer");
        renderer.resize_surface(640, 480).expect("UI surface");
        renderer
            .render_ui(&context, output)
            .expect("render retained focus");
        renderer.present_surface().expect("present focus");
        if let Some(state) = app.ui_state.as_mut() {
            let native_time = state
                .take_egui_input(app.window.as_ref().expect("window"))
                .time
                .expect("native clock");
            assert!(
                context.time() <= native_time,
                "Focus probe advanced beyond the native clock: {} > {native_time}",
                context.time()
            );
        }
    }
    eprintln!("PASS retained media focus: {label}, set={set}, native UI rendered without commands");
}
