use egui::{Color32, Painter, Rect, Shape, epaint::Mesh};
use std::sync::Arc;

/// Paint a one-physical-pixel RGB-inverting border in the normal UI draw order.
/// No native device references or source pixels cross the application boundary.
pub fn paint_selection_outline(painter: &Painter, rect: Rect) {
    let mesh = outline_mesh(rect, painter.ctx().pixels_per_point());
    if !mesh.indices.is_empty() {
        painter.add(Shape::Callback(egui::PaintCallback {
            rect,
            callback: Arc::new(egui_directx11::InvertMesh(mesh)),
        }));
    }
}

fn outline_mesh(rect: Rect, pixels_per_point: f32) -> Mesh {
    let mut mesh = Mesh::default();
    if !rect.is_finite() || !rect.is_positive() {
        return mesh;
    }
    let pixel = 1.0;
    let min = (rect.min.to_vec2() * pixels_per_point).round().to_pos2();
    let max = (rect.max.to_vec2() * pixels_per_point)
        .round()
        .to_pos2()
        .max(min + egui::Vec2::splat(1.0));
    let rect = Rect::from_min_max(min, max);
    let mut add = |rect| mesh.add_colored_rect(rect * (1.0 / pixels_per_point), Color32::WHITE);
    // Disjoint strips avoid inverting corner pixels twice. Even tiny selections
    // retain a single pixel, without antialiasing, fill, handles or outside shade.
    let top = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + pixel));
    add(top);
    if rect.height() > pixel {
        add(Rect::from_min_max(
            egui::pos2(rect.left(), rect.bottom() - pixel),
            rect.max,
        ));
    }
    if rect.height() > 2.0 * pixel {
        add(Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + pixel),
            egui::pos2(rect.left() + pixel, rect.bottom() - pixel),
        ));
        if rect.width() > pixel {
            add(Rect::from_min_max(
                egui::pos2(rect.right() - pixel, rect.top() + pixel),
                egui::pos2(rect.right(), rect.bottom() - pixel),
            ));
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_is_one_physical_pixel_without_corner_overlap_at_fractional_dpi() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for size in [
                egui::vec2(0.1, 0.1),
                egui::vec2(1.0, 20.0),
                egui::vec2(40.0, 20.0),
            ] {
                let rect = Rect::from_min_size(egui::pos2(2.3, 3.7), size);
                let mesh = outline_mesh(rect, scale);
                let min = (rect.min.to_vec2() * scale).round();
                let max = (rect.max.to_vec2() * scale)
                    .round()
                    .max(min + egui::Vec2::splat(1.0));
                let mut pixels = std::collections::HashSet::new();
                for quad in mesh.vertices.as_chunks::<4>().0 {
                    let bounds = Rect::from_points(
                        &quad
                            .iter()
                            .map(|vertex| vertex.pos * scale)
                            .collect::<Vec<_>>(),
                    );
                    for y in bounds.top().round() as i32..bounds.bottom().round() as i32 {
                        for x in bounds.left().round() as i32..bounds.right().round() as i32 {
                            assert!(pixels.insert((x, y)), "corner was drawn twice at {scale}");
                            assert!(
                                x == min.x as i32
                                    || x == max.x as i32 - 1
                                    || y == min.y as i32
                                    || y == max.y as i32 - 1
                            );
                        }
                    }
                }
                let width = (max.x - min.x) as usize;
                let height = (max.y - min.y) as usize;
                assert_eq!(
                    pixels.len(),
                    width * height - width.saturating_sub(2) * height.saturating_sub(2)
                );
            }
        }
        assert!(outline_mesh(Rect::NOTHING, 1.0).indices.is_empty());
    }
}
