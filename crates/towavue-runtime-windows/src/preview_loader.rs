use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use towavue_core::MediaKind;

use crate::{Cancellation, MediaPreview, PreviewCache};

pub const VISIBLE_PREVIEW_LIMIT: usize = 64;

pub struct LoadedPreview {
    pub generation: u64,
    pub path: PathBuf,
    pub result: Result<MediaPreview, String>,
}

pub struct WarmedPreview {
    pub generation: u64,
    pub path: PathBuf,
    /// False stops this speculative pass; errors skip only the failed source.
    pub result: Result<bool, String>,
}

enum Completion {
    Pixels(Result<MediaPreview, String>),
    Warmed(Result<bool, String>),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PreviewStage {
    Memory,
    Disk,
    Generate,
}

#[derive(Clone)]
struct PendingPreview {
    path: PathBuf,
    kind: MediaKind,
    prefetch: bool,
    cache_only: bool,
    stage: PreviewStage,
    disk_key: Option<String>,
}

struct ActivePreview {
    cache_only: bool,
    path: PathBuf,
    kind: MediaKind,
    cancellation: Cancellation,
}

#[derive(Default)]
struct Mailbox {
    generation: u64,
    pending: Vec<PendingPreview>,
    active: Option<ActivePreview>,
    completed: Vec<(MediaKind, LoadedPreview)>,
    warmed: Option<(MediaKind, WarmedPreview)>,
    closed: bool,
}

pub struct PreviewLoader {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
}

impl PreviewLoader {
    pub fn is_idle(&self) -> bool {
        let mailbox = self.shared.0.lock().expect("preview mailbox");
        mailbox.active.is_none() && mailbox.pending.is_empty()
    }

    pub fn new(cache: PreviewCache, notify: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        let wic = crate::image::jpeg_wic_preview::worker_enabled();
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("towavue-filmstrip".into())
            .spawn(move || {
                let _wic = crate::image::jpeg_wic_preview::WorkerScope::new(wic);
                let disk_keys = std::cell::RefCell::new(None);
                run_worker(
                    worker_shared,
                    notify,
                    |path, kind, cancellation, stage, disk_key| {
                        let cache = cache.cancellable(cancellation.clone());
                        if stage == PreviewStage::Memory {
                            disk_keys.borrow_mut().take();
                            let (key, preview) = cache.cached_filmstrip_key(path, kind).ok()?;
                            *disk_key = Some(key);
                            preview
                        } else {
                            // Only a scheduling hint for this sweep. A later request checks again;
                            // generation still checks disk if another consumer stores an entry.
                            let mut keys = disk_keys.borrow_mut();
                            let keys = keys.get_or_insert_with(|| cache.disk_entry_keys());
                            let key = disk_key.as_deref()?;
                            if !keys.contains(key) {
                                return None;
                            }
                            // Disk image reads are bounded and never generate/probe media.
                            cache
                                .read_cached_image(path, key)
                                .ok()
                                .flatten()
                                .map(|preview| MediaPreview {
                                    image: preview.image,
                                    duration: None,
                                })
                        }
                    },
                    |path, kind, cancellation| {
                        cache
                            .cancellable(cancellation.clone())
                            .filmstrip(path, kind)
                            .map_err(|error| error.to_string())
                    },
                    |path, kind, cancellation| {
                        cache
                            .cancellable(cancellation.clone())
                            .warm_filmstrip(path, kind)
                            .map_err(|error| error.to_string())
                    },
                );
            })?;
        Ok(Self { shared })
    }

    pub fn request(&self, paths: Vec<(PathBuf, MediaKind)>) -> u64 {
        self.request_prioritized(paths, usize::MAX)
    }

