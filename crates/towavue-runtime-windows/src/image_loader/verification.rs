use std::time::Duration;

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

pub(super) enum Outcome {
    Completed,
    Failed,
    Unsupported,
    BudgetRejected,
    Superseded,
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
