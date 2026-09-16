use super::{DecodeError, EditTimeline, MediaTime, Path, check_cancelled, query};
use crate::export::frame::{FrameSource, SourceLease};
use std::collections::VecDeque;
use std::sync::Arc;

const CAPACITY: usize = 32;

/// Worker-side, bounded scalar PTS reuse. No decoder, file handle or pixels survive
/// a query. Call only off the UI thread: identity checks and cache misses perform IO.
#[derive(Default)]
pub struct FrameStepCache {
    source: Option<Arc<FrameSource>>,
    timeline: Option<EditTimeline>,
    window: Window,
    #[cfg(test)]
    pub(super) misses: usize,
}

impl FrameStepCache {
    pub fn adjacent(
        &mut self,
        path: &Path,
        target: MediaTime,
        forward: bool,
        timeline: Option<&EditTimeline>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<MediaTime>, DecodeError> {
        check_cancelled(cancelled)?;
        let source = source_stamp(path);
        if source.is_some()
            && source == self.source
            && timeline == self.timeline.as_ref()
            && let Some(next) = self.window.adjacent(target, forward)
        {
            check_cancelled(cancelled)?;
            return Ok(Some(next));
        }
        // A failed, cancelled, changed-source or incomplete scan must not leave a
        // partially observed interval available to the next request.
        self.source = None;
        self.timeline = None;
        self.window.clear();
        #[cfg(test)]
        {
            self.misses += 1;
        }
        let mut observed = Window::default();
        let result = query(
            path,
            target,
            forward,
            timeline,
            cancelled,
            Some(&mut observed),
        )?;
        check_cancelled(cancelled)?;
        if source.is_some() && !observed.unordered && source_stamp(path) == source {
            self.source = source;
            self.timeline = timeline.cloned();
            self.window = observed;
        }
        Ok(result)
    }
}

fn source_stamp(path: &Path) -> Option<Arc<FrameSource>> {
    // Reuse the existing file-id/size/creation/write/change-time identity. The
    // short read lease ends here, before native scanning; metadata is rechecked
    // afterward. Busy/unverifiable inputs keep the uncached query path.
    Some(SourceLease::open(path).ok()?.source)
}

#[derive(Default)]
pub(super) struct Window {
    times: VecDeque<MediaTime>,
    unordered: bool,
}

impl Window {
    pub(super) fn clear(&mut self) {
        self.times.clear();
        self.unordered = false;
    }

    pub(super) fn observe(&mut self, time: MediaTime) {
        if self.unordered {
            return;
        }
        if let Some(&previous) = self.times.back() {
            if time < previous {
                self.times.clear();
                self.unordered = true;
                return;
            }
            if time == previous {
                return;
            }
        }
        if self.times.len() == CAPACITY {
            self.times.pop_front();
        }
        self.times.push_back(time);
    }

