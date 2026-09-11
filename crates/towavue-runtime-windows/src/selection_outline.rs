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

/// Paint a white 20%-opacity difference fill with opaque one-pixel dashed sides.
/// Only plain geometry crosses into the renderer; image selection is unchanged.
pub fn paint_time_selection(painter: &Painter, rect: Rect) {
    let (fill, sides) = time_selection_meshes(rect, painter.ctx().pixels_per_point());
    if !fill.indices.is_empty() {
        painter.add(Shape::Callback(egui::PaintCallback {
            rect,
            callback: Arc::new(egui_directx11::InvertMesh(fill)),
        }));
        painter.add(sides);
    }
}

fn time_selection_meshes(rect: Rect, pixels_per_point: f32) -> (Mesh, Mesh) {
    let mut fill = Mesh::default();
    let mut sides = Mesh::default();
    if !rect.is_finite() || !rect.is_positive() {
        return (fill, sides);
    }
    let min = (rect.min.to_vec2() * pixels_per_point).round().to_pos2();
    let max = (rect.max.to_vec2() * pixels_per_point)
        .round()
        .to_pos2()
        .max(min + egui::Vec2::splat(1.0));
    let bounds = Rect::from_min_max(min, max);
    fill.add_colored_rect(bounds / pixels_per_point, Color32::from_white_alpha(51));
    let mut y = bounds.top();
    while y < bounds.bottom() {
        let bottom = (y + 2.0).min(bounds.bottom());
        sides.add_colored_rect(
            Rect::from_min_max(
                egui::pos2(bounds.left(), y),
                egui::pos2(bounds.left() + 1.0, bottom),
            ) / pixels_per_point,
            Color32::WHITE,
        );
        if bounds.width() > 1.0 {
            sides.add_colored_rect(
                Rect::from_min_max(
                    egui::pos2(bounds.right() - 1.0, y),
                    egui::pos2(bounds.right(), bottom),
                ) / pixels_per_point,
                Color32::WHITE,
            );
        }
        y += 4.0;
    }
    (fill, sides)
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
    fn time_selection_has_one_fill_and_only_one_pixel_dashed_sides() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for size in [
                egui::vec2(0.1, 0.1),
                egui::vec2(1.0, 20.0),
                egui::vec2(40.0, 21.0),
            ] {
                let rect = Rect::from_min_size(egui::pos2(2.3, 3.7), size);
                let (fill, sides) = time_selection_meshes(rect, scale);
                let min = (rect.min.to_vec2() * scale).round();
                let max = (rect.max.to_vec2() * scale)
                    .round()
                    .max(min + egui::Vec2::splat(1.0));
                assert_eq!(fill.vertices.len(), 4);
                assert_eq!(fill.indices.len(), 6);
                assert!(
                    fill.vertices
                        .iter()
                        .all(|vertex| vertex.color == Color32::from_white_alpha(51))
                );
                let bounds = fill.calc_bounds() * scale;
                assert!((bounds.min.to_vec2() - min).length() < 0.001);
                assert!((bounds.max.to_vec2() - max).length() < 0.001);
                let mut pixels = std::collections::HashSet::new();
                for quad in sides.vertices.as_chunks::<4>().0 {
                    assert!(quad.iter().all(|vertex| vertex.color == Color32::WHITE));
                    let bounds = Rect::from_points(
                        &quad
                            .iter()
                            .map(|vertex| vertex.pos * scale)
                            .collect::<Vec<_>>(),
                    );
                    assert!((bounds.width() - 1.0).abs() < 0.001);
                    assert!(bounds.height() <= 2.001);
                    for y in bounds.top().round() as i32..bounds.bottom().round() as i32 {
                        for x in bounds.left().round() as i32..bounds.right().round() as i32 {
                            assert!(
                                pixels.insert((x, y)),
                                "no double-drawn sides on tiny selections"
                            );
                        }
                    }
                }
                for y in min.y as i32..max.y as i32 {
                    for x in min.x as i32..max.x as i32 {
                        assert_eq!(
                            pixels.contains(&(x, y)),
                            (x == min.x as i32 || x == max.x as i32 - 1)
                                && (y - min.y as i32) % 4 < 2,
                            "only dashed side pixels, without a top/bottom border"
                        );
                    }
                }
            }
        }
        for rect in [
            Rect::NOTHING,
            Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::ZERO),
        ] {
            let (fill, sides) = time_selection_meshes(rect, 1.0);
            assert!(fill.indices.is_empty() && sides.indices.is_empty());
        }
    }

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
