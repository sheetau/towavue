use crate::*;

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
                app.image_view.crop_preview = false;
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
                assert!(!app.image_view.crop_preview);
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
