use crate::timeline_input;
use egui::{Context, Id, Pos2, Rect, Response};

// The slowest band begins 240 logical points above the seek bar.
const BAND_HEIGHT: f32 = 80.0;
const SPEEDS: [f32; 4] = [1.0, 0.5, 0.25, 0.1];
const LABELS: [&str; 4] = [
    "Seeking · Normal speed",
    "Seeking · 1/2 speed",
    "Seeking · 1/4 speed",
    "Seeking · 1/10 speed",
];

#[derive(Clone)]
struct State {
    owner: Id,
    rect: Rect,
    raw: Pos2,
    x: f32,
    frame: Option<u64>,
    dragging: bool,
}

fn state_id() -> Id {
    Id::new("precision-seek")
}

fn stage(rect: Rect, y: f32) -> usize {
    (((rect.center().y - y).max(0.0) / BAND_HEIGHT) as usize).min(3)
}

// Integrate each crossed band so coalescing a straight movement into one event
// gives the same result as several events along that movement.
fn scaled_delta(rect: Rect, from: Pos2, to: Pos2) -> f32 {
    let mut cuts = vec![0.0, 1.0];
    if to.y != from.y {
        for band in 1..=3 {
            let y = rect.center().y - BAND_HEIGHT * band as f32;
            let t = (y - from.y) / (to.y - from.y);
            if t > 0.0 && t < 1.0 {
                cuts.push(t);
            }
        }
    }
    cuts.sort_by(f32::total_cmp);
    (to.x - from.x)
        * cuts
            .windows(2)
            .map(|interval| {
                let y = egui::lerp(from.y..=to.y, (interval[0] + interval[1]) * 0.5);
                (interval[1] - interval[0]) * SPEEDS[stage(rect, y)]
            })
            .sum::<f32>()
}

pub(super) fn status(context: &Context) -> Option<&'static str> {
    let state = context.data(|data| data.get_temp::<State>(state_id()))?;
    let response = context.read_response(state.owner)?;
    (state.dragging && timeline_input::is_dragging(&response))
        .then(|| LABELS[stage(state.rect, state.raw.y)])
}

pub(super) fn adjust(response: &Response, mut drag: timeline_input::Drag) -> timeline_input::Drag {
    let context = &response.ctx;
    let before = status(context);
    let mut state = context.data(|data| data.get_temp::<State>(state_id()));
    if drag.started
        && let Some(origin) = drag.origin
    {
        state = Some(State {
            owner: response.id,
            rect: response.rect,
            raw: origin,
            x: origin.x,
            frame: None,
            dragging: false,
        });
    }
    if let Some(active) = &mut state
        && active.owner == response.id
    {
        if drag.origin.is_none() || active.rect != response.rect {
            if drag.origin.is_some() {
                timeline_input::cancel(context);
                drag = timeline_input::Drag::default();
            }
            state = None;
        } else if active.frame != Some(context.cumulative_frame_nr()) {
            let events = context.input(|input| input.events.clone());
            let mut begun = !drag.started;
            let travel = super::compact_travel(response.rect);
            for event in events {
                if let egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } = event
                {
                    begun = true;
                    continue;
                }
                if !begun {
                    continue;
                }
                let (position, release) = match event {
                    egui::Event::PointerMoved(position) => (position, false),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        ..
                    } => (pos, true),
                    _ => continue,
                };
                active.x = (active.x + scaled_delta(active.rect, active.raw, position))
                    .clamp(travel.left(), travel.right());
                active.raw = position;
                if release {
                    break;
                }
            }
            active.frame = Some(context.cumulative_frame_nr());
            active.dragging = drag.dragging;
        }
        if let Some(active) = &state
            && drag.position.is_some()
        {
            drag.position = Some(Pos2::new(active.x, active.raw.y));
        }
    }
    context.data_mut(|data| {
        if let Some(state) = state {
            data.insert_temp(state_id(), state);
        } else {
            data.remove::<State>(state_id());
        }
    });
    if before != status(context) || drag.released {
        // The status bar is laid out before the seek control.
        context.request_repaint();
    }
    drag
}

#[cfg(test)]
mod tests;
