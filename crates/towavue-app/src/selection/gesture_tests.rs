use crate::*;

#[test]
fn outside_click_clears_selection_but_drags_controls_and_cancellation_do_not() {
    let Some(_) = tests::isolated_test_root(
        "selection::gesture_tests::outside_click_clears_selection_but_drags_controls_and_cancellation_do_not",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.media_kind = Some(MediaKind::Image);
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 500.0));
    let image = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(400.0, 300.0));
    let original = UnitRect {
        min: UnitPoint { x: 0.0, y: 0.2 },
        max: UnitPoint { x: 0.6, y: 0.8 },
    };
    let inside = selection_rect(image, original).center();
    let outside = egui::pos2(450.0, 250.0);
    let margin = egui::pos2(50.0, 250.0);
    let event = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    for start in [outside, margin] {
        for case in 0..8 {
            let context = fonts::test_context();
            let mut time = 0.0;
            let mut frame = |app: &mut Application<_>, events| {
                time += 0.05;
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(viewport),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        ui.add_enabled_ui(case != 4, |ui| {
                            let mut response = ui.interact(
                                viewport,
                                "outside-selection".into(),
                                egui::Sense::click_and_drag(),
                            );
                            if case == 5 {
                                response.interact_rect = image.shrink(60.0);
                            }
                            let pointer = ui.input(|input| input.pointer.hover_pos());
                            app.update_selection(&response, image, (400, 300), false, pointer);
                        });
                    },
                )
            };
            app.image_view.selection = Some(original);
            app.image_view.fit();
            app.image_view.pan = (7.0, 9.0);
            frame(&mut app, vec![egui::Event::PointerMoved(start)]);
            if case == 1 || case == 2 {
                frame(&mut app, vec![event(start, true)]);
                assert_eq!(app.image_view.selection, Some(original));
                if case == 2 {
                    app.cancel_view_drag();
                }
                frame(&mut app, vec![event(start, false)]);
            } else {
                let end = if case == 3 {
                    start + egui::vec2(-30.0, 30.0)
                } else {
                    start
                };
                let mut events = vec![
                    event(start, true),
                    egui::Event::PointerMoved(end),
                    event(end, false),
                ];
                if case == 6 {
                    events.extend([event(inside, true), event(inside, false)]);
                } else if case == 7 {
                    events = vec![
                        event(inside, true),
                        event(inside, false),
                        event(start, true),
                        event(start, false),
                    ];
                }
                frame(&mut app, events);
            }
            if matches!(case, 0 | 1 | 6 | 7) {
                assert_eq!(
                    app.image_view.selection, None,
                    "start={start:?}, case={case}"
                );
            } else if case == 3 && start == outside {
                assert_ne!(app.image_view.selection, Some(original));
                assert!(app.image_view.selection.is_some());
            } else {
                assert_eq!(
                    app.image_view.selection,
                    Some(original),
                    "start={start:?}, case={case}"
                );
            }
            assert_eq!(
                matches!(app.image_view.zoom, ZoomMode::Custom(_)),
                case == 7
            );
            if case != 7 {
                assert_eq!(app.image_view.pan, (7.0, 9.0));
            }
            assert!(app.view_drag.is_none());
            assert!(app.edits.is_empty());
        }
    }
}

