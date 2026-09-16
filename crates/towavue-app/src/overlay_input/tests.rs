use crate::*;

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn palette_outside_wheel_keeps_it_open_and_press_hands_off_without_stealing_inside_input() {
    let Some(root) = tests::isolated_test_root(
        "overlay_input::tests::palette_outside_wheel_keeps_it_open_and_press_hands_off_without_stealing_inside_input",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let context = fonts::test_context();
        context.global_style_mut(chrome::style);
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context.clone());
        let path = root.join("image.png");
        app.tabs.open_new(path.clone(), MediaKind::Image);
        app.path = Some(path.clone());
        app.media_kind = Some(MediaKind::Image);
        app.state = PlaybackState::Paused;
        app.image = Some(
            ImagePresentation::from_decoded(
                &context,
                &path,
                DecodedImage {
                    animation_plays: 0,
                    format: "fixture",
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width: 1200,
                        height: 800,
                        rgba: vec![255; 1200 * 800 * 4],
                        delay: Duration::ZERO,
                    }],
                }
                .into(),
            )
            .expect("image"),
        );
        let frame = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 600.0),
                    )),
                    viewports: [(
                        egui::ViewportId::ROOT,
                        egui::ViewportInfo {
                            native_pixels_per_point: Some(density),
                            ..Default::default()
                        },
                    )]
                    .into_iter()
                    .collect(),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            for action in actions {
                app.handle_ui_action(action);
            }
            output
        };
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        app.dispatch(CommandId::ToggleCommandPalette);
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let query = context
            .read_response("command-palette-query".into())
            .expect("query")
            .rect
            .center();
        let outside = egui::pos2(35.0, 440.0);
        assert!(
            !context
                .memory(|memory| memory.area_rect("command-palette"))
                .expect("palette bounds")
                .contains(outside)
        );
        let wheel = || egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 5.0),
            modifiers: egui::Modifiers::CTRL,
            phase: egui::TouchPhase::Move,
        };
        let before = app.image_view;
        frame(&mut app, vec![egui::Event::PointerMoved(query), wheel()]);
        assert_eq!(app.image_view, before, "wheel inside does not reach image");
        frame(&mut app, vec![egui::Event::PointerMoved(outside), wheel()]);
        assert_ne!(app.image_view, before, "wheel outside zooms image");
        assert!(app.palette_open, "outside wheel does not dismiss palette");
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(query), button(query, true)],
        );
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(outside), button(outside, false)],
        );
        assert!(
            app.palette_open,
            "dragging out of the query does not dismiss it"
        );
        assert!(
            app.view_drag.is_none(),
            "query drag does not become image drag"
        );
        frame(&mut app, vec![button(outside, true)]);
        assert!(!app.palette_open, "outside press closes before release");
        assert!(
            app.view_drag.is_some(),
            "the same press begins an image gesture"
        );
        frame(&mut app, vec![button(outside, false)]);
        assert!(app.view_drag.is_none());
        app.dispatch(CommandId::ToggleCommandPalette);
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let logo = egui::pos2(20.0, 18.0);
        frame(&mut app, vec![egui::Event::PointerMoved(logo)]);
        frame(&mut app, vec![button(logo, true)]);
        assert!(!app.palette_open);
        frame(&mut app, vec![button(logo, false)]);
        frame(&mut app, vec![]);
        assert!(
            egui::Popup::is_any_open(&context),
            "same click opens the menu"
        );
        let layers = context.memory(|memory| memory.layer_ids().collect::<Vec<_>>());
        let menu = layers
            .into_iter()
            .find(|layer| egui::Popup::is_id_open(&context, layer.id))
            .expect("menu layer");
        let inside = context
            .memory(|memory| memory.area_rect(menu.id))
            .expect("menu geometry")
            .center();
        let before = app.image_view;
        frame(&mut app, vec![egui::Event::PointerMoved(inside), wheel()]);
        assert_eq!(app.image_view, before, "wheel over a menu stays inside it");
        frame(&mut app, vec![egui::Event::PointerMoved(outside), wheel()]);
        assert_ne!(
            app.image_view, before,
            "outside wheel also works with a menu"
        );
        assert!(egui::Popup::is_any_open(&context));
        frame(&mut app, vec![button(outside, true)]);
        assert!(
            !egui::Popup::is_any_open(&context),
            "outside menu press closes before release"
        );
        frame(&mut app, vec![button(outside, false)]);
        app.dispatch(CommandId::ToggleCommandPalette);
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        app.pending_guard = Some(GuardedAction::Exit);
        let before = app.image_view;
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(outside),
                wheel(),
                button(outside, true),
                button(outside, false),
            ],
        );
        assert!(app.pending_guard.is_some());
        assert_eq!(
            app.image_view, before,
            "save confirmation retains modal ownership"
        );
    }
}

#[test]
fn menu_outside_press_preserves_nested_hit_regions_and_opener_toggle() {
    let context = fonts::test_context();
    let mut root = None;
    let mut nested = None;
    let mut action = None;
    let mut chosen = false;
    let mut background = false;
    let mut frame = |events| {
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                super::dismiss_menu_on_outside_press(&context);
                root = Some(
                    ui.menu_button("Root", |ui| {
                        nested = Some(
                            ui.menu_button("Nested", |ui| {
                                let response = ui.button("Nested action");
                                action = Some(response.rect.center());
                                if response.clicked() {
                                    chosen = true;
                                    ui.close();
                                }
                            })
                            .response
                            .rect
                            .center(),
                        );
                    })
                    .response
                    .rect
                    .center(),
                );
                background |= ui
                    .put(
                        egui::Rect::from_min_size(
                            egui::pos2(500.0, 400.0),
                            egui::vec2(100.0, 40.0),
                        ),
                        egui::Button::new("Outside"),
                    )
                    .clicked();
            },
        );
        (root, nested, action, chosen, background)
    };
    let opener = frame(vec![]).0.expect("opener");
    frame(vec![egui::Event::PointerMoved(opener)]);
    frame(vec![button(opener, true)]);
    frame(vec![button(opener, false)]);
    let submenu = frame(vec![]).1.expect("submenu opener");
    frame(vec![egui::Event::PointerMoved(submenu)]);
    let inside = frame(vec![]).2.expect("nested action");
    frame(vec![egui::Event::PointerMoved(inside)]);
    assert!(
        !frame(vec![button(inside, true)]).3,
        "press inside does not prematurely dismiss"
    );
    assert!(egui::Popup::is_any_open(&context));
    assert!(
        frame(vec![button(inside, false)]).3,
        "nested action receives its click"
    );
    for _ in 0..2 {
        frame(vec![]);
    }
    frame(vec![egui::Event::PointerMoved(opener)]);
    frame(vec![button(opener, true)]);
    frame(vec![button(opener, false)]);
    frame(vec![]);
    assert!(egui::Popup::is_any_open(&context));
    frame(vec![button(opener, true)]);
    frame(vec![button(opener, false)]);
    assert!(
        !egui::Popup::is_any_open(&context),
        "opener closes instead of reopening on release"
    );
    frame(vec![button(opener, true)]);
    frame(vec![button(opener, false)]);
    frame(vec![]);
    let outside = egui::pos2(540.0, 420.0);
    frame(vec![egui::Event::PointerMoved(outside)]);
    frame(vec![button(outside, true)]);
    assert!(!egui::Popup::is_any_open(&context));
    assert!(
        frame(vec![button(outside, false)]).4,
        "outside button receives the same gesture"
    );
}
