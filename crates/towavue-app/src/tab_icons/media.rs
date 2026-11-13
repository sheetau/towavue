use super::*;
use egui::{TextureHandle, TextureOptions};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub(super) fn color(kind: Kind) -> Option<Color32> {
    Some(match kind {
        Kind::Image => Color32::from_rgb(0x14, 0x7e, 0xff),
        Kind::Video => Color32::from_rgb(0x7b, 0x3d, 0xff),
        Kind::Audio => Color32::from_rgb(0xff, 0x3d, 0x4d),
        _ => return None,
    })
}

pub(super) fn paint(ui: &Ui, rect: Rect, kind: Kind, opacity: u8) -> bool {
    let Some(color) = color(kind) else {
        return false;
    };
    let density = ui.ctx().pixels_per_point();
    let size = (16.0 * density).round().clamp(1.0, 256.0) as u32;
    let id = egui::Id::new(("media-kind-icon", kind));
    let cached = ui
        .ctx()
        .data(|data| data.get_temp::<(u32, TextureHandle)>(id));
    let texture = if let Some((old_size, texture)) = cached
        && old_size == size
    {
        texture
    } else {
        let Some(pixels) = render(kind, size) else {
            return true;
        };
        let texture = ui.ctx().load_texture(
            "media-kind-icon",
            egui::ColorImage::from_rgba_premultiplied([size as usize; 2], pixels.data()),
            TextureOptions::LINEAR,
        );
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, (size, texture.clone())));
        texture
    };
    let origin = (rect.center() * density - egui::Vec2::splat(size as f32 * 0.5)).round() / density;
    let bounds = Rect::from_min_size(origin, egui::Vec2::splat(size as f32 / density));
    let color = color.gamma_multiply(f32::from(opacity) / 255.0);
    ui.painter()
        .rect_filled(bounds, 3, color.gamma_multiply(0.15));
    ui.painter().image(
        texture.id(),
        bounds,
        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        color,
    );
    true
}

// Owner-supplied 10-unit artwork, centered without scaling in a 16-point box.
// Retained SVGs in assets/media-icons are the source for these native paths.
fn render(kind: Kind, size: u32) -> Option<Pixmap> {
    let mut image = Pixmap::new(size, size)?;
    let mut path = PathBuilder::new();
    match kind {
        Kind::Image => {
            path.move_to(0.5, 9.5);
            path.line_to(3.28, 6.72);
            path.cubic_to(4.23, 5.77, 5.77, 5.77, 6.72, 6.72);
            path.line_to(9.5, 9.5);
        }
        Kind::Video => {
            path.move_to(2.91, 0.87);
            path.line_to(8.12, 4.19);
            path.cubic_to(8.72, 4.57, 8.72, 5.44, 8.12, 5.82);
            path.line_to(2.91, 9.14);
            path.cubic_to(2.27, 9.55, 1.42, 9.09, 1.42, 8.32);
            path.line_to(1.42, 1.68);
            path.cubic_to(1.42, 0.92, 2.26, 0.45, 2.91, 0.86);
            path.close();
        }
        Kind::Audio => {
            path.move_to(5.0, 7.72);
            path.line_to(5.0, 1.18);
            path.cubic_to(5.0, 0.84, 5.35, 0.61, 5.66, 0.75);
            path.line_to(8.14, 1.83);
            path.push_circle(3.43, 7.72, 1.57);
        }
        _ => return None,
    }
    let transform =
        Transform::from_scale(size as f32 / 16.0, size as f32 / 16.0).pre_translate(3.0, 3.0);
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    image.stroke_path(
        &path.finish()?,
        &paint,
        &Stroke {
            width: 1.0,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        },
        transform,
        None,
    );
    if kind == Kind::Image {
        let circle = PathBuilder::from_circle(1.08, 3.07, 1.08)?;
        image.fill_path(&circle, &paint, FillRule::Winding, transform, None);
    }
    Some(image)
}
