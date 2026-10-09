use super::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Barrier, Condvar, Mutex};
use std::time::Duration;

thread_local! {
    static FAILED_SPAWNS: Cell<u8> = const { Cell::new(0) };
}

pub(super) fn spawn_fails(lane: usize) -> bool {
    FAILED_SPAWNS.with(|mask| mask.get() & (1 << lane) != 0)
}

struct SpawnFailures(u8);
impl SpawnFailures {
    fn set(mask: u8) -> Self {
        Self(FAILED_SPAWNS.with(|value| value.replace(mask)))
    }
}
impl Drop for SpawnFailures {
    fn drop(&mut self) {
        FAILED_SPAWNS.with(|value| value.set(self.0));
    }
}

#[test]
fn every_spawn_failure_combination_reads_each_lane_once_and_keeps_order() {
    for mask in 0..4 {
        let _failure = SpawnFailures::set(mask);
        let caller = std::thread::current().id();
        let calls = [
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
        ];
        let fonts = read_font_lanes(|lane| {
            calls[lane].fetch_add(1, Ordering::SeqCst);
            let inline = lane == 2 || mask & (1 << lane) != 0;
            assert_eq!(std::thread::current().id() == caller, inline);
            (lane..11)
                .step_by(3)
                .map(|ordinal| (ordinal, ordinal))
                .collect()
        });
        assert_eq!(fonts, (0..11).collect::<Vec<_>>());
        assert!(calls.iter().all(|count| count.load(Ordering::SeqCst) == 1));
    }
}

#[test]
fn reverse_lane_completion_preserves_original_family_precedence() {
    let progress = (Mutex::new(Vec::new()), Condvar::new());
    let fonts = read_font_lanes(|lane| {
        let (lock, changed) = &progress;
        let completed = lock.lock().expect("completion lock");
        let (mut completed, timeout) = changed
            .wait_timeout_while(completed, Duration::from_secs(5), |completed| {
                completed.len() != 2 - lane
            })
            .expect("completion wait");
        assert!(!timeout.timed_out(), "all font workers started");
        completed.push(lane);
        changed.notify_all();
        (lane..11)
            .step_by(3)
            .map(|ordinal| (ordinal, ordinal))
            .collect()
    });
    assert_eq!(*progress.0.lock().expect("completion order"), [2, 1, 0]);
    assert_eq!(fonts, (0..11).collect::<Vec<_>>());
}

#[test]
fn caller_and_helper_panics_join_every_started_lane_before_unwinding() {
    for panic_lane in 0..3 {
        let started = Barrier::new(3);
        let retired = AtomicUsize::new(0);
        struct Retire<'a>(&'a AtomicUsize);
        impl Drop for Retire<'_> {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let result = std::panic::catch_unwind(|| {
            read_font_lanes(|lane| {
                let _retire = Retire(&retired);
                started.wait();
                assert_ne!(lane, panic_lane, "Injected font lane panic");
                vec![(lane, lane)]
            })
        });
        assert!(result.is_err());
        assert_eq!(
            retired.load(Ordering::SeqCst),
            3,
            "all scoped reads retired"
        );
    }
}

#[test]
fn inline_fallback_preserves_installed_bytes_names_and_face_indices() {
    let Some(directory) = std::env::var_os("WINDIR") else {
        eprintln!("SKIP installed font comparison: WINDIR is unavailable");
        return;
    };
    let directory = PathBuf::from(directory).join("Fonts");
    let parallel = read_fallbacks(&directory);
    let inline = {
        let _failure = SpawnFailures::set(3);
        read_fallbacks(&directory)
    };
    assert_eq!(parallel.len(), inline.len());
    for (parallel, inline) in parallel.iter().zip(inline) {
        assert_eq!(parallel.name, inline.name);
        assert_eq!(parallel.index, inline.index);
        assert!(parallel.data == inline.data, "installed font bytes match");
    }
}
