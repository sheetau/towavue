use egui::{Mesh, Rect, epaint::Vertex};

/// Clip textured waveform triangles at the timeline's true rounded ends.
/// Interior triangles retain their original vertices and UVs. Only corner
/// intersections need interpolation; no texture download or reraster is needed.
pub(super) fn rounded(mut mesh: Mesh, rect: Rect, start: bool, end: bool) -> Mesh {
    if (!start && !end) || !rect.is_positive() {
        return mesh;
    }
    let radius = (crate::chrome::RADIUS_XS as f32)
        .min(rect.width() * 0.5)
        .min(rect.height() * 0.5);
    let mut planes = Vec::new();
    for (corner, direction, enabled) in [
        (rect.left_top(), egui::vec2(1.0, 1.0), start),
        (rect.right_top(), egui::vec2(-1.0, 1.0), end),
        (rect.right_bottom(), egui::vec2(-1.0, -1.0), end),
        (rect.left_bottom(), egui::vec2(1.0, -1.0), start),
    ] {
        if !enabled {
            continue;
        }
        let center = corner + direction * radius;
        for segment in 0..4 {
            let point = |i: usize| {
                let angle = i as f32 * std::f32::consts::FRAC_PI_2 / 4.0;
                center - egui::vec2(angle.cos() * direction.x, angle.sin() * direction.y) * radius
            };
            let a = point(segment);
            let b = point(segment + 1);
            let normal = egui::vec2(-(b.y - a.y), b.x - a.x);
            let normal = if normal.dot(center - a) < 0.0 {
                -normal
            } else {
                normal
            };
            planes.push((a, normal));
        }
    }
    let indices = std::mem::take(&mut mesh.indices);
    for triangle in indices.as_chunks::<3>().0 {
        let vertices =
            [triangle[0], triangle[1], triangle[2]].map(|index| mesh.vertices[index as usize]);
        if planes
            .iter()
            .all(|(a, normal)| vertices.iter().all(|v| normal.dot(v.pos - *a) >= 0.0))
        {
            mesh.indices.extend_from_slice(triangle);
            continue;
        }
        let mut polygon = vertices.to_vec();
        for (a, normal) in &planes {
            if polygon.is_empty() {
                break;
            }
            let mut clipped = Vec::with_capacity(polygon.len() + 1);
            let mut previous = *polygon.last().expect("nonempty polygon");
            let distance = |point: egui::Pos2| {
                (f64::from(point.x) - f64::from(a.x)) * f64::from(normal.x)
                    + (f64::from(point.y) - f64::from(a.y)) * f64::from(normal.y)
            };
            let mut before = distance(previous.pos);
            for current in polygon {
                let after = distance(current.pos);
                if (before >= 0.0) != (after >= 0.0) {
                    let t = before / (before - after);
                    clipped.push(Vertex {
                        pos: interpolate(previous.pos, current.pos, t),
                        uv: interpolate(previous.uv, current.uv, t),
                        // Waveform meshes use a uniform tint, including gain previews.
                        color: current.color,
                    });
                }
                if after >= 0.0 {
                    clipped.push(current);
                }
                previous = current;
                before = after;
            }
            polygon = clipped;
        }
        let base = mesh.vertices.len() as u32;
        for i in 1..polygon.len().saturating_sub(1) {
            mesh.indices
                .extend_from_slice(&[base, base + i as u32, base + i as u32 + 1]);
        }
        mesh.vertices.extend(polygon);
    }
    mesh
}

fn interpolate(a: egui::Pos2, b: egui::Pos2, t: f64) -> egui::Pos2 {
    egui::pos2(
        (f64::from(a.x) + (f64::from(b.x) - f64::from(a.x)) * t) as f32,
        (f64::from(a.y) + (f64::from(b.y) - f64::from(a.y)) * t) as f32,
    )
}

/// Bound quads before triangulation so their interpolation stays screen-local.
pub(super) fn add_visible_rect(
    mesh: &mut Mesh,
    destination: Rect,
    uv: Rect,
    clip: Rect,
    color: egui::Color32,
) {
    let visible = destination.intersect(clip);
    if !visible.is_positive() {
        return;
    }
    let at = |point: egui::Pos2| {
        egui::pos2(
            (f64::from(uv.left())
                + f64::from(uv.width()) * (f64::from(point.x) - f64::from(destination.left()))
                    / f64::from(destination.width())) as f32,
            (f64::from(uv.top())
                + f64::from(uv.height()) * (f64::from(point.y) - f64::from(destination.top()))
                    / f64::from(destination.height())) as f32,
        )
    };
    mesh.add_rect_with_uv(
        visible,
        Rect::from_min_max(at(visible.min), at(visible.max)),
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Pos2;

    #[test]
    fn corner_clipping_preserves_texture_coordinates_and_square_scrolled_edges() {
        let rect = Rect::from_min_max(Pos2::ZERO, egui::pos2(100.0, 40.0));
        let mut mesh = Mesh::with_texture(egui::TextureId::Managed(4));
        mesh.add_rect_with_uv(
            rect,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        let clipped = rounded(mesh.clone(), rect, true, false);
        assert!(clipped.is_valid());
        for index in &clipped.indices {
            let v = clipped.vertices[*index as usize];
            assert!((v.uv.x - v.pos.x / 100.0).abs() < 1e-5);
            assert!((v.uv.y - v.pos.y / 40.0).abs() < 1e-5);
            assert!(v.pos.distance(rect.left_top()) >= 1.0);
            assert!(v.pos.distance(rect.left_bottom()) >= 1.0);
        }
        assert!(
            clipped
                .indices
                .iter()
                .any(|i| clipped.vertices[*i as usize].pos == rect.right_top())
        );
        assert_eq!(rounded(mesh.clone(), rect, false, false), mesh);
    }
}
