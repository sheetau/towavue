use crate::chrome;
use egui::{Pos2, Rect, Ui};
use towavue_core::{MediaTime, TimeRange};

const RULER_HEIGHT: f32 = 16.0;
const BAR_HEIGHT: f32 = 10.0;
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
        let span = (self.span * factor).clamp(minimum.min(self.duration), self.duration);
        if span == self.span {
            return;
        }
        self.span = span;
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
    ruler_shape: Option<egui::layers::ShapeIdx>,
    hover_key: egui::Id,
    key: egui::Id,
    fps: f64,
    minimum: f64,
}

impl Layout {
    #[cfg(test)]
    pub fn plain(rect: Rect, duration: MediaTime) -> Self {
        Self {
            waveform: rect,
            ruler: Rect::NOTHING,
            scrollbar: Rect::NOTHING,
            ruler_shape: None,
            hover_key: egui::Id::NULL,
            key: egui::Id::NULL,
            fps: 0.0,
            minimum: 0.0,
            view: View {
                start: 0.0,
                span: duration.as_seconds_f64().max(f64::EPSILON),
                duration: duration.as_seconds_f64().max(f64::EPSILON),
            },
        }
    }
    pub fn suppress_hover(self, context: &egui::Context) {
        if self.ruler_shape.is_none() {
            return;
        }
        let frame = context.cumulative_frame_nr();
        context.data_mut(|data| data.insert_temp(self.hover_key, frame));
    }
    /// Quantize only when a frame occupies more than one logical point.
    pub fn quantize(self, time: MediaTime) -> MediaTime {
        if self.fps > 0.0 && f64::from(self.waveform.width()) / self.view.span / self.fps > 1.0 {
            let frame = (time.as_seconds_f64() * self.fps)
                .round()
                .clamp(0.0, (self.view.duration * self.fps + 1e-9).floor());
            MediaTime::from_nanoseconds((frame / self.fps * 1e9).round() as i64)
        } else {
            time
        }
    }
    pub fn frame_step(self) -> i64 {
        if self.fps > 0.0 && f64::from(self.waveform.width()) / self.view.span / self.fps > 1.0 {
            (1e9 / self.fps).round() as i64
        } else {
            1
        }
    }
    pub fn drag_feedback(
        self,
        context: &egui::Context,
        scroll: bool,
        ruler_time: Option<MediaTime>,
    ) {
        let frame = context.cumulative_frame_nr();
        if scroll
            && context.pointer_latest_pos().is_some_and(|point| {
                let speed = edge_scroll(point.x, context.content_rect());
                (speed < 0.0 && self.view.start > 0.0)
                    || (speed > 0.0 && self.view.start + self.view.span < self.view.duration)
            })
        {
            context.request_repaint();
        }
        context.data_mut(|data| {
            data.insert_temp(self.key.with("auto-scroll"), scroll);
            data.insert_temp(self.hover_key.with("drag"), (frame, ruler_time));
        });
    }
    pub fn clip_waveform(self, mesh: egui::Mesh) -> egui::Mesh {
        crate::timeline_waveform_clip::rounded(
            mesh,
            self.waveform,
            self.view.start <= f64::EPSILON,
            self.view.start + self.view.span >= self.view.duration - f64::EPSILON,
        )
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
    // A fixed zoom limit avoids changing the viewport after navigation when
    // true endpoints restore their five-point inset.
    let minimum =
        f64::from((rect.width() + 2.0 * chrome::PADDING as f32).max(1.0)) / (fps.max(1.0) * 50.0);
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
                let factor = (-delta * 0.01).exp();
                if (factor < 1.0 && view.span <= minimum.min(view.duration))
                    || (factor > 1.0 && view.span >= view.duration)
                {
                    continue;
                }
                let before = content_rect(rect, view);
                let anchor = f64::from((pointer.x - before.left()) / before.width().max(1.0));
                let time = view.start + anchor * view.span;
                view.zoom(factor, anchor, minimum);
                let after = content_rect(rect, view);
                view.start = time
                    - f64::from((pointer.x - after.left()) / after.width().max(1.0)) * view.span;
                view.clamp(minimum);
            } else {
                view.start -=
                    delta / f64::from(content_rect(rect, view).width().max(1.0)) * view.span;
                view.clamp(minimum);
            }
        }
    }
    // Advance once per frame, even when egui repeats layout. Only owned time
    // gestures opt in; a gain adjustment or an open popup cannot pan the view.
    let frame_key = key.with("auto-scroll-frame");
    if enabled
        && !egui::Popup::is_any_open(ui.ctx())
        && crate::timeline_input::is_active(ui.ctx())
        && ui
            .ctx()
            .data(|data| data.get_temp::<bool>(key.with("auto-scroll")))
            == Some(true)
        && ui.ctx().data(|data| data.get_temp::<u64>(frame_key)) != Some(frame)
        && ui.input(|input| {
            input.focused && input.pointer.any_down() && !input.key_pressed(egui::Key::Escape)
        })
        && let Some(pointer) = ui.ctx().pointer_latest_pos()
    {
        let window = ui.ctx().content_rect();
        let speed = edge_scroll(pointer.x, window);
        if speed != 0.0 {
            let before = view.start;
            view.start +=
                f64::from(speed * ui.input(|input| input.stable_dt).min(0.05)) * view.span;
            view.clamp(minimum);
            ui.ctx().data_mut(|data| data.insert_temp(frame_key, frame));
            if view.start != before {
                ui.ctx().request_repaint();
            }
        }
    }
    let response = ui.interact(
        scrollbar,
        id.with("time-scrollbar"),
        egui::Sense::click_and_drag(),
    );
    let scale = ScrollScale::new(scrollbar, view.duration, minimum);
    let thumb = scale.thumb(view);
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
            && ui.input(|input| input.pointer.primary_down())
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
                view = scale.drag(drag.view, point.x - drag.origin, drag.edge);
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
                egui::CursorIcon::Default
            });
        }
    }
    let at_start = view.start <= f64::EPSILON;
    let at_end = view.start + view.span >= view.duration - f64::EPSILON;
    let waveform = content_rect(waveform, view);
    let ruler = content_rect(ruler, view);
    ui.ctx().data_mut(|data| data.insert_temp(key, view));
    let corners = egui::CornerRadius {
        nw: if at_start { chrome::RADIUS_XS } else { 0 },
        sw: if at_start { chrome::RADIUS_XS } else { 0 },
        ne: if at_end { chrome::RADIUS_XS } else { 0 },
        se: if at_end { chrome::RADIUS_XS } else { 0 },
    };
    ui.painter()
        .with_clip_rect(waveform)
        .rect_filled(waveform, corners, chrome::SURFACE);
    Layout {
        waveform,
        ruler,
        scrollbar,
        view,
        // Ruler content is filled after input, below the CTI painted by selection.
        ruler_shape: Some(ui.painter().with_clip_rect(ruler).add(egui::Shape::Noop)),
        hover_key: key.with("ruler-hover-suppressed"),
        key,
        fps,
        minimum,
    }
}

