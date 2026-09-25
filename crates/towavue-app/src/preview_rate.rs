use crate::*;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn preview_rate_for(&self, id: TabId) -> f32 {
        self.preview_rates.get(&id).copied().unwrap_or_else(|| {
            // Retain interpretation of existing legacy rate edits. All current
            // rate controls write transport state; new speed edits use Stretch.
            self.edits
                .get(&id)
                .map_or(1.0, |history| history.state().rate)
        })
    }

    pub(super) fn preview_rate(&self) -> f32 {
        self.tabs
            .active()
            .map_or(1.0, |tab| self.preview_rate_for(tab.id))
    }

    pub(super) fn set_preview_rate(&mut self, rate: f32) {
        if !rate.is_finite()
            || !matches!(self.media_kind, Some(MediaKind::Audio | MediaKind::Video))
        {
            return;
        }
        let Some(id) = self.tabs.active().map(|tab| tab.id) else {
            return;
        };
        self.cancel_hold_speed();
        let rate = rate.clamp(0.25, 4.0);
        if rate == self.preview_rate() {
            return;
        }
        let ended = self.state == PlaybackState::Ended;
        let paused = self.state != PlaybackState::Playing;
        if self.session.is_some() && !self.set_temporary_rate(rate, paused) {
            return;
        }
        if ended {
            self.state = PlaybackState::Ended;
        }
        self.preview_rates.insert(id, rate);
        self.set_status(towavue_core::localization::formatted::preview_rate(
            self.language(),
            rate,
        ));
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests;
