use std::time::{Duration, Instant};

use egui::{Color32, Rect, Ui, pos2, vec2};
use towavue_core::TabId;

#[derive(Default)]
pub struct Hud {
    notice: Option<(TabId, u64, Instant)>,
}

impl Hud {
    pub fn changed(&mut self, tab: TabId, generation: u64, now: Instant) {
        self.notice = Some((tab, generation, now + Duration::from_millis(1200)));
    }

    pub fn show(
        &mut self,
        ui: &Ui,
        owner: Option<(TabId, u64)>,
        media: Option<Rect>,
        volume: f32,
        now: Instant,
    ) {
        let Some((tab, generation, until)) = self.notice else {
            return;
        };
        if now >= until || owner != Some((tab, generation)) {
            self.notice = None;
            return;
        }
        ui.ctx().request_repaint_after(until - now);
        let viewport = ui.max_rect();
        if !viewport.is_positive() {
            return;
        }
        let (track, fill) = geometry(viewport, media, volume);
        let painter = ui.painter_at(viewport);
        painter.rect_filled(track, 1.5, Color32::from_gray(76));
        if fill.is_positive() {
            painter.rect_filled(fill, 1.5, Color32::WHITE);
        }
    }
}

fn geometry(viewport: Rect, media: Option<Rect>, volume: f32) -> (Rect, Rect) {
    let inner = viewport.shrink(8.0_f32.min(viewport.width().min(viewport.height()) / 4.0));
    let top = media.is_some_and(|media| {
        media.top() >= viewport.top() + 24.0 && media.left() < viewport.left() + 24.0
    });
    let length = if top { inner.width() } else { inner.height() }.min(244.0);
    let thickness = 3.0_f32.min(inner.width().min(inner.height()));
    let track = if top {
        Rect::from_min_size(
            pos2(inner.center().x - length / 2.0, inner.top()),
            vec2(length, thickness),
        )
    } else {
        Rect::from_min_size(
            pos2(inner.left(), inner.center().y - length / 2.0),
            vec2(thickness, length),
        )
    };
    let mut fill = track;
    let ratio = (volume / 2.0).clamp(0.0, 1.0);
    if top {
        fill.max.x = egui::lerp(track.x_range(), ratio);
    } else {
        fill.min.y = egui::lerp(track.y_range(), 1.0 - ratio);
    }
    (track, fill)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_fits_letterboxes_and_expires_without_input_or_focus() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            let mut tabs = towavue_core::TabSet::default();
            let tab = tabs.open_new("video.mp4".into(), towavue_core::MediaKind::Video);
            let start = Instant::now();
            let mut hud = Hud::default();
            hud.changed(tab, 1, start);
            let mut frame = |elapsed, generation| {
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(500.0, 400.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        hud.show(
                            ui,
                            Some((tab, generation)),
                            None,
                            1.0,
                            start + Duration::from_millis(elapsed),
                        )
                    },
                )
            };
            let visible = frame(0, 1);
            assert!(visible.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == Color32::WHITE)));
            assert!(context.memory(|memory| memory.focused()).is_none());
            assert!(!frame(1200, 1).shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == Color32::WHITE)));
            hud.changed(tab, 1, start);
            let _ = context.run_ui(Default::default(), |ui| {
                hud.show(ui, Some((tab, 2)), None, 1.0, start)
            });
            assert!(
                hud.notice.is_none(),
                "a different source must not inherit the notice"
            );
            for size in [vec2(500.0, 400.0), vec2(8.0, 8.0)] {
                let viewport = Rect::from_min_size(pos2(30.0, 40.0), size);
                for (media, horizontal) in [
                    (None, false),
                    (
                        Some(Rect::from_min_max(
                            viewport.min + vec2(0.0, size.y / 5.0),
                            viewport.max,
                        )),
                        size.y > 120.0,
                    ),
                ] {
                    for volume in [0.0, 0.5, 1.0, 2.0] {
                        let (track, fill) = geometry(viewport, media, volume);
                        assert!(viewport.contains_rect(track));
                        assert!(track.contains_rect(fill));
                        let fraction = if horizontal {
                            fill.width() / track.width()
                        } else {
                            fill.height() / track.height()
                        };
                        assert!((fraction - volume / 2.0).abs() < 0.001);
                        if size.y > 50.0 {
                            assert_eq!(track.width() > track.height(), horizontal);
                        }
                    }
                }
            }
        }
    }
}
