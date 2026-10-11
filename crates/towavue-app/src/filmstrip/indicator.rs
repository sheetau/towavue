use super::*;

pub(super) const GAP: f32 = 10.0;
pub(super) const TEXT_GAP: f32 = 25.0;
pub(super) const TEXT_WIDTH: f32 = layout::DIAMETER * 1.5;

/// One rectangle in content coordinates, independent of keyboard ownership.
/// Scrolling translates it with the thumbnails; changing selection interpolates
/// both position and aspect ratio without retaining an outgoing indicator.
#[derive(Default)]
pub(super) struct Indicator {
    rect: Option<Rect>,
    time: Option<f64>,
    viewport: Option<Vec2>,
    snap_frame: Option<u64>,
}

impl Indicator {
    pub fn resume(&mut self) {
        self.time = None;
        self.snap_frame = None;
    }

    pub fn paint(&mut self, ui: &egui::Ui, target: Rect, origin: egui::Pos2) -> Rect {
        let time = ui.input(|input| input.time);
        let elapsed = self
            .time
            .map_or(0.0, |last| (time - last).clamp(0.0, 1.0 / 30.0));
        self.time = Some(time);
        let target = target.translate(-origin.to_vec2()).expand(GAP);
        let frame = ui.ctx().cumulative_frame_nr();
        if self.rect.is_none() || self.viewport != Some(ui.clip_rect().size()) {
            self.viewport = Some(ui.clip_rect().size());
            self.snap_frame = Some(frame);
        }
        let mut rect = self.rect.unwrap_or(target);
        let blend = (1.0 - (-elapsed / 0.055).exp()) as f32;
        rect.min += (target.min - rect.min) * blend;
        rect.max += (target.max - rect.max) * blend;
        if self.snap_frame == Some(frame) {
            rect = target;
        }
        if rect
            .min
            .distance(target.min)
            .max(rect.max.distance(target.max))
            < 0.5 / ui.ctx().pixels_per_point()
        {
            rect = target;
        } else {
            ui.ctx().request_repaint();
        }
        self.rect = Some(rect);
        let rect = rect.translate(origin.to_vec2());
        for (corner, direction) in [
            (rect.left_top(), egui::vec2(1.0, 1.0)),
            (rect.right_top(), egui::vec2(-1.0, 1.0)),
            (rect.left_bottom(), egui::vec2(1.0, -1.0)),
            (rect.right_bottom(), egui::vec2(-1.0, -1.0)),
        ] {
            ui.painter().add(egui::Shape::line(
                vec![
                    corner + egui::vec2(direction.x * 5.0, 0.0),
                    corner,
                    corner + egui::vec2(0.0, direction.y * 5.0),
                ],
                egui::Stroke::new(1.0, crate::chrome::FOREGROUND),
            ));
        }
        rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_view_restores_its_indicator_without_interpolating_from_another_tab() {
        let Some(root) = crate::tests::isolated_test_root(
            "filmstrip::indicator::tests::tab_view_restores_its_indicator_without_interpolating_from_another_tab",
        ) else {
            return;
        };
        let context = crate::fonts::test_context();
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                .expect("strip");
        let a = Rect::from_min_size(egui::pos2(50.0, 80.0), egui::vec2(120.0, 60.0));
        let b = Rect::from_min_size(egui::pos2(380.0, 60.0), egui::vec2(60.0, 120.0));
        let paint = |strip: &mut Filmstrip, target, time| {
            let mut drawn = Rect::NOTHING;
            let _ = context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 480.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    drawn = strip.indicator.paint(ui, target, egui::Pos2::ZERO);
                },
            );
            drawn
        };
        let before = paint(&mut strip, a, 1.0);
        let view = strip.take_view();
        paint(&mut strip, b, 2.0);
        strip.restore_view(view);
        assert_eq!(paint(&mut strip, a, 30.0), before);
        assert_eq!(paint(&mut strip, a, 30.01), before);
    }

    #[test]
    fn selection_brackets_interpolate_and_survive_blank_clicks_window_switches_and_pages() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            let root =
                std::env::temp_dir().join(format!("towavue-indicator-{}", std::process::id()));
            let snapshot = FolderSnapshot {
                folder_identity: towavue_core::ShellIdentity::new(vec![]),
                folder_path: root.clone(),
                items: (0..30)
                    .map(|index| towavue_core::FolderMediaItem {
                        identity: towavue_core::ShellIdentity::new(vec![]),
                        path: root.join(format!("{index}.png")),
                        kind: MediaKind::Image,
                    })
                    .collect(),
                sort_columns: vec![],
                source: towavue_core::FolderSnapshotSource::NaturalNameFallback,
                generation: 1,
                captured_at: std::time::SystemTime::now(),
            };
            let mut strip = Filmstrip::new(
                PreviewCache::new(root.join("cache")).expect("test value"),
                || {},
            )
            .expect("test value");
            for (index, size) in [[160, 80], [80, 160]].into_iter().enumerate() {
                let texture = context.load_texture(
                    "aspect",
                    egui::ColorImage::filled(size, Color32::WHITE),
                    Default::default(),
                );
                strip
                    .previews
                    .insert(snapshot.items[index].path.clone(), Ok((texture, None)));
            }
            let mut time = 0.0;
            let mut frame = |strip: &mut Filmstrip, events, focused| {
                time += 1.0 / 60.0;
                let output = context.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        events,
                        focused,
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(640.0, 480.0),
                        )),
                        ..Default::default()
                    },
                    |_| {
                        let mut actions = Vec::new();
                        strip.show(
                            &context,
                            context.content_rect(),
                            Some(&snapshot),
                            Some(&snapshot.items[0].path),
                            true,
                            &mut actions,
                        );
                        assert!(actions.is_empty());
                        if context.current_pass_index() == 0 {
                            context.request_discard("indicator must advance once per frame");
                        }
                    },
                );
                let corners: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Path(path)
                            if path.points.len() == 3 && path.stroke.width == 1.0 =>
                        {
                            Some(path)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(corners.len(), 4, "one persistent set of four corners");
                for corner in corners {
                    for pair in corner.points.windows(2) {
                        assert!((pair[0].distance(pair[1]) - 5.0).abs() < 0.01);
                    }
                }
                output
            };
            for _ in 0..3 {
                frame(&mut strip, vec![], true);
            }
            let old = strip.indicator.rect.expect("test value");
            let target = strip
                .card_paths
                .iter()
                .find(|(_, _, index)| *index == 1)
                .expect("test value")
                .0;
            let bounds = context.read_response(target).expect("test value").rect;
            frame(
                &mut strip,
                vec![egui::Event::PointerMoved(bounds.center())],
                true,
            );
            assert_eq!(strip.selection.as_ref(), Some(&snapshot.items[1].path));
            frame(&mut strip, vec![], true);
            let moving = strip.indicator.rect.expect("test value");
            assert!(
                moving.center().x > old.center().x && moving.center().x < old.center().x + 130.0,
                "{old:?} -> {moving:?}"
            );
            assert!(moving.width() < old.width() && moving.height() > old.height());
            for _ in 0..40 {
                frame(&mut strip, vec![], true);
            }
            assert_eq!(
                context
                    .read_response(target)
                    .expect("test value")
                    .rect
                    .size(),
                bounds.size(),
                "no thumbnail zoom"
            );
            let selected = strip.selection.clone();
            let offset = strip.scroll_offset;
            let blank = egui::pos2(20.0, 20.0);
            for pressed in [true, false] {
                frame(
                    &mut strip,
                    vec![egui::Event::PointerButton {
                        pos: blank,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    true,
                );
            }
            frame(&mut strip, vec![egui::Event::WindowFocused(false)], false);
            frame(
                &mut strip,
                vec![
                    egui::Event::WindowFocused(true),
                    egui::Event::PointerMoved(egui::pos2(320.0, 220.0)),
                ],
                true,
            );
            for _ in 0..3 {
                frame(&mut strip, vec![], true);
            }
            assert_eq!(strip.selection, selected);
            assert_eq!(strip.scroll_offset, offset);
            let page = |key| egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            };
            frame(&mut strip, vec![page(egui::Key::PageDown)], true);
            assert!(
                strip.scroll_offset > offset && strip.scroll_offset < offset + 624.0,
                "page scroll interpolates"
            );
            for _ in 0..45 {
                frame(&mut strip, vec![], true);
            }
            assert!(
                (strip.scroll_offset - offset - 624.0).abs() < 0.01,
                "one viewport, once across discarded passes"
            );
            assert_eq!(strip.selection, selected);
            strip.focus_current();
            frame(&mut strip, vec![], true);
            assert_eq!(strip.scroll_offset, 0.0);
            assert_eq!(strip.selection.as_ref(), Some(&snapshot.items[0].path));
        }
    }
}
