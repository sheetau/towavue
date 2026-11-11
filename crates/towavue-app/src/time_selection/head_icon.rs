use egui::{Painter, Pos2, TextureHandle, Ui};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

const SVG: &str = include_str!("../../assets/timeline-cti.svg");

/// The supplied Figma vector uses only straight segments. Retain that asset as
/// the source of truth and rasterize its compound silhouette at native density.
/// The node is rotated 180 degrees in the design's CTI parent.
fn render(width: u32, height: u32) -> Option<Pixmap> {
    let data = SVG.split("<path d=\"").nth(1)?.split('"').next()?;
    let numbers = data.replace(['M', 'L', 'Z', ','], " ");
    let coordinates: Vec<f32> = numbers
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let mut path = PathBuilder::new();
    for (index, point) in coordinates.as_chunks::<2>().0.iter().enumerate() {
        if index == 0 {
            path.move_to(point[0], point[1]);
        } else {
            path.line_to(point[0], point[1]);
        }
    }
    path.close();
    let path = path.finish()?;
    let mut pixels = Pixmap::new(width, height)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    let transform = Transform::from_row(
        -(width as f32) / 9.00001,
        0.0,
        0.0,
        -(height as f32) / 10.0,
        width as f32,
        height as f32,
    );
    pixels.fill_path(&path, &paint, FillRule::Winding, transform, None);
    Some(pixels)
}

pub(super) fn paint(ui: &Ui, painter: &Painter, origin: Pos2) {
    let density = ui.ctx().pixels_per_point();
    let size = [
        (9.0 * density).round().max(1.0) as u32,
        (10.0 * density).round().max(1.0) as u32,
    ];
    let id = egui::Id::new("timeline-cti-artwork");
    let cached = ui
        .ctx()
        .data(|data| data.get_temp::<([u32; 2], TextureHandle)>(id));
    let texture = if let Some((old_size, texture)) = cached
        && old_size == size
    {
        texture
    } else {
        let Some(pixels) = render(size[0], size[1]) else {
            return;
        };
        let texture = ui.ctx().load_texture(
            "timeline-cti",
            egui::ColorImage::from_rgba_premultiplied(
                [size[0] as usize, size[1] as usize],
                pixels.data(),
            ),
            egui::TextureOptions::LINEAR,
        );
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, (size, texture.clone())));
        texture
    };
    let origin = (origin * density).round() / density;
    painter.image(
        texture.id(),
        egui::Rect::from_min_size(origin, egui::vec2(9.0, 10.0)),
        egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
        crate::chrome::FOREGROUND,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supplied_head_keeps_its_open_center_at_each_density() {
        for density in [1, 2, 3] {
            let image = render(9 * density, 10 * density).expect("supplied SVG");
            assert_eq!(
                image
                    .pixel(4 * density, 8 * density)
                    .expect("opening")
                    .alpha(),
                0
            );
            assert!(image.pixel(4 * density, 2 * density).expect("head").alpha() > 200);
        }
    }
}