    fn adjacent(&self, target: MediaTime, forward: bool) -> Option<MediaTime> {
        // Only exact observed anchors have proven neighbors. In particular the
        // edges do not imply EOF, and separate scans/edit spans are never joined.
        let index = self.times.iter().position(|time| *time == target)?;
        self.times
            .get(if forward {
                index + 1
            } else {
                index.checked_sub(1)?
            })
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_bounded_distinct_and_never_guesses_at_unknown_edges() {
        let mut window = Window::default();
        for ns in 0..100 {
            window.observe(MediaTime::from_nanoseconds(ns * 10));
            window.observe(MediaTime::from_nanoseconds(ns * 10));
        }
        assert_eq!(window.times.len(), CAPACITY);
        for (ns, forward, expected) in [
            (680, false, None),
            (990, true, None),
            (685, false, None),
            (670, true, None),
            (690, false, Some(680)),
            (980, true, Some(990)),
        ] {
            assert_eq!(
                window.adjacent(MediaTime::from_nanoseconds(ns), forward),
                expected.map(MediaTime::from_nanoseconds)
            );
        }
        window.observe(MediaTime::ZERO);
        window.observe(MediaTime::from_nanoseconds(2000));
        assert!(window.unordered && window.times.is_empty());
        window.clear();
        window.observe(MediaTime::ZERO);
        assert!(!window.unordered && window.times.len() == 1);
    }

    #[test]
    fn cache_validates_source_identity_edits_cancellation_and_busy_sources() {
        let root = std::env::temp_dir().join(format!(
            "towavue-step-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir(&root).expect("owned directory");
        let path = root.join("source.mp4");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4"),
            &path,
        )
        .expect("owned source");
        let original = std::fs::read(&path).expect("source bytes");
        let time = |ns| MediaTime::from_nanoseconds(ns);
        let mut cache = FrameStepCache::default();
        let mut cursor = cache
            .adjacent(&path, time(1_500_000_000), false, None, &|| false)
            .expect("prime")
            .expect("previous");
        let misses = cache.misses;
        for _ in 0..10 {
            let expected =
                super::super::adjacent_video_frame(&path, cursor, false, None, &|| false)
                    .expect("independent probe");
            assert_eq!(
                cache
                    .adjacent(&path, cursor, false, None, &|| false)
                    .expect("cached"),
                expected
            );
            cursor = expected.expect("neighbor");
        }
        assert_eq!(cache.misses, misses, "ten predecessors reuse one scan");

        assert!(matches!(
            cache.adjacent(&path, cursor, false, None, &|| true),
            Err(DecodeError::ConsumerClosed)
        ));
        assert_eq!(cache.misses, misses, "cancelled hit performs no scan");

        let modified = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        let replacement = root.join("replacement.mp4");
        std::fs::write(&replacement, &original).expect("same-sized replacement");
        std::fs::File::options()
            .write(true)
            .open(&replacement)
            .expect("replacement")
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .expect("preserve mtime");
        std::fs::rename(&path, root.join("old.mp4")).expect("no retained source handle");
        std::fs::rename(&replacement, &path).expect("replace source with same bytes/mtime");
        cache
            .adjacent(&path, cursor, false, None, &|| false)
            .expect("changed file id");
        assert_eq!(cache.misses, misses + 1);

        // Preserve file ID, size and mtime: native change time must still invalidate.
        let previous_identity = source_stamp(&path).expect("identity before rewrite");
        {
            use std::io::Write;
            let mut writer = std::fs::File::options()
                .write(true)
                .open(&path)
                .expect("in-place writer");
            writer.write_all(&original).expect("same-byte rewrite");
            writer
                .set_times(std::fs::FileTimes::new().set_modified(modified))
                .expect("restore mtime");
        }
        assert_ne!(
            source_stamp(&path).expect("rewritten identity"),
            previous_identity
        );
        cache
            .adjacent(&path, cursor, false, None, &|| false)
            .expect("changed source metadata");
        assert_eq!(cache.misses, misses + 2);

        let mut plan =
            EditTimeline::new(time(2_000_000_000), towavue_core::PlaybackRange::default())
                .expect("plan");
        assert!(plan.apply(towavue_core::TimelineEdit::Delete(
            towavue_core::TimeRange::new(time(200_000_000), time(400_000_000)).expect("cut")
        )));
        let expected =
            super::super::adjacent_video_frame(&path, cursor, false, Some(&plan), &|| false)
                .expect("edited control");
        assert_eq!(
            cache
                .adjacent(&path, cursor, false, Some(&plan), &|| false)
                .expect("changed edits"),
            expected
        );
        assert_eq!(cache.misses, misses + 3);

        let calls = std::sync::atomic::AtomicUsize::new(0);
        assert!(matches!(
            cache.adjacent(&path, time(1_800_000_000), false, None, &|| calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                >= 25),
            Err(DecodeError::ConsumerClosed)
        ));
        assert!(
            cache.source.is_none() && cache.window.times.is_empty(),
            "cancelled scan publishes nothing"
        );
        assert_eq!(
            cache
                .adjacent(&path, cursor, false, None, &|| false)
                .expect("recovery"),
            super::super::adjacent_video_frame(&path, cursor, false, None, &|| false)
                .expect("control")
        );

        // A competing write handle prevents a trustworthy read lease, not viewing.
        let writer = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("owned writer");
        for _ in 0..2 {
            cache
                .adjacent(&path, cursor, false, None, &|| false)
                .expect("uncached busy source");
            assert!(cache.source.is_none());
        }
        drop(writer);
        assert_eq!(std::fs::read(&path).expect("unchanged source"), original);
        std::fs::remove_dir_all(root).expect("remove owned fixtures");
    }
}
