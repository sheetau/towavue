use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::image::{IMAGE_BYTE_LIMIT, decode_image_cancellable};
use crate::{DecodedImage, ImageDecodeError};

pub struct LoadedImages {
    pub generation: u64,
    pub images: Vec<(PathBuf, Result<DecodedImage, ImageDecodeError>)>,
}

#[derive(Default)]
struct Mailbox {
    generation: u64,
    pending: Option<Vec<PathBuf>>,
    completed: Option<LoadedImages>,
    closed: bool,
}

/// One decoder with latest-only request and result slots, independent of window/GPU lifetime.
pub struct ImageLoader {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
}

impl ImageLoader {
    pub fn new(notify: impl Fn() + Send + 'static) -> Result<Self, std::io::Error> {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("towavue-images".into())
            .spawn(move || {
                run_worker(worker_shared, notify, |path, budget, current| {
                    decode_image_cancellable(path, budget, current)
                });
            })?;
        Ok(Self { shared })
    }

    pub fn request(&self, paths: Vec<PathBuf>) -> u64 {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("image mailbox");
        mailbox.generation = mailbox.generation.wrapping_add(1);
        mailbox.pending = (!paths.is_empty()).then_some(paths);
        mailbox.completed = None;
        ready.notify_one();
        mailbox.generation
    }

    pub fn take_completed(&self) -> Option<LoadedImages> {
        self.shared
            .0
            .lock()
            .expect("image mailbox")
            .completed
            .take()
    }
}

impl Drop for ImageLoader {
    fn drop(&mut self) {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("image mailbox");
        mailbox.closed = true;
        mailbox.pending = None;
        mailbox.completed = None;
        ready.notify_one();
        // A codec may still be finishing one frame. Only the worker owns its decoder;
        // cancellation prevents publication without blocking window close on that frame.
    }
}

fn run_worker(
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    notify: impl Fn(),
    decode: impl Fn(
        &std::path::Path,
        usize,
        &dyn Fn() -> bool,
    ) -> Result<DecodedImage, ImageDecodeError>,
) {
    let (mutex, ready) = &*shared;
    loop {
        let (generation, paths) = {
            let mut mailbox = ready
                .wait_while(mutex.lock().expect("image mailbox"), |mailbox| {
                    !mailbox.closed && mailbox.pending.is_none()
                })
                .expect("image mailbox");
            if mailbox.closed {
                return;
            }
            (
                mailbox.generation,
                mailbox.pending.take().expect("pending image request"),
            )
        };
        let is_current = || {
            let mailbox = mutex.lock().expect("image mailbox");
            !mailbox.closed && mailbox.generation == generation
        };
        let mut remaining = IMAGE_BYTE_LIMIT;
        let mut images = Vec::new();
        for path in paths {
            if !is_current() {
                break;
            }
            let result = decode(&path, remaining, &is_current);
            if let Ok(image) = &result {
                remaining -= image.retained_bytes();
            }
            images.push((path, result));
        }
        let publish = {
            let mut mailbox = mutex.lock().expect("image mailbox");
            if mailbox.closed {
                return;
            }
            if mailbox.generation != generation {
                false
            } else {
                mailbox.completed = Some(LoadedImages { generation, images });
                true
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
    fn pages_share_a_budget_and_clearing_a_request_discards_queued_results() {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    ready_tx.send(()).expect("notification");
                },
                |path, budget, _| {
                    if path == std::path::Path::new("broken.png") {
                        assert_eq!(budget, IMAGE_BYTE_LIMIT - 4);
                        return Err(ImageDecodeError::UnknownFormat);
                    }
                    assert_eq!(budget, IMAGE_BYTE_LIMIT);
                    Ok(DecodedImage {
                        format: "PNG",
                        frames: vec![crate::DecodedImageFrame {
                            width: 1,
                            height: 1,
                            rgba: vec![255; 4],
                            delay: Duration::ZERO,
                        }],
                    })
                },
            )
        });
        loader.request(vec!["first.png".into(), "broken.png".into()]);
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("result");
        let result = loader.take_completed().expect("pages");
        assert!(result.images[0].1.is_ok());
        assert!(matches!(
            result.images[1].1,
            Err(ImageDecodeError::UnknownFormat)
        ));
        loader.request(vec!["first.png".into()]);
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("result");
        loader.request(Vec::new());
        assert!(loader.take_completed().is_none());
        drop(loader);
        worker.join().expect("worker exits on close");
    }

    #[test]
    fn newer_request_replaces_pending_work_and_rejects_in_flight_results() {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    ready_tx.send(()).expect("notification");
                },
                |path, _, current| {
                    started_tx.send(path.to_owned()).expect("started receiver");
                    if path == std::path::Path::new("first.png") {
                        resume_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("release first decode");
                        assert!(!current());
                    }
                    Ok(DecodedImage {
                        format: "PNG",
                        frames: Vec::new(),
                    })
                },
            )
        });
        loader.request(vec!["first.png".into(), "obsolete-page.png".into()]);
        assert_eq!(
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("first decode"),
            PathBuf::from("first.png")
        );
        loader.request(vec!["second.png".into()]);
        let generation = loader.request(vec!["latest.png".into()]);
        resume_tx.send(()).expect("resume worker");
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("latest result");
        assert_eq!(
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("next decode"),
            PathBuf::from("latest.png")
        );
        let result = loader.take_completed().expect("result");
        assert_eq!(result.generation, generation);
        assert_eq!(result.images[0].0, PathBuf::from("latest.png"));
        assert!(started_rx.try_recv().is_err());
        assert!(ready_rx.try_recv().is_err());
        drop(loader);
        worker.join().expect("worker exits on close");
    }
}
