use super::{PlaybackClock, media_time, playlist};
use egui::TextureHandle;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use towavue_core::{FolderSnapshot, ImageViewState, MediaKind, MediaTime, PlaybackState};
use towavue_runtime_windows::{AudioOutputEvent, GraphicsDevice, PlaybackSession};

pub(super) struct RetainedPlaybackTab {
    pub path: PathBuf,
    pub kind: MediaKind,
    pub instance: u64,
    pub origin: Option<(crate::window_host::WindowKey, u64)>,
    pub session: Option<PlaybackSession>,
    pub clock: Option<PlaybackClock>,
    pub state: PlaybackState,
    pub audio_drained: bool,
    pub decode_finished: bool,
    pub pending_time: Option<MediaTime>,
    pub duration: Option<Duration>,
    pub waveform: Option<TextureHandle>,
    pub view: ImageViewState,
    pub timeline_open: bool,
    pub time_selection: Option<towavue_core::TimeRange>,
    pub playback_selection: Option<towavue_core::TimeRange>,
    pub filmstrip_open: bool,
    pub filmstrip_view: crate::filmstrip::View,
    pub playlist: playlist::Playlist,
    pub folder_snapshot: Option<FolderSnapshot>,
    pub error: Option<String>,
    pub status: Option<(String, Instant)>,
    pub seek_latencies: Vec<Duration>,
    pub drift_samples: Vec<Duration>,
    pub metrics_recorded: bool,
    pub graphics_epoch: u64,
    pub recovery_position: Option<MediaTime>,
    pub video_suspended: bool,
}

pub(super) fn end(
    session: Option<&PlaybackSession>,
    duration: Option<Duration>,
) -> Option<MediaTime> {
    let duration = session
        .and_then(PlaybackSession::timeline)
        .map(|plan| plan.duration())
        .or_else(|| {
            duration
                .filter(|duration| !duration.is_zero())
                .map(media_time)
        });
    match (duration, session.and_then(PlaybackSession::range_end)) {
        (Some(duration), Some(end)) => Some(duration.min(end)),
        (duration, end) => duration.or(end),
    }
}

pub(super) fn position(
    session: Option<&PlaybackSession>,
    clock: Option<&PlaybackClock>,
    audio_drained: bool,
    pending_time: Option<MediaTime>,
    duration: Option<Duration>,
) -> MediaTime {
    let audio = session.and_then(PlaybackSession::audio_position);
    let position = (!audio_drained)
        .then_some(audio)
        .flatten()
        .or_else(|| clock.map(PlaybackClock::position))
        .or(audio)
        .or(pending_time)
        .or_else(|| session.map(PlaybackSession::target))
        .unwrap_or(MediaTime::ZERO);
    let duration = if session.is_some_and(|session| session.timeline().is_some()) {
        None
    } else {
        duration
    };
    let position = duration
        .filter(|duration| !duration.is_zero())
        .map_or(position, |duration| position.min(media_time(duration)));
    session
        .and_then(PlaybackSession::range_end)
        .map_or(position, |end| position.min(end))
}

impl RetainedPlaybackTab {
    pub fn position(&self) -> MediaTime {
        self.recovery_position.unwrap_or_else(|| {
            position(
                self.session.as_ref(),
                self.clock.as_ref(),
                self.audio_drained,
                self.pending_time,
                self.duration,
            )
        })
    }

    fn end(&self) -> Option<MediaTime> {
        end(self.session.as_ref(), self.duration)
    }

    pub fn fail(&mut self, error: String) {
        let position = self.position();
        self.anchor(position, true);
        if let Some(session) = &mut self.session {
            let _ = session.set_paused(true);
        }
        self.state = PlaybackState::Faulted;
        self.error = Some(error.clone());
        self.status = Some((error, Instant::now()));
    }

    fn anchor(&mut self, position: MediaTime, paused: bool) {
        let rate = self.session.as_ref().map_or(1.0, PlaybackSession::rate);
        let clock = if paused {
            PlaybackClock::paused(position, rate)
        } else {
            PlaybackClock::new(position, rate)
        };
        self.clock = Some(clock);
    }

