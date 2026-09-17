use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Move,
    New,
    Forbidden,
}

pub(crate) struct Badge {
    window: towavue_runtime_windows::DragBadge,
    image: Option<(Kind, u32)>,
    monitor: Option<(egui::Rect, f32)>,
}

impl Badge {
    pub(crate) fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            window: towavue_runtime_windows::DragBadge::new()?,
            image: None,
            monitor: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn verify_placement(&self, kind: Kind, point: (i32, i32), density: f32) {
        let size = (23.0 * density).round() as u32;
        let offset = (12.0 * density).round() as i32;
        assert_eq!(self.image, Some((kind, size)));
        assert_eq!(
            self.window
                .verification_bounds()
                .expect("native badge bounds"),
            (point.0 + offset, point.1 + offset, size as i32, size as i32)
        );
    }

    pub(crate) fn density_at(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        point: (i32, i32),
    ) -> f32 {
        let contains = |rect: egui::Rect| {
            point.0 as f32 >= rect.left()
                && (point.0 as f32) < rect.right()
                && point.1 as f32 >= rect.top()
                && (point.1 as f32) < rect.bottom()
        };
        if let Some((rect, density)) = self.monitor
            && contains(rect)
        {
            return density;
        }
        self.monitor = event_loop.available_monitors().find_map(|monitor| {
            let origin = monitor.position();
            let size = monitor.size();
            let rect = egui::Rect::from_min_size(
                egui::pos2(origin.x as f32, origin.y as f32),
                egui::vec2(size.width as f32, size.height as f32),
            );
            contains(rect).then_some((rect, monitor.scale_factor() as f32))
        });
        self.monitor.map_or(1.0, |(_, density)| density)
    }

    pub(crate) fn update(
        &mut self,
        kind: Kind,
        point: (i32, i32),
        density: f32,
        visible: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let size = (23.0 * density).round().clamp(1.0, 256.0) as u32;
        if self.image != Some((kind, size)) {
            let pixels = render(kind, size).ok_or("Could not render drag badge")?;
            self.window.set_image(size, pixels.data())?;
            self.image = Some((kind, size));
        }
        let offset = (12.0 * density).round() as i32;
        self.window.move_to(
            (
                point.0.saturating_add(offset),
                point.1.saturating_add(offset),
            ),
            visible,
        )?;
        Ok(())
    }
}

// The reference SVG coordinates and CSS are from Monapad's cursor.html:
// Tabler arrow-right, browser-plus and ban. See assets/drag-badge/README.md.
// Transform geometry before stroking to retain CSS non-scaling-stroke (1 DIP).
fn render(kind: Kind, size: u32) -> Option<Pixmap> {
    let mut image = Pixmap::new(size, size)?;
    let density = size as f32 / 23.0;
    let transform = Transform::from_scale(density, density);
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(255, 255, 255, 191);
    image.fill_path(
        &rounded(0.0, 23.0, 5.0)?,
        &paint,
        FillRule::Winding,
        transform,
        None,
    );
    paint.set_color_rgba8(128, 128, 128, 255);
    let border = Stroke {
        width: 1.0,
        ..Default::default()
    };
    image.stroke_path(&rounded(0.5, 22.5, 4.5)?, &paint, &border, transform, None);
    if kind == Kind::Forbidden {
        paint.set_color_rgba8(255, 0, 0, 255);
    } else {
        paint.set_color_rgba8(0, 0, 0, 255);
    }
    let mut builder = PathBuilder::new();
    match kind {
        Kind::Move => {
            builder.move_to(5.0, 12.0);
            builder.line_to(19.0, 12.0);
            builder.move_to(13.0, 18.0);
            builder.line_to(19.0, 12.0);
            builder.move_to(13.0, 6.0);
            builder.line_to(19.0, 12.0);
        }
        Kind::New => {
            builder.move_to(4.0, 8.0);
            builder.line_to(20.0, 8.0);
            builder.move_to(12.0, 20.0);
            builder.line_to(6.0, 20.0);
            let k = 2.0 * (1.0 - 0.552_284_8);
            builder.cubic_to(4.0 + k, 20.0, 4.0, 20.0 - k, 4.0, 18.0);
            builder.line_to(4.0, 6.0);
            builder.cubic_to(4.0, 4.0 + k, 4.0 + k, 4.0, 6.0, 4.0);
            builder.line_to(18.0, 4.0);
            builder.cubic_to(20.0 - k, 4.0, 20.0, 4.0 + k, 20.0, 6.0);
            builder.line_to(20.0, 12.0);
            builder.move_to(8.0, 4.0);
            builder.line_to(8.0, 8.0);
            builder.move_to(16.0, 19.0);
            builder.line_to(22.0, 19.0);
            builder.move_to(19.0, 16.0);
            builder.line_to(19.0, 22.0);
        }
        Kind::Forbidden => {
            builder.push_circle(12.0, 12.0, 9.0);
            builder.move_to(5.7, 5.7);
            builder.line_to(18.3, 18.3);
        }
    }
    let scale = density * 17.0 / 24.0;
    let path = builder.finish()?.transform(Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        3.0 * density,
        2.5 * density,
    ))?;
    let stroke = Stroke {
        width: density,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    };
    image.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    Some(image)
}

fn rounded(min: f32, max: f32, radius: f32) -> Option<Path> {
    let k = radius * (1.0 - 0.552_284_8);
    let mut path = PathBuilder::new();
    path.move_to(min + radius, min);
    path.line_to(max - radius, min);
    path.cubic_to(max - k, min, max, min + k, max, min + radius);
    path.line_to(max, max - radius);
    path.cubic_to(max, max - k, max - k, max, max - radius, max);
    path.line_to(min + radius, max);
    path.cubic_to(min + k, max, min, max - k, min, max - radius);
    path.line_to(min, min + radius);
    path.cubic_to(min, min + k, min + k, min, min + radius, min);
    path.close();
    path.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_badges_keep_transparent_corners_premultiplication_and_distinct_symbols() {
        for size in [23, 29, 35, 46, 69, 92] {
            let images = [Kind::Move, Kind::New, Kind::Forbidden]
                .map(|kind| render(kind, size).expect("badge"));
            for image in &images {
                assert_eq!(image.data().len(), (size * size * 4) as usize);
                assert_eq!(&image.data()[..4], &[0; 4], "rounded transparent corner");
                assert!(
                    image
                        .data()
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| p[..3].iter().all(|c| *c <= p[3]))
                );
                assert!(
                    image.data().as_chunks::<4>().0.contains(&[191; 4]),
                    "75% white fill"
                );
            }
            assert_ne!(images[0].data(), images[1].data());
            for image in &images[..2] {
                assert!(
                    image
                        .data()
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| p[0] == p[1] && p[1] == p[2])
                );
            }
            assert!(
                images[2]
                    .data()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| u16::from(p[0]) > u16::from(p[1]) + 50),
                "red forbidden symbol"
            );
            assert_eq!(
                render(Kind::New, size).expect("repeat render").data(),
                images[1].data()
            );
        }
    }
}
