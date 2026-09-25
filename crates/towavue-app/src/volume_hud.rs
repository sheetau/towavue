use std::time::{Duration, Instant};

use egui::{Color32, Rect, Ui, pos2, vec2};
use towavue_core::TabId;

const HOLD: Duration = Duration::from_millis(1200);
const FADE: Duration = Duration::from_millis(120);

#[derive(Clone, Copy)]
struct Notice {
    owner: (TabId, u64),
    started: Instant,
    until: Instant,
}

impl Notice {
    fn opacity(self, now: Instant, dragging: bool) -> f32 {
        let enter = (now.saturating_duration_since(self.started).as_secs_f32()
            / FADE.as_secs_f32())
        .min(1.0);
        let exit = if dragging {
            1.0
        } else {
            1.0 - (now.saturating_duration_since(self.until).as_secs_f32() / FADE.as_secs_f32())
                .min(1.0)
        };
        enter.min(exit)
    }
}

#[derive(Default)]
pub struct Hud {
    notice: Option<Notice>,
    dragging: bool,
}

impl Hud {
    pub fn changed(&mut self, tab: TabId, generation: u64, now: Instant) {
        if let Some(notice) = &mut self.notice
            && notice.owner == (tab, generation)
        {
            if now >= notice.until {
                // Renew from the current opacity, even if drawing was suspended.
                notice.started = now - FADE.mul_f32(notice.opacity(now, self.dragging));
            }
            notice.until = now + HOLD;
        } else {
            self.notice = Some(Notice {
                owner: (tab, generation),
                started: now,
                until: now + HOLD,
            });
            self.dragging = false;
        }
    }

