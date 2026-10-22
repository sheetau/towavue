use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use towavue_core::MediaTime;

/// One output generation's scalar clock. The native owner alone changes state;
/// readers evaluate the existing wall component without calling WASAPI. No
/// device sample is extrapolated. The lock never covers native calls or waits.
pub(super) struct PositionClock(Mutex<State>);

struct State {
    anchor: MediaTime,
    rate: f32,
    device_nanoseconds: u128,
    wall_elapsed: Duration,
    started_at: Option<Instant>,
    skips: VecDeque<(u128, Duration)>,
}

impl State {
    fn wall_at(&self, now: Instant) -> Duration {
        self.wall_elapsed.saturating_add(
            self.started_at
                .map_or(Duration::ZERO, |start| now.saturating_duration_since(start)),
        )
    }

    fn position_at(&self, now: Instant) -> MediaTime {
        let elapsed = self.device_nanoseconds.max(self.wall_at(now).as_nanos());
        let skipped = self
            .skips
            .iter()
            .take_while(|(at, _)| *at <= elapsed)
            .fold(Duration::ZERO, |total, (_, duration)| {
                total.saturating_add(*duration)
            });
        let elapsed = elapsed.min(i64::MAX as u128) as i64;
        let scaled = (elapsed as f64 * f64::from(self.rate)).min(i64::MAX as f64) as i64;
        MediaTime::from_nanoseconds(self.anchor.as_nanoseconds().saturating_add(scaled))
            .saturating_add(skipped)
    }

    fn retire_skips(&mut self, now: Instant) {
        let elapsed = self.device_nanoseconds.max(self.wall_at(now).as_nanos());
        while self.skips.front().is_some_and(|(at, _)| *at <= elapsed) {
            let (_, duration) = self.skips.pop_front().expect("due skip");
            self.anchor = self.anchor.saturating_add(duration);
        }
    }

    fn pause_at(&mut self, now: Instant) {
        self.wall_elapsed = self.wall_at(now);
        self.started_at = None;
    }
}

impl PositionClock {
    pub(super) fn new(anchor: MediaTime) -> Self {
        Self(Mutex::new(State {
            anchor,
            rate: 1.0,
            device_nanoseconds: 0,
            wall_elapsed: Duration::ZERO,
            started_at: None,
            skips: VecDeque::new(),
        }))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // Only clock metadata is accessed under this lock. Retain the coherent
        // state even during unwinding so the run guard can always freeze it.
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub(super) fn position(&self) -> MediaTime {
        let state = self.state();
        state.position_at(Instant::now())
    }

    pub(super) fn run(&self, rate: f32) -> RunClock<'_> {
        self.state().rate = rate;
        RunClock(self)
    }
}

/// Scoped to render_audio_loop, not the reusable WASAPI thread. Every return
/// and unwind freezes this generation before its terminal event or replacement.
pub(super) struct RunClock<'a>(&'a PositionClock);

impl RunClock<'_> {
    pub(super) fn start(&self) {
        let mut state = self.0.state();
        if state.started_at.is_none() {
            state.started_at = Some(Instant::now());
        }
    }

    pub(super) fn pause(&self) {
        self.0.state().pause_at(Instant::now());
    }

    pub(super) fn publish_device(&self, nanoseconds: u128) {
        let mut state = self.0.state();
        state.device_nanoseconds = nanoseconds;
        state.retire_skips(Instant::now());
    }

    pub(super) fn skip_at(&self, output_nanoseconds: u128, source_duration: Duration) {
        let mut state = self.0.state();
        state.retire_skips(Instant::now());
        // The native owner schedules in submitted-PCM order. Pending entries
        // cover only the bounded audio queue; past entries become scalar offset.
        state.skips.push_back((output_nanoseconds, source_duration));
    }
}

