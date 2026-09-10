use std::collections::VecDeque;
use std::sync::Arc;

use towavue_core::{MediaKind, MediaTime, PlaybackGeneration, PlaybackState, TabId};
use towavue_runtime_windows::{LatestTask, adjacent_video_frame};

use crate::{AppEvent, Application};

#[cfg(test)]
#[path = "audio_step_tests.rs"]
mod audio_tests;
#[cfg(test)]
mod tests;

pub(super) struct FrameSteps {
    worker: LatestTask,
    serial: u64,
    pending: Option<Request>,
    queued: VecDeque<bool>,
    // Resolved PTS may precede presentation during rapid, ordered input.
    cursor: Option<MediaTime>,
}

#[derive(Clone, Copy)]
struct Request {
    serial: u64,
    tab: TabId,
    media: u64,
    generation: PlaybackGeneration,
    forward: bool,
}

impl FrameSteps {
    pub(super) fn holds_frame(&self) -> bool {
        self.cursor.is_some()
    }

    pub(super) fn new() -> std::io::Result<Self> {
        Ok(Self {
            worker: LatestTask::new("towavue-frame-step")?,
            serial: 0,
            pending: None,
            queued: VecDeque::new(),
            cursor: None,
        })
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn step_audio(&mut self, forward: bool) {
        if self.media_kind != Some(MediaKind::Audio)
            || !matches!(
                self.state,
                PlaybackState::Playing | PlaybackState::Paused | PlaybackState::Ended
            )
            || self.session.is_none()
            || self.modal_input_blocked()
        {
            return;
        }
        self.cancel_hold_speed();
        let position = self.current_position();
        let step = std::time::Duration::from_millis(10);
        let target = if forward {
            position.saturating_add(step)
        } else {
            position.saturating_sub(step)
        };
        if let Err(error) = self
            .session
            .as_mut()
            .expect("audio session")
            .set_paused(true)
        {
            self.fail(error.to_string());
            return;
        }
        if let Some(clock) = &mut self.clock {
            clock.set_paused(true);
        }
        self.state = PlaybackState::Paused;
        self.seek_to(target);
    }

    pub(super) fn cancel_frame_steps(&mut self) {
        self.frame_steps.worker.clear();
        self.frame_steps.serial = self.frame_steps.serial.wrapping_add(1);
        self.frame_steps.pending = None;
        self.frame_steps.queued.clear();
        self.frame_steps.cursor = None;
    }

    pub(super) fn step_video_frame(&mut self, forward: bool) {
        if self.media_kind != Some(MediaKind::Video)
            || !matches!(
                self.state,
                PlaybackState::Playing | PlaybackState::Paused | PlaybackState::Ended
            )
            || self.modal_input_blocked()
        {
            return;
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if self.frame_steps.cursor.is_none() {
            if session.video_refresh_pending() || session.current_video_time().is_none() {
                self.set_status("Wait for the current video frame".into());
                return;
            }
            self.frame_steps.cursor = session.current_video_time();
        }
        if self.frame_steps.queued.len() + usize::from(self.frame_steps.pending.is_some()) >= 32 {
            self.set_status("Frame step queue is full (32 operations)".into());
            return;
        }
        if let Err(error) = session.set_paused(true) {
            self.fail(error.to_string());
            return;
        }
        if let Some(clock) = &mut self.clock {
            clock.set_paused(true);
        }
        self.state = PlaybackState::Paused;
        self.frame_steps.queued.push_back(forward);
        self.start_frame_step();
        self.refresh_title();
        self.request_redraw();
    }

    fn start_frame_step(&mut self) {
        if self.frame_steps.pending.is_some() {
            return;
        }
        let Some(forward) = self.frame_steps.queued.pop_front() else {
            return;
        };
        let (Some(path), Some(tab), Some(base)) = (
            self.path.clone(),
            self.tabs.active().map(|tab| tab.id),
            self.frame_steps.cursor,
        ) else {
            self.cancel_frame_steps();
            return;
        };
        let plan = match self.history_timeline() {
            Ok(plan) => plan,
            Err(error) => {
                self.cancel_frame_steps();
                self.set_status(error.into());
                return;
            }
        };
        self.frame_steps.serial = self.frame_steps.serial.wrapping_add(1);
        let serial = self.frame_steps.serial;
        self.frame_steps.pending = Some(Request {
            serial,
            tab,
            media: self.media_generation,
            generation: self.generation,
            forward,
        });
        let notify = Arc::clone(&self.notify);
        self.frame_steps.worker.submit(move |cancellation| {
            let result = adjacent_video_frame(&path, base, forward, plan.as_ref(), &|| {
                cancellation.is_cancelled()
            })
            .map_err(|error| error.to_string());
            if !cancellation.is_cancelled() {
                notify(AppEvent::FrameStep(serial, result));
            }
        });
        self.set_status(
            if forward {
                "Finding next video frame"
            } else {
                "Finding previous video frame"
            }
            .into(),
        );
    }

    pub(super) fn finish_frame_step(
        &mut self,
        serial: u64,
        result: Result<Option<MediaTime>, String>,
    ) {
        let Some(request) = self
            .frame_steps
            .pending
            .filter(|request| request.serial == serial)
        else {
            return;
        };
        if self.tabs.active().map(|tab| tab.id) != Some(request.tab)
            || self.media_generation != request.media
            || self.generation != request.generation
            || self.state != PlaybackState::Paused
            || self.media_kind != Some(MediaKind::Video)
            || self.session.is_none()
            || self.modal_input_blocked()
            || self.palette_open
            || self.grid_open
        {
            self.cancel_frame_steps();
            return;
        }
        self.frame_steps.pending = None;
        match result {
            Ok(Some(target)) => {
                let queued = std::mem::take(&mut self.frame_steps.queued);
                self.seek_to(target);
                if self.state != PlaybackState::Paused || self.generation == request.generation {
                    return;
                }
                self.frame_steps.cursor = Some(target);
                self.frame_steps.queued = queued;
            }
            Ok(None) => self.set_status(
                if request.forward {
                    "No next video frame"
                } else {
                    "No previous video frame"
                }
                .into(),
            ),
            Err(error) => {
                self.cancel_frame_steps();
                self.set_status(format!("Frame step: {error}"));
                return;
            }
        }
        self.start_frame_step();
        self.request_redraw();
    }
}
