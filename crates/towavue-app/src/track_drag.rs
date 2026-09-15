use crate::*;

// A press/move/release can arrive before RedrawRequested. Reuse the speed
// hold's pending press rather than acquiring a cursor lock after release.
pub(super) fn completed(response: &egui::Response) -> Option<(egui::Pos2, egui::Vec2)> {
    if !response.enabled() {
        return None;
    }
    let mut origin = hold_speed::pending_press_origin(response);
    let events = response.ctx.input(|input| input.events.clone());
    let threshold = response
        .ctx
        .options(|options| options.input_options.max_click_dist);
    for event in &events {
        match event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            } => {
                origin = (modifiers.is_none()
                    && response.interact_rect.contains(*pos)
                    && response.ctx.layer_id_at(*pos) == Some(response.layer_id))
                .then_some(*pos);
            }
            egui::Event::PointerMoved(pos) => {
                if let Some(start) = origin {
                    let delta = *pos - start;
                    if delta.y.abs() > threshold && delta.y.abs() >= delta.x.abs() {
                        origin = None;
                    }
                }
            }
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers,
            } => {
                let start = origin?;
                let delta = *pos - start;
                return (modifiers.is_none()
                    && delta.x.abs() >= 24.0
                    && delta.x.abs() > delta.y.abs())
                .then_some((start, delta));
            }
            egui::Event::PointerGone
            | egui::Event::WindowFocused(false)
            | egui::Event::Key { pressed: true, .. }
            | egui::Event::PointerButton { pressed: true, .. } => origin = None,
            _ => {}
        }
    }
    None
}

pub(super) struct Drag {
    media: u64,
    density: f64,
    distance: f64,
    // UI-thread RAII lock, released before navigation or a modal guard.
    cursor: Option<towavue_runtime_windows::PinnedCursor>,
}

impl Drag {
    fn direction(&self) -> Option<bool> {
        (self.distance.abs() >= 24.0).then_some(self.distance > 0.0)
    }

    pub(super) fn icon(&self) -> Option<chrome::Icon> {
        self.direction().map(|next| {
            if next {
                chrome::Icon::NextTrack
            } else {
                chrome::Icon::PreviousTrack
            }
        })
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    fn track_drag_enabled(&self) -> bool {
        matches!(self.media_kind, Some(MediaKind::Audio | MediaKind::Video))
            && !self.command_context().playback_blocked
            && !self.modal_input_blocked()
            && !self.palette_open
            && !self.grid_open
            && !self.filmstrip_open
            && !self
                .ui_context
                .as_ref()
                .is_some_and(egui::Popup::is_any_open)
    }

    pub(super) fn begin_track_drag(
        &mut self,
        media: u64,
        origin: egui::Pos2,
        delta: egui::Vec2,
        released: bool,
    ) {
        if media != self.media_generation
            || self.track_drag.is_some()
            || self.held_speed.is_some()
            || !self.track_drag_enabled()
        {
            return;
        }
        let Some(context) = &self.ui_context else {
            return;
        };
        let density = f64::from(context.pixels_per_point());
        let cursor = if released {
            None
        } else {
            let Some(window) = &self.window else { return };
            match towavue_runtime_windows::PinnedCursor::new(
                window.clone(),
                (f64::from(origin.x) * density, f64::from(origin.y) * density),
            ) {
                Ok(cursor) => Some(cursor),
                Err(error) => {
                    self.set_status(format!("Could not start track drag: {error}"));
                    return;
                }
            }
        };
        self.cancel_view_drag();
        self.cancel_shortcut_prefix();
        self.track_drag = Some(Drag {
            media,
            density,
            distance: 0.0,
            cursor,
        });
        self.set_status("Drag left/right for previous/next track · release to cancel".into());
        self.move_track_drag((f64::from(delta.x) * density, 0.0));
        if released {
            self.finish_track_drag(false);
        }
    }

    pub(super) fn move_track_drag(&mut self, delta: (f64, f64)) {
        if let Some(drag) = &mut self.track_drag {
            let previous = drag.direction();
            drag.distance += delta.0 / drag.density;
            if drag.direction() == previous {
                return;
            }
            let message = match drag.direction() {
                Some(true) => "Release for next track · return to center or Escape to cancel",
                Some(false) => "Release for previous track · return to center or Escape to cancel",
                None => "Drag left/right for previous/next track · release to cancel",
            };
            self.set_status(message.into());
            self.request_redraw();
        }
    }

    pub(super) fn finish_track_drag(&mut self, cancel: bool) -> bool {
        let Some(mut drag) = self.track_drag.take() else {
            return false;
        };
        drag.cursor = None;
        if !cancel
            && drag.media == self.media_generation
            && self.track_drag_enabled()
            && let Some(next) = drag.direction()
        {
            self.dispatch(if next {
                CommandId::NextMedia
            } else {
                CommandId::PreviousMedia
            });
        } else if drag.media == self.media_generation {
            self.set_status("Track drag cancelled".into());
        }
        self.request_redraw();
        true
    }

    pub(super) fn track_window_event(&mut self, event: &WindowEvent) -> bool {
        if self.track_drag.is_none() {
            return false;
        }
        match event {
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                // Queue the release for egui too, so the consumed drag cannot
                // leave a held button or become Play after navigation.
                if let (Some(window), Some(state)) = (&self.window, &mut self.ui_state) {
                    let _ = state.on_window_event(window, event);
                }
                self.finish_track_drag(false);
                true
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.logical_key == WinitKey::Named(NamedKey::Escape) =>
            {
                self.finish_track_drag(true);
                true
            }
            WindowEvent::Focused(false)
            | WindowEvent::CloseRequested
            | WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. }
            | WindowEvent::DroppedFile(_) => {
                self.finish_track_drag(true);
                false
            }
            WindowEvent::KeyboardInput { .. } | WindowEvent::MouseWheel { .. } => true,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => {
                self.finish_track_drag(true);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