fn edge_scroll(x: f32, window: Rect) -> f32 {
    // Up to one visible span per second; the outer eight points ramp smoothly.
    ((x - (window.right() - 8.0)) / 8.0).clamp(0.0, 1.0)
        - (((window.left() + 8.0) - x) / 8.0).clamp(0.0, 1.0)
}

fn content_rect(mut rect: Rect, view: View) -> Rect {
    if view.start > f64::EPSILON {
        rect.min.x -= chrome::PADDING as f32;
    }
    if view.start + view.span < view.duration - f64::EPSILON {
        rect.max.x += chrome::PADDING as f32;
    }
    rect
}

/// The handle caps have fixed width; only their inner span represents zoom.
/// A mild power curve keeps small spans selectable. For long clips, the prior
/// five-frame ruler interval has one point of inner width, and maximum zoom
/// has exactly zero. Both rendering and dragging use this invertible mapping.
struct ScrollScale {
    bar: Rect,
    duration: f64,
    minimum: f64,
    caps: f64,
    inner: f64,
    exponent: f64,
}

impl ScrollScale {
    fn new(bar: Rect, duration: f64, minimum: f64) -> Self {
        let caps = f64::from(bar.width().clamp(0.0, 10.0));
        let inner = (f64::from(bar.width()) - caps).max(0.0);
        let minimum = minimum.min(duration);
        let prior = 4.0 * minimum / (duration - minimum);
        let exponent = if inner > 1.0 && prior > 0.0 && prior < 1.0 / inner {
            ((1.0 / inner).ln() / prior.ln()).clamp(0.1, 1.0)
        } else {
            1.0
        };
        Self {
            bar,
            duration,
            minimum,
            caps,
            inner,
            exponent,
        }
    }

