use crate::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Begin(u64),
    End(u64),
}

pub(super) struct Held {
    token: u64,
    media: u64,
    rate: f32,
    pub(super) was_paused: bool,
}

#[derive(Clone, Copy)]
struct Press {
    id: egui::Id,
    token: u64,
    origin: egui::Pos2,
    start: f64,
    active: bool,
}

#[derive(Clone, Default)]
struct Input {
    press: Option<Press>,
    sequence: u64,
    claimed: Option<u64>,
    processed: Option<u64>,
    suppress: Option<egui::Id>,
    consumed: Option<(egui::Id, u64)>,
}

fn input_id() -> egui::Id {
    egui::Id::new("temporary-speed-hold")
}

fn cancel_input(context: &egui::Context) -> bool {
    context.data_mut(|data| {
        let input = data.get_temp_mut_or_default::<Input>(input_id());
        let press = input.press.take();
        if let Some(press) = press {
            input.suppress = Some(press.id);
        }
        press.is_some()
    })
}

// One owner per press, including discarded egui passes. A cancelled/held press
// cannot become a normal Play click when its eventual release arrives.
fn update(response: &egui::Response, enabled: bool) -> (Option<Action>, bool) {
    let context = &response.ctx;
    let (events, now, down, position, focused, modifiers) = context.input(|input| {
        (
            input.events.clone(),
            input.time,
            input.pointer.primary_down(),
            input.pointer.interact_pos(),
            input.focused,
            input.modifiers,
        )
    });
    let frame = context.cumulative_frame_nr();
    let mut input = context
        .data(|data| data.get_temp::<Input>(input_id()))
        .unwrap_or_default();
    let mut action = None;
    let mut first_event = 0;
    if input.claimed != Some(frame)
        && enabled
        && focused
        && response.enabled()
        && !egui::Popup::is_any_open(context)
        && let Some((index, origin, modifiers)) =
            events
                .iter()
                .enumerate()
                .rev()
                .find_map(|(index, event)| match event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers,
                    } => Some((index, *pos, *modifiers)),
                    _ => None,
                })
        && modifiers.is_none()
        && response.interact_rect.contains(origin)
        && context.layer_id_at(origin) == Some(response.layer_id)
    {
        if let Some(press) = input.press.filter(|press| press.active) {
            action = Some(Action::End(press.token));
        }
        first_event = index + 1;
        input.sequence = input.sequence.wrapping_add(1);
        input.press = Some(Press {
            id: response.id,
            token: input.sequence,
            origin,
            start: now,
            active: false,
        });
        input.claimed = Some(frame);
        input.suppress = None;
    }
    if input.processed != Some(frame)
        && let Some(mut press) = input.press.filter(|press| press.id == response.id)
    {
        input.processed = Some(frame);
        let distance = context.options(|options| options.input_options.max_click_dist);
        let events = &events[first_event..];
        let released = events.iter().any(|event| {
            matches!(
                event,
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                }
            )
        });
        let moved = events.iter().any(|event| match event {
            egui::Event::PointerMoved(pos) | egui::Event::PointerButton { pos, .. } => {
                pos.distance(press.origin) > distance
            }
            _ => false,
        }) || position.is_none_or(|pos| pos.distance(press.origin) > distance);
        let interrupted = !enabled
            || !response.enabled()
            || !focused
            || !modifiers.is_none()
            || egui::Popup::is_any_open(context)
            || events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::PointerGone
                        | egui::Event::WindowFocused(false)
                        | egui::Event::Key { pressed: true, .. }
                        | egui::Event::PointerButton {
                            button: egui::PointerButton::Secondary,
                            pressed: true,
                            ..
                        }
                )
            });
        if moved || interrupted || !down || released {
            if press.active {
                action = Some(Action::End(press.token));
            }
            if press.active || moved || interrupted {
                input.suppress = Some(press.id);
                input.consumed = Some((press.id, frame));
            }
            input.press = None;
        } else if !press.active {
            let remaining = (0.4 - (now - press.start)).max(0.0);
            if remaining == 0.0 {
                press.active = true;
                input.press = Some(press);
                action = Some(Action::Begin(press.token));
            } else {
                context.request_repaint_after(Duration::from_secs_f64(remaining));
            }
        }
    }
    if input.suppress == Some(response.id) && !down {
        input.consumed = Some((response.id, frame));
        input.suppress = None;
    }
    let consumed = input.consumed == Some((response.id, frame))
        || input.suppress == Some(response.id)
        || input
            .press
            .is_some_and(|press| press.id == response.id && press.active);
    context.data_mut(|data| data.insert_temp(input_id(), input));
    (action, consumed)
}