    /// Complete viewport work, including cache misses, before offscreen prefetch.
    /// Within each class memory/disk hits still precede generation.
    pub fn request_prioritized(&self, paths: Vec<(PathBuf, MediaKind)>, foreground: usize) -> u64 {
        let mut wanted = Vec::new();
        for (index, (path, kind)) in paths.into_iter().enumerate() {
            if !wanted
                .iter()
                .any(|(old, old_kind, _)| *old == path && *old_kind == kind)
            {
                wanted.push((path, kind, index >= foreground));
                if wanted.len() == VISIBLE_PREVIEW_LIMIT {
                    break;
                }
            }
        }
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("preview mailbox");
        if let Some(active) = &mailbox.active {
            let retained = wanted
                .iter()
                .find(|(path, kind, _)| *path == active.path && *kind == active.kind);
            let foreground_pending = wanted.iter().any(|(path, kind, prefetch)| {
                !prefetch
                    && !mailbox
                        .completed
                        .iter()
                        .any(|(ready_kind, preview)| ready_kind == kind && preview.path == *path)
            });
            if active.cache_only
                || retained.is_none()
                || (retained.is_some_and(|(_, _, prefetch)| *prefetch) && foreground_pending)
            {
                active.cancellation.cancel();
            }
        }
        mailbox.generation = mailbox.generation.wrapping_add(1);
        let generation = mailbox.generation;
        mailbox.warmed = None;
        mailbox.completed.retain_mut(|(kind, preview)| {
            preview.generation = generation;
            wanted
                .iter()
                .any(|(path, wanted_kind, _)| *path == preview.path && wanted_kind == kind)
        });
        mailbox.pending = wanted
            .into_iter()
            .filter(|(path, kind, _)| {
                !mailbox
                    .completed
                    .iter()
                    .any(|(ready_kind, preview)| ready_kind == kind && preview.path == *path)
            })
            .map(|(path, kind, prefetch)| PendingPreview {
                path,
                kind,
                prefetch,
                cache_only: false,
                // Another preview consumer may have populated the shared cache since our miss.
                stage: PreviewStage::Memory,
                disk_key: None,
            })
            .collect();
        ready.notify_one();
        generation
    }

    /// Replace idle work with one cache-only request on the same worker. A later
    /// display request always preempts it, even when it wants the same path.
    pub fn request_warming(&self, path: PathBuf, kind: MediaKind) -> u64 {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("preview mailbox");
        if let Some(active) = &mailbox.active
            && (!active.cache_only || active.path != path || active.kind != kind)
        {
            active.cancellation.cancel();
        }
        mailbox.generation = mailbox.generation.wrapping_add(1);
        let generation = mailbox.generation;
        mailbox.completed.clear();
        if let Some((previous_kind, previous)) = &mut mailbox.warmed {
            if *previous_kind == kind && previous.path == path {
                previous.generation = generation;
            } else {
                mailbox.warmed = None;
            }
        }
        mailbox.pending = if mailbox.warmed.is_some() {
            Vec::new()
        } else {
            vec![PendingPreview {
                path,
                kind,
                prefetch: true,
                cache_only: true,
                stage: PreviewStage::Generate,
                disk_key: None,
            }]
        };
        ready.notify_one();
        generation
    }

    pub fn take_warmed(&self) -> Option<WarmedPreview> {
        self.shared
            .0
            .lock()
            .expect("preview mailbox")
            .warmed
            .take()
            .map(|(_, result)| result)
    }

