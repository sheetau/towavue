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
    begin_frame(context);
    let mut position = context.data(|data| {
        data.get_temp::<Position>(Id::new("wheel-position"))
            .and_then(|position| position.start)
    });
    let events = context.input(|input| input.events.clone());
    let mut result = 0.0;
    for event in events {
        match event {
            Event::PointerMoved(pos) | Event::PointerButton { pos, .. } => position = Some(pos),
            Event::PointerGone | Event::WindowFocused(false) => position = None,
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
                result += delta.y
                    / if unit == egui::MouseWheelUnit::Point {
                        50.0
                    } else {
                        1.0
                    };
            }
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

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
