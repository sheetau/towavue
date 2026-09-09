use crate::{MediaTime, PlaybackRange};

/// A nonempty half-open interval on either the source or edited time axis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeRange {
    start: MediaTime,
    end: MediaTime,
}

impl TimeRange {
    pub fn new(start: MediaTime, end: MediaTime) -> Option<Self> {
        (start >= MediaTime::ZERO && start < end).then_some(Self { start, end })
    }

    pub fn start(self) -> MediaTime {
        self.start
    }
    pub fn end(self) -> MediaTime {
        self.end
    }
    pub fn duration(self) -> MediaTime {
        MediaTime::from_nanoseconds(self.end.as_nanoseconds() - self.start.as_nanoseconds())
    }
}

/// Selection coordinates refer to the edited timeline immediately before this operation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimelineEdit {
    Keep(TimeRange),
    Delete(TimeRange),
    SetVolume(TimeRange, f32),
    Stretch(TimeRange, MediaTime),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimelineSpan {
    source: TimeRange,
    duration: MediaTime,
    volume: f32,
}

impl TimelineSpan {
    pub fn source(&self) -> TimeRange {
        self.source
    }
    pub fn duration(&self) -> MediaTime {
        self.duration
    }
    pub fn volume(&self) -> f32 {
        self.volume
    }
    pub fn rate(&self) -> f64 {
        self.source.duration().as_nanoseconds() as f64 / self.duration.as_nanoseconds() as f64
    }

    fn source_offset(&self, offset: i64) -> i64 {
        (i128::from(offset) * i128::from(self.source.duration().as_nanoseconds())
            / i128::from(self.duration.as_nanoseconds())) as i64
    }
}

/// An ordered view of one source; deleting time never mutates or adds source media.
#[derive(Clone, Debug, PartialEq)]
pub struct EditTimeline {
    spans: Vec<TimelineSpan>,
    duration: MediaTime,
}

impl EditTimeline {
    pub fn from_operations(
        source_duration: MediaTime,
        operations: &[crate::EditOperation],
    ) -> Option<Self> {
        let state = crate::EditState::from_operations(operations);
        let mut timeline = Self::new(source_duration, state.playback_range())?;
        for operation in operations {
            if let crate::EditOperation::Timeline(edit) = operation
                && !timeline.apply(*edit)
            {
                return None;
            }
        }
        Some(timeline)
    }

    pub fn new(source_duration: MediaTime, range: PlaybackRange) -> Option<Self> {
        let source = TimeRange::new(range.start, range.end.unwrap_or(source_duration))?;
        if source.end > source_duration {
            return None;
        }
        Some(Self {
            duration: source.duration(),
            spans: vec![TimelineSpan {
                source,
                duration: source.duration(),
                volume: 1.0,
            }],
        })
    }

    pub fn spans(&self) -> &[TimelineSpan] {
        &self.spans
    }
    pub fn duration(&self) -> MediaTime {
        self.duration
    }

    /// At a join, select the following span. EOF maps to the final source endpoint.
    pub fn source_time(&self, edited: MediaTime) -> Option<MediaTime> {
        let mut offset = edited.as_nanoseconds();
        if offset < 0 || edited > self.duration {
            return None;
        }
        for (index, span) in self.spans.iter().enumerate() {
            if offset < span.duration.as_nanoseconds() || index + 1 == self.spans.len() {
                return Some(MediaTime::from_nanoseconds(
                    span.source.start.as_nanoseconds() + span.source_offset(offset),
                ));
            }
            offset -= span.duration.as_nanoseconds();
        }
        None
    }

    /// Deleted source positions have no edited-time coordinate.
    pub fn edited_time(&self, source: MediaTime) -> Option<MediaTime> {
        let mut offset = 0;
        for (index, span) in self.spans.iter().enumerate() {
            if source >= span.source.start
                && (source < span.source.end
                    || (index + 1 == self.spans.len() && source == span.source.end))
            {
                let local =
                    i128::from(source.as_nanoseconds() - span.source.start.as_nanoseconds())
                        * i128::from(span.duration.as_nanoseconds())
                        / i128::from(span.source.duration().as_nanoseconds());
                return Some(MediaTime::from_nanoseconds(offset + local as i64));
            }
            offset += span.duration.as_nanoseconds();
        }
        None
    }