    fn width(&self, span: f64) -> f64 {
        if self.duration <= self.minimum {
            return self.caps + self.inner;
        }
        self.caps
            + self.inner
                * ((span - self.minimum) / (self.duration - self.minimum))
                    .clamp(0.0, 1.0)
                    .powf(self.exponent)
    }

    fn span(&self, width: f64) -> f64 {
        if self.inner <= 0.0 {
            return self.duration;
        }
        self.minimum
            + (self.duration - self.minimum)
                * ((width - self.caps) / self.inner)
                    .clamp(0.0, 1.0)
                    .powf(1.0 / self.exponent)
    }

    fn thumb(&self, view: View) -> Rect {
        let width = self.width(view.span);
        let travel = (f64::from(self.bar.width()) - width).max(0.0);
        let fraction = if view.span < view.duration {
            view.start / (view.duration - view.span)
        } else {
            0.0
        };
        Rect::from_min_size(
            egui::pos2(self.bar.left() + (travel * fraction) as f32, self.bar.top()),
            egui::vec2(width as f32, self.bar.height()),
        )
    }

    fn drag(&self, initial: View, delta: f32, edge: i8) -> View {
        if delta == 0.0 {
            return initial;
        }
        let width = self.width(initial.span);
        let total = f64::from(self.bar.width());
        let left = if initial.span < initial.duration {
            (total - width) * initial.start / (initial.duration - initial.span)
        } else {
            0.0
        };
        let right = left + width;
        let delta = f64::from(delta);
        let (left, width) = match edge {
            -1 => {
                let left = (left + delta).clamp(0.0, (right - self.caps).max(0.0));
                (left, right - left)
            }
            1 => (
                left,
                (right + delta - left).clamp(self.caps, (total - left).max(self.caps)),
            ),
            _ => ((left + delta).clamp(0.0, (total - width).max(0.0)), width),
        };
        let mut view = initial;
        // Panning must preserve exact zoom, including at the minimum.
        if edge != 0 {
            view.span = self.span(width);
        }
        let travel = total - self.width(view.span);
        view.start = if travel > 0.0 {
            left / travel * (view.duration - view.span)
        } else {
            0.0
        };
        view.clamp(self.minimum);
        view
    }
}