#[test]
fn corners_resize_both_edges_and_shift_keeps_the_opposite_corner_and_ratio() {
    let Some(_) = tests::isolated_test_root(
        "selection::gesture_tests::corners_resize_both_edges_and_shift_keeps_the_opposite_corner_and_ratio",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.media_kind = Some(MediaKind::Image);
    let context = fonts::test_context();
    let image = egui::Rect::from_min_size(egui::pos2(30.0, 40.0), egui::vec2(1000.0, 500.0));
    let original = UnitRect {
        min: UnitPoint { x: 0.2, y: 0.3 },
        max: UnitPoint { x: 0.6, y: 0.7 },
    };
    let mut time = 0.0;
    let mut frame = |app: &mut Application<_>, events, shift| {
        time += 0.05;
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 700.0),
                )),
                time: Some(time),
                events,
                modifiers: if shift {
                    egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::NONE
                },
                ..Default::default()
            },
            |ui| {
                let response = ui.interact(image, "corner".into(), egui::Sense::click_and_drag());
                let pointer = ui.input(|input| input.pointer.hover_pos());
                app.update_selection(&response, image, (1000, 500), shift, pointer);
            },
        )
    };
    for left in [false, true] {
        for top in [false, true] {
            for shift in [false, true] {
                app.image_view.selection = Some(original);
                let selected = selection_rect(image, original);
                let start = egui::pos2(
                    if left {
                        selected.left()
                    } else {
                        selected.right()
                    },
                    if top {
                        selected.top()
                    } else {
                        selected.bottom()
                    },
                );
                let target = image.min
                    + egui::vec2(
                        if left { 50.0 } else { 950.0 },
                        if top { 25.0 } else { 475.0 },
                    );
                for _ in 0..3 {
                    frame(&mut app, vec![egui::Event::PointerMoved(start)], shift);
                }
                let output = frame(&mut app, vec![], shift);
                assert_eq!(
                    output.platform_output.cursor_icon,
                    if left == top {
                        egui::CursorIcon::ResizeNwSe
                    } else {
                        egui::CursorIcon::ResizeNeSw
                    }
                );
                let button = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: if shift {
                        egui::Modifiers::SHIFT
                    } else {
                        egui::Modifiers::NONE
                    },
                };
                let output = frame(&mut app, vec![button(start, true)], shift);
                assert_eq!(
                    output.platform_output.cursor_icon,
                    egui::CursorIcon::Crosshair
                );
                frame(&mut app, vec![egui::Event::PointerMoved(target)], shift);
                let changed = app.image_view.selection.expect("selection");
                assert_eq!(
                    if left { changed.max.x } else { changed.min.x },
                    if left { original.max.x } else { original.min.x }
                );
                assert_eq!(
                    if top { changed.max.y } else { changed.min.y },
                    if top { original.max.y } else { original.min.y }
                );
                assert!(changed.width() > original.width() && changed.height() > original.height());
                assert!(
                    changed.min.x >= 0.0
                        && changed.min.y >= 0.0
                        && changed.max.x <= 1.0
                        && changed.max.y <= 1.0
                );
                if shift {
                    assert!(
                        (changed.width() / changed.height() - original.width() / original.height())
                            .abs()
                            < 0.0001
                    );
                }
                app.cancel_view_drag();
                frame(&mut app, vec![button(target, false)], shift);
                assert_eq!(app.image_view.selection, Some(original));
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(start),
                        button(start, true),
                        egui::Event::PointerMoved(target),
                        button(target, false),
                    ],
                    shift,
                );
                assert!(app.image_view.selection.expect("committed").width() > original.width());
                assert!(app.view_drag.is_none());
            }
        }
    }
}

#[test]
fn selection_translation_keeps_pixel_extent_and_cancels_without_panning() {
    let Some(_) = tests::isolated_test_root(
        "selection::gesture_tests::selection_translation_keeps_pixel_extent_and_cancels_without_panning",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.media_kind = Some(MediaKind::Image);
    let context = fonts::test_context();
    let image = egui::Rect::from_min_size(egui::pos2(30.0, 40.0), egui::vec2(1000.0, 500.0));
    let crop = PixelCrop {
        x: 203,
        y: 107,
        width: 391,
        height: 211,
    };
    let original = crop.unit_rect((1000, 500));
    let start = selection_rect(image, original).center();
    let frame = |app: &mut Application<_>, events| {
        let mut owned = false;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let response = ui.interact(
                    image,
                    "move-selection".into(),
                    egui::Sense::click_and_drag(),
                );
                let pointer = ui.input(|input| input.pointer.hover_pos());
                owned = app.move_image_selection(&response, image, (1000, 500), pointer);
            },
        );
        (owned, output)
    };
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Secondary,
        modifiers: egui::Modifiers::NONE,
    };
    for delta in [
        egui::vec2(35.0, -21.0),
        egui::vec2(2000.0, 2000.0),
        egui::vec2(-2000.0, -2000.0),
    ] {
        app.image_view.selection = Some(original);
        app.image_view.pan = (10.0, 20.0);
        frame(&mut app, vec![egui::Event::PointerMoved(start)]);
        assert!(frame(&mut app, vec![button(start, true)]).0);
        let (_, output) = frame(&mut app, vec![egui::Event::PointerMoved(start + delta)]);
        assert_eq!(
            output.platform_output.cursor_icon,
            egui::CursorIcon::AllScroll
        );
        let result = PixelCrop::from_selection(
            app.image_view.selection.expect("moved"),
            (1000, 500),
            MediaKind::Image,
        )
        .expect("crop");
        assert_eq!((result.width, result.height), (crop.width, crop.height));
        assert_eq!(
            result.x,
            (crop.x as f32 + delta.x).clamp(0.0, (1000 - crop.width) as f32) as u32
        );
        assert_eq!(
            result.y,
            (crop.y as f32 + delta.y).clamp(0.0, (500 - crop.height) as f32) as u32
        );
        app.cancel_view_drag();
        frame(&mut app, vec![button(start + delta, false)]);
        assert_eq!(app.image_view.selection, Some(original));
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                button(start, true),
                egui::Event::PointerMoved(start + delta),
                button(start + delta, false),
            ],
        );
        assert_eq!(
            app.image_view.selection,
            Some(result.unit_rect((1000, 500)))
        );
        assert_eq!(app.image_view.pan, (10.0, 20.0));
        assert!(app.view_drag.is_none());
        assert!(app.edits.is_empty());
    }
}
