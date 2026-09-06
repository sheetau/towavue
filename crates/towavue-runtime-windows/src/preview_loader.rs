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
                run_worker(worker_shared, notify, |path, kind, cancellation| {
                    cache
                        .cancellable(cancellation.clone())
                        .filmstrip(path, kind)
                        .map_err(|error| error.to_string())
                });
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
        for (path, kind) in paths {
            {
                let mailbox = mutex.lock().expect("preview mailbox");
                if mailbox.closed || mailbox.generation != generation {
                    break;
                }
            }
            let result = generate(&path, kind, &cancellation);
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
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

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
        for close in [false, true] {
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
                    || ready_tx.send(()).expect("notify result"),
                    |path, _, cancellation| {
                        started_tx.send(path.to_owned()).expect("notify start");
                        if path == std::path::Path::new("old.png") {
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .expect("release generation");
                            assert!(cancellation.is_cancelled());
                        } else {
                            assert!(!cancellation.is_cancelled());
                        }
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
