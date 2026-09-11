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

#[derive(Default)]
struct Mailbox {
    generation: u64,
    pending: Option<Vec<(PathBuf, MediaKind)>>,
    completed: Vec<LoadedPreview>,
    closed: bool,
    cancellation: Cancellation,
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
                run_worker(
                    worker_shared,
                    notify,
                    |path, kind, cancellation| {
                        cache
                            .cancellable(cancellation.clone())
                            .cached_filmstrip(path, kind)
                            .ok()
                            .flatten()
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
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("preview mailbox");
        mailbox.cancellation.cancel();
        mailbox.cancellation = Cancellation::default();
        mailbox.generation = mailbox.generation.wrapping_add(1);
        mailbox.pending =
            (!paths.is_empty()).then(|| paths.into_iter().take(VISIBLE_PREVIEW_LIMIT).collect());
        mailbox.completed.clear();
        ready.notify_one();
        mailbox.generation
    }

    pub fn take_completed(&self) -> Vec<LoadedPreview> {
        std::mem::take(&mut self.shared.0.lock().expect("preview mailbox").completed)
    }
}

impl Drop for PreviewLoader {
    fn drop(&mut self) {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("preview mailbox");
        mailbox.closed = true;
        mailbox.cancellation.cancel();
        mailbox.pending = None;
        mailbox.completed.clear();
        ready.notify_one();
        // Cancel the owned child without joining work that may still be inside native probing.
    }
}

fn run_worker(
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    notify: impl Fn(),
    cached: impl Fn(&std::path::Path, MediaKind, &Cancellation) -> Option<MediaPreview>,
    generate: impl Fn(&std::path::Path, MediaKind, &Cancellation) -> Result<MediaPreview, String>,
) {
    let (mutex, ready) = &*shared;
    loop {
        let (generation, paths, cancellation) = {
            let mut mailbox = ready
                .wait_while(mutex.lock().expect("preview mailbox"), |mailbox| {
                    !mailbox.closed && mailbox.pending.is_none()
                })
                .expect("preview mailbox");
            if mailbox.closed {
                return;
            }
            (
                mailbox.generation,
                mailbox.pending.take().expect("pending previews"),
                mailbox.cancellation.clone(),
            )
        };
        let publish = |path, result| {
            let publish = {
                let mut mailbox = mutex.lock().expect("preview mailbox");
                if mailbox.closed || mailbox.generation != generation {
                    false
                } else {
                    mailbox.completed.push(LoadedPreview {
                        generation,
                        path,
                        result,
                    });
                    true
                }
            };
            if publish {
                notify();
            }
        };
        let mut missing = Vec::new();
        for (path, kind) in paths {
            if cancellation.is_cancelled() {
                break;
            }
            if let Some(preview) = cached(&path, kind, &cancellation) {
                publish(path, Ok(preview));
            } else {
                missing.push((path, kind));
            }
        }
        for (path, kind) in missing {
            if cancellation.is_cancelled() {
                break;
            }
            let result = generate(&path, kind, &cancellation);
            publish(path, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

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
                |path, _, _| {
                    path.starts_with("warm").then(|| MediaPreview {
                        image: crate::PreviewImage {
                            width: 1,
                            height: 1,
                            rgba: vec![1, 2, 3, 255],
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
                    .is_ok_and(|preview| preview.image.rgba == [1, 2, 3, 255])
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
                |_, _, _| None,
                |_, _, _| Err("fixture failure".into()),
            )
        });
        let generation = loader.request(
            (0..1000)
                .map(|i| (PathBuf::from(format!("{i}.png")), MediaKind::Image))
                .collect(),
        );
        for _ in 0..VISIBLE_PREVIEW_LIMIT {
            rx.recv_timeout(Duration::from_secs(5))
                .expect("progress notification");
        }
        let results = loader.take_completed();
        assert_eq!(results.len(), VISIBLE_PREVIEW_LIMIT);
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
        for (close, cache_hit) in [(false, false), (true, false), (false, true), (true, true)] {
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
                    |path, _, cancellation| {
                        cache_hit.then(|| {
                            block(path, cancellation);
                            MediaPreview {
                                image: crate::PreviewImage {
                                    width: 1,
                                    height: 1,
                                    rgba: vec![1, 2, 3, 255],
                                },
                                duration: None,
                            }
                        })
                    },
                    |path, _, cancellation| {
                        assert!(!cache_hit, "cached result must not be generated");
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