impl Drop for RunClock<'_> {
    fn drop(&mut self) {
        self.pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(value: i64) -> MediaTime {
        MediaTime::from_nanoseconds(value)
    }

    #[test]
    fn wall_advances_between_publications_without_extrapolating_device() {
        let clock = PositionClock::new(ns(3_000_000_000));
        let run = clock.run(1.0);
        let t = Instant::now();
        clock.state().started_at = Some(t);
        run.publish_device(20_000_000);
        assert_eq!(
            clock.state().position_at(t + Duration::from_millis(5)),
            ns(3_020_000_000)
        );
        assert_eq!(
            clock.state().position_at(t + Duration::from_millis(19)),
            ns(3_020_000_000)
        );
        assert_eq!(
            clock.state().position_at(t + Duration::from_millis(25)),
            ns(3_025_000_000)
        );
    }

    #[test]
    fn pause_excludes_idle_time_and_resume_retains_elapsed_at_each_rate() {
        for rate in [0.5, 1.0, 2.0] {
            let clock = PositionClock::new(ns(700));
            let _run = clock.run(rate);
            let t = Instant::now();
            let mut state = clock.state();
            assert_eq!(state.position_at(t + Duration::from_secs(60)), ns(700));
            state.started_at = Some(t);
            state.pause_at(t + Duration::from_millis(10));
            let expected = 700 + (10_000_000.0 * rate) as i64;
            assert_eq!(state.position_at(t + Duration::from_secs(60)), ns(expected));
            state.started_at = Some(t + Duration::from_secs(60));
            assert_eq!(
                state.position_at(t + Duration::from_millis(60_010)),
                ns(700 + (20_000_000.0 * rate) as i64)
            );
        }
    }

    #[test]
    fn all_exit_paths_freeze_and_replacement_has_independent_anchor_and_rate() {
        let old = PositionClock::new(ns(100));
        let result = std::panic::catch_unwind(|| {
            let run = old.run(2.0);
            run.start();
            run.publish_device(10_000_000);
            panic!("injected native failure");
        });
        assert!(result.is_err());
        assert!(old.state().started_at.is_none());
        let frozen = old.position();
        assert_eq!(
            old.state()
                .position_at(Instant::now() + Duration::from_secs(60)),
            frozen
        );
        let next = PositionClock::new(ns(3_000_000_000));
        let _next_run = next.run(0.5);
        assert_eq!(next.position(), ns(3_000_000_000));
        assert_eq!(old.position(), frozen);
    }

    #[test]
    fn clock_saturates_and_handles_instants_before_start() {
        let clock = PositionClock::new(ns(i64::MAX - 5));
        let run = clock.run(2.0);
        let t = Instant::now();
        clock.state().started_at = Some(t + Duration::from_secs(1));
        assert_eq!(clock.state().position_at(t), ns(i64::MAX - 5));
        run.publish_device(u128::MAX);
        assert_eq!(clock.position(), ns(i64::MAX));
    }

    #[test]
    fn skips_begin_at_pcm_boundaries_without_scaling_the_source_gap() {
        for rate in [0.5, 1.0, 2.0] {
            let clock = PositionClock::new(ns(10_000_000_000));
            let run = clock.run(rate);
            run.skip_at(1_000_000_000, Duration::from_secs(2));
            run.skip_at(1_500_000_000, Duration::from_millis(500));
            run.publish_device(900_000_000);
            assert_eq!(
                clock.position(),
                ns(10_000_000_000 + (900_000_000.0 * rate) as i64)
            );
            run.publish_device(1_000_000_000);
            assert_eq!(
                clock.position(),
                ns(12_000_000_000 + (1_000_000_000.0 * rate) as i64)
            );
            assert_eq!(clock.state().skips.len(), 1);
            run.publish_device(1_500_000_000);
            assert_eq!(
                clock.position(),
                ns(12_500_000_000 + (1_500_000_000.0 * rate) as i64)
            );
            assert!(clock.state().skips.is_empty());
        }
    }

    #[test]
    fn wall_clock_crosses_scheduled_skip_and_pause_does_not_advance_it() {
        let clock = PositionClock::new(ns(0));
        let run = clock.run(1.0);
        run.skip_at(100_000_000, Duration::from_secs(2));
        let t = Instant::now();
        let mut state = clock.state();
        state.started_at = Some(t);
        assert_eq!(
            state.position_at(t + Duration::from_millis(99)),
            ns(99_000_000)
        );
        assert_eq!(
            state.position_at(t + Duration::from_millis(100)),
            ns(2_100_000_000)
        );
        state.pause_at(t + Duration::from_millis(50));
        assert_eq!(
            state.position_at(t + Duration::from_secs(30)),
            ns(50_000_000)
        );
        state.started_at = Some(t + Duration::from_secs(30));
        assert_eq!(
            state.position_at(t + Duration::from_millis(30_050)),
            ns(2_100_000_000)
        );
        drop(state);
        drop(run);
        let fresh = PositionClock::new(ns(0));
        assert_eq!(fresh.position(), ns(0), "new seek does not inherit skips");
    }
}

#[cfg(test)]
mod control_tests {
    use super::super::*;

    #[test]
    fn accepted_pause_freezes_public_cursor_before_worker_acknowledgement() {
        let (audio_tx, _audio_rx) = mpsc::sync_channel(1);
        let (control_tx, control_rx) = mpsc::channel();
        let (_event_tx, event_rx) = mpsc::channel();
        let position_clock = Arc::new(PositionClock::new(MediaTime::from_nanoseconds(100)));
        let worker_clock = Arc::clone(&position_clock);
        let run = worker_clock.run(1.0);
        let output = AudioOutput {
            format: AudioFormat {
                sample_rate: 48_000,
                channels: 2,
            },
            audio_tx,
            control_tx,
            control_wake: Arc::new(ControlWake::default()),
            initial_audio_queued: Arc::new(AtomicBool::new(false)),
            event_rx,
            position_clock,
            paused_position: Cell::new(None),
            volume: Arc::new(AtomicU32::new(0)),
            drained: Arc::new(AtomicBool::new(false)),
            thread: None,
        };
        run.publish_device(1_000);
        output.set_paused(true).expect("pause queued");
        let paused = output.position();
        assert_eq!(paused, MediaTime::from_nanoseconds(1_100));
        // Deliberately withhold native control processing and publish a later
        // device sample. The public paused position must still be exact.
        run.publish_device(9_000);
        assert_eq!(output.position(), paused);
        output.set_paused(true).expect("duplicate pause queued");
        assert_eq!(output.position(), paused);
        output.set_paused(false).expect("resume queued");
        assert_eq!(output.position(), MediaTime::from_nanoseconds(9_100));
        assert!(matches!(
            control_rx.try_recv(),
            Ok(AudioControl::SetPaused(true))
        ));
        assert!(matches!(
            control_rx.try_recv(),
            Ok(AudioControl::SetPaused(true))
        ));
        assert!(matches!(
            control_rx.try_recv(),
            Ok(AudioControl::SetPaused(false))
        ));
        drop(control_rx);
        assert!(output.set_paused(true).is_err());
        run.publish_device(10_000);
        assert_eq!(output.position(), MediaTime::from_nanoseconds(10_100));
    }
}
