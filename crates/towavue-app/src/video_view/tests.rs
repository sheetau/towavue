use super::*;

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

#[test]
fn video_right_drag_moves_selection_on_its_pixel_grid_without_panning() {
    let Some(_) = crate::tests::isolated_test_root(
        "video_view::tests::video_right_drag_moves_selection_on_its_pixel_grid_without_panning",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.media_kind = Some(MediaKind::Video);
    app.timeline_open = true;
    let viewport = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(600.0, 360.0));
    let crop = PixelCrop {
        x: 20,
        y: 12,
        width: 40,
        height: 24,
    };
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Secondary,
        modifiers: egui::Modifiers::NONE,
    };
    for density in [1.0, 1.25, 2.0] {
        for (size, aspect, zoom) in [
            ((120, 80), 1.5, ZoomMode::Fit),
            ((80, 120), 2.0 / 3.0, ZoomMode::Fit),
            ((121, 81), 1.5, ZoomMode::Fit),
            ((120, 80), 1.5, ZoomMode::Custom(8.0 * density)),
        ] {
            let context = fonts::test_context();
            context.set_pixels_per_point(density);
            let original = crop.unit_rect(size);
            app.image_view.fit();
            app.image_view.zoom = zoom;
            let full = rect(viewport, size, aspect, density, app.image_view);
            let start = selection_rect(full, original).center();
            let frame = |app: &mut Application<_>, events, focused| {
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(viewport),
                        events,
                        focused,
                        ..Default::default()
                    },
                    |ui| {
                        let response = ui.interact(
                            viewport,
                            "video-move".into(),
                            egui::Sense::click_and_drag(),
                        );
                        app.update_video_view(ui, &response, full, size);
                        app.update_selection(
                            &response,
                            full,
                            size,
                            false,
                            ui.input(|input| input.pointer.hover_pos()),
                        );
                    },
                )
            };
            for delta in [
                egui::vec2(11.4, -3.4),
                egui::Vec2::splat(1000.0),
                egui::Vec2::splat(-1000.0),
            ] {
                let end = start + delta / egui::vec2(size.0 as f32, size.1 as f32) * full.size();
                let expected = PixelCrop {
                    x: (20.0 + (delta.x / 2.0).round() * 2.0)
                        .clamp(0.0, ((size.0 - 40) / 2 * 2) as f32) as u32,
                    y: (12.0 + (delta.y / 2.0).round() * 2.0)
                        .clamp(0.0, ((size.1 - 24) / 2 * 2) as f32) as u32,
                    ..crop
                }
                .unit_rect(size);
                for mode in 0..4 {
                    app.image_view.selection = Some(original);
                    app.image_view.pan = (0.0, 0.0);
                    frame(&mut app, vec![egui::Event::PointerMoved(start)], true);
                    if mode == 0 {
                        frame(
                            &mut app,
                            vec![
                                button(start, true),
                                egui::Event::PointerMoved(end),
                                button(end, false),
                            ],
                            true,
                        );
                    } else {
                        let output = frame(&mut app, vec![button(start, true)], true);
                        assert_eq!(
                            output.platform_output.cursor_icon,
                            egui::CursorIcon::AllScroll
                        );
                        assert!(matches!(
                            app.view_drag,
                            Some(ViewDrag::MoveSelection { .. })
                        ));
                        let output = frame(&mut app, vec![egui::Event::PointerMoved(end)], true);
                        assert_eq!(
                            output.platform_output.cursor_icon,
                            egui::CursorIcon::AllScroll
                        );
                        assert_eq!(app.image_view.selection, Some(expected));
                        if mode == 2 {
                            frame(&mut app, vec![], false);
                        } else if mode == 3 {
                            app.filmstrip_open = true;
                            frame(&mut app, vec![], true);
                            app.filmstrip_open = false;
                        }
                        frame(&mut app, vec![button(end, false)], true);
                    }
                    assert_eq!(
                        app.image_view.selection,
                        Some(if mode >= 2 { original } else { expected })
                    );
                    assert_eq!(app.image_view.pan, (0.0, 0.0));
                    assert_eq!(app.image_view.zoom, zoom);
                    assert!(app.view_drag.is_none());
                    assert!(app.edits.is_empty());
                }
            }
            // Press ownership does not change when an outside drag crosses the selection.
            app.image_view.selection = Some(original);
            let outside = full.intersect(viewport).right_bottom() - egui::vec2(1.0, 1.0);
            frame(&mut app, vec![egui::Event::PointerMoved(outside)], true);
            frame(
                &mut app,
                vec![
                    button(outside, true),
                    egui::Event::PointerMoved(start),
                    button(start, false),
                ],
                true,
            );
            assert_eq!(app.image_view.selection, Some(original));
            assert_eq!(
                app.image_view.pan,
                ((start - outside).x, (start - outside).y)
            );
            app.image_view.pan = (0.0, 0.0);
            app.timeline_open = false;
            frame(
                &mut app,
                vec![
                    button(start, true),
                    egui::Event::PointerMoved(start + egui::vec2(50.0, 10.0)),
                    button(start + egui::vec2(50.0, 10.0), false),
                ],
                true,
            );
            assert_eq!(app.image_view.selection, Some(original));
            assert_eq!(app.image_view.pan, (0.0, 0.0));
            app.timeline_open = true;
        }
    }
}

