use egui::{Color32, Rect, TextureHandle, TextureOptions, Ui};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Outline,
    Filled,
    Right,
    Left,
}

pub(super) fn paint(ui: &Ui, rect: Rect, selected: bool, direction: Option<bool>, color: Color32) {
    let kind = match direction {
        Some(true) => Kind::Left,
        Some(false) => Kind::Right,
        None if selected => Kind::Filled,
        None => Kind::Outline,
    };
    let density = ui.ctx().pixels_per_point();
    let size = (16.0 * density).round().clamp(1.0, 256.0) as u32;
    // One entry per button/context. DPI and state changes replace the old texture.
    let id = ui.id().with("reading-icon");
    let cached = ui
        .ctx()
        .data(|data| data.get_temp::<(Kind, u32, TextureHandle)>(id));
    let texture = if let Some((old_kind, old_size, texture)) = cached
        && (old_kind, old_size) == (kind, size)
    {
        texture
    } else {
        let Some(pixels) = render(kind, size) else {
            return;
        };
        let texture = ui.ctx().load_texture(
            "reading-icon",
            egui::ColorImage::from_rgba_premultiplied([size as usize; 2], pixels.data()),
            TextureOptions::LINEAR,
        );
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, (kind, size, texture.clone())));
        texture
    };
    // Place each raster texel on a physical pixel instead of filtering it again
    // at a fractional origin after an otherwise correct DPI-sized rasterization.
    let origin = (rect.center() * density - egui::Vec2::splat(size as f32 * 0.5)).round() / density;
    let bounds = Rect::from_min_size(origin, egui::Vec2::splat(size as f32 / density));
    ui.painter().image(
        texture.id(),
        bounds,
        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        color,
    );
}

// Literal reference geometry, in its original 24-unit view box. The outline and
// direction paths come from pfp_cropper; the filled path is Tabler's book icon.
// See assets/reading-icons/README.md and the retained SVG/license sources.
fn render(kind: Kind, size: u32) -> Option<Pixmap> {
    if kind == Kind::Left {
        // Mirror the finished pixels to keep the two previews exactly symmetric.
        let source = render(Kind::Right, size)?;
        let mut result = Pixmap::new(size, size)?;
        for y in 0..size as usize {
            for x in 0..size as usize {
                let from = (y * size as usize + x) * 4;
                let to = (y * size as usize + size as usize - 1 - x) * 4;
                result.data_mut()[to..to + 4].copy_from_slice(&source.data()[from..from + 4]);
            }
        }
        return Some(result);
    }
    let mut image = Pixmap::new(size, size)?;
    let mut path = PathBuilder::new();
    if kind == Kind::Filled {
        path.move_to(21.5, 5.134);
        arc(&mut path, [21.5, 5.134], [21.993, 5.882], 1.0, true);
        path.line_to(22.0, 6.0);
        path.line_to(22.0, 19.0);
        arc(&mut path, [22.0, 19.0], [20.5, 19.866], 1.0, true);
        arc(&mut path, [20.5, 19.866], [13.0, 19.6], 8.0, false);
        path.line_to(13.0, 4.426);
        arc(&mut path, [13.0, 4.426], [21.5, 5.134], 10.0, true);
        path.close();
        path.move_to(11.0, 4.427);
        path.line_to(11.001, 19.601);
        arc(&mut path, [11.001, 19.601], [3.767, 19.718], 8.0, false);
        let mut point = [3.767, 19.718];
        for delta in [
            [-0.327, 0.18],
            [-0.103, 0.044],
            [-0.049, 0.016],
            [-0.11, 0.026],
            [-0.061, 0.01],
            [-0.117, 0.006],
            [-0.042, 0.0],
            [-0.11, -0.012],
            [-0.077, -0.014],
            [-0.108, -0.032],
            [-0.126, -0.056],
            [-0.095, -0.056],
            [-0.089, -0.067],
            [-0.06, -0.056],
            [-0.073, -0.082],
            [-0.064, -0.089],
            [-0.022, -0.036],
            [-0.032, -0.06],
            [-0.044, -0.103],
            [-0.016, -0.049],
            [-0.026, -0.11],
            [-0.01, -0.061],
            [-0.004, -0.049],
            [-0.002, -13.068],
        ] {
            point = [point[0] + delta[0], point[1] + delta[1]];
            path.line_to(point[0], point[1]);
        }
        let next = [point[0] + 0.5, point[1] - 0.866];
        arc(&mut path, point, next, 1.0, true);
        arc(
            &mut path,
            next,
            [next[0] + 8.5, next[1] - 0.707],
            10.0,
            true,
        );
        path.close();
    } else {
        for y in [19.0, 6.0] {
            path.move_to(3.0, y);
            if kind == Kind::Outline {
                arc(&mut path, [3.0, y], [12.0, y], 9.0, true);
                arc(&mut path, [12.0, y], [21.0, y], 9.0, true);
            } else {
                path.cubic_to(5.78, y - 1.61, 9.22, y - 1.61, 12.0, y);
                if y == 6.0 {
                    path.cubic_to(14.78, y - 1.61, 18.22, y - 1.61, 21.0, y);
                }
            }
        }
        for x in [3.0, 12.0, 21.0] {
            path.move_to(x, 6.0);
            path.line_to(
                x,
                if kind == Kind::Right && x == 21.0 {
                    12.5
                } else {
                    19.0
                },
            );
        }
        if kind == Kind::Right {
            path.move_to(18.0, 20.83);
            path.line_to(21.0, 17.83);
            path.line_to(18.0, 14.83);
            path.move_to(21.0, 17.83);
            path.line_to(15.53, 17.83);
        }
    }
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(255, 255, 255, 255);
    let transform = Transform::from_scale(size as f32 / 24.0, size as f32 / 24.0);
    let path = path.finish()?;
    if kind == Kind::Filled {
        image.fill_path(&path, &paint, FillRule::Winding, transform, None);
    } else {
        image.stroke_path(
            &path,
            &paint,
            &Stroke {
                width: 2.0,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Default::default()
            },
            transform,
            None,
        );
    }
    Some(image)
}

