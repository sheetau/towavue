use super::*;
use towavue_core::{EditTimeline, PlaybackRange};

#[cfg(test)]
#[path = "timeline_panel_tests.rs"]
mod panel_tests;

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

pub(super) fn panel_resize_enabled(
    ui: &egui::Ui,
    panel: egui::Id,
    collapse_requested: bool,
) -> bool {
    let enabled = ui.is_enabled() && !egui::Popup::is_any_open(ui.ctx());
    let interrupted = !collapse_requested
        && ui.input(|input| {
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
    if cancelled || interrupted {
        // stop_dragging can look like a release in later layout passes too.
        // Keep cancellation effective for the entire frame, not just this pass.
        let frame = ui.ctx().cumulative_frame_nr();
        ui.ctx().data_mut(|data| {
            data.insert_temp(panel.with("cancel-resize-frame"), frame);
        });
    }
    if enabled
        && !cancelled
        && let Some(response) = ui.ctx().read_response(panel.with("__resize"))
    {
        tab_focus::observe_pointer_control(&response, "timeline-resize");
    }
    enabled && !cancelled
}

pub(super) fn panel_collapse_requested(
    context: &egui::Context,
    panel: egui::Id,
    bottom: f32,
    minimum: f32,
) -> bool {
    if context
        .data(|data| data.get_temp::<bool>(panel.with("cancel-resize")))
        .unwrap_or(false)
        || context.data(|data| data.get_temp::<u64>(panel.with("cancel-resize-frame")))
            == Some(context.cumulative_frame_nr())
        || !context
            .read_response(panel.with("__resize"))
            .is_some_and(|response| {
                response.dragged_by(egui::PointerButton::Primary)
                    || response.drag_stopped_by(egui::PointerButton::Primary)
            })
    {
        return false;
    }
    // Resolve the first decisive event, not the final pointer location. Crossing
    // the dead zone commits while held; an earlier cancellation/release wins.
    context.input(|input| {
        input
            .events
            .iter()
            .find_map(|event| match event {
                egui::Event::PointerMoved(pos) if bottom - pos.y < minimum - 8.0 => Some(true),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => Some(bottom - pos.y < minimum - 8.0),
                egui::Event::WindowFocused(false)
                | egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => Some(false),
                _ => None,
            })
            .unwrap_or(false)
    })
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn collapse_timeline(&mut self, tab: TabId, instance: u64) {
        if self.tabs.active_id() != Some(tab)
            || self.media_generation != instance
            || !self.timeline_is_visible()
            || self.filmstrip_open
            || self.preview_input_blocked()
        {
            return;
        }
        self.dispatch(CommandId::ToggleTimeline);
    }

    pub(super) fn set_time_selection(&mut self, selection: Option<towavue_core::TimeRange>) {
        if self.time_selection != selection {
            let position = self.current_position();
            let previous = self.playback_selection;
            self.time_selection = selection;
            self.playback_selection = selection.filter(|range| {
                self.playback_duration()
                    .is_some_and(|duration| range.end() <= media_time(duration))
                    && (previous.is_some()
                        || (self.state == PlaybackState::Playing
                            && position >= range.start()
                            && position < range.end()))
            });
            if self.playback_selection != previous {
                let target = self.playback_selection.map_or(position, |range| {
                    let repeat = match self.media_kind {
                        Some(MediaKind::Video) => self.video_repeat,
                        Some(MediaKind::Audio) => {
                            self.audio_mode().0 != towavue_core::RepeatMode::Off
                        }
                        _ => false,
                    };
                    if self.state == PlaybackState::Playing && repeat && position >= range.end() {
                        range.start()
                    } else {
                        position.max(range.start()).min(range.end())
                    }
                });
                self.seek_to(target);
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
        let volume = state.volume * self.playback_volume();
        let Some(session) = &mut self.session else {
            return;
        };
        session.set_volume(volume);
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
pub(super) fn remap_position(
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
    plan: Option<&EditTimeline>,
    master_volume: f32,
    preview: Option<(towavue_core::TimeRange, f32)>,
) -> Vec<(egui::Rect, egui::Rect)> {
    let source_seconds = source_duration.as_secs_f64();
    let duration = plan.map_or(source_seconds, |plan| plan.duration().as_seconds_f64());
    if source_seconds <= 0.0 || duration <= 0.0 {
        return Vec::new();
    }
    let preview = preview.map(|(range, factor)| {
        (
            range,
            factor.min(
                plan.and_then(|plan| plan.volume_scale_limit(range))
                    .unwrap_or(towavue_core::MAX_VOLUME),
            ),
        )
    });
    let mut regions = Vec::new();
    let mut append = |start: f64, end: f64, source_start: f64, source_end: f64, saved_gain: f32| {
        // Split only display geometry at preview boundaries. No timeline clone,
        // PCM decode, waveform rasterization or texture update is needed.
        let mut cuts = [start, start, end, end];
        if let Some((range, _)) = preview {
            cuts[1] = range.start().as_seconds_f64().clamp(start, end);
            cuts[2] = range.end().as_seconds_f64().clamp(start, end);
        }
        cuts.sort_by(f64::total_cmp);
        for pair in cuts.windows(2).filter(|pair| pair[0] < pair[1]) {
            let gain = f64::from(master_volume)
                * f64::from(saved_gain)
                * preview
                    .filter(|(range, _)| {
                        pair[0] >= range.start().as_seconds_f64()
                            && pair[1] <= range.end().as_seconds_f64()
                    })
                    .map_or(1.0, |(_, factor)| f64::from(factor));
            if gain <= 0.0 {
                continue;
            }
            let destination = egui::Rect::from_center_size(
                egui::pos2(
                    rect.left() + rect.width() * ((pair[0] + pair[1]) / (2.0 * duration)) as f32,
                    rect.center().y,
                ),
                egui::vec2(
                    rect.width() * ((pair[1] - pair[0]) / duration) as f32,
                    rect.height() * gain.min(1.0) as f32,
                ),
            );
            let source_at = |time| {
                (source_start + (source_end - source_start) * ((time - start) / (end - start)))
                    / source_seconds
            };
            let uv = egui::Rect::from_min_max(
                egui::pos2(
                    source_at(pair[0]) as f32,
                    (0.5 - 0.5 / gain.max(1.0)) as f32,
                ),
                egui::pos2(
                    source_at(pair[1]) as f32,
                    (0.5 + 0.5 / gain.max(1.0)) as f32,
                ),
            );
            regions.push((destination, uv));
        }
    };
    if let Some(plan) = plan {
        let mut offset = 0.0;
        for span in plan.spans() {
            let start = offset;
            offset += span.duration().as_seconds_f64();
            append(
                start,
                offset,
                span.source().start().as_seconds_f64(),
                span.source().end().as_seconds_f64(),
                span.volume(),
            );
        }
    } else {
        append(0.0, source_seconds, 0.0, source_seconds, 1.0);
    }
    regions
}

#[cfg(test)]
#[path = "timeline_edit_tests.rs"]
mod tests;
