use std::time::{Duration, Instant};

const HOLD: Duration = Duration::from_millis(1200);
const FADE: Duration = Duration::from_millis(120);

#[derive(Clone)]
struct Visibility {
    muted: bool,
    opacity: f32,
    updated: Instant,
    hide_at: Option<Instant>,
}

impl Visibility {
    fn update(&mut self, desired: Option<bool>, now: Instant) {
        let previous_hide = self.hide_at;
        match desired {
            Some(muted) => {
                self.muted = muted;
                self.hide_at = None;
            }
            None if self.hide_at.is_none() => self.hide_at = Some(now + HOLD),
            None => {}
        }
        let hiding = self.hide_at.is_some_and(|until| now >= until);
        let since = if hiding {
            self.updated.max(self.hide_at.expect("hiding deadline"))
        } else if previous_hide.is_some_and(|until| now >= until) {
            // Resume at the opacity reached while rendering was suspended.
            self.opacity = (self.opacity
                - now
                    .saturating_duration_since(
                        self.updated
                            .max(previous_hide.expect("previous hiding deadline")),
                    )
                    .as_secs_f32()
                    / FADE.as_secs_f32())
            .max(0.0);
            now
        } else {
            self.updated
        };
        let step = now.saturating_duration_since(since).as_secs_f32() / FADE.as_secs_f32();
        self.opacity = (self.opacity + if hiding { -step } else { step }).clamp(0.0, 1.0);
        self.updated = now;
    }
}

pub fn tab_audio_visibility(
    context: &egui::Context,
    id: egui::Id,
    desired: Option<bool>,
    now: Instant,
) -> Option<(bool, f32)> {
    let mut state = context
        .data(|data| data.get_temp::<Visibility>(id))
        .or_else(|| {
            desired.map(|muted| Visibility {
                muted,
                opacity: 0.0,
                updated: now,
                hide_at: None,
            })
        })?;
    state.update(desired, now);
    if state.opacity == 0.0 && state.hide_at.is_some_and(|until| now >= until) {
        context.data_mut(|data| data.remove::<Visibility>(id));
        return None;
    }
    if state.opacity < 1.0 || state.hide_at.is_some_and(|until| now >= until) {
        context.request_repaint();
    } else if let Some(until) = state.hide_at {
        context.request_repaint_after(until.saturating_duration_since(now));
    }
    let result = (state.muted, state.opacity);
    context.data_mut(|data| data.insert_temp(id, state));
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_fades_holds_after_pause_and_resumes_without_flashing() {
        let context = egui::Context::default();
        let id = egui::Id::new("tab");
        let start = Instant::now();
        assert_eq!(tab_audio_visibility(&context, id, None, start), None);
        assert_eq!(
            tab_audio_visibility(&context, id, Some(false), start),
            Some((false, 0.0))
        );
        assert_eq!(
            tab_audio_visibility(&context, id, Some(false), start + FADE),
            Some((false, 1.0))
        );
        let paused = start + FADE;
        assert_eq!(
            tab_audio_visibility(&context, id, None, paused),
            Some((false, 1.0))
        );
        assert_eq!(
            tab_audio_visibility(&context, id, None, paused + HOLD),
            Some((false, 1.0))
        );
        let middle = paused + HOLD + FADE / 2;
        assert_eq!(
            tab_audio_visibility(&context, id, None, middle),
            Some((false, 0.5))
        );
        assert_eq!(
            tab_audio_visibility(&context, id, Some(true), middle),
            Some((true, 0.5))
        );
        assert_eq!(
            tab_audio_visibility(&context, id, Some(true), middle + FADE),
            Some((true, 1.0))
        );
        tab_audio_visibility(&context, id, None, middle + FADE);
        assert_eq!(
            tab_audio_visibility(&context, id, None, middle + HOLD + FADE * 2),
            None
        );
        assert_eq!(
            tab_audio_visibility(&context, egui::Id::new("other source"), None, middle),
            None
        );
    }
}
