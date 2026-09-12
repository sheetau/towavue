use super::*;
use towavue_core::{EditTimeline, PlaybackRange};

pub(super) fn resizing_panel(context: &egui::Context, panel: egui::Id) -> bool {
    // egui 0.35's Panel keeps the pre-drag size until its resize handle releases.
    // Its handle ID is internal; the rendered resize regression guards this dependency.
    context.dragged_id() == Some(panel.with("__resize"))
}

pub(super) fn cancel_panel_resize(context: &egui::Context, panel: egui::Id) -> bool {
    if !context
        .read_response(panel.with("__resize"))
        .is_some_and(|response| response.dragged() || response.drag_stopped())
    {
        return false;
    }
    context.stop_dragging();
    context.data_mut(|data| data.insert_temp(panel.with("cancel-resize"), true));
    true
}

pub(super) fn panel_resize_enabled(ui: &egui::Ui, panel: egui::Id) -> bool {
    let enabled = ui.is_enabled() && !egui::Popup::is_any_open(ui.ctx());
    let interrupted = ui.input(|input| {
        input
            .events
            .iter()
            .take_while(|event| {
                !matches!(
                    event,
                    egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        ..
                    }
                )
            })
            .any(|event| {
                matches!(
                    event,
                    egui::Event::WindowFocused(false)
                        | egui::Event::Key {
                            key: egui::Key::Escape,
                            pressed: true,
                            ..
                        }
                )
            })
    });
    if !enabled || interrupted {
        cancel_panel_resize(ui.ctx(), panel);
    }
    // stop_dragging also reports drag_stopped. Suppress Panel's release calculation
    // for this pass so cancellation cannot persist the pointer's final height.
    let cancelled = ui.ctx().data_mut(|data| {
        data.remove_temp::<bool>(panel.with("cancel-resize"))
            .unwrap_or(false)
    });
    enabled && !cancelled
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn set_time_selection(&mut self, selection: Option<towavue_core::TimeRange>) {
        if self.time_selection != selection {
            let position = self.current_position();
            self.time_selection = selection;
            if self.playback_selection.take().is_some() {
                self.seek_to(position);
            }
        }
        self.request_redraw();
    }

    pub(super) fn play_time_selection(&mut self) {
        let Some(range) = self.time_selection else {
            return;
        };
        if self.session.is_none()
            || matches!(self.state, PlaybackState::Loading | PlaybackState::Faulted)
            || !self
                .playback_duration()
                .is_some_and(|duration| range.end() <= media_time(duration))
        {
            return;
        }
        self.playback_selection = Some(range);
        self.seek_to(range.start());
        if matches!(self.state, PlaybackState::Paused | PlaybackState::Ended) {
            self.toggle_pause();
        }
        if self.state == PlaybackState::Playing {
            self.set_status(
                "Playing selected time · Space pauses · Escape returns to full range".into(),
            );
        }
    }

    pub(super) fn set_time_selection_endpoint(&mut self, start: bool) {
        let Some(duration) = self.playback_duration().map(media_time) else {
            return;
        };
        let position = self.current_position().min(duration);
        let (a, b) = if start {
            (
                position,
                self.time_selection.map_or(duration, |range| range.end()),
            )
        } else {
            (
                self.time_selection
                    .map_or(MediaTime::ZERO, |range| range.start()),
                position,
            )
        };
        if let Some(range) = towavue_core::TimeRange::new(a, b) {
            self.set_time_selection(Some(range));
        }
    }
    pub(super) fn history_timeline(&self) -> Result<Option<EditTimeline>, &'static str> {
        let history = self.tabs.active().and_then(|tab| self.edits.get(&tab.id));
        let Some(history) = history.filter(|history| {
            history
                .operations()
                .iter()
                .any(|op| matches!(op, EditOperation::Timeline(_)))
        }) else {
            return Ok(None);
        };
        let duration = self
            .media_duration
            .ok_or("Wait for the source duration before editing time")?;
        history
            .timeline(media_time(duration))
            .map(Some)
            .ok_or("Invalid timeline history")
    }

    pub(super) fn prepare_timeline_edit(&mut self, operation: EditOperation) -> bool {
        let (Some(tab), Some(kind), Some(duration)) =
            (self.tabs.active(), self.media_kind, self.media_duration)
        else {
            self.set_status("Wait for the source duration before editing time".into());
            return false;
        };
        let mut candidate = self.edits.get(&tab.id).cloned().unwrap_or_default();
        if !candidate.push(operation, kind) || candidate.timeline(media_time(duration)).is_none() {
            self.set_status("Timeline unchanged: invalid range, gain or duration".into());
            return false;
        }
        true
    }

    pub(super) fn playback_duration(&self) -> Option<Duration> {
        self.session
            .as_ref()
            .and_then(PlaybackSession::timeline)
            .map(|plan| Duration::from_nanos(plan.duration().as_nanoseconds() as u64))
            .or(self.media_duration)
    }

    pub(super) fn playback_range(&self) -> PlaybackRange {
        if let Some(range) = self.playback_selection {
            return PlaybackRange {
                start: range.start(),
                end: Some(range.end()),
            };
        }
        self.session
            .as_ref()
            .filter(|session| session.timeline().is_some())
            .map_or_else(
                || self.edit_state().playback_range(),
                PlaybackSession::range,
            )
    }

    pub(super) fn sync_playback_edits(&mut self) {
        let state = self.edit_state();
        let plan = match self.history_timeline() {
            Ok(plan) => plan,
            Err(error) => {
                self.set_status(error.into());
                return;
            }
        };
        let position = self.current_position();
        let Some(session) = &mut self.session else {
            return;
        };
        session.set_volume(state.volume);
        let changed = session.timeline() != plan.as_ref();
        let target = if changed {
            remap_position(session.timeline(), plan.as_ref(), position)
        } else {
            position
        };
        let range_changed = plan.is_none()
            && self.playback_selection.is_none()
            && session.range() != state.playback_range();
        if changed || range_changed || session.rate() != state.rate {
            if changed {
                self.time_selection = None;
                self.playback_selection = None;
                self.thumbnail_worker.clear();
                self.thumbnail_loading = None;
                self.hover_thumbnail = None;
                self.failed_thumbnails.clear();
                self.tab_preview.clear();
            }
            self.seek_to(target);
        }
    }
}

