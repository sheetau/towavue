use egui::{Align2, Color32, Rect, Ui};
use towavue_core::{EditOperation, EditState, MediaTime};

pub fn timeline(
    ui: &Ui,
    rect: Rect,
    state: &EditState,
    duration: MediaTime,
    source_preview: bool,
    identity: egui::Id,
) -> Option<EditOperation> {
    let mut preview = state.clone();
    let mut chosen = None;
    let mut dragging = false;
    let mut grips = Vec::new();
    let (cancelled, release) = ui.input(|input| {
        (
            !input.focused
                || input.key_pressed(egui::Key::Escape)
                || input.events.contains(&egui::Event::WindowFocused(false)),
            input.events.iter().rev().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => Some(*pos),
                _ => None,
            }),
        )
    });
    for start in [true, false] {
        let time = if start {
            state.trim_start.unwrap_or(MediaTime::ZERO)
        } else {
            state.trim_end.unwrap_or(duration)
        };
        let x = egui::lerp(
            rect.x_range(),
            (time.as_seconds_f64() / duration.as_seconds_f64()) as f32,
        );
        // Separate lanes keep both endpoints reachable when their x positions overlap.
        let lane_height = ((rect.height() - 40.0) / 2.0).max(1.0);
        let y = rect.top() + 20.0 + lane_height * if start { 0.5 } else { 1.5 };
        let grip_rect = |x: f32| {
            Rect::from_center_size(
                egui::pos2(x.clamp(rect.left() + 7.0, rect.right() - 7.0), y),
                egui::vec2(14.0, lane_height),
            )
        };
        let response = ui
            .interact(
                grip_rect(x),
                identity.with(start),
                egui::Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
            .on_hover_text(if start {
                "Drag trim start · release to apply · Escape to cancel"
            } else {
                "Drag trim end · release to apply · Escape to cancel"
            });
        let mut grip_x = x;
        let mut valid = true;
        if response.dragged_by(egui::PointerButton::Primary)
            || response.drag_stopped_by(egui::PointerButton::Primary)
        {
            if cancelled {
                ui.ctx().stop_dragging();
            } else if let Some(pointer) = release.or(response.interact_pointer_pos()) {
                dragging = true;
                grip_x = pointer.x.clamp(rect.left(), rect.right());
                let ratio = (f64::from(grip_x) - f64::from(rect.left())) / f64::from(rect.width());
                let target = MediaTime::from_nanoseconds(
                    (duration.as_nanoseconds() as f64 * ratio).round() as i64,
                );
                let operation = if start {
                    EditOperation::SetTrimStart(target)
                } else {
                    EditOperation::SetTrimEnd(target)
                };
                let mut candidate = state.clone();
                if start {
                    candidate.trim_start = Some(target);
                } else {
                    candidate.trim_end = Some(target);
                }
                valid = candidate.trim_is_valid(Some(duration));
                if valid {
                    preview = candidate;
                }
                if response.drag_stopped_by(egui::PointerButton::Primary) {
                    chosen = Some(operation);
                }
            }
        }
        grips.push((grip_rect(grip_x), valid, response.hovered()));
    }
    show(ui, rect, &preview, duration, source_preview && !dragging);
    let painter = ui.painter().with_clip_rect(rect);
    for (grip, valid, hovered) in grips {
        painter.rect_filled(grip, 2.0, Color32::from_gray(if hovered { 65 } else { 38 }));
        let padding = (grip.height() * 0.25).min(3.0);
        painter.vline(
            grip.center().x,
            (grip.top() + padding)..=(grip.bottom() - padding),
            (
                2.0,
                if valid {
                    Color32::WHITE
                } else {
                    Color32::LIGHT_RED
                },
            ),
        );
    }
    if dragging {
        painter.text(
            rect.left_bottom() + egui::vec2(20.0, -4.0),
            Align2::LEFT_BOTTOM,
            "Release to apply · Esc to cancel",
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
    }
    chosen
}

pub fn label(state: &EditState, duration: MediaTime) -> Option<String> {
    if state.trim_start.is_none() && state.trim_end.is_none() {
        return None;
    }
    Some(format!(
        "Trim {} – {} · playback and export",
        timestamp(state.trim_start.unwrap_or(MediaTime::ZERO)),
        timestamp(state.trim_end.unwrap_or(duration)),
    ))
}

pub fn show(ui: &Ui, rect: Rect, state: &EditState, duration: MediaTime, source_preview: bool) {
    let Some(mut label) = label(state, duration) else {
        return;
    };
    let position = |time: MediaTime| {
        egui::lerp(
            rect.x_range(),
            (time.as_seconds_f64() / duration.as_seconds_f64()).clamp(0.0, 1.0) as f32,
        )
    };
    let start = position(state.trim_start.unwrap_or(MediaTime::ZERO));
    let end = position(state.trim_end.unwrap_or(duration));
    let painter = ui.painter().with_clip_rect(rect);
    let font = egui::FontId::proportional(12.0);
    let fits = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= rect.width() - 12.0
    };
    if !fits(&label) {
        label = format!(
            "Trim {} – {}",
            timestamp(state.trim_start.unwrap_or(MediaTime::ZERO)),
            timestamp(state.trim_end.unwrap_or(duration)),
        );
    }
    for excluded in [
        Rect::from_min_max(rect.min, egui::pos2(start, rect.bottom())),
        Rect::from_min_max(egui::pos2(end, rect.top()), rect.max),
    ] {
        painter.rect_filled(excluded, 0.0, Color32::from_black_alpha(160));
    }
    painter.hline(start..=end, rect.bottom() - 2.0, (2.0, Color32::WHITE));
    for x in [start, end] {
        painter.vline(
            x.clamp(rect.left() + 1.0, rect.right() - 1.0),
            rect.y_range(),
            (1.0, Color32::WHITE),
        );
    }
    painter.rect_filled(
        Rect::from_min_size(rect.min, egui::vec2(rect.width(), 20.0)),
        0.0,
        Color32::from_black_alpha(210),
    );
    painter.text(
        rect.min + egui::vec2(6.0, 3.0),
        Align2::LEFT_TOP,
        label,
        font.clone(),
        Color32::WHITE,
    );
    if source_preview {
        let label = if fits("Outside trim · Play returns to start") {
            "Outside trim · Play returns to start"
        } else {
            "Outside trim · Play → start"
        };
        painter.text(
            rect.left_bottom() + egui::vec2(6.0, -6.0),
            Align2::LEFT_BOTTOM,
            label,
            font,
            Color32::WHITE,
        );
    }
}

