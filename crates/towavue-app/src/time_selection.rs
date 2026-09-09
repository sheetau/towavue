use egui::{Color32, Rect, Response, Ui};
use towavue_core::{MediaTime, TimeRange};

#[derive(Default)]
pub(super) struct Output {
    pub selection: Option<Option<TimeRange>>,
    pub seek: Option<MediaTime>,
}

pub(super) fn show(
    ui: &Ui,
    response: &Response,
    duration: MediaTime,
    position: MediaTime,
    selection: Option<TimeRange>,
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
    let drag = if enabled {
        crate::timeline_input::seek_drag(response)
    } else {
        crate::timeline_input::cancel(ui.ctx());
        Default::default()
    };
    let mode_id = response.id.with("time-selection-mode");
    if let (Some(origin), Some(pointer)) = (drag.origin, drag.position) {
        if drag.started {
            ui.ctx().data_mut(|data| {
                data.insert_temp(mode_id, (origin.x - x_at(position)).abs() <= 8.0)
            });
        }
        let seek = ui.ctx().data_mut(|data| {
            *data.get_temp_mut_or_insert_with(mode_id, || (origin.x - x_at(position)).abs() <= 8.0)
        });
        if drag.dragging && !seek {
            let a = at(origin.x);
            let b = at(pointer.x);
            preview = TimeRange::new(a.min(b), a.max(b));
            if drag.released {
                output.selection = Some(preview);
            }
        } else {
            head = at(pointer.x);
            if drag.released {
                output.seek = Some(head);
                if !drag.dragging && selection.is_some() {
                    output.selection = Some(None);
                }
            }
        }
        if drag.released {
            ui.ctx().data_mut(|data| data.remove::<bool>(mode_id));
        }
    } else {
        ui.ctx().data_mut(|data| data.remove::<bool>(mode_id));
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
        painter.rect_stroke(
            Rect::from_min_max(
                egui::pos2(x_at(range.start()), rect.top() + 1.0),
                egui::pos2(x_at(range.end()), rect.bottom() - 1.0),
            ),
            0.0,
            (1.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
    painter.vline(x_at(head), rect.y_range(), (2.0, crate::chrome::FOREGROUND));
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
        if control.has_focus() {
            painter.rect_stroke(
                bounds.shrink(1.0),
                0.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        if let Some(value) = crate::seekbar::value_input(
            &control,
            if start {
                "Time selection start (seconds)"
            } else {
                "Time selection end (seconds)"
            },
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
    output
}

#[cfg(test)]
mod tests {
    use super::*;
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
                let result = show(ui, &response, time(10.0), time(0.0), selection, enabled);
                if result.seek.is_some() || result.selection.is_some() {
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
            for (start, end, seek) in [
                (120.0, 320.0, false),
                (320.0, 120.0, false),
                (20.0, 220.0, true),
                (220.0, 220.0, true),
            ] {
                let context = egui::Context::default();
                frame(&context, vec![], true, None);
                let events = vec![
                    button(start, true),
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
                        Some(TimeRange::new(time(2.5), time(7.5)))
                    );
                }
                assert!(frame(&context, vec![], true, None).is_empty());
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
        let stale = UiAction::TimeSelection(tab, app.generation.next(), None);
        app.handle_ui_action(stale);
        assert_eq!(app.time_selection, selected);
        app.pending_guard = Some(GuardedAction::CloseTab(tab));
        app.handle_ui_action(UiAction::TimeSelection(tab, app.generation, None));
        app.dispatch(CommandId::DeleteTimeSelection);
        assert_eq!(app.time_selection, selected);
        assert!(app.edits.is_empty());
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