    /// Reject invalid/overflowing edits atomically. Stretch preserves relative speeds.
    pub fn apply(&mut self, edit: TimelineEdit) -> bool {
        let range = match edit {
            TimelineEdit::Keep(range)
            | TimelineEdit::Delete(range)
            | TimelineEdit::SetVolume(range, _)
            | TimelineEdit::Stretch(range, _) => range,
        };
        if range.end > self.duration {
            return false;
        }
        if let TimelineEdit::SetVolume(_, volume) = edit
            && (!volume.is_finite() || !(0.0..=2.0).contains(&volume))
        {
            return false;
        }
        if let TimelineEdit::Stretch(_, duration) = edit
            && duration <= MediaTime::ZERO
        {
            return false;
        }
        let mut spans = Vec::new();
        let mut cursor = 0_i64;
        let mut selected = 0_i64;
        for span in &self.spans {
            let length = span.duration.as_nanoseconds();
            let a = (range.start.as_nanoseconds() - cursor).clamp(0, length);
            let b = (range.end.as_nanoseconds() - cursor).clamp(0, length);
            for (start, end, inside) in [(0, a, false), (a, b, true), (b, length, false)] {
                if start == end
                    || matches!(edit, TimelineEdit::Keep(_) if !inside)
                    || matches!(edit, TimelineEdit::Delete(_) if inside)
                {
                    continue;
                }
                let Some(source) = TimeRange::new(
                    MediaTime::from_nanoseconds(
                        span.source.start.as_nanoseconds() + span.source_offset(start),
                    ),
                    MediaTime::from_nanoseconds(
                        span.source.start.as_nanoseconds() + span.source_offset(end),
                    ),
                ) else {
                    return false;
                };
                let mut next = TimelineSpan {
                    source,
                    duration: MediaTime::from_nanoseconds(end - start),
                    volume: span.volume,
                };
                if inside {
                    match edit {
                        TimelineEdit::SetVolume(_, volume) => next.volume = volume,
                        TimelineEdit::Stretch(_, duration) => {
                            let scale = |position: i64| {
                                i128::from(position) * i128::from(duration.as_nanoseconds())
                                    / i128::from(range.duration().as_nanoseconds())
                            };
                            let stretched = scale(selected + end - start) - scale(selected);
                            let source_length = i128::from(source.duration().as_nanoseconds());
                            if stretched <= 0
                                || stretched > i128::from(i64::MAX)
                                || source_length * 4 < stretched
                                || source_length > stretched * 4
                            {
                                return false;
                            }
                            next.duration = MediaTime::from_nanoseconds(stretched as i64);
                            selected += end - start;
                        }
                        _ => {}
                    }
                }
                spans.push(next);
            }
            cursor += length;
        }
        let duration = spans.iter().try_fold(0_i64, |sum, span| {
            sum.checked_add(span.duration.as_nanoseconds())
        });
        let Some(duration) = duration else {
            return false;
        };
        let mut merged: Vec<TimelineSpan> = Vec::new();
        for span in spans {
            if let Some(previous) = merged.last_mut()
                && previous.source.end == span.source.start
                && previous.volume == span.volume
                && i128::from(previous.source.duration().as_nanoseconds())
                    * i128::from(span.duration.as_nanoseconds())
                    == i128::from(span.source.duration().as_nanoseconds())
                        * i128::from(previous.duration.as_nanoseconds())
            {
                previous.source.end = span.source.end;
                previous.duration = MediaTime::from_nanoseconds(
                    previous.duration.as_nanoseconds() + span.duration.as_nanoseconds(),
                );
            } else {
                merged.push(span);
            }
        }
        self.spans = merged;
        self.duration = MediaTime::from_nanoseconds(duration);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn time(value: i64) -> MediaTime {
        MediaTime::from_nanoseconds(value)
    }
    fn range(start: i64, end: i64) -> TimeRange {
        TimeRange::new(time(start), time(end)).expect("range")
    }
    fn timeline() -> EditTimeline {
        EditTimeline::new(time(1000), PlaybackRange::default()).expect("timeline")
    }

    #[test]
    fn restoring_gain_does_not_leave_artificial_source_cuts() {
        let mut plan = timeline();
        let original = plan.clone();
        assert!(plan.apply(TimelineEdit::SetVolume(range(200, 600), 0.5)));
        assert_eq!(plan.spans.len(), 3);
        assert!(plan.apply(TimelineEdit::SetVolume(range(0, 1000), 1.0)));
        assert_eq!(plan, original);
    }

    #[test]
    fn history_restores_timeline_and_saved_state_across_branching() {
        use crate::{EditHistory, EditOperation, MediaKind};
        let mut history = EditHistory::default();
        history.push(EditOperation::SetTrimStart(time(100)), MediaKind::Audio);
        history.push(EditOperation::SetTrimEnd(time(900)), MediaKind::Audio);
        history.push(
            EditOperation::Timeline(TimelineEdit::Delete(range(100, 300))),
            MediaKind::Audio,
        );
        history.mark_saved();
        let saved = history.timeline(time(1000)).expect("saved plan");
        assert_eq!(saved.spans[0].source, range(100, 200));
        assert_eq!(saved.spans[1].source, range(400, 900));
        let operation = EditOperation::Timeline(TimelineEdit::Keep(range(50, 150)));
        assert!(!history.push(operation, MediaKind::Image));
        assert!(!history.is_dirty());
        assert!(history.push(operation, MediaKind::Audio));
        let edited = history.timeline(time(1000)).expect("edited plan");
        assert!(history.undo());
        assert_eq!(history.timeline(time(1000)), Some(saved));
        assert!(!history.is_dirty());
        assert!(history.redo());
        assert_eq!(history.timeline(time(1000)), Some(edited));
        assert!(history.undo());
        assert!(history.push(
            EditOperation::Timeline(TimelineEdit::SetVolume(range(0, 600), 0.5)),
            MediaKind::Audio
        ));
        assert!(!history.redo());
        assert!(history.is_dirty());
        let before_delete = history.timeline(time(1000)).expect("before full deletion");
        history.push(
            EditOperation::Timeline(TimelineEdit::Delete(range(0, 600))),
            MediaKind::Audio,
        );
        let empty = history
            .timeline(time(1000))
            .expect("empty timeline is undoable");
        assert!(empty.spans().is_empty());
        assert_eq!(empty.duration(), MediaTime::ZERO);
        assert_eq!(empty.source_time(MediaTime::ZERO), None);
        assert_eq!(empty.edited_time(time(100)), None);
        assert!(history.undo());
        assert_eq!(history.timeline(time(1000)), Some(before_delete));
    }

    #[test]
    fn repeated_deletions_match_an_independent_discrete_source_list() {
        for first_start in 0..20 {
            for first_end in first_start + 1..=20 {
                if first_start == 0 && first_end == 20 {
                    continue;
                }
                let mut expected: Vec<i64> = (0..20)
                    .filter(|value| *value < first_start || *value >= first_end)
                    .collect();
                let mut plan = EditTimeline::new(time(20), PlaybackRange::default()).expect("plan");
                assert!(plan.apply(TimelineEdit::Delete(range(first_start, first_end))));
                if expected.len() > 2 {
                    expected.remove(1);
                    assert!(plan.apply(TimelineEdit::Delete(range(1, 2))));
                }
                assert_eq!(plan.duration(), time(expected.len() as i64));
                for (index, source) in expected.iter().enumerate() {
                    assert_eq!(plan.source_time(time(index as i64)), Some(time(*source)));
                    assert_eq!(plan.edited_time(time(*source)), Some(time(index as i64)));
                }
            }
        }
    }

    #[test]
    fn delete_and_keep_use_the_current_edited_axis_and_exact_join() {
        let mut plan = timeline();
        assert!(plan.apply(TimelineEdit::Delete(range(200, 400))));
        assert_eq!(plan.duration(), time(800));
        assert_eq!(plan.source_time(time(199)), Some(time(199)));
        assert_eq!(plan.source_time(time(200)), Some(time(400)));
        assert_eq!(plan.edited_time(time(300)), None);
        assert!(plan.apply(TimelineEdit::Keep(range(100, 300))));
        assert_eq!(
            plan.spans
                .iter()
                .map(|span| span.source)
                .collect::<Vec<_>>(),
            [range(100, 200), range(400, 500)]
        );
        assert_eq!(plan.source_time(time(200)), Some(time(500)));
        assert_eq!(plan.edited_time(time(500)), Some(time(200)));
        assert_eq!(plan.source_time(time(-1)), None);
        assert_eq!(plan.source_time(time(201)), None);
    }

    #[test]
    fn partial_volume_and_stretch_preserve_source_and_other_spans() {
        let mut plan = timeline();
        assert!(plan.apply(TimelineEdit::SetVolume(range(200, 600), 0.0)));
        assert!(plan.apply(TimelineEdit::Stretch(range(100, 700), time(900))));
        assert_eq!(plan.duration(), time(1300));
        assert_eq!(
            plan.spans
                .iter()
                .map(|span| span.duration.as_nanoseconds())
                .collect::<Vec<_>>(),
            [100, 150, 600, 150, 300]
        );
        assert_eq!(plan.spans[2].volume(), 0.0);
        assert_eq!(plan.spans[2].rate(), 2.0 / 3.0);
        assert_eq!(plan.source_time(time(250)), Some(time(200)));
        assert_eq!(plan.edited_time(time(600)), Some(time(850)));
        assert!(plan.apply(TimelineEdit::Delete(range(250, 850))));
        assert_eq!(plan.duration(), time(700));
        assert_eq!(plan.source_time(time(250)), Some(time(600)));
    }

    #[test]
    fn invalid_edits_leave_the_entire_plan_unchanged() {
        let mut plan = timeline();
        let before = plan.clone();
        for edit in [
            TimelineEdit::Delete(range(0, 1001)),
            TimelineEdit::Keep(range(0, 1001)),
            TimelineEdit::SetVolume(range(0, 1000), f32::NAN),
            TimelineEdit::SetVolume(range(0, 1000), f32::INFINITY),
            TimelineEdit::SetVolume(range(0, 1000), -1.0),
            TimelineEdit::Stretch(range(0, 1000), time(0)),
            TimelineEdit::Stretch(range(0, 1000), time(249)),
            TimelineEdit::Stretch(range(0, 1000), time(4001)),
        ] {
            assert!(!plan.apply(edit));
            assert_eq!(plan, before);
        }
        assert!(TimeRange::new(time(-1), time(0)).is_none());
        assert!(TimeRange::new(time(1), time(1)).is_none());
    }

    #[test]
    fn large_timestamps_and_stretch_rounding_do_not_drift_or_overflow() {
        let mut plan =
            EditTimeline::new(time(i64::MAX), PlaybackRange::default()).expect("large source");
        let before = plan.clone();
        assert!(!plan.apply(TimelineEdit::Stretch(range(0, 1000), time(2000))));
        assert_eq!(plan, before);
        assert_eq!(plan.source_time(time(i64::MAX)), Some(time(i64::MAX)));
        let mut plan = timeline();
        for (a, b) in [(0, 333), (333, 666)] {
            assert!(plan.apply(TimelineEdit::SetVolume(range(a, b), 0.5)));
        }
        assert!(plan.apply(TimelineEdit::Stretch(range(0, 1000), time(1001))));
        assert_eq!(plan.duration(), time(1001));
        assert_eq!(
            plan.spans
                .iter()
                .map(|span| span.duration.as_nanoseconds())
                .sum::<i64>(),
            1001
        );
        assert_eq!(plan.source_time(time(1001)), Some(time(1000)));
    }
}
