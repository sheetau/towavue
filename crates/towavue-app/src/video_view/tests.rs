use super::*;

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
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
