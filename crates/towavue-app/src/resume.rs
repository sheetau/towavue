use crate::*;
use towavue_core::EditTimeline;
use towavue_runtime_windows::{
    VideoResume, VideoResumeEvent, VideoResumeHistory, VideoResumeSource,
};

pub(super) struct Owner {
    source: VideoResumeSource,
    saved: Duration,
    written: Instant,
    // Ended also describes an explicit paused end-frame preview.
    pub(super) natural_end: bool,
}

impl Owner {
    fn record(
        &mut self,
        history: &VideoResumeHistory,
        session: Option<&PlaybackSession>,
        position: MediaTime,
        duration: Option<Duration>,
        state: PlaybackState,
        force: bool,
    ) {
        if (!force && self.written.elapsed() < Duration::from_secs(5))
            || !matches!(
                state,
                PlaybackState::Playing | PlaybackState::Paused | PlaybackState::Ended
            )
        {
            return;
        }
        let Some(session) = session else { return };
        let state = if state == PlaybackState::Ended && !self.natural_end {
            PlaybackState::Paused
        } else {
            state
        };
        let Some(position) = source_position(session.timeline(), position, duration, state) else {
            return;
        };
        self.written = Instant::now();
        if self.saved == position {
            return;
        }
        history.remember(self.source.clone(), position, std::time::SystemTime::now());
        self.saved = position;
    }
}

fn source_position(
    plan: Option<&EditTimeline>,
    position: MediaTime,
    duration: Option<Duration>,
    state: PlaybackState,
) -> Option<Duration> {
    let source = match plan {
        Some(plan) => plan.source_time(position)?,
        None => position,
    };
    let source = Duration::from_nanos(u64::try_from(source.as_nanoseconds()).ok()?);
    Some(
        if state == PlaybackState::Ended && duration.is_some_and(|duration| source >= duration) {
            Duration::ZERO
        } else {
            source
        },
    )
}

// This also runs from Drop, where no event-loop callback or N bound is needed.
pub(super) fn record<N>(app: &mut Application<N>, force: bool) {
    let Some(history) = &app.resume_history else {
        return;
    };
    if let Some(owner) = &mut app.resume_owner {
        let position = playback_tab::position(
            app.session.as_ref(),
            app.clock.as_ref(),
            app.audio_drained,
            app.pending_time,
            app.media_duration,
        );
        owner.record(
            history,
            app.session.as_ref(),
            position,
            app.media_duration,
            app.state,
            force,
        );
    }
    for saved in app.retained_playback.values_mut() {
        if let Some(owner) = &mut saved.resume {
            let position = playback_tab::position(
                saved.session.as_ref(),
                saved.clock.as_ref(),
                saved.audio_drained,
                saved.pending_time,
                saved.duration,
            );
            owner.record(
                history,
                saved.session.as_ref(),
                position,
                saved.duration,
                saved.state,
                force,
            );
        }
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn clear_video_resume(&mut self) {
        if self.resume_history.is_none() {
            self.set_status(
                "Could not clear video positions: resume history is unavailable".into(),
            );
            return;
        }
        // Snapshot unchanged active/retained owners before clearing: closing a
        // paused tab afterward must not recreate its pre-clear position. These
        // writes precede the cutoff and are discarded by the serialized clear.
        record(self, true);
        if let Some(history) = &self.resume_history {
            history.clear(std::time::SystemTime::now());
        }
        self.resume_revision = self.resume_revision.wrapping_add(1);
        self.resume_open = None;
        if self.state == PlaybackState::Loading
            && self.media_kind == Some(MediaKind::Video)
            && self.session.is_none()
            && let Some(path) = self.path.clone()
        {
            self.request_resume_or_open(path);
        }
    }

    pub(super) fn request_resume_or_open(&mut self, path: PathBuf) {
        if self.media_kind == Some(MediaKind::Video)
            && let Some(history) = &self.resume_history
        {
            self.state = PlaybackState::Loading;
            self.resume_revision = self.resume_revision.wrapping_add(1);
            history.load(self.resume_revision, path);
            self.refresh_title();
            self.request_redraw();
        } else {
            self.open_playback_path(path, None);
        }
    }
    pub(super) fn handle_video_resume(&mut self, event: VideoResumeEvent) {
        match event {
            VideoResumeEvent::Loaded {
                token,
                path,
                result,
            } => {
                if token != self.resume_revision
                    || self.path.as_ref() != Some(&path)
                    || self.media_kind != Some(MediaKind::Video)
                    || self.state != PlaybackState::Loading
                    || self.session.is_some()
                    || self.resume_open.is_some()
                {
                    return;
                }
                let resume = match result {
                    Ok(resume) => Some(resume),
                    Err(error) => {
                        self.set_status(format!("Video resume unavailable: {error}"));
                        None
                    }
                };
                if resume.is_some() && self.history_timeline().is_err() {
                    self.resume_open = resume;
                    self.load_duration(path);
                } else {
                    self.open_playback_path(path, resume);
                }
            }
            VideoResumeEvent::SaveFailed(error) => {
                self.set_status(format!("Could not save video position: {error}"))
            }
            VideoResumeEvent::ClearFailed(error) => {
                self.set_status(format!("Could not clear video positions: {error}"))
            }
        }
    }

    pub(super) fn install_resume(&mut self, resume: VideoResume) {
        let position = resume.position;
        self.resume_owner = Some(Owner {
            source: resume.source,
            saved: position.unwrap_or_default(),
            written: Instant::now(),
            natural_end: false,
        });
        if position.is_some() || self.history_timeline().is_ok_and(|plan| plan.is_some()) {
            self.restore_resume_position(media_time(position.unwrap_or_default()));
        }
    }

    pub(super) fn restore_resume_position(&mut self, source: MediaTime) {
        if let Ok(plan) = self.history_timeline() {
            let target = timeline_edit::remap_position(None, plan.as_ref(), source);
            self.seek_to(target);
        }
    }
}

#[cfg(test)]
mod tests;
