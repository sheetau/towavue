use crate::chrome;
use egui::{Pos2, Rect, Ui};
use towavue_core::{MediaTime, TimeRange};

const RULER_HEIGHT: f32 = 16.0;
const BAR_HEIGHT: f32 = 9.0;
const GAP: f32 = 5.0;

/// Edited-time viewport, independent of playback and waveform requests.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct View {
    pub start: f64,
    pub span: f64,
    duration: f64,
}

impl View {
    fn fit(&mut self, duration: f64, minimum: f64) {
        let was_full = self.span <= 0.0 || self.span >= self.duration;
        self.duration = duration.max(f64::EPSILON);
        if was_full {
            self.start = 0.0;
            self.span = self.duration;
        }
        self.clamp(minimum);
    }

    fn clamp(&mut self, minimum: f64) {
        self.span = self.span.clamp(minimum.min(self.duration), self.duration);
        self.start = self.start.clamp(0.0, self.duration - self.span);
    }

    fn zoom(&mut self, factor: f64, anchor: f64, minimum: f64) {
        let fraction = anchor.clamp(0.0, 1.0);
        let time = self.start + fraction * self.span;
        self.span = (self.span * factor).clamp(minimum.min(self.duration), self.duration);
        self.start = time - fraction * self.span;
        self.clamp(minimum);
    }

    pub fn full_rect(self, visible: Rect) -> Rect {
        Rect::from_min_size(
            Pos2::new(
                visible.left() - (self.start / self.span) as f32 * visible.width(),
                visible.top(),
            ),
            egui::vec2(
                (self.duration / self.span) as f32 * visible.width(),
                visible.height(),
            ),
        )
    }

    pub fn x(self, rect: Rect, seconds: f64) -> f32 {
        rect.left() + ((seconds - self.start) / self.span) as f32 * rect.width()
    }
}

#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub waveform: Rect,
    pub ruler: Rect,
    pub scrollbar: Rect,
    pub view: View,
}

impl Layout {
    #[cfg(test)]
    pub fn plain(rect: Rect, duration: MediaTime) -> Self {
        Self {
            waveform: rect,
            ruler: Rect::NOTHING,
            scrollbar: Rect::NOTHING,
            view: View {
                start: 0.0,
                span: duration.as_seconds_f64().max(f64::EPSILON),
                duration: duration.as_seconds_f64().max(f64::EPSILON),
            },
        }
    }
    pub fn head_top(self) -> f32 {
        if self.ruler.is_positive() {
            self.ruler.bottom() - 10.0
        } else {
            self.waveform.top()
        }
    }
    pub fn interaction(self) -> Rect {
        if self.ruler.is_positive() {
            self.ruler.union(self.waveform)
        } else {
            self.waveform
        }
    }
}

#[derive(Clone, Copy, Default)]
struct BarDrag {
    origin: f32,
    view: View,
    original: View,
    edge: i8,
}

