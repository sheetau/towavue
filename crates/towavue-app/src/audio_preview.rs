use crate::*;
use towavue_core::{AudioTrack, AudioTrackId, AudioTrackSelection};

#[derive(Clone, Debug)]
pub(super) struct Choice {
    path: PathBuf,
    selection: AudioTrackSelection,
}

impl Choice {
    pub(super) fn relocate(&mut self, source: &Path, target: &Path) {
        if self.path == source {
            self.path = target.to_owned();
        }
    }
}

pub(crate) fn track_label(
    language: localization::Language,
    index: usize,
    track: &AudioTrack,
) -> String {
    let mut label = format!(
        "{} {}",
        localization::Text::AudioTrack.in_language(language),
        index + 1
    );
    if let Some(title) = &track.title {
        label.push_str(" · ");
        label.push_str(&title.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    if let Some(language) = track.language.as_deref().filter(|value| *value != "und") {
        label.push_str(" (");
        label.push_str(&language.split_whitespace().collect::<Vec<_>>().join(" "));
        label.push(')');
    }
    label
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn audio_selection_for(
        &self,
        id: Option<TabId>,
        path: &Path,
    ) -> AudioTrackSelection {
        id.and_then(|id| self.audio_preview_choices.get(&id))
            .filter(|choice| choice.path == path)
            .map_or(AudioTrackSelection::Default, |choice| choice.selection)
    }

    pub(super) fn audio_selection(&self) -> AudioTrackSelection {
        if self.media_kind != Some(MediaKind::Video) {
            return AudioTrackSelection::Default;
        }
        self.path
            .as_deref()
            .map_or(AudioTrackSelection::Default, |path| {
                self.audio_selection_for(self.displayed_tab, path)
            })
    }

    pub(super) fn waveform_audio_track(&self) -> Option<AudioTrackId> {
        match self.audio_selection() {
            AudioTrackSelection::Default => None,
            AudioTrackSelection::Track(track) => Some(track),
            AudioTrackSelection::All => self
                .session
                .as_ref()?
                .audio_tracks()
                .tracks
                .first()
                .map(|track| track.id),
        }
    }

    pub(super) fn cycle_audio_track(&mut self) {
        let Some(track) = self
            .session
            .as_ref()
            .and_then(|session| session.audio_tracks().next_track(self.audio_selection()))
        else {
            return;
        };
        self.select_audio_track(AudioTrackSelection::Track(track));
    }

    pub(super) fn select_audio_track(&mut self, selection: AudioTrackSelection) {
        if self.media_kind != Some(MediaKind::Video)
            || self.modal_input_blocked()
            || self.command_context().playback_blocked
            || selection == self.audio_selection()
        {
            return;
        }
        let (Some(id), Some(path)) = (self.displayed_tab, self.path.clone()) else {
            return;
        };
        if self.tabs.active_id() != Some(id) {
            return;
        }
        self.cancel_hold_speed();
        let position = self.current_position();
        let language = self.language();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let label = match selection {
            AudioTrackSelection::Track(track) => {
                let Some((index, track)) = session
                    .audio_tracks()
                    .tracks
                    .iter()
                    .enumerate()
                    .find(|(_, candidate)| candidate.id == track)
                else {
                    return;
                };
                track_label(language, index, track)
            }
            AudioTrackSelection::Default => localization::Text::AudioTrackDefault
                .in_language(language)
                .into(),
            AudioTrackSelection::All => localization::Text::AudioTrackAll
                .in_language(language)
                .into(),
        };
        match session.set_audio_selection_at(position, selection) {
            Ok(generation) => {
                self.audio_preview_choices
                    .insert(id, Choice { path, selection });
                self.generation = generation;
                let mut clock = PlaybackClock::new(session.target(), session.rate());
                clock.set_paused(self.state != PlaybackState::Playing);
                self.clock = Some(clock);
                self.pending_time = None;
                self.decode_finished = false;
                self.audio_drained = !session.has_audio();
                self.metrics_recorded = false;
                self.pending_seek_started = None;
                self.waveform_worker.clear();
                self.waveform_request = self.waveform_request.wrapping_add(1);
                self.waveform_loading = false;
                self.waveform = None;
                self.waveform_detail = waveform_detail::Detail::default();
                if self.timeline_open {
                    self.load_waveform();
                }
                self.set_status(label);
                self.request_redraw();
            }
            Err(error) => self.fail_with_message(error.to_string(), error.message(self.language())),
        }
    }
}

#[cfg(test)]
mod tests;
