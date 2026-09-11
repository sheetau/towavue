use egui::{Context, Event, Id, Pos2, Response};

#[derive(Clone, Default)]
struct Position {
    frame: Option<u64>,
    start: Option<Pos2>,
    end: Option<Pos2>,
}

pub fn begin_frame(context: &Context) {
    let frame = context.cumulative_frame_nr();
    let end = context.input(|input| input.pointer.hover_pos());
    context.data_mut(|data| {
        let position = data.get_temp_mut_or_default::<Position>(Id::new("wheel-position"));
        if position.frame != Some(frame) {
            position.start = position.end;
            position.end = end;
            position.frame = Some(frame);
        }
    });
}

pub fn volume_delta(context: &Context, targets: &[Response]) -> f32 {
    positioned_events(context)
        .into_iter()
        .filter_map(|(position, event)| match event {
            Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } if modifiers.is_none()
                && position.is_some_and(|pos| {
                    targets.iter().any(|target| {
                        target.enabled()
                            && target.interact_rect.contains(pos)
                            && context.layer_id_at(pos) == Some(target.layer_id)
                    })
                }) =>
            {
                Some(
                    delta.y
                        / if unit == egui::MouseWheelUnit::Point {
                            50.0
                        } else {
                            1.0
                        },
                )
            }
            _ => None,
        })
        .sum()
}

pub fn zoom_events(context: &Context, target: &Response) -> Vec<(Pos2, f32)> {
    if !target.enabled()
        || context.input(|input| {
            !input.focused
                || input
                    .events
                    .iter()
                    .any(|event| matches!(event, Event::WindowFocused(false)))
        })
    {
        return Vec::new();
    }
    if let Some(touch) = context.input(|input| input.multi_touch()) {
        return context
            .input(|input| input.pointer.hover_pos())
            .filter(|_| target.hovered() && touch.zoom_delta != 1.0)
            .map(|position| (position, touch.zoom_delta))
            .into_iter()
            .collect();
    }
    if context.input(|input| {
        input.pointer.any_down()
            || input
                .events
                .iter()
                .any(|event| matches!(event, Event::PointerButton { pressed: true, .. }))
    }) {
        return Vec::new();
    }
    let options = context.options(|options| options.input_options);
    let page_height = context.input(|input| input.viewport_rect().height());
    positioned_events(context)
        .into_iter()
        .filter_map(|(position, event)| {
            let position = position.filter(|pos| {
                target.interact_rect.contains(*pos)
                    && context.layer_id_at(*pos) == Some(target.layer_id)
            })?;
            let factor = match event {
                Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    phase: egui::TouchPhase::Move,
                } if modifiers.matches_any(options.zoom_modifier) => {
                    let points = match unit {
                        egui::MouseWheelUnit::Point => 1.0,
                        egui::MouseWheelUnit::Line => options.line_scroll_speed,
                        egui::MouseWheelUnit::Page => page_height,
                    };
                    (options.scroll_zoom_speed * points * (delta.x + delta.y)).exp()
                }
                Event::Zoom(factor) => factor,
                _ => return None,
            };
            (factor != 1.0).then_some((position, factor))
        })
        .collect()
}

fn positioned_events(context: &Context) -> Vec<(Option<Pos2>, Event)> {
    begin_frame(context);
    let mut position = context.data(|data| {
        data.get_temp::<Position>(Id::new("wheel-position"))
            .and_then(|position| position.start)
    });
    let events = context.input(|input| input.events.clone());
    let mut result = Vec::new();
    for event in events {
        match event {
            Event::PointerMoved(pos) | Event::PointerButton { pos, .. } => position = Some(pos),
            Event::PointerGone | Event::WindowFocused(false) => position = None,
            Event::MouseWheel { .. } | Event::Zoom(_) => result.push((position, event)),
            _ => {}
        }
    }
    result
}

#[derive(Default)]
pub struct Scroll {
    input: egui::InputState,
    frame: Option<u64>,
}

impl Scroll {
    pub fn clear(&mut self) {
        self.input = egui::InputState::default();
    }

