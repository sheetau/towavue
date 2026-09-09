use super::*;
use towavue_core::{EditTimeline, PlaybackRange};

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
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
        let range_changed = plan.is_none() && session.range() != state.playback_range();
        if changed || range_changed || session.rate() != state.rate {
            if changed {
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
