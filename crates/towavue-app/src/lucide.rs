use egui::{Color32, Rect, TextureHandle, TextureOptions, Ui};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Pixmap, Stroke, Transform};

mod paths;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Kind {
    Play,
    Pause,
    Previous,
    Next,
    Repeat,
    RepeatOne,
    Shuffle,
}

/// Shared, pinned SVG geometry for UI glyphs and the native taskbar transport.
/// White premultiplied texels allow UI tinting without another rasterization.
pub(crate) fn render(kind: Kind, size: u32, logical_size: f32) -> Option<Pixmap> {
    if !(1..=256).contains(&size) || !logical_size.is_finite() || logical_size <= 0.0 {
        return None;
    }
    let mut image = Pixmap::new(size, size)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    let transform = Transform::from_scale(size as f32 / 24.0, size as f32 / 24.0);
    let stroke = Stroke {
        width: 24.0 / logical_size,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    };
    let solid = matches!(kind, Kind::Play | Kind::Pause | Kind::Previous | Kind::Next);
    for (path, closed) in paths::paths(kind) {
        if solid && closed {
            image.fill_path(&path, &paint, FillRule::Winding, transform, None);
        }
        image.stroke_path(&path, &paint, &stroke, transform, None);
    }
    Some(image)
}

pub(crate) fn paint(ui: &Ui, rect: Rect, kind: Kind, color: Color32) {
    let density = ui.ctx().pixels_per_point();
    let size = (16.0 * density).round().clamp(1.0, 256.0) as u32;
    // Seven slots per context, replaced on DPI changes; tab/button count does not
    // grow the cache. Hover/selection tint never creates another texture.
    let id = egui::Id::new(("lucide-icon", kind));
    let cached = ui
        .ctx()
        .data(|data| data.get_temp::<(u32, TextureHandle)>(id));
    let texture = if let Some((old_size, texture)) = cached
        && old_size == size
    {
        texture
    } else {
        let Some(pixels) = render(kind, size, 16.0) else {
            return;
        };
        let texture = ui.ctx().load_texture(
            "lucide-icon",
            egui::ColorImage::from_rgba_premultiplied([size as usize; 2], pixels.data()),
            TextureOptions::LINEAR,
        );
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, (size, texture.clone())));
        texture
    };
    let origin = (rect.center() * density - egui::Vec2::splat(size as f32 * 0.5)).round() / density;
    let bounds = Rect::from_min_size(origin, egui::Vec2::splat(size as f32 / density));
    ui.painter().image(
        texture.id(),
        bounds,
        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lucide_rasters_keep_solid_transport_and_thin_open_paths_at_each_density() {
        let kinds = [
            Kind::Play,
            Kind::Pause,
            Kind::Previous,
            Kind::Next,
            Kind::Repeat,
            Kind::RepeatOne,
            Kind::Shuffle,
        ];
        for logical in [16.0, 32.0] {
            for density in [1.0, 1.25, 1.5, 2.0, 3.0] {
                let size = (logical * density) as u32;
                let images = kinds.map(|kind| render(kind, size, logical).expect("raster"));
                for (index, image) in images.iter().enumerate() {
                    let pixels = image.data().as_chunks::<4>().0;
                    assert!(
                        pixels
                            .iter()
                            .all(|p| p[0] == p[1] && p[1] == p[2] && p[2] == p[3])
                    );
                    assert!(pixels.iter().any(|p| p[3] > 200));
                    // No opaque stroke is clipped at the canvas boundary.
                    let at = |x: u32, y: u32| pixels[(y * size + x) as usize][3];
                    for edge in 0..size {
                        assert!(
                            [
                                at(edge, 0),
                                at(edge, size - 1),
                                at(0, edge),
                                at(size - 1, edge)
                            ]
                            .iter()
                            .all(|alpha| *alpha < 128)
                        );
                    }
                    for other in &images[index + 1..] {
                        assert_ne!(image.data(), other.data());
                    }
                    if let Some(directory) = std::env::var_os("TOWAVUE_LUCIDE_ARTIFACTS") {
                        let directory = std::path::PathBuf::from(directory);
                        std::fs::create_dir_all(&directory).expect("artifact directory");
                        std::fs::write(
                            directory.join(format!("{}-{size}-{index}.rgba", logical as u32)),
                            image.data(),
                        )
                        .expect("raster artifact");
                    }
                }
                let at = |index: usize, x: f32, y: f32| {
                    let x = (x * size as f32 / 24.0).floor() as usize;
                    let y = (y * size as f32 / 24.0).floor() as usize;
                    images[index].data()[(y * size as usize + x) * 4 + 3]
                };
                assert_eq!(at(0, 10.0, 12.0), 255, "solid play interior");
                assert_eq!(at(1, 7.0, 12.0), 255, "solid pause bar");
                // At 16 px the central pixel overlaps the rounded bar stroke
                // by one sixth of a pixel; the gap still stays mostly transparent.
                assert!(at(1, 12.0, 12.0) < 64, "pause gap remains open");
                assert_eq!(at(4, 12.0, 12.0), 0, "repeat is not filled");
                // Integrate a straight skip bar across its width. One logical
                // point survives antialiasing; it cannot regress to the old 1.4.
                let row = (size / 2) as usize;
                let coverage: f32 = (0..(size / 4) as usize)
                    .map(|x| images[2].data()[(row * size as usize + x) * 4 + 3] as f32 / 255.0)
                    .sum();
                assert!(
                    (coverage - density).abs() < 0.12,
                    "stroke coverage {coverage}, density {density}"
                );
            }
        }
    }
}