// These SVGs use only circular, small arcs. Subdivide at 90 degrees and retain
// exact endpoints; cubic approximation error is far below one output pixel.
fn arc(path: &mut PathBuilder, from: [f32; 2], to: [f32; 2], radius: f32, sweep: bool) {
    let from = egui::vec2(from[0], from[1]);
    let to = egui::vec2(to[0], to[1]);
    let chord = to - from;
    let height = (radius * radius - chord.length_sq() * 0.25).max(0.0).sqrt();
    let center = (from + to) * 0.5
        + egui::vec2(-chord.y, chord.x).normalized() * height * if sweep { 1.0 } else { -1.0 };
    let start = (from - center).angle();
    let end = (to - center).angle();
    let delta = if sweep {
        (end - start).rem_euclid(std::f32::consts::TAU)
    } else {
        -(start - end).rem_euclid(std::f32::consts::TAU)
    };
    let count = (delta.abs() / std::f32::consts::FRAC_PI_2).ceil() as usize;
    let step = delta / count as f32;
    let k = 4.0 / 3.0 * (step * 0.25).tan();
    for index in 0..count {
        let a = start + step * index as f32;
        let b = a + step;
        let p = center + egui::Vec2::angled(a) * radius;
        let q = if index + 1 == count {
            to
        } else {
            center + egui::Vec2::angled(b) * radius
        };
        let c1 = p + egui::vec2(-a.sin(), a.cos()) * (radius * k);
        let c2 = q - egui::vec2(-b.sin(), b.cos()) * (radius * k);
        path.cubic_to(c1.x, c1.y, c2.x, c2.y, q.x, q.y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_icons_keep_gutter_round_strokes_and_mirrored_entry_arrows_at_each_density() {
        for size in [16, 20, 32] {
            let images = [Kind::Outline, Kind::Filled, Kind::Right, Kind::Left]
                .map(|kind| render(kind, size).expect("icon"));
            let alpha_sum = |image: &Pixmap| {
                image
                    .data()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| u32::from(p[3]))
                    .sum::<u32>()
            };
            assert!(alpha_sum(&images[1]) > alpha_sum(&images[0]) * 3 / 2);
            assert_ne!(images[0].data(), images[2].data());
            for image in &images {
                assert!(
                    image
                        .data()
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|p| (1..255).contains(&p[3]))
                );
                assert!(image.data()[..size as usize * 4].iter().all(|b| *b == 0));
            }
            let middle = ((size / 2 * size + size / 2) * 4 + 3) as usize;
            assert!(
                images[1].data()[middle] < images[1].data()[middle - 8],
                "filled book retains its center gutter"
            );
            assert!(images[0].data()[middle] > 0, "outline retains its spine");
            if let Some(directory) = std::env::var_os("TOWAVUE_READING_ICON_ARTIFACTS") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).expect("owned artifact directory");
                for (index, image) in images.iter().enumerate() {
                    std::fs::write(directory.join(format!("{size}-{index}.rgba")), image.data())
                        .expect("icon pixels");
                }
            }
            for y in 0..size {
                for x in 0..size {
                    let a = ((y * size + x) * 4) as usize;
                    let b = ((y * size + size - 1 - x) * 4) as usize;
                    assert_eq!(&images[2].data()[a..a + 4], &images[3].data()[b..b + 4]);
                }
            }
        }
    }

    #[test]
    fn button_caches_physical_artwork_and_keeps_hit_geometry_across_states() {
        let context = egui::Context::default();
        for density in [1.0, 1.25, 2.0] {
            context.set_pixels_per_point(density);
            for (selected, direction) in [
                (false, None),
                (false, Some(false)),
                (false, Some(true)),
                (true, None),
            ] {
                let mut ids = Vec::new();
                for enabled in [true, true, false] {
                    let output = context.run_ui(Default::default(), |ui| {
                        let response =
                            crate::chrome::reading_button(ui, enabled, selected, direction);
                        assert_eq!(response.rect.size(), egui::Vec2::splat(24.0));
                        assert_eq!(response.enabled(), enabled);
                    });
                    let mesh = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh)
                                if mesh.texture_id != egui::TextureId::default() =>
                            {
                                Some(mesh)
                            }
                            _ => None,
                        })
                        .expect("reading icon");
                    assert_eq!(mesh.calc_bounds().size(), egui::Vec2::splat(16.0));
                    let physical = mesh.calc_bounds().min * density;
                    assert!(physical.distance(physical.round()) < 0.001);
                    ids.push(mesh.texture_id);
                }
                assert_eq!(ids[0], ids[1]);
                assert_eq!(
                    ids[1], ids[2],
                    "disabled state tints the same cached pixels"
                );
            }
        }
    }
}