pub(super) fn show(
    ui: &Ui,
    id: egui::Id,
    rect: Rect,
    duration: MediaTime,
    fps: f64,
    enabled: bool,
) -> Layout {
    let ruler = rect.with_max_y(rect.top() + RULER_HEIGHT.min(rect.height()));
    let scrollbar = rect.with_min_y((rect.bottom() - BAR_HEIGHT).max(ruler.bottom()));
    let waveform = Rect::from_min_max(
        egui::pos2(rect.left(), (ruler.bottom() + GAP).min(scrollbar.top())),
        egui::pos2(
            rect.right(),
            (scrollbar.top() - GAP).max((ruler.bottom() + GAP).min(scrollbar.top())),
        ),
    );
    let minimum = f64::from(rect.width().max(1.0)) / (fps.max(1.0) * 30.0);
    let key = id.with("time-viewport");
    let mut view = ui
        .ctx()
        .data(|data| data.get_temp::<View>(key))
        .unwrap_or_default();
    view.fit(duration.as_seconds_f64(), minimum);
    let can_navigate = enabled
        && !egui::Popup::is_any_open(ui.ctx())
        && !crate::timeline_input::is_active(ui.ctx());
    let wheel_key = key.with("wheel-frame");
    let frame = ui.ctx().cumulative_frame_nr();
    if can_navigate && ui.ctx().data(|data| data.get_temp::<u64>(wheel_key)) != Some(frame) {
        let wheel = crate::wheel_input::timeline_events(ui.ctx(), rect, ui.layer_id());
        if !wheel.is_empty() {
            ui.ctx().data_mut(|data| data.insert_temp(wheel_key, frame));
            ui.input_mut(|input| input.smooth_scroll_delta = egui::Vec2::ZERO);
        }
        for (pointer, delta, zoom) in wheel {
            if zoom {
                let anchor = f64::from((pointer.x - rect.left()) / rect.width().max(1.0));
                view.zoom((-delta * 0.01).exp(), anchor, minimum);
            } else {
                view.start -= delta / f64::from(rect.width().max(1.0)) * view.span;
                view.clamp(minimum);
            }
        }
    }
    let response = ui.interact(
        scrollbar,
        id.with("time-scrollbar"),
        egui::Sense::click_and_drag(),
    );
    let thumb = thumb_rect(scrollbar, view);
    let drag_key = key.with("bar-drag");
    let interrupted =
        !can_navigate || ui.input(|input| !input.focused || input.key_pressed(egui::Key::Escape));
    if interrupted {
        if ui.ctx().dragged_id() == Some(response.id) {
            ui.ctx().stop_dragging();
        }
        if let Some(drag) = ui
            .ctx()
            .data_mut(|data| data.remove_temp::<BarDrag>(drag_key))
        {
            view = drag.original;
            view.clamp(minimum);
        }
    } else {
        if response.is_pointer_button_down_on()
            && let Some(origin) = ui.input(|input| input.pointer.press_origin())
        {
            ui.ctx().set_dragged_id(response.id);
            ui.ctx().data_mut(|data| {
                data.get_temp_mut_or_insert_with(drag_key, || {
                    let edge = if (origin.x - thumb.left()).abs() <= 5.0 {
                        -1
                    } else if (origin.x - thumb.right()).abs() <= 5.0 {
                        1
                    } else {
                        0
                    };
                    let mut initial = view;
                    if !thumb.contains(origin) && edge == 0 {
                        initial.start =
                            f64::from((origin.x - scrollbar.left()) / scrollbar.width().max(1.0))
                                * initial.duration
                                - initial.span * 0.5;
                        initial.clamp(minimum);
                    }
                    BarDrag {
                        origin: origin.x,
                        view: initial,
                        original: view,
                        edge,
                    }
                });
            });
        }
        if let Some(drag) = ui.ctx().data(|data| data.get_temp::<BarDrag>(drag_key)) {
            if let Some(point) = response.interact_pointer_pos() {
                let delta =
                    f64::from((point.x - drag.origin) / scrollbar.width().max(1.0)) * view.duration;
                view = drag.view;
                match drag.edge {
                    -1 => {
                        let end = view.start + view.span;
                        view.start = (view.start + delta).clamp(0.0, (end - minimum).max(0.0));
                        view.span = end - view.start;
                    }
                    1 => {
                        view.span = (view.span + delta)
                            .clamp(minimum.min(view.duration), view.duration - view.start);
                    }
                    _ => {
                        view.start += delta;
                    }
                }
                view.clamp(minimum);
            }
            if !ui.input(|input| input.pointer.primary_down()) {
                ui.ctx().data_mut(|data| data.remove::<BarDrag>(drag_key));
            }
        } else if response.clicked()
            && let Some(point) = response.interact_pointer_pos()
        {
            view.start = f64::from((point.x - scrollbar.left()) / scrollbar.width().max(1.0))
                * view.duration
                - view.span * 0.5;
            view.clamp(minimum);
        }
        if response.hovered() {
            let edge = ui.ctx().pointer_hover_pos().is_some_and(|point| {
                (point.x - thumb.left()).abs() <= 5.0 || (point.x - thumb.right()).abs() <= 5.0
            });
            ui.ctx().set_cursor_icon(if edge {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::Grab
            });
        }
    }
    ui.ctx().data_mut(|data| data.insert_temp(key, view));
    ui.painter()
        .rect_filled(waveform, chrome::RADIUS_XS, chrome::SURFACE);
    Layout {
        waveform,
        ruler,
        scrollbar,
        view,
    }
}

fn thumb_rect(bar: Rect, view: View) -> Rect {
    Rect::from_x_y_ranges(
        bar.left() + (view.start / view.duration) as f32 * bar.width()
            ..=bar.left() + ((view.start + view.span) / view.duration) as f32 * bar.width(),
        bar.y_range(),
    )
}