    pub fn delta(&mut self, ui: &egui::Ui, rect: egui::Rect, enabled: bool) -> egui::Vec2 {
        let context = ui.ctx();
        let frame = context.cumulative_frame_nr();
        if self.frame == Some(frame) {
            return egui::Vec2::ZERO;
        }
        self.frame = Some(frame);
        if !enabled
            || !ui.is_enabled()
            || egui::Popup::is_any_open(context)
            || context.input(|input| {
                !input.focused
                    || input.pointer.any_down()
                    || input.events.iter().any(|event| {
                        matches!(
                            event,
                            Event::WindowFocused(false)
                                | Event::PointerButton { pressed: true, .. }
                        )
                    })
            })
        {
            self.clear();
            return egui::Vec2::ZERO;
        }
        let events = positioned_events(context)
            .into_iter()
            .filter_map(|(position, event)| {
                let ends = matches!(
                    event,
                    Event::MouseWheel {
                        phase: egui::TouchPhase::End | egui::TouchPhase::Cancel,
                        ..
                    }
                );
                (ends
                    || position.is_some_and(|pos| {
                        rect.contains(pos) && context.layer_id_at(pos) == Some(ui.layer_id())
                    }))
                .then_some(event)
            })
            .collect();
        let input = context.input(|input| egui::RawInput {
            screen_rect: Some(rect),
            time: Some(input.time),
            predicted_dt: input.stable_dt,
            events,
            ..Default::default()
        });
        self.input = std::mem::take(&mut self.input).begin_pass(
            input,
            true,
            context.pixels_per_point(),
            context.options(|options| options.input_options),
        );
        if self.input.is_scrolling() {
            context.request_repaint();
        }
        self.input.smooth_scroll_delta()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_zoom_preserves_units_event_positions_and_input_ownership() {
        let context = Context::default();
        let inside = egui::pos2(100.0, 100.0);
        let outside = egui::pos2(350.0, 250.0);
        let wheel = |unit, delta, modifiers, phase| Event::MouseWheel {
            unit,
            delta: egui::vec2(0.0, delta),
            modifiers,
            phase,
        };
        let frame = |events, enabled, focused, extra_pass| {
            let mut zooms = Vec::new();
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events,
                    focused,
                    ..Default::default()
                },
                |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        begin_frame(ui.ctx());
                        let target = ui.allocate_rect(
                            egui::Rect::from_min_size(
                                egui::pos2(20.0, 20.0),
                                egui::vec2(200.0, 160.0),
                            ),
                            egui::Sense::hover(),
                        );
                        zooms.extend(zoom_events(ui.ctx(), &target));
                        if extra_pass && ui.ctx().current_pass_index() == 0 {
                            ui.ctx().request_discard("verify wheel is not replayed");
                        }
                    });
                },
            );
            zooms
        };
        for _ in 0..3 {
            frame(vec![Event::PointerMoved(inside)], true, true, false);
        }
        let options = context.options(|options| options.input_options);
        for (unit, delta, points) in [
            (egui::MouseWheelUnit::Point, 60.0, 60.0),
            (
                egui::MouseWheelUnit::Line,
                -2.0,
                -2.0 * options.line_scroll_speed,
            ),
            (egui::MouseWheelUnit::Page, 0.1, 30.0),
        ] {
            let zooms = frame(
                vec![wheel(
                    unit,
                    delta,
                    egui::Modifiers::CTRL,
                    egui::TouchPhase::Move,
                )],
                true,
                true,
                true,
            );
            assert_eq!(zooms.len(), 1);
            assert_eq!(zooms[0].0, inside);
            assert!((zooms[0].1 - (points * options.scroll_zoom_speed).exp()).abs() < 0.0001);
            assert!(frame(vec![], true, true, false).is_empty());
        }
        for phase in [
            egui::TouchPhase::Start,
            egui::TouchPhase::End,
            egui::TouchPhase::Cancel,
        ] {
            assert!(
                frame(
                    vec![wheel(
                        egui::MouseWheelUnit::Point,
                        60.0,
                        egui::Modifiers::CTRL,
                        phase
                    )],
                    true,
                    true,
                    false
                )
                .is_empty()
            );
        }
        let zoom = || {
            wheel(
                egui::MouseWheelUnit::Point,
                60.0,
                egui::Modifiers::CTRL,
                egui::TouchPhase::Move,
            )
        };
        let zooms = frame(
            vec![
                zoom(),
                Event::PointerMoved(outside),
                zoom(),
                Event::PointerMoved(inside),
                Event::Zoom(1.25),
                Event::PointerGone,
                zoom(),
            ],
            true,
            true,
            false,
        );
        assert_eq!(zooms.len(), 2);
        assert_eq!(zooms[1], (inside, 1.25));
        assert!(frame(vec![zoom(), Event::PointerMoved(inside)], true, true, false).is_empty());
        for (enabled, focused, modifiers, press) in [
            (false, true, egui::Modifiers::CTRL, false),
            (true, false, egui::Modifiers::CTRL, false),
            (true, true, egui::Modifiers::NONE, false),
            (true, true, egui::Modifiers::CTRL, true),
        ] {
            let mut events = vec![
                Event::PointerMoved(inside),
                wheel(
                    egui::MouseWheelUnit::Point,
                    60.0,
                    modifiers,
                    egui::TouchPhase::Move,
                ),
            ];
            if press {
                for pressed in [true, false] {
                    events.push(Event::PointerButton {
                        pos: inside,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers,
                    });
                }
            }
            assert!(frame(events, enabled, focused, false).is_empty());
        }
    }

    #[test]
    fn scoped_scroll_preserves_distance_and_cancels_without_resuming_old_tail() {
        for dt in [1.0 / 30.0, 1.0 / 120.0] {
            let context = Context::default();
            let mut scroll = Scroll::default();
            let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(200.0, 180.0));
            let mut time = 0.0;
            let mut frame = |events, enabled| {
                time += dt;
                let mut delta = egui::Vec2::ZERO;
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            Pos2::ZERO,
                            egui::vec2(400.0, 300.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        begin_frame(ui.ctx());
                        delta += scroll.delta(ui, rect, enabled);
                        assert_eq!(
                            scroll.delta(ui, rect, enabled),
                            egui::Vec2::ZERO,
                            "one advancement per frame"
                        );
                    },
                );
                delta
            };
            for _ in 0..3 {
                frame(vec![Event::PointerMoved(rect.center())], true);
            }
            let wheel = |unit, y, phase, modifiers| Event::MouseWheel {
                unit,
                delta: egui::vec2(0.0, y),
                phase,
                modifiers,
            };
            for (unit, y, expected) in [
                (
                    egui::MouseWheelUnit::Line,
                    -3.0,
                    -context.options(|options| options.input_options.line_scroll_speed) * 3.0,
                ),
                (egui::MouseWheelUnit::Point, -5.0, -5.0),
                (egui::MouseWheelUnit::Page, -1.0, -rect.height()),
            ] {
                let mut sum = frame(
                    vec![
                        Event::PointerMoved(rect.center()),
                        wheel(unit, y, egui::TouchPhase::Move, egui::Modifiers::NONE),
                    ],
                    true,
                )
                .y;
                for _ in 0..100 {
                    sum += frame(vec![], true).y;
                }
                assert!(
                    (sum - expected).abs() < 0.001,
                    "preserve {unit:?} distance at dt={dt}: {sum} vs {expected}"
                );
            }
            let first = frame(
                vec![wheel(
                    egui::MouseWheelUnit::Line,
                    -3.0,
                    egui::TouchPhase::Move,
                    egui::Modifiers::NONE,
                )],
                true,
            );
            assert!(first.y < 0.0);
            assert_eq!(frame(vec![], false), egui::Vec2::ZERO);
            for _ in 0..30 {
                assert_eq!(frame(vec![], true), egui::Vec2::ZERO);
            }
            frame(
                vec![wheel(
                    egui::MouseWheelUnit::Point,
                    0.0,
                    egui::TouchPhase::Start,
                    egui::Modifiers::NONE,
                )],
                true,
            );
            assert_eq!(
                frame(
                    vec![wheel(
                        egui::MouseWheelUnit::Point,
                        -20.0,
                        egui::TouchPhase::Move,
                        egui::Modifiers::NONE
                    )],
                    true
                )
                .y,
                -20.0,
                "touch movement has no added latency"
            );
            assert_eq!(
                frame(
                    vec![
                        Event::PointerMoved(egui::pos2(350.0, 250.0)),
                        wheel(
                            egui::MouseWheelUnit::Point,
                            0.0,
                            egui::TouchPhase::End,
                            egui::Modifiers::NONE
                        )
                    ],
                    true
                ),
                egui::Vec2::ZERO
            );
        }
    }

    #[test]
    fn volume_events_keep_their_positions_across_frames_targets_and_passes() {
        let context = Context::default();
        let first = egui::pos2(40.0, 40.0);
        let second = egui::pos2(220.0, 220.0);
        let outside = egui::pos2(350.0, 30.0);
        let wheel = |delta| Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, delta),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        };
        let frame = |events, extra_pass, overlap| {
            let mut deltas = Vec::new();
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    begin_frame(ui.ctx());
                    let targets = [
                        ui.interact(
                            egui::Rect::from_center_size(first, egui::vec2(60.0, 60.0)),
                            Id::new("first"),
                            egui::Sense::hover(),
                        ),
                        ui.interact(
                            egui::Rect::from_center_size(
                                if overlap { first } else { second },
                                egui::vec2(60.0, 60.0),
                            ),
                            Id::new("second"),
                            egui::Sense::hover(),
                        ),
                    ];
                    deltas.push(volume_delta(ui.ctx(), &targets));
                    if extra_pass && ui.ctx().current_pass_index() == 0 {
                        ui.ctx().request_discard("verify stable wheel origin");
                    }
                },
            );
            deltas
        };
        for _ in 0..3 {
            frame(vec![Event::PointerMoved(first)], false, false);
        }
        assert_eq!(
            frame(
                vec![
                    wheel(-1.0),
                    Event::PointerMoved(second),
                    wheel(-2.0),
                    Event::PointerMoved(outside),
                    wheel(-4.0)
                ],
                true,
                false
            ),
            [-3.0, 0.0],
            "egui consumes wheel events after the first pass"
        );
        assert_eq!(
            frame(vec![wheel(-1.0), Event::PointerMoved(first)], false, false),
            [0.0],
            "initial wheel still belongs to prior outside position"
        );
        assert_eq!(
            frame(
                vec![wheel(-1.0), Event::PointerGone, wheel(-2.0)],
                false,
                false
            ),
            [-1.0]
        );
        assert_eq!(frame(vec![wheel(-1.0)], false, false), [0.0]);
        assert_eq!(
            frame(vec![Event::PointerMoved(first), wheel(-1.0)], false, true),
            [-1.0],
            "overlapping targets must not double count"
        );
    }
}