#[cfg(test)]
mod tests;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    fn hold_enabled(&self) -> bool {
        matches!(self.media_kind, Some(MediaKind::Video | MediaKind::Audio))
            && matches!(self.state, PlaybackState::Playing | PlaybackState::Paused)
            && self.session.is_some()
            && !self.modal_input_blocked()
            && !self.palette_open
            && !self.grid_open
            && !self.filmstrip_open
            && self
                .playback_duration()
                .is_none_or(|duration| !duration.is_zero())
    }

    pub(super) fn hold_response(
        &self,
        response: &egui::Response,
        actions: &mut Vec<UiAction>,
    ) -> bool {
        let (action, consumed) = update(response, self.hold_enabled());
        if let Some(action) = action {
            actions.push(UiAction::HoldSpeed(
                self.media_generation,
                self.generation,
                action,
            ));
        }
        consumed
    }

    pub(super) fn handle_hold_speed(
        &mut self,
        media: u64,
        generation: PlaybackGeneration,
        action: Action,
    ) {
        match action {
            Action::Begin(token) => {
                let held = self.ui_context.as_ref().is_some_and(|context| {
                    context.data(|data| {
                        data.get_temp::<Input>(input_id()).is_some_and(|input| {
                            input
                                .press
                                .is_some_and(|press| press.token == token && press.active)
                        })
                    })
                });
                if media != self.media_generation
                    || generation != self.generation
                    || !held
                    || !self.hold_enabled()
                    || self.held_speed.is_some()
                {
                    return;
                }
                self.begin_hold_speed(token);
            }
            Action::End(token) => {
                if self
                    .held_speed
                    .as_ref()
                    .is_some_and(|held| held.token == token && held.media == media)
                {
                    self.cancel_hold_speed();
                }
            }
        }
    }

    fn begin_hold_speed(&mut self, token: u64) {
        self.cancel_frame_steps();
        let held = Held {
            token,
            media: self.media_generation,
            rate: self.playback_rate(),
            was_paused: self.state == PlaybackState::Paused,
        };
        self.held_speed = Some(held);
        if self.set_temporary_rate(2.0, false) {
            self.set_status("2× while held · release to restore playback".into());
        }
    }

    pub(super) fn cancel_hold_speed(&mut self) -> bool {
        let input = self.ui_context.as_ref().is_some_and(cancel_input);
        let Some(held) = self.held_speed.take() else {
            return input;
        };
        if held.media == self.media_generation
            && self.session.is_some()
            && !matches!(self.state, PlaybackState::Loading | PlaybackState::Faulted)
        {
            let ended = self.state == PlaybackState::Ended;
            if self.set_temporary_rate(held.rate, held.was_paused || ended) {
                if ended {
                    self.state = PlaybackState::Ended;
                }
                self.set_status(format!("Playback restored · {:.2}×", held.rate));
            }
        }
        true
    }

    fn set_temporary_rate(&mut self, rate: f32, paused: bool) -> bool {
        let position = self.current_position();
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        match session.set_rate_at(position, rate, paused) {
            Ok(generation) => {
                self.generation = generation;
                self.state = if paused {
                    PlaybackState::Paused
                } else {
                    PlaybackState::Playing
                };
                self.pending_time = None;
                let mut clock = PlaybackClock::new(session.target(), rate);
                clock.set_paused(paused);
                self.clock = Some(clock);
                self.decode_finished = false;
                self.audio_drained = !session.has_audio();
                self.metrics_recorded = false;
                self.pending_seek_started = None;
                self.refresh_title();
                self.request_redraw();
                true
            }
            Err(error) => {
                self.fail(error.to_string());
                false
            }
        }
    }
}