pub(super) fn paint(
    ui: &Ui,
    layout: Layout,
    fps: f64,
    position: MediaTime,
    selection: Option<TimeRange>,
) {
    let painter = ui.painter().with_clip_rect(layout.interaction());
    let view = layout.view;
    let minimum = view.span / f64::from(layout.ruler.width().max(1.0)) * 28.0;
    let (step, frames) = tick_step(minimum, fps);
    let first = (view.start / step).ceil() as u64;
    let last = ((view.start + view.span) / step).floor() as u64;
    let mut label_right = f32::NEG_INFINITY;
    for tick in first..=last.min(first + 512) {
        let seconds = tick as f64 * step;
        let x = view.x(layout.ruler, seconds);
        painter.vline(
            x,
            layout.ruler.bottom() - 4.0..=layout.ruler.bottom(),
            (1.0, chrome::FAINT),
        );
        // At the 30-point/frame limit, retain every frame tick. Wider boundary
        // labels can omit the following label without hiding its tick.
        if x >= label_right + 4.0 {
            let label = painter.text(
                egui::pos2(x, layout.ruler.top()),
                egui::Align2::LEFT_TOP,
                tick_label(seconds, step, fps, frames),
                egui::FontId::proportional(chrome::TEXT_SM),
                chrome::MUTED,
            );
            label_right = label.right();
        }
    }
    if ui.rect_contains_pointer(layout.interaction())
        && let Some(point) = ui.ctx().pointer_hover_pos()
    {
        painter.vline(point.x, layout.ruler.y_range(), (1.0, chrome::FOREGROUND));
    }
    let painter = ui.painter().with_clip_rect(layout.scrollbar);
    painter.rect_filled(
        layout.scrollbar,
        chrome::RADIUS_XS,
        chrome::FLOATING_BACKGROUND,
    );
    let thumb = thumb_rect(layout.scrollbar, view);
    painter.rect_filled(thumb, chrome::RADIUS_XS, chrome::SURFACE);
    if let Some(range) = selection {
        let left = layout.scrollbar.left()
            + (range.start().as_seconds_f64() / view.duration) as f32 * layout.scrollbar.width();
        let right = layout.scrollbar.left()
            + (range.end().as_seconds_f64() / view.duration) as f32 * layout.scrollbar.width();
        painter.rect_stroke(
            Rect::from_x_y_ranges(left..=right, layout.scrollbar.y_range()),
            chrome::RADIUS_XS,
            (1.0, chrome::FAINT),
            egui::StrokeKind::Inside,
        );
    }
    let x = layout.scrollbar.left()
        + (position.as_seconds_f64() / view.duration) as f32 * layout.scrollbar.width();
    painter.vline(
        x.min(layout.scrollbar.right() - 0.5),
        layout.scrollbar.y_range(),
        (1.0, chrome::FOREGROUND),
    );
}

fn tick_step(minimum: f64, fps: f64) -> (f64, bool) {
    for frames in [1.0, 5.0] {
        if frames / fps >= minimum {
            return (frames / fps, true);
        }
    }
    for seconds in [
        1.0, 2.0, 5.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0,
    ] {
        if seconds >= minimum {
            return (seconds, false);
        }
    }
    ((minimum / 3600.0).ceil() * 3600.0, false)
}

fn tick_label(seconds: f64, step: f64, fps: f64, frames: bool) -> String {
    if seconds < 0.0001 {
        return if frames {
            "0f".into()
        } else if step >= 60.0 {
            "0m".into()
        } else {
            "0s".into()
        };
    }
    if frames {
        let frame = (seconds * fps).round() as u64;
        let nominal = fps.round().max(1.0) as u64;
        if frame.is_multiple_of(nominal) {
            format!("{:02}:{:02}f", frame / nominal, 0)
        } else {
            format!("{:02}f", frame % nominal)
        }
    } else {
        let second = seconds.round() as u64;
        if step >= 60.0 {
            format!("{:02}m", second / 60)
        } else if step >= 15.0 || second.is_multiple_of(60) {
            format!("{:02}:{:02}s", second / 60, second % 60)
        } else {
            format!("{:02}s", second % 60)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoom_keeps_pointer_time_and_bounds_thirty_points_per_frame() {
        let mut view = View::default();
        view.fit(600.0, 1.0);
        let anchor = view.start + view.span * 0.3;
        view.zoom(0.5, 0.3, 1.0);
        assert!((view.start + view.span * 0.3 - anchor).abs() < 1e-9);
        view.zoom(1e-20, 0.5, 1.0);
        assert_eq!(view.span, 1.0);
        view.zoom(1e20, 0.5, 1.0);
        assert_eq!(view.start, 0.0);
        assert_eq!(view.span, 600.0);
        view.fit(300.0, 1.0);
        assert_eq!(view.span, 300.0);
    }
    #[test]
    fn ruler_progresses_through_seconds_and_frames() {
        assert_eq!(tick_step(60.1, 30.0), (120.0, false));
        assert_eq!(tick_step(2.1, 30.0), (5.0, false));
        assert_eq!(tick_step(0.1, 30.0), (5.0 / 30.0, true));
        assert_eq!(tick_step(28.0 / (30.0 * 30.0), 30.0), (1.0 / 30.0, true));
        assert_eq!(tick_label(60.0, 5.0, 30.0, false), "01:00s");
        assert_eq!(tick_label(1.0, 1.0 / 30.0, 30.0, true), "01:00f");
    }
}

#[cfg(test)]
#[path = "timeline_view_tests.rs"]
mod input_tests;