fn timestamp(time: MediaTime) -> String {
    let millis = time.as_nanoseconds().max(0) / 1_000_000;
    format!(
        "{:02}:{:02}.{:03}",
        millis / 60_000,
        millis / 1_000 % 60,
        millis % 1_000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_endpoint_positions_remain_reachable_and_crossing_is_marked_invalid() {
        let state = EditState {
            trim_start: Some(MediaTime::from_nanoseconds(4_999_000_000)),
            trim_end: Some(MediaTime::from_nanoseconds(5_001_000_000)),
            ..Default::default()
        };
        for (start, target_x, invalid) in [
            (true, 140.0, false),
            (false, 300.0, false),
            (true, 300.0, true),
        ] {
            let context = egui::Context::default();
            let rect = Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(400.0, 100.0));
            let frame = |events| {
                let mut chosen = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        if let Some(operation) = timeline(
                            ui,
                            rect,
                            &state,
                            MediaTime::from_nanoseconds(10_000_000_000),
                            false,
                            egui::Id::new("overlap"),
                        ) {
                            chosen.push(operation);
                        }
                    },
                );
                (output, chosen)
            };
            for _ in 0..3 {
                frame(vec![]);
            }
            let y = if start { 55.0 } else { 85.0 };
            let origin = egui::pos2(220.0, y);
            let target = egui::pos2(target_x, y);
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(vec![egui::Event::PointerMoved(origin)]);
            frame(vec![button(origin, true)]);
            let (output, chosen) = frame(vec![egui::Event::PointerMoved(target)]);
            assert!(chosen.is_empty());
            let has_invalid_grip = output.shapes.iter().any(|shape| matches!(
                &shape.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == Color32::LIGHT_RED
            ));
            assert_eq!(has_invalid_grip, invalid);
            let (_, chosen) = frame(vec![button(target, false)]);
            let target = MediaTime::from_nanoseconds(if target_x == 140.0 {
                3_000_000_000
            } else {
                7_000_000_000
            });
            assert_eq!(
                chosen,
                [if start {
                    EditOperation::SetTrimStart(target)
                } else {
                    EditOperation::SetTrimEnd(target)
                }]
            );
        }
    }

    #[test]
    fn trim_grips_commit_once_without_seek_and_cancel_on_escape_focus_or_identity_change() {
        for (start, cancel) in [
            (true, 0),
            (false, 0),
            (true, 1),
            (true, 2),
            (true, 3),
            (true, 4),
            (false, 4),
            (true, 5),
            (false, 5),
        ] {
            let context = egui::Context::default();
            let rect = Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(400.0, 100.0));
            let state = EditState::default();
            let duration = MediaTime::from_nanoseconds(10_000_000_000);
            let frame = |events, focused, identity| {
                let mut edits = Vec::new();
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(480.0, 200.0),
                        )),
                        events,
                        focused,
                        ..Default::default()
                    },
                    |ui| {
                        let seek = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                        if let Some(operation) =
                            timeline(ui, rect, &state, duration, false, egui::Id::new(identity))
                        {
                            edits.push(operation);
                        }
                        assert!(
                            !seek.clicked() && !seek.drag_stopped(),
                            "a trim gesture must not seek"
                        );
                    },
                );
                edits
            };
            for _ in 0..3 {
                frame(vec![], true, 0);
            }
            let y = if start { 55.0 } else { 85.0 };
            let origin = egui::pos2(if start { 27.0 } else { 413.0 }, y);
            let target = egui::pos2(220.0, y);
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            for events in [
                vec![egui::Event::PointerMoved(origin)],
                vec![button(origin, true)],
                vec![egui::Event::PointerMoved(
                    origin + egui::vec2(if start { 20.0 } else { -20.0 }, 0.0),
                )],
                vec![egui::Event::PointerMoved(target)],
            ] {
                assert!(frame(events, true, 0).is_empty());
            }
            if cancel == 1 {
                frame(
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    true,
                    0,
                );
            }
            if cancel == 2 {
                frame(vec![], false, 0);
            }
            let identity = usize::from(cancel == 3);
            if cancel == 3 {
                frame(vec![], true, identity);
            }
            let mut release = vec![
                button(target, false),
                egui::Event::PointerMoved(egui::pos2(380.0, y)),
                egui::Event::PointerGone,
            ];
            if cancel == 4 {
                release.extend([
                    egui::Event::WindowFocused(false),
                    egui::Event::WindowFocused(true),
                ]);
            }
            if cancel == 5 {
                release.push(egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let edits = frame(release, true, identity);
            let expected = if start {
                EditOperation::SetTrimStart(MediaTime::from_nanoseconds(5_000_000_000))
            } else {
                EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(5_000_000_000))
            };
            assert_eq!(edits, if cancel == 0 { vec![expected] } else { vec![] });
            assert!(frame(vec![], true, identity).is_empty());
            assert_eq!(state, EditState::default());
        }
    }

    #[test]
    fn narrow_trim_feedback_keeps_both_source_endpoints_and_play_hint_visible() {
        let context = egui::Context::default();
        let rect = Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(224.0, 50.0));
        let state = EditState {
            trim_start: Some(MediaTime::from_nanoseconds(2_500_000_000)),
            trim_end: Some(MediaTime::from_nanoseconds(62_750_000_000)),
            ..Default::default()
        };
        let output = context.run_ui(Default::default(), |ui| {
            timeline(
                ui,
                rect,
                &state,
                MediaTime::from_nanoseconds(120_000_000_000),
                true,
                egui::Id::new("narrow"),
            );
        });
        let texts = output
            .shapes
            .iter()
            .filter_map(|shape| {
                let egui::Shape::Text(text) = &shape.shape else {
                    return None;
                };
                let bounds = Rect::from_min_size(text.pos, text.galley.size());
                assert!(
                    shape.clip_rect.contains_rect(bounds),
                    "{} is clipped: {bounds:?}",
                    text.galley.text()
                );
                for shape in &output.shapes {
                    if let egui::Shape::Rect(grip) = &shape.shape
                        && grip.fill == Color32::from_gray(38)
                    {
                        assert!(
                            grip.rect.intersect(bounds).area() <= 0.0,
                            "grip obscures feedback"
                        );
                    }
                }
                Some(text.galley.text())
            })
            .collect::<Vec<_>>();
        assert!(texts.contains(&"Trim 00:02.500 – 01:02.750"));
        assert!(
            texts
                .iter()
                .any(|text| text.contains("Play") && text.contains("start"))
        );
    }

    #[test]
    fn trim_overlay_shades_only_excluded_source_intervals() {
        let context = egui::Context::default();
        let rect = Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(420.0, 130.0));
        let state = EditState {
            trim_start: Some(MediaTime::from_nanoseconds(2_500_000_000)),
            trim_end: Some(MediaTime::from_nanoseconds(7_500_000_000)),
            ..Default::default()
        };
        let output = context.run_ui(Default::default(), |ui| {
            show(
                ui,
                rect,
                &state,
                MediaTime::from_nanoseconds(10_000_000_000),
                false,
            );
        });
        let shaded: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(shade) if shade.fill == Color32::from_black_alpha(160) => {
                    assert_eq!(shape.clip_rect, rect);
                    Some(shade.rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            shaded,
            [
                Rect::from_min_max(rect.min, egui::pos2(120.0, 130.0)),
                Rect::from_min_max(egui::pos2(320.0, 30.0), rect.max),
            ]
        );
    }

    #[test]
    fn trim_label_preserves_subsecond_endpoints_and_implicit_source_edges() {
        let duration = MediaTime::from_nanoseconds(30_000_000_000);
        let mut state = EditState::default();
        assert_eq!(label(&state, duration), None);
        state.trim_end = Some(MediaTime::from_nanoseconds(2_833_333_333));
        assert_eq!(
            label(&state, duration).as_deref(),
            Some("Trim 00:00.000 – 00:02.833 · playback and export")
        );
        state.trim_start = state.trim_end.take();
        assert_eq!(
            label(&state, duration).as_deref(),
            Some("Trim 00:02.833 – 00:30.000 · playback and export")
        );
    }
}
