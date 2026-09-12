use egui::{Context, Id, PointerButton, Pos2, Response};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Seek,
    VideoSeek,
}

#[derive(Clone, Copy)]
struct Active {
    id: Id,
    kind: Kind,
    origin: Pos2,
    modifiers: egui::Modifiers,
    direction: Option<egui::Vec2>,
    dragging: bool,
    opens_timeline: Option<bool>,
}

impl Active {
    fn moved(&mut self, position: Pos2, threshold: f32) {
        let delta = position - self.origin;
        if delta.length() > threshold {
            self.dragging = true;
            self.direction.get_or_insert(delta);
            if self.kind == Kind::VideoSeek && self.opens_timeline.is_none() {
                self.opens_timeline = Some(delta.y < 0.0 && -delta.y > delta.x.abs());
            }
        }
    }
}

#[derive(Clone, Default)]
struct State {
    active: Option<Active>,
    // A press is claimed once, including discarded UI passes and overlapping controls.
    claimed_frame: Option<u64>,
}

#[derive(Default)]
pub struct Drag {
    pub started: bool,
    pub origin: Option<Pos2>,
    pub modifiers: egui::Modifiers,
    pub direction: Option<egui::Vec2>,
    pub position: Option<Pos2>,
    pub released: bool,
    pub dragging: bool,
    pub open_timeline: bool,
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

pub fn is_dragging(response: &Response) -> bool {
    response.ctx.data(|data| {
        data.get_temp::<State>(state_id())
            .and_then(|state| state.active)
            .is_some_and(|active| active.id == response.id && active.dragging)
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

#[cfg(test)]
pub fn seek_commit(response: &Response) -> Option<Pos2> {
    let drag = seek_drag(response);
    if drag.released { drag.position } else { None }
}

pub fn seek_drag(response: &Response) -> Drag {
    update(response, Kind::Seek)
}

pub fn video_seek_drag(response: &Response) -> Drag {
    update(response, Kind::VideoSeek)
}

fn update(response: &Response, kind: Kind) -> Drag {
    let context = &response.ctx;
    let (events, pointer, decided_drag, interrupted) = context.input(|input| {
        let lost_focus = input.events.contains(&egui::Event::WindowFocused(false));
        // A later cancellation must not undo an already released gesture.
        // Without a focus event, an inactive window still cannot start one.
        let interrupted = (!input.focused && !lost_focus)
            || input
                .events
                .iter()
                .take_while(|event| {
                    !matches!(
                        event,
                        egui::Event::PointerButton {
                            button: PointerButton::Primary,
                            pressed: false,
                            ..
                        }
                    )
                })
                .any(|event| {
                    matches!(
                        event,
                        egui::Event::WindowFocused(false)
                            | egui::Event::Key {
                                key: egui::Key::Escape,
                                pressed: true,
                                ..
                            }
                    )
                });
        (
            input.events.clone(),
            input.pointer.hover_pos(),
            input.pointer.is_decidedly_dragging(),
            interrupted,
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
                modifiers,
            } => Some((index, *pos, *modifiers)),
            _ => None,
        });
    let mut first_event = 0;
    let mut started = false;
    if let Some((index, origin, modifiers)) = press
        && state.claimed_frame != Some(frame)
        && response.interact_rect.contains(origin)
        && context.layer_id_at(origin) == Some(response.layer_id)
    {
        state.active = Some(Active {
            id: response.id,
            kind,
            origin,
            modifiers,
            direction: None,
            dragging: false,
            opens_timeline: None,
        });
        state.claimed_frame = Some(frame);
        first_event = index + 1;
        started = true;
    }
    let mut result = Drag {
        started,
        ..Default::default()
    };
    if let Some(mut active) = state.active.filter(|active| active.id == response.id) {
        result.origin = Some(active.origin);
        result.modifiers = active.modifiers;
        let threshold = context.options(|options| options.input_options.max_click_dist);
        for event in &events[first_event..] {
            match event {
                egui::Event::PointerMoved(pos) => active.moved(*pos, threshold),
                egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    active.moved(*pos, threshold);
                    result.position = Some(*pos);
                    result.released = true;
                    break;
                }
                _ => {}
            }
        }
        active.dragging |= decided_drag;
        result.dragging = active.dragging;
        result.direction = active.direction;
        if active.opens_timeline == Some(true) {
            state.active = None;
            state.claimed_frame = Some(frame);
            if context.dragged_id() == Some(active.id) {
                context.stop_dragging();
            }
            result = Drag {
                open_timeline: true,
                ..Default::default()
            };
        } else if result.released {
            state.active = None;
        } else {
            result.position = pointer;
            state.active = Some(active);
        }
    } else if matches!(kind, Kind::Seek | Kind::VideoSeek)
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
    fn video_direction_is_locked_without_changing_other_seek_gestures() {
        let origin = egui::pos2(100.0, 100.0);
        for (delta, opens) in [
            (egui::vec2(0.0, -20.0), true),
            (egui::vec2(12.0, -20.0), true),
            (egui::vec2(20.0, -12.0), false),
            (egui::vec2(-20.0, -20.0), false),
            (egui::vec2(0.0, 20.0), false),
        ] {
            for kind in [Kind::VideoSeek, Kind::Seek] {
                let mut active = Active {
                    id: Id::new("test"),
                    kind,
                    origin,
                    modifiers: egui::Modifiers::NONE,
                    direction: None,
                    dragging: false,
                    opens_timeline: None,
                };
                active.moved(origin + egui::vec2(1.0, -1.0), 6.0);
                assert!(!active.dragging);
                assert_eq!(active.opens_timeline, None);
                active.moved(origin + delta, 6.0);
                assert!(active.dragging);
                let expected = (kind == Kind::VideoSeek).then_some(opens);
                assert_eq!(active.opens_timeline, expected);
                active.moved(origin + egui::vec2(100.0, -100.0), 6.0);
                assert_eq!(active.opens_timeline, expected);
            }
        }
    }

    #[test]
    fn upward_video_drag_opens_once_and_never_seeks_on_release_or_another_pass() {
        for blocked in 0..5 {
            for batched in [false, true] {
                let context = Context::default();
                let rect =
                    egui::Rect::from_min_max(egui::pos2(20.0, 100.0), egui::pos2(420.0, 112.0));
                let origin = egui::pos2(100.0, 106.0);
                let end = origin - egui::vec2(0.0, 50.0);
                let button = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                };
                let mut open_count = 0;
                let mut seek_count = 0;
                let mut frame = |events, interrupt| {
                    let mut passes = 0;
                    let _ = context.run_ui(
                        egui::RawInput {
                            events,
                            focused: !(interrupt && blocked == 2),
                            ..Default::default()
                        },
                        |ui| {
                            if interrupt && blocked == 1 {
                                ui.disable();
                            }
                            if interrupt && blocked == 3 {
                                egui::Popup::open_id(ui.ctx(), "block".into());
                            }
                            let response =
                                ui.interact(rect, "compact".into(), egui::Sense::click_and_drag());
                            let drag = video_seek_drag(&response);
                            open_count += usize::from(drag.open_timeline);
                            seek_count += usize::from(drag.released);
                            let background = ui.interact(
                                rect.expand(100.0),
                                "timeline".into(),
                                egui::Sense::click_and_drag(),
                            );
                            seek_count += usize::from(seek_commit(&background).is_some());
                            passes += 1;
                            if passes == 1 {
                                ui.ctx()
                                    .request_discard("gesture is consumed across passes");
                            }
                        },
                    );
                };
                frame(vec![egui::Event::PointerMoved(origin)], false);
                let mut motion = vec![egui::Event::PointerMoved(end)];
                if blocked == 4 {
                    motion.push(egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                if batched {
                    motion.insert(0, button(origin, true));
                    motion.push(button(end, false));
                    frame(motion, true);
                } else {
                    frame(vec![button(origin, true)], false);
                    frame(motion, true);
                    frame(vec![button(end, false)], true);
                }
                frame(vec![], true);
                assert_eq!(
                    open_count,
                    usize::from(blocked == 0),
                    "blocked={blocked}, batched={batched}"
                );
                assert_eq!(seek_count, 0);
                assert!(!is_active(&context));
            }
        }
    }

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
