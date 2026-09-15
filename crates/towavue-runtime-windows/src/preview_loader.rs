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
    stage: PreviewStage,
    disk_key: Option<String>,
}

struct ActivePreview {
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
    closed: bool,
}

pub struct PreviewLoader {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
}

impl PreviewLoader {
    pub fn new(cache: PreviewCache, notify: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("towavue-filmstrip".into())
            .spawn(move || {
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
                );
            })?;
        Ok(Self { shared })
    }

    pub fn request(&self, paths: Vec<(PathBuf, MediaKind)>) -> u64 {
        let mut wanted = Vec::new();
        for item in paths {
            if !wanted.contains(&item) {
                wanted.push(item);
                if wanted.len() == VISIBLE_PREVIEW_LIMIT {
                    break;
                }
            }
        }
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("preview mailbox");
        if let Some(active) = &mailbox.active
            && !wanted
                .iter()
                .any(|(path, kind)| *path == active.path && *kind == active.kind)
        {
            active.cancellation.cancel();
        }
        mailbox.generation = mailbox.generation.wrapping_add(1);
        let generation = mailbox.generation;
        mailbox.completed.retain_mut(|(kind, preview)| {
            preview.generation = generation;
            wanted
                .iter()
                .any(|(path, wanted_kind)| *path == preview.path && wanted_kind == kind)
        });
        mailbox.pending = wanted
            .into_iter()
            .filter(|(path, kind)| {
                !mailbox
                    .completed
                    .iter()
                    .any(|(ready_kind, preview)| ready_kind == kind && preview.path == *path)
            })
            .map(|(path, kind)| PendingPreview {
                path,
                kind,
                // Another preview consumer may have populated the shared cache since our miss.
                stage: PreviewStage::Memory,
                disk_key: None,
            })
            .collect();
        ready.notify_one();
        generation
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
            // Memory hits precede disk images, then generation; retain request order per stage.
            let index = mailbox
                .pending
                .iter()
                .enumerate()
                .min_by_key(|(_, work)| work.stage)
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
                path: work.path.clone(),
                kind: work.kind,
                cancellation: cancellation.clone(),
            });
            (work, cancellation)
        };
        let result = if work.stage != PreviewStage::Generate {
            cached(
                &work.path,
                work.kind,
                &cancellation,
                work.stage,
                &mut work.disk_key,
            )
            .map(Ok)
        } else {
            Some(generate(&work.path, work.kind, &cancellation))
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
                mailbox.completed.push((
                    work.kind,
                    LoadedPreview {
                        generation,
                        path: work.path,
                        result,
                    },
                ));
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
}