#[test]
fn video_view_fit_cover_actual_and_custom_use_physical_rows_and_exact_sar() {
    let viewport = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(701.0, 403.0));
    for density in [1.0, 1.25, 2.0] {
        for aspect in [0.5, 1.0, 1.5, 4.0 / 3.0] {
            for size in [(321, 179), (1920, 1080), (16_384, 1)] {
                let fit = rect(viewport, size, aspect, density, ImageViewState::default());
                let expected = fitted_video_rect(viewport, size, aspect);
                close(fit.width(), expected.width());
                close(fit.height(), expected.height());
                let mut view = ImageViewState::default();
                view.actual_size();
                let actual = rect(viewport, size, aspect, density, view);
                close(actual.height() * density, size.1 as f32);
                close(actual.width() * density, size.0 as f32 * aspect);
                view.cover();
                let cover = rect(viewport, size, aspect, density, view);
                assert!(
                    cover.width() >= viewport.width() - 0.001
                        && cover.height() >= viewport.height() - 0.001
                );
                view.zoom = ZoomMode::Custom(0.37);
                view.pan = (9.0, -12.0);
                let custom = rect(viewport, size, aspect, density, view);
                close(custom.height() * density, size.1 as f32 * 0.37);
                close(custom.center().x, viewport.center().x + 9.0);
                close(custom.center().y, viewport.center().y - 12.0);
                let larger = rect(viewport.expand(40.0), size, aspect, density, view);
                close(larger.height(), custom.height());
            }
        }
    }
}

#[test]
fn video_view_clipping_preserves_transformed_uv_without_pixel_rounding() {
    let viewport = egui::Rect::from_min_size(egui::pos2(40.0, 60.0), egui::vec2(400.0, 200.0));
    let full = egui::Rect::from_min_size(egui::pos2(-80.5, -30.25), egui::vec2(640.0, 360.0));
    for operations in [
        vec![],
        vec![EditOperation::RotateClockwise],
        vec![
            EditOperation::FlipHorizontal,
            EditOperation::RotateCounterclockwise,
        ],
    ] {
        let transform = ImageTransform::new((640, 360), &operations);
        let (visible, uv) = clipped(viewport, full, transform.uv).expect("intersection");
        assert_eq!(visible, viewport);
        for (point, actual) in [
            visible.left_top(),
            visible.right_top(),
            visible.right_bottom(),
            visible.left_bottom(),
        ]
        .into_iter()
        .zip(uv)
        {
            let relative = (point - full.min) / full.size();
            let expected = bilinear_uv(transform.uv, relative.x, relative.y);
            close(actual.x, expected.x);
            close(actual.y, expected.y);
        }
        let original = clipped(full.expand(50.0), full, transform.uv).expect("whole frame");
        assert_eq!(original, (full, transform.uv));
    }
    assert!(
        clipped(
            viewport,
            full.translate(egui::vec2(2000.0, 0.0)),
            ImageTransform::new((1, 1), &[]).uv
        )
        .is_none()
    );
}
