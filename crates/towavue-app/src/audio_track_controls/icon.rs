use egui::{Color32, Rect, TextureHandle, TextureOptions, Ui};
use tiny_skia::{LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Rasterize one connected SVG stroke so joins and caps share one coverage mask.
/// Overlapping egui segments/disks otherwise accumulate antialias fringes.
pub(super) fn paint(ui: &Ui, bounds: Rect, ink: Color32) {
    let size = (16.0 * ui.ctx().pixels_per_point())
        .round()
        .clamp(1.0, 256.0) as u32;
    let id = egui::Id::new("mute-track-icon");
    let cached = ui
        .ctx()
        .data(|data| data.get_temp::<(u32, TextureHandle)>(id));
    let texture = if let Some((old, texture)) = cached
        && old == size
    {
        texture
    } else {
        let pixels = render(size);
        let texture = ui.ctx().load_texture(
            "mute-track-icon",
            egui::ColorImage::from_rgba_premultiplied([size as usize; 2], pixels.data()),
            TextureOptions::LINEAR,
        );
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, (size, texture.clone())));
        texture
    };
    ui.painter().image(
        texture.id(),
        bounds,
        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        ink,
    );
}

fn render(size: u32) -> Pixmap {
    let mut pixels = Pixmap::new(size, size).expect("bounded icon dimensions");
    let mut path = PathBuilder::new();
    path.move_to(11.5, 11.5);
    path.line_to(11.5, 4.5);
    path.line_to(8.0, 8.0);
    path.line_to(4.5, 4.5);
    path.line_to(4.5, 11.5);
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    pixels.stroke_path(
        &path.finish().expect("M polyline"),
        &paint,
        &Stroke {
            width: 1.0,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        },
        Transform::from_scale(size as f32 / 16.0, size as f32 / 16.0),
        None,
    );
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_stroke_has_symmetric_coverage_and_no_corner_protrusions() {
        for size in [16, 20, 24, 28, 32] {
            let pixels = render(size);
            let alpha = |x: u32, y: u32| pixels.data()[((y * size + x) * 4 + 3) as usize];
            let mut covered = 0;
            for y in 0..size {
                for x in 0..size {
                    let a = alpha(x, y);
                    assert!(
                        a.abs_diff(alpha(size - 1 - x, y)) <= 2,
                        "horizontal symmetry at {size}: {x},{y}"
                    );
                    if a > 0 {
                        covered += 1;
                        // Seven-unit path plus a half-unit stroke on each side.
                        assert!(x >= size / 4 && x < size * 3 / 4);
                        assert!(y >= size / 4 && y < size * 3 / 4);
                    }
                }
            }
            assert!(covered > 0);
            assert_eq!(alpha(size / 2, size * 5 / 8), 0, "space below the M valley");
        }
    }
}
