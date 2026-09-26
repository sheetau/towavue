use crate::MediaTime;

/// A subtitle stream index belonging to one source, never a native handle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SubtitleTrackId(usize);

impl SubtitleTrackId {
    pub const fn from_index(index: usize) -> Self {
        Self(index)
    }
    pub const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubtitleTrack {
    pub id: SubtitleTrackId,
    pub title: Option<String>,
    pub language: Option<String>,
}

/// Tenths of a second. Positive values delay subtitles; edits remain unchanged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubtitleDelay(i32);

impl SubtitleDelay {
    pub const fn from_tenths(tenths: i32) -> Self {
        Self(tenths)
    }
    pub const fn tenths(self) -> i32 {
        self.0
    }
    pub fn lookup_time(self, source: MediaTime) -> MediaTime {
        MediaTime::from_nanoseconds(
            source
                .as_nanoseconds()
                .saturating_sub(i64::from(self.0) * 100_000_000),
        )
    }
}

#[derive(Clone, Debug)]
pub struct SubtitleCue<T> {
    start: MediaTime,
    end: MediaTime,
    content: T,
}

impl<T> SubtitleCue<T> {
    /// Source timestamps may precede zero; never turn an invalid interval into
    /// a guessed duration. Endpoints are half-open, including after a seek.
    pub fn new(start: MediaTime, end: MediaTime, content: T) -> Option<Self> {
        (start < end).then_some(Self {
            start,
            end,
            content,
        })
    }
    pub fn start(&self) -> MediaTime {
        self.start
    }
    pub fn end(&self) -> MediaTime {
        self.end
    }
    pub fn content(&self) -> &T {
        &self.content
    }
}

#[derive(Clone, Debug)]
pub struct SubtitleTimeline<T> {
    cues: Vec<SubtitleCue<T>>,
    prefix_end: Vec<MediaTime>,
}

impl<T> SubtitleTimeline<T> {
    pub fn new(mut cues: Vec<SubtitleCue<T>>) -> Self {
        cues.sort_by_key(SubtitleCue::start);
        let mut latest = MediaTime::from_nanoseconds(i64::MIN);
        let prefix_end = cues
            .iter()
            .map(|cue| {
                latest = latest.max(cue.end);
                latest
            })
            .collect();
        Self { cues, prefix_end }
    }
    pub fn cues(&self) -> &[SubtitleCue<T>] {
        &self.cues
    }
    /// Both partitions are monotone even for nested/overlapping subtitles.
    /// Filter only candidate overlaps, without allocation or a playback cursor.
    pub fn active(
        &self,
        source: MediaTime,
        delay: SubtitleDelay,
    ) -> impl Iterator<Item = (usize, &SubtitleCue<T>)> {
        let time = delay.lookup_time(source);
        let end = self.cues.partition_point(|cue| cue.start <= time);
        let start = self.prefix_end[..end].partition_point(|end| *end <= time);
        self.cues[start..end]
            .iter()
            .enumerate()
            .filter(move |(_, cue)| time < cue.end)
            .map(move |(index, cue)| (start + index, cue))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditOperation, EditTimeline, TimeRange, TimelineEdit};
    fn ms(value: i64) -> MediaTime {
        MediaTime::from_nanoseconds(value * 1_000_000)
    }
    #[test]
    fn intervals_survive_overlap_seeks_signed_delay_and_edited_source_mapping() {
        let cue = |start, end, text| SubtitleCue::new(ms(start), ms(end), text).expect("cue");
        let subtitles = SubtitleTimeline::new(vec![
            cue(2000, 3000, "later"),
            cue(-100, 200, "lead"),
            cue(0, 4000, "long"),
            cue(1000, 1500, "short"),
        ]);
        let visible = |time, shift| {
            subtitles
                .active(ms(time), SubtitleDelay::from_tenths(shift))
                .map(|(_, cue)| *cue.content())
                .collect::<Vec<_>>()
        };
        assert_eq!(visible(0, 0), ["lead", "long"]);
        assert_eq!(visible(1500, 0), ["long"]);
        assert_eq!(visible(2000, 0), ["long", "later"]);
        assert_eq!(visible(1000, 0), ["long", "short"]);
        assert_eq!(visible(2000, 1), ["long"]);
        assert_eq!(visible(1900, -1), ["long", "later"]);
        assert!(visible(4000, 0).is_empty());
        assert!(SubtitleCue::new(ms(1), ms(1), "invalid").is_none());
        let range = |a, b| TimeRange::new(ms(a), ms(b)).expect("range");
        let plan = EditTimeline::from_operations(
            ms(4000),
            &[
                EditOperation::SetTrimStart(ms(500)),
                EditOperation::Timeline(TimelineEdit::Delete(range(500, 1500))),
                EditOperation::Timeline(TimelineEdit::Stretch(range(500, 1000), ms(1000))),
            ],
        )
        .expect("edited source map");
        let source = plan.source_time(ms(500)).expect("following span at join");
        assert_eq!(source, ms(2000));
        assert_eq!(
            subtitles
                .active(source, SubtitleDelay::default())
                .map(|(_, cue)| *cue.content())
                .collect::<Vec<_>>(),
            ["long", "later"]
        );
        assert_eq!(
            SubtitleDelay::from_tenths(i32::MAX)
                .lookup_time(MediaTime::from_nanoseconds(i64::MIN))
                .as_nanoseconds(),
            i64::MIN
        );
    }
}