// Preserve source identity through edits; a removed position lands at the next surviving join.
fn remap_position(
    old: Option<&EditTimeline>,
    new: Option<&EditTimeline>,
    position: MediaTime,
) -> MediaTime {
    let source = old.map_or(Some(position), |plan| plan.source_time(position));
    let Some(source) = source else {
        return MediaTime::ZERO;
    };
    let Some(plan) = new else {
        return source;
    };
    if let Some(edited) = plan.edited_time(source) {
        return edited;
    }
    let mut offset = 0;
    for span in plan.spans() {
        if span.source().start() >= source {
            return MediaTime::from_nanoseconds(offset);
        }
        offset += span.duration().as_nanoseconds();
    }
    plan.duration()
}

pub(super) fn waveform_regions(
    rect: egui::Rect,
    source_duration: Duration,
    plan: &EditTimeline,
    master_volume: f32,
) -> Vec<(egui::Rect, egui::Rect)> {
    let source_seconds = source_duration.as_secs_f64();
    let duration = plan.duration().as_seconds_f64();
    if source_seconds <= 0.0 || duration <= 0.0 {
        return Vec::new();
    }
    let mut offset = 0.0;
    plan.spans()
        .iter()
        .filter_map(|span| {
            let start = offset;
            offset += span.duration().as_seconds_f64();
            let gain = span.volume() * master_volume;
            if gain <= 0.0 {
                return None;
            }
            let destination = egui::Rect::from_center_size(
                egui::pos2(
                    rect.left() + rect.width() * ((start + offset) / (2.0 * duration)) as f32,
                    rect.center().y,
                ),
                egui::vec2(
                    rect.width() * ((offset - start) / duration) as f32,
                    rect.height() * gain,
                ),
            );
            let uv = egui::Rect::from_min_max(
                egui::pos2(
                    (span.source().start().as_seconds_f64() / source_seconds) as f32,
                    0.0,
                ),
                egui::pos2(
                    (span.source().end().as_seconds_f64() / source_seconds) as f32,
                    1.0,
                ),
            );
            Some((destination, uv))
        })
        .collect()
}

#[cfg(test)]
#[path = "timeline_edit_tests.rs"]
mod tests;