    pub fn take_completed(&self) -> Vec<LoadedPreview> {
        std::mem::take(&mut self.shared.0.lock().expect("preview mailbox").completed)
            .into_iter()
            .map(|(_, preview)| preview)
            .collect()
    }
}

impl Drop for PreviewLoader {
    fn drop(&mut self) {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("preview mailbox");
        mailbox.closed = true;
        if let Some(active) = &mailbox.active {
            active.cancellation.cancel();
        }
        mailbox.pending.clear();
        mailbox.completed.clear();
        mailbox.warmed = None;
        ready.notify_one();
        // Cancel the owned child without joining work that may still be inside native probing.
    }
}

fn run_worker(
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    notify: impl Fn(),
    cached: impl Fn(
        &std::path::Path,
        MediaKind,
        &Cancellation,
        PreviewStage,
        &mut Option<String>,
    ) -> Option<MediaPreview>,
    generate: impl Fn(&std::path::Path, MediaKind, &Cancellation) -> Result<MediaPreview, String>,
    warm: impl Fn(&std::path::Path, MediaKind, &Cancellation) -> Result<bool, String>,
) {
    let (mutex, ready) = &*shared;
    loop {
        let (mut work, cancellation) = {
            let mut mailbox = ready
                .wait_while(mutex.lock().expect("preview mailbox"), |mailbox| {
                    !mailbox.closed && mailbox.pending.is_empty()
                })
                .expect("preview mailbox");
            if mailbox.closed {
                return;
            }
            // Visible misses must not wait for speculative metadata/probing.
            // Preserve cache-hit-first ordering within each demand class.
            let index = mailbox
                .pending
                .iter()
                .enumerate()
                .min_by_key(|(_, work)| (work.prefetch, work.stage))
                .expect("pending work")
                .0;
            let mut work = mailbox.pending[index].clone();
            if work.stage == PreviewStage::Disk && mailbox.pending.len() == 1 {
                // Nothing can overtake a single remaining request. Its ordinary load already
                // checks disk; avoid enumerating the whole cache just to schedule that lookup.
                work.stage = PreviewStage::Generate;
            }
            let cancellation = Cancellation::default();
            mailbox.active = Some(ActivePreview {
                cache_only: work.cache_only,
                path: work.path.clone(),
                kind: work.kind,
                cancellation: cancellation.clone(),
            });
            (work, cancellation)
        };
        let result = if work.cache_only {
            Some(Completion::Warmed(warm(
                &work.path,
                work.kind,
                &cancellation,
            )))
        } else if work.stage != PreviewStage::Generate {
            cached(
                &work.path,
                work.kind,
                &cancellation,
                work.stage,
                &mut work.disk_key,
            )
            .map(|preview| Completion::Pixels(Ok(preview)))
        } else {
            Some(Completion::Pixels(generate(
                &work.path,
                work.kind,
                &cancellation,
            )))
        };
        let publish = {
            let mut mailbox = mutex.lock().expect("preview mailbox");
            mailbox.active = None;
            if mailbox.closed || cancellation.is_cancelled() {
                false
            } else if let Some(result) = result {
                mailbox
                    .pending
                    .retain(|pending| pending.path != work.path || pending.kind != work.kind);
                let generation = mailbox.generation;
                match result {
                    Completion::Pixels(result) => mailbox.completed.push((
                        work.kind,
                        LoadedPreview {
                            generation,
                            path: work.path,
                            result,
                        },
                    )),
                    Completion::Warmed(result) => {
                        mailbox.warmed = Some((
                            work.kind,
                            WarmedPreview {
                                generation,
                                path: work.path,
                                result,
                            },
                        ))
                    }
                }
                true
            } else {
                if let Some(pending) = mailbox
                    .pending
                    .iter_mut()
                    .find(|pending| pending.path == work.path && pending.kind == work.kind)
                {
                    pending.disk_key = work.disk_key;
                    pending.stage =
                        if work.stage == PreviewStage::Memory && work.kind == MediaKind::Image {
                            PreviewStage::Disk
                        } else {
                            PreviewStage::Generate
                        };
                }
                false
            }
        };
        if publish {
            notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    // Existing display-only controls still exercise the production worker.
    fn run_worker(
        shared: Arc<(Mutex<Mailbox>, Condvar)>,
        notify: impl Fn(),
        cached: impl Fn(
            &std::path::Path,
            MediaKind,
            &Cancellation,
            PreviewStage,
            &mut Option<String>,
        ) -> Option<MediaPreview>,
        generate: impl Fn(&std::path::Path, MediaKind, &Cancellation) -> Result<MediaPreview, String>,
    ) {
        super::run_worker(shared, notify, cached, generate, |_, _, _| {
            panic!("unexpected warming")
        });
    }

    #[test]
    fn viewport_misses_finish_before_prefetch_probes_and_keep_cache_hits_first() {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = PreviewLoader {
            shared: Arc::clone(&shared),
        };
        let trace = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&trace);
        let (sent, ready) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    let _ = sent.send(());
                },
                |path, _, _, _, _| {
                    observed
                        .lock()
                        .expect("trace")
                        .push(format!("cache:{}", path.display()));
                    (path == std::path::Path::new("hit.png")).then(|| MediaPreview {
                        image: crate::PreviewImage {
                            width: 1,
                            height: 1,
                            rgba: vec![1, 2, 3, 255].into(),
                        },
                        duration: None,
                    })
                },
                |path, _, _| {
                    observed
                        .lock()
                        .expect("trace")
                        .push(format!("generate:{}", path.display()));
                    Err("owned miss".into())
                },
            )
        });
        let generation = loader.request_prioritized(
            ["visible.png", "hit.png", "older.png"]
                .map(|path| (path.into(), MediaKind::Image))
                .into(),
            2,
        );
        for _ in 0..3 {
            ready
                .recv_timeout(Duration::from_secs(5))
                .expect("completion");
        }
        let results = loader.take_completed();
        assert_eq!(
            results
                .iter()
                .map(|result| result.path.as_path())
                .collect::<Vec<_>>(),
            ["hit.png", "visible.png", "older.png"].map(std::path::Path::new)
        );
        assert!(results.iter().all(|result| result.generation == generation));
        drop(loader);
        worker.join().expect("worker");
        let trace = trace.lock().expect("trace");
        assert!(
            trace
                .iter()
                .position(|event| event == "generate:visible.png")
                .expect("visible generation")
                < trace
                    .iter()
                    .position(|event| event == "cache:older.png")
                    .expect("prefetch probe")
        );
    }

    #[test]
    fn new_viewport_preempts_unneeded_active_work_and_reuses_promoted_or_completed_requests() {
        for (initial_foreground, promote, completed) in [
            (0, false, false),
            (1, false, false),
            (0, true, false),
            (1, true, false),
            (0, false, true),
        ] {
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let loader = PreviewLoader {
                shared: Arc::clone(&shared),
            };
            let (entered, started) = mpsc::channel();
            let (release, gate) = mpsc::channel();
            let (sent, ready) = mpsc::channel();
            let first = std::sync::atomic::AtomicBool::new(true);
            let worker = thread::spawn(move || {
                run_worker(
                    shared,
                    || {
                        let _ = sent.send(());
                    },
                    |_, _, _, _, _| None,
                    |path, _, cancellation| {
                        if path == std::path::Path::new("background.png")
                            && first.swap(false, std::sync::atomic::Ordering::SeqCst)
                        {
                            entered.send(cancellation.clone()).expect("entered");
                            gate.recv_timeout(Duration::from_secs(5)).expect("release");
                        }
                        Err(format!("owned generation: {}", path.display()))
                    },
                )
            });
            if completed {
                loader.request_prioritized(
                    ["visible.png", "background.png"]
                        .map(|path| (path.into(), MediaKind::Image))
                        .into(),
                    1,
                );
            } else {
                loader.request_prioritized(
                    vec![("background.png".into(), MediaKind::Image)],
                    initial_foreground,
                );
            }
            let active = started
                .recv_timeout(Duration::from_secs(5))
                .expect("active prefetch");
            let order = if promote {
                ["background.png", "visible.png"]
            } else {
                ["visible.png", "background.png"]
            };
            let generation = loader
                .request_prioritized(order.map(|path| (path.into(), MediaKind::Image)).into(), 1);
            assert_eq!(active.is_cancelled(), !promote && !completed);
            release.send(()).expect("resume worker");
            for _ in 0..2 {
                ready
                    .recv_timeout(Duration::from_secs(5))
                    .expect("completion");
            }
            let results = loader.take_completed();
            assert_eq!(
                results.len(),
                2,
                "cancelled prefetch cannot publish an extra stale result"
            );
            assert_eq!(
                results
                    .iter()
                    .map(|result| result.path.as_path())
                    .collect::<Vec<_>>(),
                order.map(std::path::Path::new)
            );
            assert!(results.iter().all(|result| result.generation == generation));
            drop(loader);
            worker.join().expect("worker");
        }
    }

    #[test]
    fn a_single_remaining_request_skips_the_disk_priority_sweep() {
        for memory_tail in [false, true] {
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let loader = PreviewLoader {
                shared: Arc::clone(&shared),
            };
            let stages = Arc::new(Mutex::new(Vec::new()));
            let observed = Arc::clone(&stages);
            let (notify, ready) = mpsc::channel();
            let worker = thread::spawn(move || {
                run_worker(
                    shared,
                    || notify.send(()).expect("notify"),
                    |path, _, _, stage, _| {
                        observed.lock().expect("stages").push(stage);
                        (path == std::path::Path::new("memory.png")).then(|| MediaPreview {
                            image: crate::PreviewImage {
                                width: 1,
                                height: 1,
                                rgba: vec![1, 2, 3, 255].into(),
                            },
                            duration: None,
                        })
                    },
                    |_, _, _| Err("ordinary load/cache fallback".into()),
                )
            });
            let mut paths = vec![(PathBuf::from("single.png"), MediaKind::Image)];
            if memory_tail {
                paths.push(("memory.png".into(), MediaKind::Image));
            }
            let count = paths.len();
            loader.request(paths);
            for _ in 0..count {
                ready
                    .recv_timeout(Duration::from_secs(5))
                    .expect("completion");
            }
            let results = loader.take_completed();
            drop(loader);
            worker.join().expect("worker exits");
            assert_eq!(results.len(), count);
            assert!(
                stages
                    .lock()
                    .expect("stages")
                    .iter()
                    .all(|stage| *stage == PreviewStage::Memory),
                "one remaining miss has nothing to overtake"
            );
            assert_eq!(
                results.last().expect("single request").path,
                PathBuf::from("single.png")
            );
        }
    }

    #[test]
    fn overlapping_requests_keep_active_generation_and_reprioritize_the_remaining_work() {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = PreviewLoader {
            shared: Arc::clone(&shared),
        };
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let (notify, ready) = mpsc::channel();
        let warmed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_warmed = Arc::clone(&warmed);
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    let _ = notify.send(());
                },
                |path, _, _, _, _| {
                    (path == std::path::Path::new("warming.png")
                        && worker_warmed.load(std::sync::atomic::Ordering::SeqCst))
                    .then(|| MediaPreview {
                        image: crate::PreviewImage {
                            width: 1,
                            height: 1,
                            rgba: vec![9, 0, 0, 255].into(),
                        },
                        duration: None,
                    })
                },
                |path, _, cancellation| {
                    started
                        .send((path.to_owned(), cancellation.clone()))
                        .expect("started");
                    if path == std::path::Path::new("shared.png") {
                        release_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("release shared work");
                    }
                    Err("fixture".into())
                },
            )
        });
        let paths = |names: &[&str]| {
            names
                .iter()
                .map(|name| (PathBuf::from(name), MediaKind::Image))
                .collect()
        };
        loader.request(paths(&[
            "shared.png",
            "obsolete.png",
            "kept.png",
            "warming.png",
        ]));
        let (path, cancellation) = started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("active work");
        assert_eq!(path, PathBuf::from("shared.png"));
        warmed.store(true, std::sync::atomic::Ordering::SeqCst);
        loader.request(paths(&["new.png", "shared.png", "kept.png", "warming.png"]));
        let generation =
            loader.request(paths(&["kept.png", "new.png", "shared.png", "warming.png"]));
        let retained = !cancellation.is_cancelled();
        release.send(()).expect("finish shared work");
        assert!(retained, "scrolling must not cancel a still-needed preview");
        for _ in 0..4 {
            ready
                .recv_timeout(Duration::from_secs(5))
                .expect("latest results");
        }
        let results = loader.take_completed();
        assert_eq!(
            results
                .iter()
                .map(|item| item.path.as_path())
                .collect::<Vec<_>>(),
            ["shared.png", "warming.png", "kept.png", "new.png"].map(std::path::Path::new)
        );
        assert!(results.iter().all(|item| item.generation == generation));
        for name in ["kept.png", "new.png"] {
            assert_eq!(
                started_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("remaining work")
                    .0,
                PathBuf::from(name)
            );
        }
        assert!(
            started_rx.try_recv().is_err(),
            "no duplicate or obsolete generation"
        );
        drop(loader);
        worker.join().expect("worker exits");
    }

    #[test]
    fn requests_preserve_unconsumed_results_and_cache_reads_only_for_matching_keys() {
        for kind in [MediaKind::Image, MediaKind::Video] {
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let loader = PreviewLoader {
                shared: Arc::clone(&shared),
            };
            let (started, started_rx) = mpsc::channel();
            let (release, release_rx) = mpsc::channel();
            let (notify, ready) = mpsc::channel();
            let worker = thread::spawn(move || {
                run_worker(
                    shared,
                    || {
                        let _ = notify.send(());
                    },
                    |path, kind, cancellation, _, _| {
                        started.send((path.to_owned(), kind)).expect("cache read");
                        if path == std::path::Path::new("active") {
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .expect("release cache read");
                            assert!(!cancellation.is_cancelled(), "retain matching cache lookup");
                        }
                        Some(MediaPreview {
                            image: crate::PreviewImage {
                                width: 1,
                                height: 1,
                                rgba: vec![1, 2, 3, 255].into(),
                            },
                            duration: None,
                        })
                    },
                    |_, _, _| panic!("all previews are cache hits"),
                )
            });
            loader.request(
                ["shared", "removed", "active"]
                    .map(|name| (name.into(), MediaKind::Image))
                    .into(),
            );
            for name in ["shared", "removed", "active"] {
                assert_eq!(
                    started_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("cache start"),
                    (PathBuf::from(name), MediaKind::Image)
                );
            }
            for _ in 0..2 {
                ready
                    .recv_timeout(Duration::from_secs(5))
                    .expect("queued result");
            }
            let generation = loader.request(vec![
                ("shared".into(), kind),
                ("active".into(), MediaKind::Image),
            ]);
            let retained = loader.take_completed();
            assert_eq!(retained.len(), usize::from(kind == MediaKind::Image));
            assert!(
                retained
                    .iter()
                    .all(|item| item.path == std::path::Path::new("shared")
                        && item.generation == generation)
            );
            release.send(()).expect("finish cache read");
            let remaining = if kind == MediaKind::Image { 1 } else { 2 };
            for _ in 0..remaining {
                ready
                    .recv_timeout(Duration::from_secs(5))
                    .expect("remaining result");
            }
            let results = loader.take_completed();
            assert_eq!(results.len(), remaining);
            assert!(
                results
                    .iter()
                    .all(|item| item.generation == generation && item.result.is_ok())
            );
            if kind == MediaKind::Video {
                assert_eq!(
                    started_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("changed kind reloaded"),
                    (PathBuf::from("shared"), kind)
                );
            }
            assert!(
                started_rx.try_recv().is_err(),
                "matching results and cache reads are not repeated"
            );
            drop(loader);
            worker.join().expect("worker exits");
        }
    }

    #[test]
    fn removing_then_readding_a_path_does_not_revive_cancelled_work() {
        for cache_stage in [
            PreviewStage::Memory,
            PreviewStage::Disk,
            PreviewStage::Generate,
        ] {
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let loader = PreviewLoader {
                shared: Arc::clone(&shared),
            };
            let (started, started_rx) = mpsc::channel();
            let (release, release_rx) = mpsc::channel();
            let (notify, ready) = mpsc::channel();
            let worker = thread::spawn(move || {
                let count = std::cell::Cell::new(0_u8);
                let work = |cancellation: &Cancellation| {
                    let value = count.get() + 1;
                    count.set(value);
                    if value == 1 {
                        started.send(cancellation.clone()).expect("first work");
                        release_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("release obsolete work");
                        assert!(cancellation.is_cancelled());
                    } else {
                        assert!(!cancellation.is_cancelled());
                    }
                    MediaPreview {
                        image: crate::PreviewImage {
                            width: 1,
                            height: 1,
                            rgba: vec![value, 0, 0, 255].into(),
                        },
                        duration: None,
                    }
                };
                run_worker(
                    shared,
                    || {
                        let _ = notify.send(());
                    },
                    |_, _, cancellation, stage, _| {
                        (stage == cache_stage).then(|| work(cancellation))
                    },
                    |_, _, cancellation| Ok(work(cancellation)),
                );
                assert_eq!(count.get(), 2, "only the new request can publish");
            });
            let mut initial = vec![("same.png".into(), MediaKind::Image)];
            if cache_stage == PreviewStage::Disk {
                initial.push(("never.png".into(), MediaKind::Image));
            }
            loader.request(initial);
            let cancellation = started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("active work");
            loader.request(Vec::new());
            let generation = loader.request(vec![("same.png".into(), MediaKind::Image)]);
            assert!(cancellation.is_cancelled());
            release.send(()).expect("finish obsolete work");
            ready
                .recv_timeout(Duration::from_secs(5))
                .expect("new result");
            let results = loader.take_completed();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].generation, generation);
            assert_eq!(
                results[0]
                    .result
                    .as_ref()
                    .expect("preview")
                    .image
                    .rgba
                    .as_slice(),
                [2, 0, 0, 255]
            );
            drop(loader);
            worker.join().expect("worker exits");
        }
    }

    #[test]
    fn cached_previews_publish_before_slow_generation_without_reordering_misses() {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = PreviewLoader {
            shared: Arc::clone(&shared),
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || ready_tx.send(()).expect("notify"),
                |path, _, _, _, _| {
                    path.starts_with("warm").then(|| MediaPreview {
                        image: crate::PreviewImage {
                            width: 1,
                            height: 1,
                            rgba: vec![1, 2, 3, 255].into(),
                        },
                        duration: None,
                    })
                },
                |path, _, _| {
                    started_tx.send(path.to_owned()).expect("started");
                    if path == std::path::Path::new("slow.png") {
                        release_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("release slow preview");
                    }
                    Err("uncached fixture".into())
                },
            )
        });
        let generation = loader.request(vec![
            ("slow.png".into(), MediaKind::Image),
            ("warm/image.png".into(), MediaKind::Image),
            ("warm/video.mp4".into(), MediaKind::Video),
            ("warm/audio.wav".into(), MediaKind::Audio),
            ("later.png".into(), MediaKind::Image),
        ]);
        assert_eq!(
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("slow start"),
            PathBuf::from("slow.png")
        );
        let cached = loader.take_completed();
        release_tx.send(()).expect("release");
        assert_eq!(
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("later start"),
            PathBuf::from("later.png")
        );
        for _ in 0..5 {
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("all results");
        }
        let generated = loader.take_completed();
        drop(loader);
        worker.join().expect("worker exits");
        assert_eq!(
            cached
                .iter()
                .map(|item| item.path.as_path())
                .collect::<Vec<_>>(),
            ["warm/image.png", "warm/video.mp4", "warm/audio.wav"].map(std::path::Path::new)
        );
        assert!(cached.iter().all(|item| {
            item.generation == generation
                && item
                    .result
                    .as_ref()
                    .is_ok_and(|preview| preview.image.rgba.as_slice() == [1, 2, 3, 255])
        }));
        assert_eq!(
            generated
                .iter()
                .map(|item| item.path.as_path())
                .collect::<Vec<_>>(),
            ["slow.png", "later.png"].map(std::path::Path::new)
        );
        assert!(
            generated
                .iter()
                .all(|item| item.generation == generation && item.result.is_err())
        );
        assert!(
            started_rx.try_recv().is_err(),
            "cache hits are not generated again"
        );
    }

    #[test]
    fn preview_requests_and_results_are_bounded_and_publish_progress() {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = PreviewLoader {
            shared: Arc::clone(&shared),
        };
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || tx.send(()).expect("notify progress"),
                |_, _, _, _, _| None,
                |_, _, _| Err("fixture failure".into()),
            )
        });
        let generation = loader.request(
            (0..1000)
                .map(|i| (PathBuf::from(format!("{}.png", i / 3)), MediaKind::Image))
                .collect(),
        );
        for _ in 0..VISIBLE_PREVIEW_LIMIT {
            rx.recv_timeout(Duration::from_secs(5))
                .expect("progress notification");
        }
        let results = loader.take_completed();
        assert_eq!(results.len(), VISIBLE_PREVIEW_LIMIT);
        assert_eq!(
            results
                .iter()
                .map(|item| &item.path)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            VISIBLE_PREVIEW_LIMIT
        );
        assert!(
            results
                .iter()
                .all(|item| item.generation == generation && item.result.is_err())
        );
        assert!(loader.take_completed().is_empty());
        drop(loader);
        worker.join().expect("worker exits");
    }

    #[test]
    fn scroll_and_close_reject_in_flight_previews_without_waiting() {
        for (close, cache_stage) in [false, true].into_iter().flat_map(|close| {
            [
                PreviewStage::Memory,
                PreviewStage::Disk,
                PreviewStage::Generate,
            ]
            .map(|stage| (close, stage))
        }) {
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let loader = PreviewLoader {
                shared: Arc::clone(&shared),
            };
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::channel();
            let worker = thread::spawn(move || {
                let block = |path: &std::path::Path, cancellation: &Cancellation| {
                    started_tx.send(path.to_owned()).expect("notify start");
                    if path == std::path::Path::new("old.png") {
                        release_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("release generation");
                        assert!(cancellation.is_cancelled());
                    } else {
                        assert!(!cancellation.is_cancelled());
                    }
                };
                run_worker(
                    shared,
                    || ready_tx.send(()).expect("notify result"),
                    |path, _, cancellation, stage, _| {
                        (stage == cache_stage).then(|| {
                            block(path, cancellation);
                            MediaPreview {
                                image: crate::PreviewImage {
                                    width: 1,
                                    height: 1,
                                    rgba: vec![1, 2, 3, 255].into(),
                                },
                                duration: None,
                            }
                        })
                    },
                    |path, _, cancellation| {
                        assert!(
                            cache_stage != PreviewStage::Memory,
                            "memory hit must not be generated"
                        );
                        block(path, cancellation);
                        Err("fixture failure".into())
                    },
                )
            });
            loader.request(vec![
                ("old.png".into(), MediaKind::Image),
                ("never.png".into(), MediaKind::Image),
            ]);
            assert_eq!(
                started_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("old request starts"),
                PathBuf::from("old.png")
            );
            loader.request(vec![("intermediate.png".into(), MediaKind::Image)]);
            let generation = loader.request(vec![("latest.png".into(), MediaKind::Image)]);
            if close {
                drop(loader);
                release_tx.send(()).expect("release closed worker");
                worker.join().expect("closed worker exits");
                assert!(ready_rx.try_recv().is_err());
            } else {
                release_tx.send(()).expect("release stale request");
                ready_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("latest result");
                let results = loader.take_completed();
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].path, PathBuf::from("latest.png"));
                assert_eq!(results[0].generation, generation);
                assert_eq!(
                    started_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("latest request starts"),
                    PathBuf::from("latest.png")
                );
                drop(loader);
                worker.join().expect("worker exits");
            }
            assert!(started_rx.try_recv().is_err());
        }
    }
    #[test]
    fn cache_only_completions_are_typed_preemptible_and_never_reused_as_pixels() {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = PreviewLoader {
            shared: Arc::clone(&shared),
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            super::run_worker(
                shared,
                || {
                    let _ = ready_tx.send(());
                },
                |_, _, _, _, _| None,
                |_, _, _| {
                    Ok(MediaPreview {
                        image: crate::PreviewImage {
                            width: 1,
                            height: 1,
                            rgba: vec![1, 2, 3, 255].into(),
                        },
                        duration: None,
                    })
                },
                |path, _, cancellation| match path.to_str().expect("owned name") {
                    "blocked.png" => {
                        started_tx.send(cancellation.clone()).expect("started");
                        release_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("release");
                        Ok(true)
                    }
                    "full.png" => Ok(false),
                    "broken.png" => Err("bad source".into()),
                    _ => Ok(true),
                },
            )
        });
        loader.request_warming("blocked.png".into(), MediaKind::Image);
        let cancelled = started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("active warming");
        let generation = loader.request(vec![("blocked.png".into(), MediaKind::Image)]);
        assert!(
            cancelled.is_cancelled(),
            "display demand preempts even the same path"
        );
        release_tx.send(()).expect("release old work");
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("display result");
        assert!(
            loader.take_warmed().is_none(),
            "stale warming cannot publish"
        );
        let result = loader.take_completed().pop().expect("pixels");
        assert_eq!(result.generation, generation);
        assert_eq!(
            result.result.expect("ready").image.rgba.as_slice(),
            [1, 2, 3, 255]
        );

        for (path, expected) in [
            ("ready.png", Ok(true)),
            ("full.png", Ok(false)),
            ("broken.png", Err("bad source".to_owned())),
        ] {
            loader.request_warming(path.into(), MediaKind::Image);
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("warming result");
            assert!(
                loader.take_completed().is_empty(),
                "warming never returns UI pixels"
            );
            let generation = loader.request_warming(path.into(), MediaKind::Image);
            let result = loader.take_warmed().expect("reuse unconsumed result");
            assert_eq!(result.generation, generation);
            assert_eq!(result.result, expected);
            assert!(loader.is_idle());
        }
        loader.request_warming("ready.png".into(), MediaKind::Image);
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("completed warming");
        let generation = loader.request(vec![("ready.png".into(), MediaKind::Image)]);
        assert!(
            loader.take_warmed().is_none(),
            "display replacement discards cache-only status"
        );
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("fresh display result");
        assert_eq!(loader.take_completed()[0].generation, generation);
        loader.request_warming("blocked.png".into(), MediaKind::Image);
        let cancelled = started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("active warming");
        drop(loader);
        assert!(cancelled.is_cancelled(), "closing stops speculative work");
        release_tx.send(()).expect("release closing work");
        worker.join().expect("worker exits");
        assert!(ready_rx.try_recv().is_err(), "closed work never notifies");
    }
}