    pub fn suspend_video_if_bounded(&mut self) {
        if self.kind != MediaKind::Video || self.video_suspended || self.end().is_none() {
            return;
        }
        let position = self.position();
        if let Some(session) = &mut self.session {
            match session.set_video_visible(false, position) {
                Ok(()) => {
                    self.video_suspended = true;
                    self.pending_time = None;
                    self.decode_finished = false;
                }
                Err(error) => self.fail(error.to_string()),
            }
        }
    }

    pub fn poll(&mut self) {
        if self.recovery_position.is_some() || self.session.is_none() {
            return;
        }
        let event = self
            .session
            .as_ref()
            .and_then(PlaybackSession::try_audio_event);
        match event {
            Some(AudioOutputEvent::Drained) => {
                if let Some(position) = self
                    .session
                    .as_ref()
                    .and_then(PlaybackSession::audio_position)
                {
                    self.anchor(position, self.state != PlaybackState::Playing);
                }
                self.audio_drained = true;
            }
            Some(AudioOutputEvent::EndpointChanged) => {
                let position = self.position();
                match self.session.as_mut().expect("live session").seek(position) {
                    Ok(_) => {
                        self.anchor(position, self.state != PlaybackState::Playing);
                        self.pending_time = None;
                        self.decode_finished = false;
                        self.audio_drained = !self.session.as_ref().expect("session").has_audio();
                    }
                    Err(error) => self.fail(error.to_string()),
                }
            }
            Some(AudioOutputEvent::Failed(error)) => self.fail(error),
            None => {}
        }
        self.suspend_video_if_bounded();
        if self.state != PlaybackState::Playing {
            return;
        }
        let position = self.position();
        let session = self.session.as_mut().expect("live session");
        if !self.video_suspended {
            // Unknown-duration video must reach a real EOF without decoding ahead of its clock.
            for _ in 0..4 {
                self.pending_time = session.pending_video_time();
                if self.pending_time.is_none_or(|time| time > position) {
                    break;
                }
                session.advance_pending();
            }
            self.pending_time = session.pending_video_time();
            self.decode_finished = session.decode_finished();
        }
        let finished = self.audio_drained
            && if self.video_suspended {
                self.end().is_some_and(|end| position >= end)
            } else {
                self.decode_finished
                    && self.pending_time.is_none()
                    && self.end().is_none_or(|end| position >= end)
            };
        if finished {
            self.anchor(self.end().unwrap_or(position), true);
            self.state = PlaybackState::Ended;
            if self.playback_selection.is_some() {
                self.status = Some((
                    "Selection ended · Shift+Space restarts · Escape returns to full range".into(),
                    Instant::now(),
                ));
            }
            if let Err(error) = self.session.as_mut().expect("session").set_paused(true) {
                self.fail(error.to_string());
            }
        }
    }

    pub fn needs_poll(&self) -> bool {
        self.session.is_some()
            && self.recovery_position.is_none()
            && self.state == PlaybackState::Playing
    }

    pub fn suspend_for_recovery(&mut self) {
        if self.recovery_position.is_none() {
            let position = self.position();
            self.recovery_position = Some(position);
            self.anchor(position, true);
        }
        if let Some(session) = &mut self.session {
            session.suspend_for_graphics_recovery(
                self.recovery_position.expect("saved recovery position"),
            );
        }
        self.waveform = None;
    }

    pub fn recover(&mut self, device: GraphicsDevice, epoch: u64) {
        let Some(position) = self.recovery_position else {
            return;
        };
        self.graphics_epoch = epoch;
        if let Some(session) = &mut self.session {
            if let Err(error) = session.replace_graphics_device(device, position) {
                self.fail(format!("Background pipeline recovery failed: {error}"));
                return;
            }
            self.audio_drained = !session.has_audio();
            self.decode_finished = false;
            self.pending_time = None;
        }
        self.recovery_position = None;
        self.anchor(position, self.state != PlaybackState::Playing);
    }
}