pub(super) fn paint(
    ui: &Ui,
    layout: Layout,
    fps: f64,
    position: MediaTime,
    selection: Option<TimeRange>,
) {
    let painter = ui.painter().with_clip_rect(layout.ruler);
    let view = layout.view;
    let minimum = if view.span <= layout.minimum && fps > 0.0 {
        1.0 / fps
    } else {
        view.span / f64::from(layout.ruler.width().max(1.0)) * 50.0
    };
    let (step, frames) = tick_step(minimum, fps);
    let first = (view.start / step).floor() as u64;
    let last = (((view.start + view.span) / step).ceil() as u64)
        .min((view.duration / step).floor() as u64);
    let mut shapes = Vec::new();
    for tick in first..=last.min(first + 512) {
        let seconds = tick as f64 * step;
        let x = crate::time_selection::cti_x(
            layout.ruler,
            view.x(layout.ruler, seconds),
            1.0 / ui.ctx().pixels_per_point(),
        );
        shapes.push(egui::Shape::line_segment(
            [
                egui::pos2(x, layout.ruler.bottom() - 4.0),
                egui::pos2(x, layout.ruler.bottom()),
            ],
            (1.0, chrome::FAINT),
        ));
        let text = painter.layout_no_wrap(
            tick_label(seconds, step, fps, frames),
            egui::FontId::proportional(chrome::TEXT_SM),
            chrome::MUTED,
        );
        let full = view.full_rect(layout.ruler);
        let left = (x - text.size().x * 0.5)
            .clamp(full.left(), (full.right() - text.size().x).max(full.left()));
        shapes.push(egui::Shape::galley(
            egui::pos2(left, layout.ruler.top() - text.mesh_bounds.top()),
            text,
            chrome::MUTED,
        ));
    }
    let frame = ui.ctx().cumulative_frame_nr();
    let suppressed = ui.data(|data| data.get_temp::<u64>(layout.hover_key));
    let moved = ui.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::PointerMoved(_)))
    });
    let suppressed = suppressed.is_some_and(|last| last == frame || !moved);
    if !suppressed {
        ui.data_mut(|data| data.remove::<u64>(layout.hover_key));
        if ui.rect_contains_pointer(layout.interaction())
            && let Some(point) = ui.ctx().pointer_hover_pos()
        {
            shapes.push(egui::Shape::line_segment(
                [
                    egui::pos2(point.x, layout.ruler.top()),
                    egui::pos2(point.x, layout.ruler.bottom()),
                ],
                (1.0, chrome::FOREGROUND),
            ));
        }
    }
    if let Some((painted, Some(time))) =
        ui.data(|data| data.get_temp::<(u64, Option<MediaTime>)>(layout.hover_key.with("drag")))
        && painted == frame
    {
        let x = view.x(layout.ruler, time.as_seconds_f64());
        shapes.push(egui::Shape::line_segment(
            [
                egui::pos2(x, layout.ruler.top()),
                egui::pos2(x, layout.ruler.bottom()),
            ],
            (1.0, chrome::FOREGROUND),
        ));
    }
    if let Some(slot) = layout.ruler_shape {
        painter.set(slot, egui::Shape::Vec(shapes));
    } else {
        painter.add(egui::Shape::Vec(shapes));
    }
    let painter = ui.painter().with_clip_rect(layout.scrollbar);
    painter.rect_filled(
        layout.scrollbar,
        chrome::RADIUS_XS,
        chrome::FLOATING_BACKGROUND,
    );
    let thumb = ScrollScale::new(layout.scrollbar, view.duration, layout.minimum).thumb(view);
    painter.rect_filled(thumb, chrome::RADIUS_XS, chrome::KEY_ACTIVE);
    for left in [true, false] {
        let cap = if left {
            thumb.with_max_x(thumb.left() + 5.0)
        } else {
            thumb.with_min_x(thumb.right() - 5.0)
        };
        let corners = egui::CornerRadius {
            nw: if left { chrome::RADIUS_XS } else { 0 },
            sw: if left { chrome::RADIUS_XS } else { 0 },
            ne: if left { 0 } else { chrome::RADIUS_XS },
            se: if left { 0 } else { chrome::RADIUS_XS },
        };
        painter.rect_filled(cap, corners, chrome::FAINT);
    }
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
        if frames / fps >= minimum * (1.0 - 1e-9) {
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
            format!("{}:{:02}f", frame / nominal, 0)
        } else {
            format!("{:02}f", frame % nominal)
        }
    } else {
        let second = seconds.round() as u64;
        if step >= 60.0 {
            format!("{}m", second / 60)
        } else if step >= 15.0 || second.is_multiple_of(60) {
            format!("{}:{:02}s", second / 60, second % 60)
        } else {
            format!("{:02}s", second % 60)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoom_keeps_pointer_time_and_bounds_fifty_points_per_frame() {
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
        assert_eq!(tick_label(60.0, 5.0, 30.0, false), "1:00s");
        assert_eq!(tick_label(1.0, 1.0 / 30.0, 30.0, true), "1:00f");
    }
}

#[cfg(test)]
#[path = "timeline_view_tests.rs"]
mod input_tests;
