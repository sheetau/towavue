use egui::{Rect, Response, Ui};
use towavue_core::{EditTimeline, MediaTime, TimeRange, TimelineEdit};

mod adjustment;

pub(super) fn focus_hint(context: &egui::Context) -> Option<String> {
    let (id, text) = context
        .data(|data| data.get_temp::<(egui::Id, String)>("time-selection-focus-hint".into()))?;
    (context.memory(|memory| memory.has_focus(id)) && !egui::Popup::is_any_open(context))
        .then_some(text)
}

fn describe_focus(response: &Response, enabled: bool, label: &str, value: f64) {
    if response.has_focus() {
        response.ctx.data_mut(|data| {
            let key = "time-selection-focus-hint".into();
            if enabled && response.enabled() {
                data.insert_temp(
                    key,
                    (
                        response.id,
                        format!("{label}: {value:.3} · Left/Right adjust"),
                    ),
                );
            } else {
                data.remove::<(egui::Id, String)>(key);
            }
        });
    }
}

#[derive(Clone, Copy)]
enum Gesture {
    Seek,
    Select,
    Resize(TimeRange, bool),
    Band(TimeRange, f32),
    Gain(TimeRange, f32),
    Stretch(TimeRange),
}

#[derive(Default)]
pub(super) struct Output {
    pub selection: Option<Option<TimeRange>>,
    pub seek: Option<MediaTime>,
    pub edit: Option<TimelineEdit>,
}

