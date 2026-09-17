use egui::{Rect, Response, Ui};
use towavue_core::{EditTimeline, MediaTime, TimeRange, TimelineEdit};

mod adjustment;
#[cfg(test)]
mod presentation_tests;

pub(super) const LABEL_SIZE: f32 = 11.0;
const LABEL_INSET: f32 = 24.0;

pub(super) fn focus_hint(context: &egui::Context) -> Option<String> {
    let (id, text) = context
        .data(|data| data.get_temp::<(egui::Id, String)>("time-selection-focus-hint".into()))?;
    (context.memory(|memory| memory.has_focus(id)) && !egui::Popup::is_any_open(context))
        .then_some(text)
}

fn describe_focus(response: &Response, enabled: bool, label: &str, value: f64, time: bool) {
    if response.has_focus() {
        response.ctx.data_mut(|data| {
            let key = "time-selection-focus-hint".into();
            if enabled && response.enabled() {
                data.insert_temp(
                    key,
                    (
                        response.id,
                        if time {
                            format!(
                                "{}: {} · Left/Right adjust",
                                label.trim_end_matches(" (seconds)"),
                                crate::format_time_precise(MediaTime::from_nanoseconds(
                                    (value * 1e9) as i64
                                ))
                            )
                        } else {
                            format!("{label}: {value:.3} · Left/Right adjust")
                        },
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
    pub gain_preview: Option<(TimeRange, f32)>,
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
    let frame = ui.ctx().cumulative_frame_nr();
    let preview_id = response.id.with("cti-preview");
    let (mut head, mut preview) = ui.ctx().data(|data| {
        data.get_temp::<(u64, MediaTime, Option<TimeRange>)>(preview_id)
            .filter(|(painted, _, _)| *painted == frame)
            .map_or((position, selection), |(_, head, preview)| (head, preview))
    });
    let pixel = 1.0 / ui.ctx().pixels_per_point();
    let bands = adjustment::bands(duration, plan);
    let gain_preview_id = response.id.with("gain-preview");
    let mut gain_preview = ui.ctx().data(|data| {
        data.get_temp::<(u64, (TimeRange, f32))>(gain_preview_id)
            .filter(|(painted, _)| enabled && *painted == frame)
            .map(|(_, preview)| preview)
    });
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
        if playhead_rect(rect, cti_x(rect, x_at(position), pixel)).contains(origin) {
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
            && let Some(range) =
                selection.or_else(|| adjustment::gain_range_at(&bands, at(origin.x)))
        {
            return Gesture::Band(range, gain);
        }
        Gesture::Select
    };
    let cursor = |mode| match mode {
        Gesture::Seek | Gesture::Resize(..) | Gesture::Stretch(_) => {
            egui::CursorIcon::ResizeHorizontal
        }
        Gesture::Band(..) | Gesture::Gain(..) => egui::CursorIcon::ResizeRow,
        Gesture::Select => egui::CursorIcon::Text,
    };
    if enabled
        && drag.origin.is_none()
        && let Some(pointer) = ui
            .ctx()
            .pointer_hover_pos()
            .filter(|point| rect.contains(*point))
    {
        use crate::hover_help::HoverHelp;
        let help = match choose(pointer, egui::Modifiers::NONE) {
            Gesture::Seek => Some((
                playhead_rect(rect, cti_x(rect, x_at(position), pixel)),
                "head",
                "Playback position · drag the playhead to seek",
            )),
            Gesture::Resize(range, start) => {
                let x = x_at(if start { range.start() } else { range.end() });
                Some((
                    Rect::from_x_y_ranges(x - 6.0..=x + 6.0, rect.y_range()),
                    if start { "start" } else { "end" },
                    if start {
                        "Selection start · drag to adjust"
                    } else {
                        "Selection end · drag to adjust"
                    },
                ))
            }
            Gesture::Band(range, gain) => {
                let y = adjustment::gain_y(rect, gain);
                Some((
                    Rect::from_x_y_ranges(
                        x_at(range.start())..=x_at(range.end()),
                        y - 4.0..=y + 4.0,
                    ),
                    "gain",
                    "Volume line · drag up/down to change the local gain (also affects export)",
                ))
            }
            Gesture::Select
                if selection.is_some_and(|range| {
                    at(pointer.x) >= range.start() && at(pointer.x) <= range.end()
                }) =>
            {
                selection.map(|range| {
                    (
                        Rect::from_x_y_ranges(
                            x_at(range.start())..=x_at(range.end()),
                            rect.y_range(),
                        ),
                        "selection",
                        "Time selection · drag to replace · Alt+drag to stretch",
                    )
                })
            }
            _ => None,
        };
        if let Some((bounds, part, text)) = help {
            ui.interact(
                bounds.intersect(response.interact_rect),
                response.id.with(("part-help", part)),
                egui::Sense::hover(),
            )
            .help_text(text);
        }
    }
    if enabled
        && response.hovered()
        && let Some(pointer) = response.hover_pos()
    {
        ui.ctx()
            .set_cursor_icon(cursor(choose(pointer, ui.input(|input| input.modifiers))));
    }
    if let (Some(origin), Some(pointer)) = (drag.origin, drag.position) {
        if drag.started {
            ui.ctx().data_mut(|data| {
                data.insert_temp(mode_id, (choose(origin, drag.modifiers), position));
            });
        }
        let (mut mode, snap_head) = ui.ctx().data_mut(|data| {
            *data
                .get_temp_mut_or_insert_with(mode_id, || (choose(origin, drag.modifiers), position))
        });
        // A selection press may seek immediately. Keep the original CTI as the
        // magnet for this gesture, with a small screen-space radius and no latch.
        let snap_head = snap_head.clamp(MediaTime::ZERO, duration);
        let snap = |point, x: f32| {
            if (x - x_at(snap_head)).abs() <= 4.0 {
                snap_head
            } else {
                point
            }
        };
        let mut began_selection = drag.started && matches!(mode, Gesture::Select);
        if let Gesture::Band(range, gain) = mode
            && let Some(direction) = drag.direction
        {
            mode = if direction.y.abs() > direction.x.abs() {
                Gesture::Gain(range, gain)
            } else {
                began_selection = true;
                Gesture::Select
            };
            ui.ctx()
                .data_mut(|data| data.insert_temp(mode_id, (mode, snap_head)));
        }
        ui.ctx().set_cursor_icon(cursor(mode));
        if drag.dragging && matches!(mode, Gesture::Gain(..) | Gesture::Stretch(_)) {
            let edit = match mode {
                Gesture::Gain(range, original) => {
                    let gain = (original - (pointer.y - origin.y) / adjustment::gain_height(rect))
                        .clamp(0.0, towavue_core::MAX_VOLUME);
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
            let point = snap(point, x_at(endpoint) + pointer.x - origin.x);
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
            let a = snap(at(origin.x), origin.x);
            let b = snap(at(pointer.x), pointer.x);
            preview = TimeRange::new(a.min(b), a.max(b));
            head = a.min(b);
            if began_selection || (drag.released && head != a) {
                output.seek = Some(head);
            }
            if drag.released {
                output.selection = Some(preview);
            }
        } else if matches!(mode, Gesture::Seek | Gesture::Select | Gesture::Band(..)) {
            head = if matches!(mode, Gesture::Select) {
                snap(at(origin.x), origin.x)
            } else {
                at(pointer.x)
            };
            if began_selection || (drag.released && !matches!(mode, Gesture::Select)) {
                output.seek = Some(head);
            }
            if drag.released
                && !drag.dragging
                && selection.is_some()
                && !matches!(mode, Gesture::Seek)
            {
                output.selection = Some(None);
            }
        }
        // A discarded UI pass must not paint the pre-gesture CTI/selection again.
        ui.ctx()
            .data_mut(|data| data.insert_temp(preview_id, (frame, head, preview)));
        if drag.released {
            ui.ctx()
                .data_mut(|data| data.remove::<(Gesture, MediaTime)>(mode_id));
        }
    } else {
        ui.ctx()
            .data_mut(|data| data.remove::<(Gesture, MediaTime)>(mode_id));
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
    if let Some(range) = preview {
        // The outline paints inside these bounds. Include each endpoint's CTI
        // pixel so the right side does not land one pixel before the stop position.
        towavue_runtime_windows::paint_time_selection(
            &painter,
            Rect::from_min_max(
                egui::pos2(
                    cti_x(rect, x_at(range.start()), pixel) - pixel * 0.5,
                    rect.top(),
                ),
                egui::pos2(
                    cti_x(rect, x_at(range.end()), pixel) + pixel * 0.5,
                    rect.bottom(),
                ),
            ),
        );
    }
    adjustment::paint(&painter, rect, &bands, gain_preview, &x_at);
    if preview != selection
        && drag.dragging
        && gain_preview.is_none()
        && let Some(range) = preview
    {
        painter.text(
            rect.center_top() + egui::vec2(0.0, 2.0),
            egui::Align2::CENTER_TOP,
            format!("Length {}", crate::format_time_precise(range.duration())),
            egui::FontId::proportional(LABEL_SIZE),
            crate::chrome::FOREGROUND,
        );
    }
    let x = cti_x(rect, x_at(head), pixel);
    let marker = playhead_rect(rect, x);
    let mut cti = egui::Mesh::default();
    for point in [
        marker.left_top(),
        marker.right_top(),
        egui::pos2(x, marker.bottom()),
    ] {
        cti.colored_vertex(point, crate::chrome::FOREGROUND);
    }
    cti.add_triangle(0, 1, 2);
    cti.add_colored_rect(
        Rect::from_min_max(
            egui::pos2((x - pixel * 0.5).max(rect.left()), rect.top()),
            egui::pos2((x + pixel * 0.5).min(rect.right()), rect.bottom()),
        ),
        crate::chrome::FOREGROUND,
    );
    painter.add(cti);
    for start in [true, false] {
        // Report the displayed drag preview without committing it to the model.
        let selection = output.selection.unwrap_or(preview);
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
                if start {
                    bounds.left_center() + egui::vec2(LABEL_INSET, 0.0)
                } else {
                    bounds.right_center() - egui::vec2(LABEL_INSET, 0.0)
                },
                if start {
                    egui::Align2::LEFT_CENTER
                } else {
                    egui::Align2::RIGHT_CENTER
                },
                format!(
                    "{} {}",
                    if start { "In" } else { "Out" },
                    crate::format_time_precise(current)
                ),
                egui::FontId::proportional(LABEL_SIZE),
                crate::chrome::FOREGROUND,
            );
        }
        let label = if start {
            "Time selection start (seconds)"
        } else {
            "Time selection end (seconds)"
        };
        // Stretch can preview an endpoint beyond the old timeline. Report it
        // without clamping, but do not edit a range the model does not own yet.
        let displayed_end = selection
            .map_or(duration, |range| range.end())
            .max(duration);
        let editable = enabled && displayed_end == duration;
        describe_focus(&control, editable, label, current.as_seconds_f64(), true);
        if let Some(value) = crate::seekbar::value_input(
            &control,
            label,
            current.as_seconds_f64(),
            0.0..=displayed_end.as_seconds_f64(),
            0.1,
            editable,
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
        output.edit =
            adjustment::values(ui, response, duration, preview, plan, gain_preview, enabled);
    }
    output.gain_preview = gain_preview.or(match output.edit {
        Some(TimelineEdit::SetVolume(range, gain)) => Some((range, gain)),
        _ => None,
    });
    if let Some(preview) = output.gain_preview {
        // Preserve release paint in discarded passes without repeating the edit.
        ui.ctx()
            .data_mut(|data| data.insert_temp(gain_preview_id, (frame, preview)));
    }
    output
}

fn cti_x(rect: Rect, x: f32, pixel: f32) -> f32 {
    let left =
        ((x / pixel).floor() * pixel).clamp(rect.left(), (rect.right() - pixel).max(rect.left()));
    left + pixel.min(rect.width()) * 0.5
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
    fn selection_endpoints_and_cti_share_the_same_physical_pixel() {
        for density in [1.0, 1.25, 1.5, 2.0] {
            let context = egui::Context::default();
            let rect =
                Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 100.0)) / density;
            for (start, end) in [
                (0.0, 10.0),
                (2.0, 6.0),
                (2.013, 6.007),
                (4.0, 4.001),
                (9.999, 10.0),
            ] {
                let selection = TimeRange::new(time(start), time(end)).expect("selected interval");
                for position in [selection.start(), selection.end()] {
                    let mut input = egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(500.0, 200.0),
                        )),
                        ..Default::default()
                    };
                    input
                        .viewports
                        .get_mut(&egui::ViewportId::ROOT)
                        .expect("viewport")
                        .native_pixels_per_point = Some(density);
                    let output = context.run_ui(input, |ui| {
                        let response = ui.interact(
                            rect,
                            "selection-axis-test".into(),
                            egui::Sense::click_and_drag(),
                        );
                        let result = show(
                            ui,
                            &response,
                            time(10.0),
                            position,
                            Some(selection),
                            None,
                            true,
                        );
                        assert!(
                            result.seek.is_none()
                                && result.selection.is_none()
                                && result.edit.is_none()
                        );
                    });
                    assert_eq!(output.pixels_per_point, density);
                    let meshes: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh) => Some(mesh),
                            _ => None,
                        })
                        .collect();
                    let cti = meshes
                        .iter()
                        .find(|mesh| mesh.vertices.len() == 7)
                        .expect("CTI");
                    let sides = meshes
                        .iter()
                        .find(|mesh| mesh.vertices.len() > 7 && mesh.vertices.len() % 4 == 0)
                        .expect("dotted boundaries");
                    let stem = Rect::from_points(
                        &cti.vertices[3..].iter().map(|v| v.pos).collect::<Vec<_>>(),
                    );
                    let side = if position == selection.start() {
                        &sides.vertices[..4]
                    } else {
                        &sides.vertices[sides.vertices.len() - 4..]
                    };
                    let side = Rect::from_points(&side.iter().map(|v| v.pos).collect::<Vec<_>>());
                    assert!(
                        (stem.left() - side.left()).abs() * density < 0.001
                            && (stem.right() - side.right()).abs() * density < 0.001,
                        "CTI {:?} and endpoint {:?} differ at {density}, range {start}..{end}",
                        stem.x_range(),
                        side.x_range()
                    );
                    assert!((stem.width() * density - 1.0).abs() < 0.001);
                    assert!(side.left() >= rect.left() && side.right() <= rect.right());
                }
            }
        }
    }

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
                    Some(if label.ends_with(" (seconds)") {
                        format!(
                            "{}: {} · Left/Right adjust",
                            label.trim_end_matches(" (seconds)"),
                            crate::format_time_precise(time(value))
                        )
                    } else {
                        format!("{label}: {value:.3} · Left/Right adjust")
                    })
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
                    describe_focus(
                        &response,
                        false,
                        label,
                        value,
                        label.ends_with(" (seconds)"),
                    );
                    assert!(
                        focus_hint(&context).is_none(),
                        "disabled control has no hint"
                    );
                    describe_focus(&response, true, label, value, label.ends_with(" (seconds)"));
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
        frame_at(context, events, enabled, selection, time(0.0))
    }
    fn frame_at(
        context: &egui::Context,
        events: Vec<egui::Event>,
        enabled: bool,
        selection: Option<TimeRange>,
        position: MediaTime,
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
                    position,
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
                if seek {
                    assert_eq!(results.len(), 1);
                    assert_eq!(results[0].seek, Some(time(5.0)));
                    assert!(results[0].selection.is_none());
                } else {
                    assert_eq!(results.len(), if batched { 1 } else { 2 });
                    let seeks: Vec<_> = results.iter().filter_map(|result| result.seek).collect();
                    let start_time = time(f64::from((start - 20.0) / 40.0));
                    let left_time = time(f64::from((start.min(end) - 20.0) / 40.0));
                    assert_eq!(
                        seeks,
                        if !batched && start > end {
                            vec![start_time, left_time]
                        } else {
                            vec![left_time]
                        }
                    );
                    assert_eq!(
                        results.last().expect("selection commit").selection,
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
    fn playhead_clicks_small_moves_and_drags_never_clear_the_selection() {
        let selected = TimeRange::new(time(2.5), time(7.5));
        for density in [1.0, 1.25, 2.0] {
            for batched in [false, true] {
                for delta in [0.0, 2.0, 200.0] {
                    let context = egui::Context::default();
                    context.set_pixels_per_point(density);
                    frame(&context, vec![], true, selected);
                    let event = |x, pressed| egui::Event::PointerButton {
                        pos: egui::pos2(x, 34.0),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    };
                    let events = vec![
                        event(20.0, true),
                        egui::Event::PointerMoved(egui::pos2(20.0 + delta, 34.0)),
                        event(20.0 + delta, false),
                    ];
                    let mut results = Vec::new();
                    if batched {
                        results.extend(frame(&context, events, true, selected));
                    } else {
                        for event in events {
                            results.extend(frame(&context, vec![event], true, selected));
                        }
                    }
                    assert_eq!(results.len(), 1);
                    assert!(
                        results[0].selection.is_none(),
                        "head input preserves selection: delta={delta}"
                    );
                    assert!(results[0].edit.is_none());
                    assert!(
                        (results[0].seek.expect("seek").as_seconds_f64() - f64::from(delta) / 40.0)
                            .abs()
                            < 0.000001
                    );
                }
            }
        }
    }

    #[test]
    fn selection_endpoints_snap_to_the_original_head_and_release_outside_the_radius() {
        for density in [1.0, 1.25, 2.0] {
            for batched in [false, true] {
                for (start, end, resize, left, right) in [
                    (223.0, 320.0, false, 5.0, 7.5),
                    (120.0, 217.0, false, 2.5, 5.0),
                    (320.0, 223.0, false, 5.0, 7.5),
                    (225.0, 320.0, false, 5.125, 7.5),
                    (120.0, 215.0, false, 2.5, 4.875),
                    (120.0, 230.0, false, 2.5, 5.25),
                    (120.0, 217.0, true, 5.0, 7.5),
                    (320.0, 223.0, true, 2.5, 5.0),
                ] {
                    let context = egui::Context::default();
                    context.set_pixels_per_point(density);
                    let selected = resize
                        .then(|| TimeRange::new(time(2.5), time(7.5)))
                        .flatten();
                    let mut position = time(5.0);
                    frame_at(&context, vec![], true, selected, position);
                    let events = vec![
                        button(start, true),
                        egui::Event::PointerMoved(egui::pos2(220.0, 70.0)),
                        egui::Event::PointerMoved(egui::pos2(end, 70.0)),
                        button(end, false),
                    ];
                    let groups = if batched {
                        vec![events]
                    } else {
                        events.into_iter().map(|event| vec![event]).collect()
                    };
                    let mut results = Vec::new();
                    for events in groups {
                        let output = frame_at(&context, events, true, selected, position);
                        // The app applies press-time seeks before later frames.
                        // Snapping must not chase that new playback position.
                        for seek in output.iter().filter_map(|result| result.seek) {
                            position = seek;
                        }
                        results.extend(output);
                    }
                    let selections: Vec<_> = results
                        .iter()
                        .filter_map(|result| result.selection)
                        .collect();
                    assert_eq!(selections.len(), 1);
                    let range = selections[0].expect("selected range");
                    for (actual, expected) in [(range.start(), left), (range.end(), right)] {
                        // Pointer ratios are f32; snapped endpoints must still
                        // use the exact stored timestamp, not its pixel inverse.
                        assert!(
                            (actual.as_seconds_f64() - expected).abs() < 0.000001,
                            "density={density} batched={batched} {start}->{end} resize={resize}"
                        );
                        if expected == 5.0 {
                            assert_eq!(actual, time(5.0));
                        }
                    }
                    assert!(results.iter().all(|result| result.edit.is_none()));
                    if resize {
                        assert!(results.iter().all(|result| result.seek.is_none()));
                    }
                }
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
            for position in [time(0.0), time(1.003), time(5.0), time(9.997), time(10.0)] {
                let x = 20.0 + position.as_seconds_f64() as f32 * 40.0;
                for (pointer, expected) in [
                    (egui::pos2(x, 34.0), egui::CursorIcon::ResizeHorizontal),
                    (egui::pos2(x, 70.0), egui::CursorIcon::Text),
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
                    let cti = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh)
                                if mesh.vertices.len() == 7
                                    && mesh.vertices.iter().all(|vertex| {
                                        vertex.color == crate::chrome::FOREGROUND
                                    }) =>
                            {
                                Some(mesh)
                            }
                            _ => None,
                        })
                        .expect("one CTI mesh");
                    assert_eq!(cti.indices.len(), 9);
                    let stem = Rect::from_points(
                        &cti.vertices[3..]
                            .iter()
                            .map(|vertex| vertex.pos)
                            .collect::<Vec<_>>(),
                    );
                    assert!(rect.contains_rect(stem));
                    assert!((stem.width() * density - 1.0).abs() < 0.001);
                    assert_eq!(stem.top(), rect.top());
                    assert_eq!(stem.bottom(), rect.bottom());
                    let marker: Vec<_> =
                        cti.vertices[..3].iter().map(|vertex| vertex.pos).collect();
                    assert!((marker[2].x - stem.center().x).abs() < 0.0001);
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
    fn selection_release_and_cancellation_follow_event_order() {
        for interrupt in [
            egui::Event::WindowFocused(false),
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ] {
            for release_first in [true, false] {
                for batched_press in [false, true] {
                    let context = egui::Context::default();
                    let selected = TimeRange::new(time(1.0), time(2.0));
                    frame(&context, vec![], true, selected);
                    let mut events = vec![
                        button(120.0, true),
                        egui::Event::PointerMoved(egui::pos2(320.0, 70.0)),
                    ];
                    if !batched_press {
                        let press = frame(&context, events.clone(), true, selected);
                        assert_eq!(press.len(), 1);
                        assert_eq!(press[0].seek, Some(time(2.5)));
                        assert!(press[0].selection.is_none() && press[0].edit.is_none());
                        events.clear();
                    }
                    if release_first {
                        events.extend([button(320.0, false), interrupt.clone()]);
                    } else {
                        events.extend([interrupt.clone(), button(320.0, false)]);
                    }
                    let results = frame(&context, events, true, selected);
                    assert_eq!(
                        results.len(),
                        usize::from(release_first),
                        "interrupt={interrupt:?}, release_first={release_first}, batched_press={batched_press}"
                    );
                    if release_first {
                        assert_eq!(
                            results[0].selection,
                            Some(TimeRange::new(time(2.5), time(7.5)))
                        );
                        assert_eq!(results[0].seek, batched_press.then_some(time(2.5)));
                        assert!(results[0].edit.is_none());
                    }
                    assert!(!crate::timeline_input::is_active(&context));
                    assert!(frame(&context, vec![], true, selected).is_empty());
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
            assert_eq!(results[0].seek, Some(time(2.5)));
        }
    }

    #[test]
    fn gain_drag_after_selection_clear_preserves_other_bands_and_history() {
        use towavue_core::{EditHistory, EditOperation, MediaKind};

        let range = |a, b| TimeRange::new(time(a), time(b)).expect("range");
        let rect = Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 100.0));
        for density in [1.0, 1.25, 2.0] {
            for batched in [false, true] {
                for (at, gain, selected, affected) in [
                    (1.0, 1.0, None, range(0.0, 2.0)),
                    (4.0, 0.5, None, range(2.0, 6.0)),
                    (8.0, 1.0, None, range(6.0, 10.0)),
                    (4.0, 0.5, Some(range(2.0, 8.0)), range(2.0, 8.0)),
                    (4.0, 0.5, Some(range(0.0, 10.0)), range(0.0, 10.0)),
                ] {
                    let context = egui::Context::default();
                    context.enable_accesskit();
                    let mut history = EditHistory::default();
                    for edit in [
                        TimelineEdit::Delete(range(1.0, 2.0)),
                        TimelineEdit::Stretch(range(3.0, 4.0), time(2.0)),
                        TimelineEdit::SetVolume(range(2.0, 6.0), 0.5),
                    ] {
                        assert!(history.push(EditOperation::Timeline(edit), MediaKind::Audio));
                    }
                    let before = history.timeline(time(10.0)).expect("edited plan");
                    let frame = |events| {
                        let mut edits = Vec::new();
                        let mut previews = Vec::new();
                        let mut input = egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(500.0, 200.0),
                            )),
                            events,
                            ..Default::default()
                        };
                        input
                            .viewports
                            .get_mut(&egui::ViewportId::ROOT)
                            .expect("viewport")
                            .native_pixels_per_point = Some(density);
                        let painted = context.run_ui(input, |ui| {
                            let response = ui.interact(
                                rect,
                                "local-gain-test".into(),
                                egui::Sense::click_and_drag(),
                            );
                            let output = show(
                                ui,
                                &response,
                                time(10.0),
                                time(0.0),
                                selected,
                                Some(&before),
                                true,
                            );
                            assert!(output.selection.is_none() && output.seek.is_none());
                            edits.extend(output.edit);
                            previews.extend(output.gain_preview);
                            if context.current_pass_index() == 0 {
                                context.request_discard("local gain release must not replay");
                            }
                        });
                        if let Some((_, gain)) = previews.last() {
                            let labels: Vec<_> = painted
                                .shapes
                                .iter()
                                .filter_map(|shape| {
                                    if let egui::Shape::Text(text) = &shape.shape {
                                        let text = text.galley.text();
                                        if text.starts_with("Volume ")
                                            || text.starts_with("Mixed (")
                                        {
                                            return Some(text.to_owned());
                                        }
                                    }
                                    None
                                })
                                .collect();
                            assert_eq!(
                                labels,
                                [format!("Volume {:.0}%", gain * 100.0)],
                                "one current gain label, including the discarded release pass"
                            );
                            if selected.is_some() && !edits.is_empty() {
                                let tree = painted
                                    .platform_output
                                    .accesskit_update
                                    .as_ref()
                                    .expect("tree");
                                let node = tree
                                    .nodes
                                    .iter()
                                    .find(|(_, node)| node.label() == Some("Local volume (%)"))
                                    .expect("release keeps the gain control");
                                assert_eq!(node.1.numeric_value(), Some(f64::from(*gain) * 100.0));
                            }
                        }
                        (edits, previews)
                    };
                    frame(vec![]);
                    let start = egui::pos2(
                        rect.left() + rect.width() * at / 10.0,
                        adjustment::gain_y(rect, gain),
                    );
                    let end = start - egui::vec2(0.0, rect.height() * 0.1875);
                    let button = |pos, pressed| egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    };
                    frame(vec![egui::Event::PointerMoved(start)]);
                    let events = vec![
                        button(start, true),
                        egui::Event::PointerMoved(end),
                        button(end, false),
                    ];
                    let (edits, previews) = if batched {
                        frame(events)
                    } else {
                        assert!(frame(vec![events[0].clone()]).0.is_empty());
                        let (edits, previews) = frame(vec![events[1].clone()]);
                        assert!(edits.is_empty());
                        assert!(!previews.is_empty());
                        assert!(
                            previews
                                .iter()
                                .all(|preview| *preview == (affected, gain + 0.5))
                        );
                        frame(vec![events[2].clone()])
                    };
                    assert_eq!(edits, vec![TimelineEdit::SetVolume(affected, gain + 0.5)]);
                    assert!(!previews.is_empty());
                    assert!(
                        previews
                            .iter()
                            .all(|preview| *preview == (affected, gain + 0.5))
                    );
                    assert!(frame(vec![]).0.is_empty());
                    assert_eq!(history.timeline(time(10.0)), Some(before.clone()));
                    let mut expected = before.clone();
                    assert!(expected.apply(TimelineEdit::SetVolume(affected, gain + 0.5)));
                    assert!(history.push(EditOperation::Timeline(edits[0]), MediaKind::Audio));
                    assert_eq!(history.timeline(time(10.0)), Some(expected.clone()));
                    assert!(history.undo());
                    assert_eq!(history.timeline(time(10.0)), Some(before));
                    assert!(history.redo());
                    assert_eq!(history.timeline(time(10.0)), Some(expected));
                }
            }
        }
    }

    #[test]
    fn gain_and_stretch_commit_once_and_keep_press_modifiers() {
        let selected = TimeRange::new(time(2.5), time(7.5));
        for batched in [false, true] {
            for (selection, stretch, gain, travel) in [
                (None, false, 0.0, 37.5),
                (selected, false, 0.0, 37.5),
                (None, false, 0.5, 18.75),
                (selected, false, 1.5, -18.75),
                (None, false, 2.0, -37.5),
                (selected, false, 2.0, -37.5),
                (None, false, 0.0, 80.0),
                (selected, false, 2.0, -80.0),
                (selected, true, 0.0, 40.0),
            ] {
                let context = egui::Context::default();
                frame(&context, vec![], true, selection);
                let origin = egui::pos2(220.0, if stretch { 70.0 } else { 80.0 });
                let end = if stretch {
                    origin + egui::vec2(80.0, 40.0)
                } else {
                    origin + egui::vec2(10.0, travel)
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
                        TimelineEdit::SetVolume(range, gain)
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
                        vec![egui::Event::PointerMoved(
                            origin
                                + if stretch {
                                    egui::vec2(80.0, 25.0)
                                } else {
                                    egui::vec2(0.0, 25.0)
                                }
                        )],
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
    fn selection_readouts_follow_held_previews_without_committing() {
        use egui::accesskit;

        let rect = Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 100.0));
        let original = TimeRange::new(time(2.5), time(5.0));
        for density in [1.0, 1.25, 2.0] {
            for discard in [false, true] {
                for cancel in [false, true] {
                    for (selection, origin, target, modifiers) in [
                        (None, 120.0, 320.0, egui::Modifiers::NONE),
                        (None, 320.0, 120.0, egui::Modifiers::NONE),
                        (original, 220.0, 320.0, egui::Modifiers::NONE),
                        (original, 180.0, 280.0, egui::Modifiers::ALT),
                        (original, 180.0, 480.0, egui::Modifiers::ALT),
                    ] {
                        let preview_length = if target > rect.right() { 10.0 } else { 5.0 };
                        let expected_labels = [
                            "In 00:00:02:500".to_owned(),
                            format!(
                                "Out {}",
                                crate::format_time_precise(time(2.5 + preview_length))
                            ),
                        ];
                        let context = egui::Context::default();
                        context.set_pixels_per_point(density);
                        context.enable_accesskit();
                        let draw = |events| {
                            let mut results = Vec::new();
                            let output = context.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(500.0, 200.0),
                                    )),
                                    events,
                                    ..Default::default()
                                },
                                |ui| {
                                    let response = ui.interact(
                                        rect,
                                        "readout-preview".into(),
                                        egui::Sense::click_and_drag(),
                                    );
                                    let result = show(
                                        ui,
                                        &response,
                                        time(10.0),
                                        time(0.0),
                                        selection,
                                        None,
                                        true,
                                    );
                                    if result.seek.is_some()
                                        || result.selection.is_some()
                                        || result.edit.is_some()
                                    {
                                        results.push(result);
                                    }
                                    if discard && context.current_pass_index() == 0 {
                                        context.request_discard(
                                            "readout preview must survive a discarded pass",
                                        );
                                    }
                                },
                            );
                            let labels: Vec<_> = output
                                .shapes
                                .iter()
                                .filter_map(|shape| {
                                    if let egui::Shape::Text(text) = &shape.shape {
                                        let text = text.galley.text();
                                        if text.starts_with("In ") || text.starts_with("Out ") {
                                            return Some(text.to_owned());
                                        }
                                    }
                                    None
                                })
                                .collect();
                            if labels == expected_labels {
                                let lengths: Vec<_> = output
                                    .shapes
                                    .iter()
                                    .filter_map(|shape| {
                                        if let egui::Shape::Text(text) = &shape.shape {
                                            return text
                                                .galley
                                                .text()
                                                .starts_with("Length ")
                                                .then(|| text.galley.text().to_owned());
                                        }
                                        None
                                    })
                                    .collect();
                                assert_eq!(
                                    lengths,
                                    [format!(
                                        "Length {}",
                                        crate::format_time_precise(time(preview_length))
                                    )],
                                    "one current length label, including the discarded release pass"
                                );
                            }
                            let tree = output
                                .platform_output
                                .accesskit_update
                                .expect("accessibility tree");
                            if labels == expected_labels
                                && discard
                                && results.iter().any(|result| {
                                    result.selection.is_some() || result.edit.is_some()
                                })
                            {
                                let node = tree
                                    .nodes
                                    .iter()
                                    .find(|(_, node)| {
                                        node.label() == Some("Selected duration (seconds)")
                                    })
                                    .expect("release keeps the duration control");
                                assert_eq!(node.1.numeric_value(), Some(preview_length));
                                if target > rect.right() {
                                    assert!(!node.1.supports_action(accesskit::Action::SetValue));
                                }
                            }
                            for (prefix, name) in [
                                ("In ", "Time selection start (seconds)"),
                                ("Out ", "Time selection end (seconds)"),
                            ] {
                                if let Some(value) =
                                    labels.iter().find_map(|label| label.strip_prefix(prefix))
                                {
                                    let parts: Vec<f64> = value
                                        .split(':')
                                        .map(|part| part.parse().expect("clock field"))
                                        .collect();
                                    assert_eq!(parts.len(), 4);
                                    let value = parts[0] * 3600.0
                                        + parts[1] * 60.0
                                        + parts[2]
                                        + parts[3] / 1000.0;
                                    let node = tree
                                        .nodes
                                        .iter()
                                        .find(|(_, node)| node.label() == Some(name))
                                        .expect("endpoint node");
                                    assert_eq!(
                                        node.1.numeric_value(),
                                        Some(value),
                                        "accessible and visible endpoints agree"
                                    );
                                    assert!(
                                        node.1.max_numeric_value().expect("upper bound") >= value
                                    );
                                    if labels == expected_labels && target > rect.right() {
                                        for action in [
                                            accesskit::Action::SetValue,
                                            accesskit::Action::Increment,
                                            accesskit::Action::Decrement,
                                        ] {
                                            assert!(!node.1.supports_action(action));
                                        }
                                    }
                                }
                            }
                            (labels, results)
                        };
                        let button = |x, pressed| egui::Event::PointerButton {
                            pos: egui::pos2(x, 70.0),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers,
                        };
                        draw(vec![]);
                        draw(vec![button(origin, true)]);
                        let (labels, actions) =
                            draw(vec![egui::Event::PointerMoved(egui::pos2(target, 70.0))]);
                        assert!(
                            actions.is_empty(),
                            "held preview must not commit or seek again"
                        );
                        assert_eq!(
                            labels, expected_labels,
                            "readouts follow the displayed range"
                        );
                        let (labels, actions) = draw(vec![if cancel {
                            egui::Event::Key {
                                key: egui::Key::Escape,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            }
                        } else {
                            button(target, false)
                        }]);
                        if cancel {
                            assert!(actions.is_empty());
                            let expected = if selection.is_some() {
                                vec!["In 00:00:02:500", "Out 00:00:05:000"]
                            } else {
                                vec![]
                            };
                            assert_eq!(labels, expected, "cancel restores the committed readout");
                        } else {
                            assert_eq!(labels, expected_labels);
                            assert_eq!(actions.len(), 1, "release commits once");
                            if modifiers.alt {
                                assert_eq!(
                                    actions[0].edit,
                                    Some(TimelineEdit::Stretch(
                                        original.expect("selection"),
                                        time(preview_length)
                                    ))
                                );
                            } else {
                                assert_eq!(
                                    actions[0].selection,
                                    Some(TimeRange::new(time(2.5), time(7.5)))
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn selection_press_and_held_preview_keep_the_cti_through_discarded_passes() {
        let rect = Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 100.0));
        for density in [1.0, 1.25, 2.0] {
            for reverse in [false, true] {
                let context = egui::Context::default();
                context.set_pixels_per_point(density);
                let draw = |events| {
                    let mut results = Vec::new();
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(500.0, 200.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            let response = ui.interact(
                                rect,
                                "cti-press-test".into(),
                                egui::Sense::click_and_drag(),
                            );
                            let result =
                                show(ui, &response, time(10.0), time(9.0), None, None, true);
                            if result.seek.is_some()
                                || result.selection.is_some()
                                || result.edit.is_some()
                            {
                                results.push(result);
                            }
                            if context.current_pass_index() == 0 {
                                context.request_discard("test final CTI paint");
                            }
                        },
                    );
                    let axis = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh) if mesh.vertices.len() == 7 => {
                                Some(mesh.vertices[2].pos.x)
                            }
                            _ => None,
                        })
                        .expect("CTI mesh survives the discarded input pass");
                    (results, axis, output.platform_output.cursor_icon)
                };
                draw(vec![]);
                let start = if reverse { 320.0 } else { 120.0 };
                let end = if reverse { 120.0 } else { 320.0 };
                let (press, axis, cursor) = draw(vec![button(start, true)]);
                assert_eq!(cursor, egui::CursorIcon::Text);
                assert_eq!(press.len(), 1);
                assert_eq!(press[0].seek, Some(time(if reverse { 7.5 } else { 2.5 })));
                assert_eq!(axis, cti_x(rect, start, 1.0 / density));
                let (held, axis, cursor) =
                    draw(vec![egui::Event::PointerMoved(egui::pos2(end, 70.0))]);
                assert!(
                    held.is_empty(),
                    "held selection does not restart the pipeline"
                );
                assert_eq!(axis, cti_x(rect, 120.0, 1.0 / density));
                assert_eq!(cursor, egui::CursorIcon::Text);
                let (released, axis, _) = draw(vec![button(end, false)]);
                assert_eq!(released.len(), 1);
                assert_eq!(
                    released[0].selection,
                    Some(TimeRange::new(time(2.5), time(7.5)))
                );
                assert_eq!(released[0].seek, reverse.then_some(time(2.5)));
                assert_eq!(axis, cti_x(rect, 120.0, 1.0 / density));
                assert!(draw(vec![]).0.is_empty());
                let (_, axis, cursor) = draw(vec![
                    egui::Event::PointerButton {
                        pos: egui::pos2(380.0, 34.0),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerMoved(egui::pos2(220.0, 70.0)),
                ]);
                assert_eq!(cursor, egui::CursorIcon::ResizeHorizontal);
                assert_eq!(axis, cti_x(rect, 220.0, 1.0 / density));
                let (released, _, _) = draw(vec![button(220.0, false)]);
                assert_eq!(released.len(), 1);
                assert_eq!(released[0].seek, Some(time(5.0)));
            }
        }
    }

    #[test]
    fn volume_line_locks_first_direction_but_horizontal_drags_still_select() {
        for vertical in [false, true] {
            let context = egui::Context::default();
            frame(&context, vec![], true, None);
            let origin = egui::pos2(120.0, 80.0);
            let end = egui::pos2(320.0, 117.5);
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
            assert_eq!(results[0].seek, (!vertical).then_some(time(2.5)));
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
        app.timeline_open = true;
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
