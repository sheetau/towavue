use egui::{Color32, Rect, TextureHandle, TextureOptions, Ui};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Outline,
    FilledRight,
    FilledLeft,
    Right,
    Left,
}

pub(super) fn paint(ui: &Ui, rect: Rect, selected: bool, direction: Option<bool>, color: Color32) {
    let kind = match (selected, direction) {
        (true, Some(true)) => Kind::FilledLeft,
        (true, _) => Kind::FilledRight,
        (false, Some(true)) => Kind::Left,
        (false, Some(false)) => Kind::Right,
        (false, None) => Kind::Outline,
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

// Owner-supplied SVG geometry; normalize the optical bounds in a 24-unit
// canvas. Outline strokes are one logical pixel in the 16-point glyph box.
// See assets/reading-icons/README.md for the retained source artwork.
fn render(kind: Kind, size: u32) -> Option<Pixmap> {
    if matches!(kind, Kind::Left | Kind::FilledLeft) {
        // Mirror final texels so left/right are exactly symmetric at every DPI.
        let source = render(
            if kind == Kind::Left {
                Kind::Right
            } else {
                Kind::FilledRight
            },
            size,
        )?;
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
    let filled = kind == Kind::FilledRight;
    let mut image = Pixmap::new(size, size)?;
    let mut path = PathBuilder::new();
    if filled {
        path.move_to(22.0, 3.0);
        path.line_to(22.0, 15.0);
        path.cubic_to(22.0, 16.66, 20.66, 18.0, 19.0, 18.0);
        path.line_to(15.0, 18.0);
        path.cubic_to(13.85, 18.0, 12.75, 18.5, 12.0, 19.36);
        path.line_to(12.0, 0.81);
        path.cubic_to(12.9, 0.29, 13.94, 0.01, 15.0, 0.01);
        path.line_to(19.0, 0.01);
        path.cubic_to(20.66, 0.0, 22.0, 1.35, 22.0, 3.0);
        path.close();
        path.move_to(7.0, 0.0);
        path.line_to(3.0, 0.0);
        path.cubic_to(1.35, 0.0, 0.0, 1.35, 0.0, 3.0);
        path.line_to(0.0, 15.0);
        path.cubic_to(0.0, 16.66, 1.35, 18.0, 3.0, 18.0);
        path.line_to(7.0, 18.0);
        path.cubic_to(8.15, 18.0, 9.25, 18.5, 10.0, 19.36);
        path.line_to(10.0, 0.81);
        path.cubic_to(9.1, 0.29, 8.06, 0.0, 7.0, 0.0);
        path.close();
        path.move_to(7.21, 9.71);
        path.line_to(4.21, 12.71);
        path.cubic_to(4.01, 12.91, 3.76, 13.0, 3.5, 13.0);
        path.cubic_to(3.24, 13.0, 2.99, 12.9, 2.79, 12.71);
        path.cubic_to(2.4, 12.32, 2.4, 11.69, 2.79, 11.3);
        path.line_to(5.08, 9.01);
        path.line_to(2.79, 6.72);
        path.cubic_to(2.4, 6.33, 2.4, 5.7, 2.79, 5.31);
        path.cubic_to(3.18, 4.92, 3.81, 4.92, 4.2, 5.31);
        path.line_to(7.2, 8.31);
        path.cubic_to(7.59, 8.7, 7.59, 9.33, 7.2, 9.72);
        path.close();
    } else {
        path.move_to(11.0, 3.0);
        path.line_to(11.0, 19.0);
        path.move_to(19.0, 17.0);
        path.cubic_to(20.1, 17.0, 21.0, 16.1, 21.0, 15.0);
        path.line_to(21.0, 3.0);
        path.cubic_to(21.0, 1.9, 20.11, 1.0, 19.0, 1.0);
        path.line_to(15.0, 1.0);
        path.cubic_to(13.43, 1.0, 11.94, 1.74, 11.0, 3.0);
        path.cubic_to(10.06, 1.74, 8.57, 1.0, 7.0, 1.0);
        path.line_to(3.0, 1.0);
        path.cubic_to(1.9, 1.0, 1.0, 1.9, 1.0, 3.0);
        path.line_to(1.0, 15.0);
        path.cubic_to(1.0, 16.1, 1.89, 17.0, 3.0, 17.0);
        path.line_to(7.0, 17.0);
        path.cubic_to(8.57, 17.0, 10.06, 17.74, 11.0, 19.0);
        path.cubic_to(11.94, 17.74, 13.43, 17.0, 15.0, 17.0);
        path.line_to(19.0, 17.0);
        path.close();
        if kind == Kind::Right {
            path.move_to(4.5, 12.0);
            path.line_to(7.5, 9.0);
            path.line_to(4.5, 6.0);
        }
    }
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(255, 255, 255, 255);
    let density = size as f32 / 24.0;
    let (scale, x, y) = if filled {
        (21.5 / 22.0, 1.25, 2.54)
    } else {
        (1.0, 1.0, 2.0)
    };
    let transform = Transform::from_row(
        scale * density,
        0.0,
        0.0,
        scale * density,
        x * density,
        y * density,
    );
    let path = path.finish()?;
    if filled {
        image.fill_path(&path, &paint, FillRule::Winding, transform, None);
    } else {
        image.stroke_path(
            &path,
            &paint,
            &Stroke {
                width: 1.5,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_icons_keep_gutter_round_strokes_and_mirrored_entry_arrows_at_each_density() {
        for size in [16, 20, 32] {
            let images = [
                Kind::Outline,
                Kind::FilledRight,
                Kind::Right,
                Kind::Left,
                Kind::FilledLeft,
            ]
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
                    assert_eq!(&images[1].data()[a..a + 4], &images[4].data()[b..b + 4]);
                }
            }
        }
    }

    #[test]
    fn reading_mode_and_entry_keep_their_cursors_during_hover_and_owned_drag() {
        for density in [1.0, 1.25, 2.0] {
            for (selected, direction) in [
                (false, None),
                (false, Some(true)),
                (false, Some(false)),
                (true, Some(true)),
                (true, Some(false)),
            ] {
                for enabled in [false, true] {
                    let context = crate::fonts::test_context();
                    context.global_style_mut(crate::chrome::style);
                    let mut target = egui::Pos2::ZERO;
                    for step in 0..5 {
                        let pointer = if step < 3 {
                            target
                        } else {
                            target + egui::vec2(80.0, 40.0)
                        };
                        let mut events = if step == 0 {
                            vec![]
                        } else {
                            vec![egui::Event::PointerMoved(pointer)]
                        };
                        if step == 2 || step == 4 {
                            events.push(egui::Event::PointerButton {
                                pos: pointer,
                                button: egui::PointerButton::Primary,
                                pressed: step == 2,
                                modifiers: egui::Modifiers::NONE,
                            });
                        }
                        let mut input = egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(320.0, 200.0),
                            )),
                            events,
                            ..Default::default()
                        };
                        input
                            .viewports
                            .get_mut(&egui::ViewportId::ROOT)
                            .expect("viewport")
                            .native_pixels_per_point = Some(density);
                        let output = context.run_ui(input, |ui| {
                            let response =
                                crate::chrome::reading_button(ui, enabled, selected, direction);
                            target = response.rect.center();
                            if step == 3 {
                                assert_eq!(response.dragged(), enabled);
                            }
                        });
                        if step > 0 {
                            let expected = if !enabled || step == 4 {
                                egui::CursorIcon::Default
                            } else if selected {
                                egui::CursorIcon::Move
                            } else {
                                egui::CursorIcon::ResizeHorizontal
                            };
                            assert_eq!(
                                output.platform_output.cursor_icon, expected,
                                "selected={selected} direction={direction:?} enabled={enabled} step={step} density={density}"
                            );
                        }
                    }
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
                (true, Some(false)),
                (true, Some(true)),
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