pub(super) fn show(
    ui: &Ui,
    response: &Response,
    duration: MediaTime,
    position: MediaTime,
    selection: Option<TimeRange>,
    plan: Option<&EditTimeline>,
    enabled: bool,
) -> Output {
    let rect = response.rect;
    let seconds = duration.as_seconds_f64();
    let at = |x: f32| {
        MediaTime::from_nanoseconds(
            (duration.as_nanoseconds() as f64 * f64::from(crate::seekbar::ratio(rect, x))).round()
                as i64,
        )
    };
    let x_at = |time: MediaTime| {
        egui::lerp(
            rect.x_range(),
            (time.as_seconds_f64() / seconds).clamp(0.0, 1.0) as f32,
        )
    };
    let mut output = Output::default();
    let mut preview = selection;
    let mut head = position;
    let bands = adjustment::bands(duration, plan);
    let mut gain_preview = None;
    let drag = if enabled {
        crate::timeline_input::seek_drag(response)
    } else {
        crate::timeline_input::cancel(ui.ctx());
        Default::default()
    };
    let mode_id = response.id.with("time-selection-mode");
    let choose = |origin: egui::Pos2, modifiers: egui::Modifiers| {
        let selected =
            selection.filter(|range| at(origin.x) >= range.start() && at(origin.x) <= range.end());
        if modifiers.alt && !modifiers.ctrl && !modifiers.shift {
            return selected.map_or(Gesture::Select, Gesture::Stretch);
        }
        if playhead_rect(rect, x_at(position)).contains(origin) {
            return Gesture::Seek;
        }
        if !modifiers.any()
            && let Some(range) = selection
        {
            let left = (origin.x - x_at(range.start())).abs();
            let right = (origin.x - x_at(range.end())).abs();
            if left.min(right) <= 6.0 {
                return Gesture::Resize(range, left <= right);
            }
        }
        let gain = adjustment::gain_at(&bands, at(origin.x));
        if !modifiers.any()
            && (selection.is_none() || selected.is_some())
            && (origin.y - adjustment::gain_y(rect, gain)).abs() <= 4.0
            && let Some(range) = selection.or_else(|| TimeRange::new(MediaTime::ZERO, duration))
        {
            return Gesture::Band(range, gain);
        }
        Gesture::Select
    };
    let cursor = |mode| match mode {
        Gesture::Seek => egui::CursorIcon::Grab,
        Gesture::Resize(..) | Gesture::Stretch(_) => egui::CursorIcon::ResizeHorizontal,
        Gesture::Band(..) | Gesture::Gain(..) => egui::CursorIcon::ResizeRow,
        Gesture::Select => egui::CursorIcon::Crosshair,
    };
    if enabled
        && response.hovered()
        && let Some(pointer) = response.hover_pos()
    {
        ui.ctx()
            .set_cursor_icon(cursor(choose(pointer, ui.input(|input| input.modifiers))));
    }
    if let (Some(origin), Some(pointer)) = (drag.origin, drag.position) {
        if drag.started {
            ui.ctx()
                .data_mut(|data| data.insert_temp(mode_id, choose(origin, drag.modifiers)));
        }
        let mut mode = ui.ctx().data_mut(|data| {
            *data.get_temp_mut_or_insert_with(mode_id, || choose(origin, drag.modifiers))
        });
        if let Gesture::Band(range, gain) = mode
            && let Some(direction) = drag.direction
        {
            mode = if direction.y.abs() > direction.x.abs() {
                Gesture::Gain(range, gain)
            } else {
                Gesture::Select
            };
            ui.ctx().data_mut(|data| data.insert_temp(mode_id, mode));
        }
        ui.ctx().set_cursor_icon(if matches!(mode, Gesture::Seek) {
            egui::CursorIcon::Grabbing
        } else {
            cursor(mode)
        });
        if drag.dragging && matches!(mode, Gesture::Gain(..) | Gesture::Stretch(_)) {
            let edit = match mode {
                Gesture::Gain(range, original) => {
                    let gain = (original
                        - (pointer.y - origin.y) * 2.0 / adjustment::gain_height(rect))
                    .clamp(0.0, 2.0);
                    gain_preview = Some((range, gain));
                    TimelineEdit::SetVolume(range, gain)
                }
                Gesture::Stretch(range) => {
                    let limits = adjustment::stretch_limits(range, plan);
                    let seconds = (range.duration().as_seconds_f64()
                        + f64::from(pointer.x - origin.x) / f64::from(rect.width()) * seconds)
                        .clamp(*limits.start(), *limits.end());
                    let length = crate::media_time(std::time::Duration::from_secs_f64(seconds));
                    preview = TimeRange::new(
                        range.start(),
                        MediaTime::from_nanoseconds(
                            range
                                .start()
                                .as_nanoseconds()
                                .saturating_add(length.as_nanoseconds()),
                        ),
                    );
                    TimelineEdit::Stretch(range, length)
                }
                _ => unreachable!(),
            };
            if drag.released && adjustment::changes_plan(duration, plan, edit) {
                output.edit = Some(edit);
            }
        } else if drag.dragging
            && let Gesture::Resize(range, start) = mode
        {
            let endpoint = if start { range.start() } else { range.end() };
            let delta = (f64::from(pointer.x) - f64::from(origin.x))
                / f64::from(rect.width().max(1.0))
                * duration.as_nanoseconds() as f64;
            let point = MediaTime::from_nanoseconds(
                ((endpoint.as_nanoseconds() as f64 + delta).round() as i64)
                    .clamp(0, duration.as_nanoseconds()),
            );
            preview = if start {
                TimeRange::new(
                    point.min(MediaTime::from_nanoseconds(
                        range.end().as_nanoseconds() - 1,
                    )),
                    range.end(),
                )
            } else {
                TimeRange::new(
                    range.start(),
                    point.max(MediaTime::from_nanoseconds(
                        range.start().as_nanoseconds() + 1,
                    )),
                )
            };
            if drag.released && preview != selection {
                output.selection = Some(preview);
            }
        } else if drag.dragging && matches!(mode, Gesture::Select) {
            let a = at(origin.x);
            let b = at(pointer.x);
            preview = TimeRange::new(a.min(b), a.max(b));
            if drag.released {
                output.selection = Some(preview);
            }
        } else if matches!(mode, Gesture::Seek | Gesture::Select | Gesture::Band(..)) {
            head = at(pointer.x);
            if drag.released {
                output.seek = Some(head);
                if !drag.dragging && selection.is_some() {
                    output.selection = Some(None);
                }
            }
        }
        if drag.released {
            ui.ctx().data_mut(|data| data.remove::<Gesture>(mode_id));
        }
    } else {
        ui.ctx().data_mut(|data| data.remove::<Gesture>(mode_id));
    }
    if let Some(value) = crate::seekbar::value_input(
        response,
        "Playback position (seconds)",
        position.as_seconds_f64(),
        0.0..=seconds,
        5.0,
        enabled,
    ) {
        output.seek = Some(crate::media_time(std::time::Duration::from_secs_f64(value)));
    }
    let painter = ui.painter().with_clip_rect(rect);
    adjustment::paint(&painter, rect, &bands, gain_preview, &x_at);
    if preview != selection
        && drag.dragging
        && gain_preview.is_none()
        && let Some(range) = preview
    {
        painter.text(
            rect.center_top() + egui::vec2(0.0, 2.0),
            egui::Align2::CENTER_TOP,
            format!("Length {:.3}s", range.duration().as_seconds_f64()),
            egui::FontId::proportional(11.0),
            crate::chrome::FOREGROUND,
        );
    }
    if let Some(range) = preview {
        towavue_runtime_windows::paint_selection_outline(
            &painter,
            Rect::from_min_max(
                egui::pos2(x_at(range.start()), rect.top() + 1.0),
                egui::pos2(x_at(range.end()), rect.bottom() - 1.0),
            ),
        );
    }
    let pixel = 1.0 / ui.ctx().pixels_per_point();
    let x = (x_at(head) / pixel).floor() * pixel;
    let x = x.clamp(rect.left(), (rect.right() - pixel).max(rect.left()));
    painter.rect_filled(
        Rect::from_min_max(
            egui::pos2(x, rect.top()),
            egui::pos2((x + pixel).min(rect.right()), rect.bottom()),
        ),
        0.0,
        crate::chrome::FOREGROUND,
    );
    let marker = playhead_rect(rect, x_at(head));
    painter.add(egui::Shape::convex_polygon(
        vec![
            marker.left_top(),
            marker.right_top(),
            egui::pos2(
                x_at(head).clamp(marker.left(), marker.right()),
                marker.bottom(),
            ),
        ],
        crate::chrome::FOREGROUND,
        egui::Stroke::NONE,
    ));
    for start in [true, false] {
        let selection = output.selection.unwrap_or(selection);
        let current = selection.map_or(if start { MediaTime::ZERO } else { duration }, |range| {
            if start { range.start() } else { range.end() }
        });
        let bounds = Rect::from_min_size(
            egui::pos2(
                if start {
                    rect.left()
                } else {
                    (rect.right() - 100.0).max(rect.left())
                },
                rect.bottom() - 20.0,
            ),
            egui::vec2(100.0_f32.min(rect.width()), 20.0),
        );
        let control = ui.interact(
            bounds,
            response.id.with(("selection-value", start)),
            egui::Sense::focusable_noninteractive(),
        );
        if selection.is_some() || control.has_focus() {
            painter.text(
                bounds.center(),
                egui::Align2::CENTER_CENTER,
                format!(
                    "{} {:.3}s",
                    if start { "In" } else { "Out" },
                    current.as_seconds_f64()
                ),
                egui::FontId::proportional(11.0),
                crate::chrome::FOREGROUND,
            );
        }
        let label = if start {
            "Time selection start (seconds)"
        } else {
            "Time selection end (seconds)"
        };
        describe_focus(&control, enabled, label, current.as_seconds_f64());
        if let Some(value) = crate::seekbar::value_input(
            &control,
            label,
            current.as_seconds_f64(),
            0.0..=seconds,
            0.1,
            enabled,
        ) {
            let value = crate::media_time(std::time::Duration::from_secs_f64(value));
            let candidate = if start {
                TimeRange::new(value, selection.map_or(duration, |range| range.end()))
            } else {
                TimeRange::new(
                    selection.map_or(MediaTime::ZERO, |range| range.start()),
                    value,
                )
            };
            if let Some(range) = candidate {
                output.selection = Some(Some(range));
            }
        }
    }
    if output.edit.is_none()
        && output.selection.is_none()
        && !crate::timeline_input::is_active(ui.ctx())
    {
        output.edit = adjustment::values(ui, response, duration, selection, plan, enabled);
    }
    output
}

