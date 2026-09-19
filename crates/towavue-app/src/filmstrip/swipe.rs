use super::*;

// Logical points keep pointer tracking and momentum independent of display density.
const MIN_SPEED: f32 = 120.0;
const MAX_SPEED: f32 = 4_000.0;
const MAX_ACCUMULATED_SPEED: f32 = 12_000.0;
const REPEAT_WINDOW: f64 = 0.3;
const DECELERATION: f32 = 4_000.0;

#[derive(PartialEq)]
struct Scope {
    folder: PathBuf,
    generation: u64,
    current: Option<PathBuf>,
    rect: Rect,
    density: f32,
}

#[derive(Default)]
pub(super) struct State {
    scope: Option<Scope>,
    last_frame: Option<u64>,
    blocked: bool,
    pub(super) exclusions: Vec<Rect>,
    pointer: Option<egui::Pos2>,
    coast: Option<(f64, f32, f32)>,
    carry: Option<(f64, f32)>,
}

impl State {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn update(
        &mut self,
        response: &egui::Response,
        snapshot: &FolderSnapshot,
        current: Option<&Path>,
        offset: &mut f32,
        maximum: f32,
        recenter: bool,
    ) {
        let context = &response.ctx;
        let frame = context.cumulative_frame_nr();
        if self.last_frame == Some(frame) {
            return;
        }
        let scope = Scope {
            folder: snapshot.folder_path.clone(),
            generation: snapshot.generation,
            current: current.map(Path::to_owned),
            rect: response.rect,
            density: context.pixels_per_point(),
        };
        let (pointer, events, time) =
            context.input(|input| (input.pointer.clone(), input.events.clone(), input.time));
        let interrupted = context.input(|input| {
            !input.focused
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::WindowFocused(false)
                            | egui::Event::Key { pressed: true, .. }
                            | egui::Event::MouseWheel { .. }
                    )
                })
        });
        let changed = self.scope.as_ref() != Some(&scope)
            || self.last_frame.is_some_and(|last| frame > last + 1);
        if changed || !response.enabled() || recenter || interrupted {
            self.pointer = None;
            self.coast = None;
            self.carry = None;
            self.blocked = pointer.primary_down();
        }
        self.scope = Some(scope);
        self.last_frame = Some(frame);
        if changed || !response.enabled() || recenter || interrupted {
            return;
        }
        let blocked = self.blocked;
        self.blocked &= pointer.primary_down();
        let (press, release) =
            crate::view_drag_button_positions(response, egui::PointerButton::Primary);
        // A press brakes immediately. Only a new background swipe may reuse the
        // remaining speed, and only if it releases promptly in the same direction.
        if pointer.any_pressed() {
            self.carry = self.coast.take().and_then(|(started, speed, _)| {
                let remaining = remaining_speed(speed, time - started);
                (!blocked
                    && press.is_some_and(|origin| {
                        !self.exclusions.iter().any(|rect| rect.contains(origin))
                    })
                    && remaining.abs() >= MIN_SPEED)
                    .then_some((time, remaining))
            });
        }
        let departed = events
            .iter()
            .any(|event| matches!(event, egui::Event::PointerGone));
        // egui may not retain a drag owner when a complete gesture arrives in
        // one frame. Use the last presented hit regions, never borrow a card
        // or scrollbar press just because the release lies over empty space.
        let batched = press.zip(release).filter(|(origin, end)| {
            origin.distance_sq(*end) > 36.0
                && !self.exclusions.iter().any(|rect| rect.contains(*origin))
        });
        let owns_drag = !blocked
            && (response.dragged_by(egui::PointerButton::Primary)
                || response.drag_stopped_by(egui::PointerButton::Primary)
                || batched.is_some()
                || (departed && self.pointer.is_some()));
        let starts_here = self.pointer.is_none() && press.is_some();
        if owns_drag && self.pointer.is_none() {
            self.pointer = batched
                .map(|(origin, _)| origin)
                .or(press)
                .or_else(|| pointer.press_origin());
        }
        if owns_drag && let Some(mut previous) = self.pointer {
            let mut started = !starts_here;
            for event in &events {
                if let egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } = event
                {
                    started = true;
                    previous = *pos;
                    continue;
                }
                if !started {
                    continue;
                }
                let (point, release) = match event {
                    egui::Event::PointerMoved(point) => (*point, false),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        ..
                    } => (*pos, true),
                    egui::Event::PointerGone => (previous, true),
                    _ => continue,
                };
                *offset = (*offset - (point.x - previous.x)).clamp(0.0, maximum);
                previous = point;
                if release {
                    let velocity = self.release_speed(time, pointer.velocity().x);
                    self.coast = (velocity.abs() >= MIN_SPEED).then_some((time, velocity, *offset));
                    break;
                }
            }
            self.pointer = (pointer.primary_down() && !departed).then_some(previous);
            if departed {
                // The OS may report the release only after reentry. Retire this
                // drag now; later outside/reentry motion must not move it twice.
                self.blocked = pointer.primary_down();
            }
            if response.dragged() {
                context.set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        } else if !pointer.primary_down() {
            self.pointer = None;
            self.carry = None;
        }
        if let Some((started, speed, initial)) = self.coast {
            let duration = f64::from(speed.abs() / DECELERATION);
            let elapsed = (time - started).clamp(0.0, duration);
            // Constant deceleration gives faster flicks more time and distance.
            // Integrate from release time so repaint frequency cannot alter travel.
            let travel = speed * (elapsed - elapsed * elapsed / (2.0 * duration)) as f32;
            *offset = (initial - travel).clamp(0.0, maximum);
            if elapsed >= duration || *offset <= 0.0 || *offset >= maximum {
                self.coast = None;
            } else {
                context.request_repaint();
            }
        }
    }

    fn release_speed(&mut self, time: f64, velocity: f32) -> f32 {
        let velocity = velocity.clamp(-MAX_SPEED, MAX_SPEED);
        let carry = self.carry.take();
        if velocity.abs() < MIN_SPEED {
            return 0.0;
        }
        let extra = carry.map_or(0.0, |(pressed, speed)| {
            if (0.0..=REPEAT_WINDOW).contains(&(time - pressed))
                && speed.signum() == velocity.signum()
            {
                remaining_speed(speed, time - pressed)
            } else {
                0.0
            }
        });
        (velocity + extra).clamp(-MAX_ACCUMULATED_SPEED, MAX_ACCUMULATED_SPEED)
    }
}

fn remaining_speed(speed: f32, elapsed: f64) -> f32 {
    speed.signum() * (speed.abs() - DECELERATION * elapsed.max(0.0) as f32).max(0.0)
}
