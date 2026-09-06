use egui::{Context, Id, PointerButton, Pos2, Response};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Seek,
    Trim,
}

#[derive(Clone, Copy)]
struct Active {
    id: Id,
    kind: Kind,
    origin: Pos2,
    dragging: bool,
}

#[derive(Clone, Default)]
struct State {
    active: Option<Active>,
    trim_identity: Option<(Id, Id)>,
    // Grips run before the background; release must not let it reuse this frame's press.
    claimed_frame: Option<u64>,
}

#[derive(Default)]
pub struct Drag {
    pub position: Option<Pos2>,
    pub released: bool,
    pub dragging: bool,
}

fn state_id() -> Id {
    Id::new("timeline-pointer-gesture")
}

pub fn is_active(context: &Context) -> bool {
    context.data(|data| {
        data.get_temp::<State>(state_id())
            .is_some_and(|state| state.active.is_some())
    })
}

pub fn cancel(context: &Context) -> bool {
    let active = context.data_mut(|data| {
        data.get_temp_mut_or_default::<State>(state_id())
            .active
            .take()
    });
    if active.is_some_and(|active| context.dragged_id() == Some(active.id)) {
        context.stop_dragging();
    }
    active.is_some()
}

pub fn retain_trim(context: &Context, identity: Id, generation: Id) {
    let changed = context.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(state_id());
        let changed = state.active.is_some_and(|active| {
            active.kind == Kind::Trim
                && (state.trim_identity != Some((identity, generation))
                    || (active.id != identity.with(true) && active.id != identity.with(false)))
        });
        state.trim_identity = Some((identity, generation));
        changed
    });
    if changed {
        cancel(context);
    }
}

pub fn seek_commit(response: &Response) -> Option<Pos2> {
    let drag = seek_drag(response);
    if drag.released { drag.position } else { None }
}

pub fn seek_drag(response: &Response) -> Drag {
    update(response, Kind::Seek)
}

pub fn trim_drag(response: &Response) -> Drag {
    update(response, Kind::Trim)
}

fn update(response: &Response, kind: Kind) -> Drag {
    let context = &response.ctx;
    let (events, pointer, decided_drag, interrupted) = context.input(|input| {
        (
            input.events.clone(),
            input.pointer.hover_pos(),
            input.pointer.is_decidedly_dragging(),
            !input.focused
                || input.key_pressed(egui::Key::Escape)
                || input.events.contains(&egui::Event::WindowFocused(false)),
        )
    });
    if interrupted || !response.enabled() || egui::Popup::is_any_open(context) {
        cancel(context);
        return Drag::default();
    }
    let mut state = context
        .data(|data| data.get_temp::<State>(state_id()))
        .unwrap_or_default();
    let frame = context.cumulative_frame_nr();
    let press = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            egui::Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                ..
            } => Some((index, *pos)),
            _ => None,
        });
    let mut first_event = 0;
    if let Some((index, origin)) = press
        && state.claimed_frame != Some(frame)
        && response.interact_rect.contains(origin)
        && context.layer_id_at(origin) == Some(response.layer_id)
    {
        state.active = Some(Active {
            id: response.id,
            kind,
            origin,
            dragging: false,
        });
        state.claimed_frame = Some(frame);
        first_event = index + 1;
    }
    let mut result = Drag::default();
    if let Some(mut active) = state.active.filter(|active| active.id == response.id) {
        let threshold = context.options(|options| options.input_options.max_click_dist);
        for event in &events[first_event..] {
            match event {
                egui::Event::PointerMoved(pos) => {
                    active.dragging |= active.origin.distance(*pos) > threshold
                }
                egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    active.dragging |= active.origin.distance(*pos) > threshold;
                    result.position = Some(*pos);
                    result.released = true;
                    break;
                }
                _ => {}
            }
        }
        active.dragging |= decided_drag;
        result.dragging = active.dragging;
        if result.released {
            state.active = None;
        } else {
            result.position = pointer;
            state.active = Some(active);
        }
    } else if kind == Kind::Seek
        && response.clicked()
        && press.is_none()
        && !events.iter().any(|event| {
            matches!(
                event,
                egui::Event::PointerButton {
                    button: PointerButton::Primary,
                    pressed: false,
                    ..
                }
            )
        })
    {
        result.position = response.interact_pointer_pos().or(response.hover_pos());
        result.released = true;
    }
    context.data_mut(|data| data.insert_temp(state_id(), state));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batched_seek_uses_press_ownership_and_commits_once_across_layout_passes() {
        for blocked in 0..5 {
            let context = Context::default();
            let rect = egui::Rect::from_min_max(egui::pos2(20.0, 20.0), egui::pos2(420.0, 120.0));
            let origin = if blocked == 4 {
                egui::pos2(10.0, 10.0)
            } else {
                egui::pos2(100.0, 50.0)
            };
            let end = egui::pos2(300.0, 50.0);
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let mut commits = Vec::new();
            for events in [
                vec![egui::Event::PointerMoved(origin)],
                vec![
                    button(origin, true),
                    egui::Event::PointerMoved(end),
                    button(end, false),
                    egui::Event::PointerMoved(egui::pos2(450.0, 50.0)),
                ],
                vec![],
            ] {
                let mut passes = 0;
                let _ = context.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        if blocked == 3 {
                            egui::Area::new("seek-cover".into())
                                .order(egui::Order::Foreground)
                                .fixed_pos(egui::pos2(75.0, 25.0))
                                .show(ui.ctx(), |ui| {
                                    ui.allocate_exact_size(
                                        egui::vec2(50.0, 50.0),
                                        egui::Sense::click_and_drag(),
                                    );
                                });
                        }
                        if blocked == 1 {
                            ui.disable();
                        }
                        if blocked == 2 {
                            ui.set_clip_rect(egui::Rect::from_min_max(
                                egui::pos2(150.0, 20.0),
                                rect.max,
                            ));
                        }
                        let response =
                            ui.interact(rect, "seek".into(), egui::Sense::click_and_drag());
                        if let Some(position) = seek_commit(&response) {
                            commits.push(position);
                        }
                        passes += 1;
                        if passes == 1 {
                            ui.ctx()
                                .request_discard("verify one commit per input gesture");
                        }
                    },
                );
                assert!(passes >= 2);
            }
            assert_eq!(
                commits,
                if blocked == 0 { vec![end] } else { vec![] },
                "blocked={blocked}"
            );
            assert!(!is_active(&context));
        }
    }
}