    pub fn show(
        &mut self,
        ui: &Ui,
        owner: Option<(TabId, u64)>,
        media: Option<Rect>,
        volume: f32,
        now: Instant,
    ) -> Option<f32> {
        let notice = self.notice?;
        let (tab, generation) = notice.owner;
        if now >= notice.until + FADE && !self.dragging || owner != Some(notice.owner) {
            self.notice = None;
            self.dragging = false;
            return None;
        }
        let interactive = now < notice.until || self.dragging;
        let viewport = ui.max_rect();
        if !viewport.is_positive() {
            return None;
        }
        let (track, _) = geometry(viewport, media, volume);
        let response = interactive.then(|| {
            ui.interact(
                track.expand(5.0).intersect(viewport),
                ui.id().with(("volume-hud", tab, generation)),
                egui::Sense::CLICK | egui::Sense::DRAG,
            )
        });
        let owned = response.as_ref().is_some_and(|response| {
            (response.is_pointer_button_down_on() && ui.input(|input| input.pointer.primary_down()))
                || response.dragged_by(egui::PointerButton::Primary)
        });
        if owned || self.dragging {
            self.changed(tab, generation, now);
        }
        self.dragging = owned;
        let changed = response
            .as_ref()
            .filter(|response| owned || response.clicked_by(egui::PointerButton::Primary))
            .and_then(|response| response.interact_pointer_pos())
            .map(|position| {
                let fraction = if track.width() > track.height() {
                    (position.x - track.left()) / track.width()
                } else {
                    (track.bottom() - position.y) / track.height()
                };
                fraction.clamp(0.0, 1.0) * towavue_core::MAX_VOLUME
            });
        let (_, fill) = geometry(viewport, media, changed.unwrap_or(volume));
        let notice = self.notice.expect("current owner");
        let opacity = notice.opacity(now, self.dragging);
        if opacity < 1.0 || now >= notice.until {
            ui.ctx().request_repaint();
        } else {
            ui.ctx()
                .request_repaint_after(notice.until.saturating_duration_since(now));
        }
        let mut painter = ui.painter_at(viewport);
        painter.multiply_opacity(opacity);
        painter.add(
            egui::epaint::Shadow {
                offset: [0, 0],
                blur: 12,
                spread: 1,
                color: Color32::from_black_alpha(80),
            }
            .as_shape(track, 2),
        );
        painter.rect_filled(
            track,
            1.5,
            crate::chrome::HOVER.gamma_multiply(ui.spacing().scroll.interact_background_opacity),
        );
        if fill.is_positive() {
            painter.rect_filled(fill, 1.5, Color32::WHITE);
        }
        changed
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
    let ratio = (volume / towavue_core::MAX_VOLUME).clamp(0.0, 1.0);
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
    fn visible_hud_clicks_and_drags_follow_both_axes_and_keep_the_deadline_alive() {
        for horizontal in [false, true] {
            let context = crate::fonts::test_context();
            let mut tabs = towavue_core::TabSet::default();
            let tab = tabs.open_new("video.mp4".into(), towavue_core::MediaKind::Video);
            let start = Instant::now();
            let mut hud = Hud::default();
            hud.changed(tab, 1, start);
            let viewport = Rect::from_min_size(egui::Pos2::ZERO, vec2(500.0, 400.0));
            let media = horizontal.then(|| viewport.with_min_y(80.0));
            let mut track = Rect::NOTHING;
            let mut frame = |hud: &mut Hud, elapsed, events| {
                let mut changed = None;
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(viewport),
                        time: Some(elapsed as f64 / 1000.0),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        track = geometry(ui.max_rect(), media, 0.5).0;
                        changed = hud.show(
                            ui,
                            Some((tab, 1)),
                            media,
                            0.5,
                            start + Duration::from_millis(elapsed),
                        );
                    },
                );
                (changed, track)
            };
            let (_, track) = frame(&mut hud, 0, vec![]);
            let event = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let center = track.center();
            let (level, _) = frame(
                &mut hud,
                10,
                vec![egui::Event::PointerMoved(center), event(center, true)],
            );
            assert!((level.expect("press updates volume") - 1.0).abs() < 0.01);
            let maximum = if horizontal {
                track.right_center()
            } else {
                track.center_top()
            };
            let (level, _) = frame(&mut hud, 2000, vec![egui::Event::PointerMoved(maximum)]);
            assert_eq!(level, Some(towavue_core::MAX_VOLUME));
            assert!(
                hud.notice.is_some(),
                "an owned drag outlives the display timer"
            );
            frame(&mut hud, 2100, vec![event(maximum, false)]);
            assert!(!hud.dragging);
            assert!(context.memory(|memory| memory.focused()).is_none());
            frame(&mut hud, 3420, vec![]);
            assert!(hud.notice.is_none());
        }
    }

    #[test]
    fn fades_renew_from_current_opacity_without_restarting_on_continuous_input() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            let mut tabs = towavue_core::TabSet::default();
            let tab = tabs.open_new("video.mp4".into(), towavue_core::MediaKind::Video);
            let start = Instant::now();
            let at = |ms| start + Duration::from_millis(ms);
            let mut hud = Hud::default();
            hud.changed(tab, 1, start);
            let frame = |hud: &mut Hud, ms| {
                let mut fill = Rect::NOTHING;
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(500.0, 400.0),
                        )),
                        time: Some(ms as f64 / 1000.0),
                        ..Default::default()
                    },
                    |ui| {
                        fill = geometry(ui.max_rect(), None, 1.0).1;
                        assert_eq!(hud.show(ui, Some((tab, 1)), None, 1.0, at(ms)), None);
                    },
                );
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) if rect.rect == fill => Some(rect.fill.a()),
                        _ => None,
                    })
                    .unwrap_or(0)
            };
            let near = |actual: u8, expected: u8| {
                assert!(actual.abs_diff(expected) <= 1, "{actual} != {expected}")
            };
            near(frame(&mut hud, 0), 0);
            near(frame(&mut hud, 30), 64);
            hud.changed(tab, 1, at(30));
            near(frame(&mut hud, 60), 128);
            near(frame(&mut hud, 90), 191);
            near(frame(&mut hud, 120), 255);
            near(frame(&mut hud, 1230), 255);
            near(frame(&mut hud, 1290), 128);
            hud.changed(tab, 1, at(1290));
            near(frame(&mut hud, 1290), 128);
            near(frame(&mut hud, 1320), 191);
            near(frame(&mut hud, 1350), 255);
            near(frame(&mut hud, 2490), 255);
            near(frame(&mut hud, 2580), 64);
            near(frame(&mut hud, 2610), 0);
            assert!(hud.notice.is_none());
            hud.changed(tab, 1, at(2700));
            // Drawing can be suspended by an overlay past the original deadline.
            hud.changed(tab, 1, at(5000));
            near(frame(&mut hud, 5000), 0);
            near(frame(&mut hud, 5060), 128);
            near(frame(&mut hud, 5120), 255);
            assert!(context.memory(|memory| memory.focused()).is_none());
        }
    }

    #[test]
    fn fading_out_hud_allows_underlying_primary_input() {
        let context = crate::fonts::test_context();
        let mut tabs = towavue_core::TabSet::default();
        let tab = tabs.open_new("video.mp4".into(), towavue_core::MediaKind::Video);
        let start = Instant::now();
        let mut hud = Hud::default();
        hud.changed(tab, 1, start);
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, vec2(500.0, 400.0));
        let mut position = egui::Pos2::ZERO;
        let mut frame = |elapsed, events| {
            let mut down = false;
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    time: Some(elapsed as f64 / 1000.0),
                    events,
                    ..Default::default()
                },
                |ui| {
                    position = geometry(ui.max_rect(), None, 1.0).0.center();
                    down = ui
                        .interact(
                            ui.max_rect(),
                            ui.id().with("under-hud"),
                            egui::Sense::CLICK | egui::Sense::DRAG,
                        )
                        .is_pointer_button_down_on();
                    assert_eq!(
                        hud.show(
                            ui,
                            Some((tab, 1)),
                            None,
                            1.0,
                            start + Duration::from_millis(elapsed)
                        ),
                        None
                    );
                },
            );
            (down, position)
        };
        frame(120, vec![]);
        let (_, position) = frame(1200, vec![]);
        let (down, _) = frame(
            1260,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(down, "the fading bar must not intercept the media gesture");
        assert!(!hud.dragging);
    }

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
                        );
                    },
                )
            };
            let visible = frame(120, 1);
            assert!(visible.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == Color32::WHITE)));
            assert!(context.memory(|memory| memory.focused()).is_none());
            assert!(!frame(1320, 1).shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == Color32::WHITE)));
            hud.changed(tab, 1, start);
            let _ = context.run_ui(Default::default(), |ui| {
                hud.show(ui, Some((tab, 2)), None, 1.0, start);
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
                    for volume in [0.0, 0.5, 1.0, 1.5, 2.0] {
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
