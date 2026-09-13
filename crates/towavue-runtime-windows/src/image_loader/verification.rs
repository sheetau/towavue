use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Aggregate diagnostics only; no source paths, pixels, or per-file retention.
#[derive(Clone, Copy, Debug, Default)]
pub struct ImageLoadMetrics {
    pub initial_cache_hits: u64,
    pub late_cache_hits: u64,
    pub prefetch_waits: u64,
    pub prefetch_wait_elapsed: Duration,
    pub foreground: ImageDecodeMetrics,
    pub prefetch: ImageDecodeMetrics,
}

/// Outcomes are observed immediately after a decoder returns, not at cache
/// insertion or presentation. Calls still in flight have no elapsed time yet.
#[derive(Clone, Copy, Debug, Default)]
pub struct ImageDecodeMetrics {
    pub calls: u64,
    pub completed: u64,
    pub failed: u64,
    pub unsupported: u64,
    pub budget_rejected: u64,
    pub superseded: u64,
    pub elapsed: Duration,
    pub max_elapsed: Duration,
    pub superseded_elapsed: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Completed,
    Failed,
    Unsupported,
    BudgetRejected,
    Superseded,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageLoadTraceKind {
    Requested,
    Planned {
        position: usize,
        worker_decoding: bool,
    },
    Considered {
        remaining_bytes: usize,
        preceding_cached_bytes: usize,
        cache_hit: bool,
    },
    PrefetchDecodeStarted,
    PrefetchReturned {
        elapsed: Duration,
        outcome: Outcome,
    },
    OriginalCached,
    ForegroundCacheHit,
    WaitStarted,
    WaitFinished {
        elapsed: Duration,
    },
    ForegroundDecodeStarted,
    ForegroundReturned {
        elapsed: Duration,
        outcome: Outcome,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct ImageLoadTraceEvent {
    pub elapsed: Duration,
    pub generation: u64,
    pub kind: ImageLoadTraceKind,
}

/// Only timing/state leaves the loader; the selected path is never in a snapshot.
#[derive(Clone, Debug, Default)]
pub struct ImageLoadTrace {
    pub events: Vec<ImageLoadTraceEvent>,
    pub dropped: usize,
}

pub(super) struct Trace {
    path: PathBuf,
    origin: Instant,
    snapshot: ImageLoadTrace,
}

impl Trace {
    pub(super) fn new(path: PathBuf, origin: Instant) -> Self {
        Self {
            path,
            origin,
            snapshot: ImageLoadTrace::default(),
        }
    }

    pub(super) fn snapshot(&self) -> ImageLoadTrace {
        self.snapshot.clone()
    }

    fn record(&mut self, path: &Path, generation: u64, kind: ImageLoadTraceKind) {
        if path != self.path {
            return;
        }
        if self.snapshot.events.len() == 64 {
            self.snapshot.dropped += 1;
            return;
        }
        self.snapshot.events.push(ImageLoadTraceEvent {
            elapsed: self.origin.elapsed(),
            generation,
            kind,
        });
    }
}

impl super::Mailbox {
    pub(super) fn trace(&mut self, path: &Path, kind: ImageLoadTraceKind) {
        if let Some(trace) = &mut self.trace {
            trace.record(path, self.generation, kind);
        }
    }
}

impl ImageDecodeMetrics {
    pub(super) fn record(&mut self, elapsed: Duration, outcome: Outcome) {
        self.elapsed += elapsed;
        self.max_elapsed = self.max_elapsed.max(elapsed);
        match outcome {
            Outcome::Completed => self.completed += 1,
            Outcome::Failed => self.failed += 1,
            Outcome::Unsupported => self.unsupported += 1,
            Outcome::BudgetRejected => self.budget_rejected += 1,
            Outcome::Superseded => {
                self.superseded += 1;
                self.superseded_elapsed += elapsed;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_path_trace_is_bounded_and_does_not_publish_paths() {
        let path = PathBuf::from("private-selected-source.png");
        let mut trace = Trace::new(path.clone(), Instant::now());
        for generation in 0..70 {
            trace.record(
                Path::new("unrelated.png"),
                generation,
                ImageLoadTraceKind::Requested,
            );
            trace.record(&path, generation, ImageLoadTraceKind::Requested);
        }
        let snapshot = trace.snapshot();
        assert_eq!(snapshot.events.len(), 64);
        assert_eq!(snapshot.dropped, 6);
        assert_eq!(snapshot.events[0].generation, 0);
        assert_eq!(snapshot.events[63].generation, 63);
        assert!(!format!("{snapshot:?}").contains("private-selected-source"));
    }

    #[test]
    fn decoder_outcomes_and_superseded_time_are_disjoint() {
        let mut metrics = ImageDecodeMetrics {
            calls: 6,
            ..Default::default()
        };
        for (index, outcome) in [
            Outcome::Completed,
            Outcome::Failed,
            Outcome::Unsupported,
            Outcome::Superseded,
            Outcome::BudgetRejected,
        ]
        .into_iter()
        .enumerate()
        {
            metrics.record(Duration::from_millis(index as u64 + 1), outcome);
        }
        assert_eq!(
            (
                metrics.completed,
                metrics.failed,
                metrics.unsupported,
                metrics.superseded
            ),
            (1, 1, 1, 1)
        );
        assert_eq!(metrics.budget_rejected, 1);
        assert_eq!(metrics.elapsed, Duration::from_millis(15));
        assert_eq!(metrics.max_elapsed, Duration::from_millis(5));
        assert_eq!(metrics.superseded_elapsed, Duration::from_millis(4));
        assert_eq!(
            metrics.calls, 6,
            "one call may remain in flight at a snapshot"
        );
    }
}