fn playhead_rect(rect: Rect, x: f32) -> Rect {
    Rect::from_min_max(
        egui::pos2((x - 6.0).max(rect.left()), rect.top()),
        egui::pos2(
            (x + 6.0).min(rect.right()),
            (rect.top() + 8.0).min(rect.bottom()),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focused_time_values_do_not_add_rectangles_to_the_selection_outline() {
        let identity = egui::Id::new("time-focus-style");
        for density in [1.0, 1.25, 2.0] {
            for (target, label, value) in [
                (
                    identity.with(("selection-value", true)),
                    "Time selection start (seconds)",
                    2.0,
                ),
                (
                    identity.with(("selection-value", false)),
                    "Time selection end (seconds)",
                    6.0,
                ),
                (
                    identity.with(("timeline-adjustment-value", false)),
                    "Local volume (%)",
                    100.0,
                ),
                (
                    identity.with(("timeline-adjustment-value", true)),
                    "Selected duration (seconds)",
                    4.0,
                ),
            ] {
                let context = crate::fonts::test_context();
                context.enable_accesskit();
                context.set_pixels_per_point(density);
                context.memory_mut(|memory| memory.request_focus(target));
                let output = context.run_ui(Default::default(), |ui| {
                    let rect =
                        Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 100.0));
                    let response = ui.interact(rect, identity, egui::Sense::click_and_drag());
                    let result = show(
                        ui,
                        &response,
                        time(10.0),
                        time(1.0),
                        TimeRange::new(time(2.0), time(6.0)),
                        None,
                        true,
                    );
                    assert!(
                        result.seek.is_none()
                            && result.selection.is_none()
                            && result.edit.is_none()
                    );
                });
                assert!(context.memory(|memory| memory.has_focus(target)));
                assert_eq!(
                    focus_hint(&context),
                    Some(format!("{label}: {value:.3} · Left/Right adjust"))
                );
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree");
                let node = &tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == target.accesskit_id())
                    .expect("value control")
                    .1;
                assert_eq!(node.label(), Some(label));
                assert_eq!(node.numeric_value(), Some(value));
                assert!(node.supports_action(egui::accesskit::Action::SetValue));
                assert!(
                    output.shapes.iter().all(|shape| !matches!(&shape.shape,
                    egui::Shape::Rect(rect) if rect.stroke != egui::Stroke::NONE)),
                    "no extra value-focus frame at density {density}"
                );
                assert_eq!(
                    output
                        .shapes
                        .iter()
                        .filter(|shape| matches!(shape.shape, egui::Shape::Callback(_)))
                        .count(),
                    1
                );
                let _ = context.run_ui(Default::default(), |ui| {
                    let response = ui.interact(
                        ui.max_rect(),
                        target,
                        egui::Sense::focusable_noninteractive(),
                    );
                    describe_focus(&response, false, label, value);
                    assert!(
                        focus_hint(&context).is_none(),
                        "disabled control has no hint"
                    );
                    describe_focus(&response, true, label, value);
                    assert!(focus_hint(&context).is_some());
                    response.surrender_focus();
                    assert!(
                        focus_hint(&context).is_none(),
                        "do not retain stale focus text"
                    );
                });
            }
        }
    }

    fn time(seconds: f64) -> MediaTime {
        crate::media_time(std::time::Duration::from_secs_f64(seconds))
    }
    fn frame(
        context: &egui::Context,
        events: Vec<egui::Event>,
        enabled: bool,
        selection: Option<TimeRange>,
    ) -> Vec<Output> {
        let mut results = Vec::new();
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 200.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let rect = Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 100.0));
                let response = ui.interact(
                    rect,
                    "time-selection-test".into(),
                    egui::Sense::click_and_drag(),
                );
                let result = show(
                    ui,
                    &response,
                    time(10.0),
                    time(0.0),
                    selection,
                    None,
                    enabled,
                );
                if result.seek.is_some() || result.selection.is_some() || result.edit.is_some() {
                    results.push(result);
                }
                if context.current_pass_index() == 0 {
                    context.request_discard("selection multiple passes");
                }
            },
        );
        results
    }
    fn button(x: f32, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(x, 70.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    #[test]
    fn horizontal_selection_and_head_seek_commit_once_in_batched_or_separate_frames() {
        for batched in [false, true] {
            for (start, end, seek, marker) in [
                (120.0, 320.0, false, false),
                (320.0, 120.0, false, false),
                (20.0, 220.0, false, false),
                (20.0, 220.0, true, true),
                (220.0, 220.0, true, false),
            ] {
                let context = egui::Context::default();
                frame(&context, vec![], true, None);
                let mut press = button(start, true);
                if marker && let egui::Event::PointerButton { pos, .. } = &mut press {
                    pos.y = 34.0;
                }
                let events = vec![
                    press,
                    egui::Event::PointerMoved(egui::pos2(end, 70.0)),
                    button(end, false),
                ];
                let mut results = Vec::new();
                if batched {
                    results.extend(frame(&context, events, true, None));
                } else {
                    for event in events {
                        results.extend(frame(&context, vec![event], true, None));
                    }
                }
                assert_eq!(results.len(), 1);
                if seek {
                    assert_eq!(results[0].seek, Some(time(5.0)));
                    assert!(results[0].selection.is_none());
                } else {
                    assert!(results[0].seek.is_none());
                    assert_eq!(
                        results[0].selection,
                        Some(TimeRange::new(
                            time(f64::from((start.min(end) - 20.0) / 40.0)),
                            time(f64::from((start.max(end) - 20.0) / 40.0))
                        ))
                    );
                }
                assert!(frame(&context, vec![], true, None).is_empty());
            }
        }
    }
    #[test]
    fn selection_edges_resize_without_seeking_and_cancel_without_committing() {
        let selected = TimeRange::new(time(2.5), time(7.5));
        for batched in [false, true] {
            for (start, end, expected) in [
                (120.0, 80.0, TimeRange::new(time(1.5), time(7.5))),
                (320.0, 360.0, TimeRange::new(time(2.5), time(8.5))),
                (125.0, 85.0, TimeRange::new(time(1.5), time(7.5))),
                (120.0, -100.0, TimeRange::new(time(0.0), time(7.5))),
                (320.0, 600.0, TimeRange::new(time(2.5), time(10.0))),
                (
                    120.0,
                    400.0,
                    TimeRange::new(MediaTime::from_nanoseconds(7_499_999_999), time(7.5)),
                ),
                (
                    320.0,
                    40.0,
                    TimeRange::new(time(2.5), MediaTime::from_nanoseconds(2_500_000_001)),
                ),
            ] {
                let context = egui::Context::default();
                frame(&context, vec![], true, selected);
                let events = vec![
                    button(start, true),
                    egui::Event::PointerMoved(egui::pos2(end, 70.0)),
                    button(end, false),
                ];
                let results = if batched {
                    frame(&context, events, true, selected)
                } else {
                    let mut results = Vec::new();
                    for event in events {
                        results.extend(frame(&context, vec![event], true, selected));
                    }
                    results
                };
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].selection, Some(expected));
                assert!(results[0].seek.is_none() && results[0].edit.is_none());
                assert!(frame(&context, vec![], true, expected).is_empty());
            }
        }
        for start in [120.0, 320.0] {
            for interruption in 0..4 {
                let context = egui::Context::default();
                frame(&context, vec![], true, selected);
                frame(
                    &context,
                    vec![
                        button(start, true),
                        egui::Event::PointerMoved(egui::pos2(220.0, 70.0)),
                    ],
                    true,
                    selected,
                );
                match interruption {
                    0 => {
                        crate::timeline_input::cancel(&context);
                    }
                    1 => {
                        frame(&context, vec![], false, selected);
                    }
                    2 => {
                        frame(
                            &context,
                            vec![egui::Event::WindowFocused(false)],
                            true,
                            selected,
                        );
                    }
                    _ => {
                        frame(
                            &context,
                            vec![egui::Event::Key {
                                key: egui::Key::Escape,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            }],
                            true,
                            selected,
                        );
                    }
                }
                assert!(frame(&context, vec![button(220.0, false)], true, selected).is_empty());
            }
        }
    }

    #[test]
    fn timeline_marker_and_line_are_inside_and_hover_cursors_match_the_target() {
        for density in [1.0, 1.25, 2.0] {
            let context = egui::Context::default();
            context.set_pixels_per_point(density);
            let rect = Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 100.0));
            for position in [time(0.0), time(5.0), time(10.0)] {
                let x = 20.0 + position.as_seconds_f64() as f32 * 40.0;
                for (pointer, expected) in [
                    (egui::pos2(x, 34.0), egui::CursorIcon::Grab),
                    (egui::pos2(x, 70.0), egui::CursorIcon::Crosshair),
                    (egui::pos2(120.0, 70.0), egui::CursorIcon::ResizeHorizontal),
                    (egui::pos2(320.0, 70.0), egui::CursorIcon::ResizeHorizontal),
                    (egui::pos2(180.0, 80.0), egui::CursorIcon::ResizeRow),
                ] {
                    let mut output = egui::FullOutput::default();
                    for _ in 0..3 {
                        output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(500.0, 200.0),
                                )),
                                events: vec![egui::Event::PointerMoved(pointer)],
                                ..Default::default()
                            },
                            |ui| {
                                let response = ui.interact(
                                    rect,
                                    "marker-test".into(),
                                    egui::Sense::click_and_drag(),
                                );
                                let result = show(
                                    ui,
                                    &response,
                                    time(10.0),
                                    position,
                                    TimeRange::new(time(2.5), time(7.5)),
                                    None,
                                    true,
                                );
                                assert!(
                                    result.seek.is_none()
                                        && result.selection.is_none()
                                        && result.edit.is_none()
                                );
                            },
                        );
                    }
                    assert_eq!(output.platform_output.cursor_icon, expected);
                    let stem = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Rect(shape) if shape.fill == crate::chrome::FOREGROUND => {
                                Some(shape.rect)
                            }
                            _ => None,
                        })
                        .expect("CTI stem");
                    assert!(rect.contains_rect(stem));
                    assert!((stem.width() * density - 1.0).abs() < 0.001);
                    let marker = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Path(shape)
                                if shape.closed && shape.fill == crate::chrome::FOREGROUND =>
                            {
                                Some(&shape.points)
                            }
                            _ => None,
                        })
                        .expect("playhead triangle");
                    assert_eq!(marker.len(), 3);
                    assert!(
                        marker
                            .iter()
                            .all(|point| rect.contains(*point) && point.y <= rect.top() + 8.0)
                    );
                    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == egui::Color32::from_white_alpha(128))));
                }
            }
        }
    }

    #[test]
    fn cancellation_retains_selection_and_a_new_press_rechooses_the_gesture() {
        for interruption in 0..4 {
            let context = egui::Context::default();
            let selected = TimeRange::new(time(1.0), time(2.0));
            frame(&context, vec![], true, selected);
            frame(&context, vec![button(20.0, true)], true, selected);
            frame(
                &context,
                vec![egui::Event::PointerMoved(egui::pos2(200.0, 70.0))],
                true,
                selected,
            );
            match interruption {
                0 => {
                    crate::timeline_input::cancel(&context);
                }
                1 => {
                    frame(&context, vec![], false, selected);
                }
                2 => {
                    frame(
                        &context,
                        vec![egui::Event::WindowFocused(false)],
                        true,
                        selected,
                    );
                }
                _ => {
                    frame(
                        &context,
                        vec![egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        true,
                        selected,
                    );
                }
            }
            assert!(frame(&context, vec![button(200.0, false)], true, selected).is_empty());
            let results = frame(
                &context,
                vec![
                    egui::Event::WindowFocused(true),
                    button(120.0, true),
                    egui::Event::PointerMoved(egui::pos2(320.0, 70.0)),
                    button(320.0, false),
                ],
                true,
                selected,
            );
            assert_eq!(results.len(), 1);
            assert_eq!(
                results[0].selection,
                Some(TimeRange::new(time(2.5), time(7.5)))
            );
            assert!(results[0].seek.is_none());
        }
    }

    #[test]
    fn gain_and_stretch_commit_once_and_keep_press_modifiers() {
        let selected = TimeRange::new(time(2.5), time(7.5));
        for batched in [false, true] {
            for (selection, stretch) in [(None, false), (selected, false), (selected, true)] {
                let context = egui::Context::default();
                frame(&context, vec![], true, selection);
                let origin = egui::pos2(220.0, if stretch { 70.0 } else { 80.0 });
                let end = if stretch {
                    origin + egui::vec2(80.0, 40.0)
                } else {
                    origin + egui::vec2(10.0, 80.0)
                };
                let press = egui::Event::PointerButton {
                    pos: origin,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: if stretch {
                        egui::Modifiers::ALT
                    } else {
                        egui::Modifiers::NONE
                    },
                };
                let release = egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                };
                let events = vec![press, egui::Event::PointerMoved(end), release];
                let results = if batched {
                    frame(&context, events, true, selection)
                } else {
                    assert!(frame(&context, vec![events[0].clone()], true, selection).is_empty());
                    assert!(frame(&context, vec![events[1].clone()], true, selection).is_empty());
                    frame(&context, vec![events[2].clone()], true, selection)
                };
                assert_eq!(results.len(), 1);
                let range = selection
                    .unwrap_or_else(|| TimeRange::new(time(0.0), time(10.0)).expect("whole range"));
                assert_eq!(
                    results[0].edit,
                    Some(if stretch {
                        TimelineEdit::Stretch(range, time(7.0))
                    } else {
                        TimelineEdit::SetVolume(range, 0.0)
                    })
                );
                assert!(results[0].seek.is_none() && results[0].selection.is_none());
                assert!(frame(&context, vec![], true, selection).is_empty());
            }
        }
    }

    #[test]
    fn adjustment_cancellation_never_commits_or_changes_selection() {
        let selected = TimeRange::new(time(2.5), time(7.5));
        for stretch in [false, true] {
            for interruption in 0..4 {
                let context = egui::Context::default();
                frame(&context, vec![], true, selected);
                let origin = egui::pos2(220.0, 80.0);
                frame(
                    &context,
                    vec![egui::Event::PointerButton {
                        pos: origin,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: if stretch {
                            egui::Modifiers::ALT
                        } else {
                            egui::Modifiers::NONE
                        },
                    }],
                    true,
                    selected,
                );
                assert!(
                    frame(
                        &context,
                        vec![egui::Event::PointerMoved(origin + egui::vec2(80.0, 25.0))],
                        true,
                        selected
                    )
                    .is_empty()
                );
                match interruption {
                    0 => {
                        crate::timeline_input::cancel(&context);
                    }
                    1 => {
                        frame(&context, vec![], false, selected);
                    }
                    2 => {
                        frame(
                            &context,
                            vec![egui::Event::WindowFocused(false)],
                            true,
                            selected,
                        );
                    }
                    _ => {
                        frame(
                            &context,
                            vec![egui::Event::Key {
                                key: egui::Key::Escape,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            }],
                            true,
                            selected,
                        );
                    }
                }
                assert!(frame(&context, vec![button(300.0, false)], true, selected).is_empty());
            }
        }
    }

    #[test]
    fn volume_line_locks_first_direction_but_horizontal_drags_still_select() {
        for vertical in [false, true] {
            let context = egui::Context::default();
            frame(&context, vec![], true, None);
            let origin = egui::pos2(120.0, 80.0);
            let end = egui::pos2(320.0, 108.0);
            let results = frame(
                &context,
                vec![
                    egui::Event::PointerButton {
                        pos: origin,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerMoved(
                        origin
                            + if vertical {
                                egui::vec2(0.0, 20.0)
                            } else {
                                egui::vec2(20.0, 0.0)
                            },
                    ),
                    egui::Event::PointerMoved(end),
                    egui::Event::PointerButton {
                        pos: end,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                true,
                None,
            );
            assert_eq!(results.len(), 1);
            if vertical {
                assert_eq!(
                    results[0].edit,
                    Some(TimelineEdit::SetVolume(
                        TimeRange::new(time(0.0), time(10.0)).expect("whole range"),
                        0.0
                    ))
                );
                assert!(results[0].selection.is_none());
            } else {
                assert_eq!(
                    results[0].selection,
                    Some(TimeRange::new(time(2.5), time(7.5)))
                );
                assert!(results[0].edit.is_none());
            }
            assert!(results[0].seek.is_none());
        }
    }

    #[test]
    fn accessible_endpoints_select_without_editing_and_guard_stale_or_modal_actions() {
        use crate::*;
        let Some(root) = crate::tests::isolated_test_root(
            "time_selection::tests::accessible_endpoints_select_without_editing_and_guard_stale_or_modal_actions",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        let context = fonts::test_context();
        context.enable_accesskit();
        app.ui_context = Some(context.clone());
        let tab = app.tabs.open_new(root.join("audio.wav"), MediaKind::Audio);
        app.media_kind = Some(MediaKind::Audio);
        app.media_duration = Some(std::time::Duration::from_secs(10));
        app.state = PlaybackState::Paused;
        let draw = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_timeline(ui, &mut actions),
            );
            (
                output.platform_output.accesskit_update.expect("tree"),
                actions,
            )
        };
        draw(&mut app, vec![]);
        let (tree, _) = draw(&mut app, vec![]);
        let event = |name, value| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(name))
                    .expect("named endpoint")
                    .0,
                data: Some(egui::accesskit::ActionData::NumericValue(value)),
            })
        };
        let (_, actions) = draw(
            &mut app,
            vec![
                event("Time selection start (seconds)", 3.0),
                event("Time selection end (seconds)", 7.0),
            ],
        );
        assert!(
            matches!(actions.as_slice(),[UiAction::TimeSelection(id,_,Some(range))] if *id==tab && *range==TimeRange::new(time(3.0),time(7.0)).expect("range"))
        );
        for action in actions {
            app.handle_ui_action(action);
        }
        let selected = app.time_selection;
        assert!(app.edits.is_empty());
        let (_, actions) = draw(&mut app, vec![event("Local volume (%)", 50.0)]);
        assert!(
            matches!(actions.as_slice(), [UiAction::TimeAdjustment(_, _, _, TimelineEdit::SetVolume(_, gain))] if *gain == 0.5)
        );
        for action in actions {
            app.handle_ui_action(action);
        }
        assert_eq!(app.time_selection, selected);
        let plan = app.edits[&tab].timeline(time(10.0)).expect("gain plan");
        assert_eq!(
            plan.spans()
                .iter()
                .map(|span| span.volume())
                .collect::<Vec<_>>(),
            vec![1.0, 0.5, 1.0]
        );
        let history = app.edits[&tab].clone();
        let edit = TimelineEdit::Stretch(selected.expect("selection"), time(8.0));
        for (id, generation, selection) in [
            (tab, app.generation.next(), selected),
            (tab, app.generation, None),
        ] {
            app.handle_ui_action(UiAction::TimeAdjustment(id, generation, selection, edit));
            assert_eq!(app.edits[&tab], history);
        }
        app.pending_guard = Some(GuardedAction::CloseTab(tab));
        app.handle_ui_action(UiAction::TimeAdjustment(
            tab,
            app.generation,
            selected,
            edit,
        ));
        assert_eq!(app.edits[&tab], history);
        app.pending_guard = None;
        let (_, actions) = draw(&mut app, vec![event("Selected duration (seconds)", 8.0)]);
        assert!(
            matches!(actions.as_slice(), [UiAction::TimeAdjustment(_, _, _, TimelineEdit::Stretch(_, length))] if *length == time(8.0))
        );
        for action in actions {
            app.handle_ui_action(action);
        }
        assert_eq!(app.time_selection, TimeRange::new(time(3.0), time(11.0)));
        assert_eq!(
            app.edits[&tab]
                .timeline(time(10.0))
                .expect("stretch plan")
                .duration(),
            time(14.0)
        );
        app.undo_edit(false);
        assert_eq!(app.edits[&tab].operations(), history.operations());
        app.undo_edit(false);
        app.time_selection = selected;
        // Undo keeps its redo branch; the following guard checks must not mutate it.
        let history = app.edits[&tab].clone();
        let stale = UiAction::TimeSelection(tab, app.generation.next(), None);
        app.handle_ui_action(stale);
        assert_eq!(app.time_selection, selected);
        app.pending_guard = Some(GuardedAction::CloseTab(tab));
        app.handle_ui_action(UiAction::TimeSelection(tab, app.generation, None));
        app.dispatch(CommandId::DeleteTimeSelection);
        assert_eq!(app.time_selection, selected);
        assert_eq!(app.edits[&tab], history);
        app.pending_guard = None;
        app.process_shortcut("Delete".parse().expect("key"));
        let plan = app.edits[&tab].timeline(time(10.0)).expect("deleted plan");
        assert_eq!(plan.duration(), time(6.0));
        assert_eq!(plan.spans()[1].source().start(), time(7.0));
        assert!(app.time_selection.is_none());
        app.undo_edit(false);
        app.handle_ui_action(UiAction::TimeSelection(tab, app.generation, selected));
        app.process_shortcut("Ctrl+Y".parse().expect("keep key"));
        let plan = app.edits[&tab].timeline(time(10.0)).expect("kept plan");
        assert_eq!(plan.duration(), time(4.0));
        assert_eq!(plan.spans()[0].source(), selected.expect("selected"));
    }
}
