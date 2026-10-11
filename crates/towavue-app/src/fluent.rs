use egui::{Color32, Rect, TextureHandle, TextureOptions, Ui};
use tiny_skia::{FillRule, Paint, Pixmap, Transform};

mod paths;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Kind {
    ImageFilled,
    VideoFilled,
    AudioFilled,
    ImageMultipleFilled,
    KeyboardFilled,
    Keyboard,
    Play,
    Pause,
    Previous,
    Next,
    Repeat,
    RepeatOff,
    RepeatOne,
    Shuffle,
    ShuffleOff,
    Image,
    ImageMultiple,
    Link,
    LinkOff,
    Video,
    Audio,
    Move,
    New,
    Forbidden,
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
    let transform = Transform::from_scale(size as f32 / 16.0, size as f32 / 16.0);
    fill(&mut image, kind, &paint, transform);
    Some(image)
}

/// Reuse exact upstream compound paths, including holes, at every native surface.
pub(crate) fn fill(image: &mut Pixmap, kind: Kind, paint: &Paint<'_>, transform: Transform) {
    // Upstream Play uses optical centering (issue #137). Its silhouette spans
    // x=4..15 in a 16-point canvas. Center its bounds for our compact buttons;
    // applying the translation before scaling also fixes native taskbar icons.
    let transform = if kind == Kind::Play {
        transform.pre_translate(-1.5, 0.0)
    } else if matches!(kind, Kind::Link | Kind::LinkOff) {
        // Vertical link connects the width and height fields.
        transform
            .pre_translate(8.0, 8.0)
            .pre_rotate(90.0)
            .pre_translate(-8.0, -8.0)
    } else {
        transform
    };
    for path in paths::paths(kind) {
        image.fill_path(&path, paint, FillRule::Winding, transform, None);
    }
}

pub(crate) fn paint(ui: &Ui, rect: Rect, kind: Kind, color: Color32) {
    let density = ui.ctx().pixels_per_point();
    let size = (16.0 * density).round().clamp(1.0, 256.0) as u32;
    // One bounded slot per icon per context, replaced on DPI changes; tab/button count does not
    // grow the cache. Hover/selection tint never creates another texture.
    let id = egui::Id::new(("fluent-icon", kind));
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
            "fluent-icon",
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
    fn play_silhouette_is_centered_at_ui_and_taskbar_densities() {
        for size in [16, 20, 24, 32, 40, 48, 64, 96] {
            let image = render(Kind::Play, size, 16.0).expect("raster");
            let xs: Vec<_> = image
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, pixel)| pixel[3] > 128)
                .map(|(index, _)| index as u32 % size)
                .collect();
            let sum = xs.iter().min().expect("test value") + xs.iter().max().expect("test value");
            assert!(
                (sum as f32 - (size - 1) as f32).abs() <= 1.0,
                "size={size}: bounds sum={sum}"
            );
        }
    }

    #[test]
    fn fluent_rasters_keep_distinct_premultiplied_unclipped_artwork_at_each_density() {
        let kinds = [
            Kind::Play,
            Kind::Pause,
            Kind::Previous,
            Kind::Next,
            Kind::Repeat,
            Kind::RepeatOff,
            Kind::RepeatOne,
            Kind::Shuffle,
            Kind::ShuffleOff,
            Kind::Image,
            Kind::ImageMultiple,
            Kind::ImageFilled,
            Kind::VideoFilled,
            Kind::AudioFilled,
            Kind::ImageMultipleFilled,
            Kind::KeyboardFilled,
            Kind::Link,
            Kind::LinkOff,
            Kind::Keyboard,
            Kind::Video,
            Kind::Audio,
            Kind::Move,
            Kind::New,
            Kind::Forbidden,
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
                    // Some upstream icons intentionally reach the canvas edge.
                    // Check their geometry instead of imposing Lucide's inset.
                    for path in paths::paths(kinds[index]) {
                        let bounds = path.bounds();
                        assert!(bounds.left() >= 0.0 && bounds.top() >= 0.0);
                        assert!(bounds.right() <= 16.0 && bounds.bottom() <= 16.0);
                    }
                    for other in &images[index + 1..] {
                        assert_ne!(image.data(), other.data());
                    }
                    if let Some(directory) = std::env::var_os("TOWAVUE_FLUENT_ARTIFACTS") {
                        let directory = std::path::PathBuf::from(directory);
                        std::fs::create_dir_all(&directory).expect("artifact directory");
                        std::fs::write(
                            directory.join(format!("{}-{size}-{index}.rgba", logical as u32)),
                            image.data(),
                        )
                        .expect("raster artifact");
                    }
                }
            }
        }
    }
}
