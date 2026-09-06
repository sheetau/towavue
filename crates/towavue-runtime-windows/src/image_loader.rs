use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::SystemTime;

use crate::image::{IMAGE_BYTE_LIMIT, decode_image_cancellable};
use crate::{DecodedImage, ImageDecodeError};

pub struct LoadedImages {
    pub generation: u64,
    pub images: Vec<(PathBuf, Result<Arc<DecodedImage>, ImageDecodeError>)>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct ImageStamp {
    bytes: u64,
    modified: SystemTime,
}

impl ImageStamp {
    fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            bytes: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    }
}

struct ImageCache {
    entries: VecDeque<(PathBuf, ImageStamp, Arc<DecodedImage>)>,
    byte_limit: usize,
    bytes: usize,
}

impl ImageCache {
    fn new(byte_limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            byte_limit,
            bytes: 0,
        }
    }

    fn take(
        &mut self,
        path: &Path,
        stamp: Option<ImageStamp>,
        remaining: usize,
    ) -> Option<Result<Arc<DecodedImage>, ImageDecodeError>> {
        let index = self.entries.iter().position(|entry| entry.0 == path)?;
        let (_, cached_stamp, image) = self.entries.remove(index)?;
        self.bytes -= image.retained_bytes();
        (Some(cached_stamp) == stamp).then(|| {
            if image.retained_bytes() <= remaining {
                Ok(image)
            } else {
                Err(ImageDecodeError::TooLarge)
            }
        })
    }

    fn insert(&mut self, path: PathBuf, stamp: ImageStamp, image: Arc<DecodedImage>) {
        let bytes = image.retained_bytes();
        if image.is_animated() || bytes > self.byte_limit {
            return;
        }
        while self.entries.len() >= 8 || self.bytes + bytes > self.byte_limit {
            let (_, _, removed) = self.entries.pop_front().expect("cached image to evict");
            self.bytes -= removed.retained_bytes();
        }
        self.bytes += bytes;
        self.entries.push_back((path, stamp, image));
    }
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
    let mut cache = ImageCache::new(256 * 1024 * 1024);
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
            let stamp = ImageStamp::read(&path);
            let result = cache
                .take(&path, stamp, remaining)
                .unwrap_or_else(|| decode(&path, remaining, &is_current).map(Arc::new));
            if let Ok(image) = &result {
                remaining -= image.retained_bytes();
                if is_current()
                    && let Some(stamp) = stamp
                    && ImageStamp::read(&path) == Some(stamp)
                {
                    cache.insert(path.clone(), stamp, Arc::clone(image));
                }
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

    fn pixel(value: u8) -> Arc<DecodedImage> {
        Arc::new(DecodedImage {
            format: "PNG",
            frames: vec![crate::DecodedImageFrame {
                width: 1,
                height: 1,
                rgba: vec![value; 4],
                delay: Duration::ZERO,
            }],
        })
    }

    #[test]
    fn decoded_cache_bounds_shared_pixels_and_invalidates_file_stamps() {
        let stamp = ImageStamp {
            bytes: 1,
            modified: SystemTime::UNIX_EPOCH,
        };
        let mut cache = ImageCache::new(8);
        let first = pixel(1);
        cache.insert("first".into(), stamp, Arc::clone(&first));
        cache.insert("second".into(), stamp, pixel(2));
        let hit = cache
            .take(Path::new("first"), Some(stamp), 8)
            .expect("cache hit")
            .expect("within request budget");
        assert!(Arc::ptr_eq(&hit, &first), "cache must not copy RGBA");
        cache.insert("first".into(), stamp, hit);
        cache.insert("third".into(), stamp, pixel(3));
        assert!(cache.take(Path::new("second"), Some(stamp), 8).is_none());
        assert_eq!(cache.bytes, 8);
        assert!(
            cache
                .take(
                    Path::new("first"),
                    Some(ImageStamp {
                        modified: stamp.modified + Duration::from_secs(1),
                        ..stamp
                    }),
                    8
                )
                .is_none()
        );
        assert_eq!(cache.bytes, 4);
        assert_eq!(
            first.frames[0].rgba,
            vec![1; 4],
            "eviction must not invalidate the UI owner"
        );
        assert!(cache.take(Path::new("third"), None, 8).is_none());
        assert_eq!(cache.bytes, 0);
        let mut animated = (*pixel(9)).clone();
        animated.frames.push(animated.frames[0].clone());
        cache.insert("animation".into(), stamp, Arc::new(animated));
        assert!(cache.entries.is_empty());
        let mut too_large = (*pixel(9)).clone();
        too_large.frames[0].rgba = vec![9; 12];
        cache.insert("large".into(), stamp, Arc::new(too_large));
        assert!(cache.entries.is_empty());
        let mut cache = ImageCache::new(100);
        for index in 0..9 {
            cache.insert(index.to_string().into(), stamp, pixel(index));
        }
        assert_eq!(cache.entries.len(), 8);
        assert_eq!(cache.bytes, 32);
        assert!(cache.take(Path::new("0"), Some(stamp), 100).is_none());
        assert!(matches!(
            cache.take(Path::new("8"), Some(stamp), 3),
            Some(Err(ImageDecodeError::TooLarge))
        ));
        assert_eq!(cache.bytes, 28);
        let retained = cache.entries[0].2.clone();
        assert_eq!(Arc::strong_count(&retained), 2);
        drop(cache);
        assert_eq!(Arc::strong_count(&retained), 1);
    }

    #[test]
    fn worker_reuses_pixels_but_redecodes_changed_and_missing_files() {
        let root = std::env::temp_dir().join(format!("towavue-image-cache-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("cache fixture directory");
        let path = root.join("image.png");
        std::fs::write(&path, [1]).expect("initial fixture");
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let (decoded_tx, decoded_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    ready_tx.send(()).expect("notify");
                },
                |path, _, _| {
                    decoded_tx.send(()).expect("count decode");
                    let bytes = std::fs::read(path).map_err(ImageDecodeError::Open)?;
                    Ok((*pixel(bytes[0])).clone())
                },
            )
        });
        let load = || {
            let generation = loader.request(vec![path.clone()]);
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("image ready");
            let completed = loader.take_completed().expect("completed");
            assert_eq!(completed.generation, generation);
            completed.images.into_iter().next().expect("image").1
        };
        let first = load().expect("first image");
        decoded_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first decode");
        loader.request(Vec::new());
        let second = load().expect("cached image");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(decoded_rx.try_recv().is_err());
        std::fs::write(&path, [2, 3]).expect("changed source size");
        let changed = load().expect("changed image");
        decoded_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("changed decode");
        assert_eq!(changed.frames[0].rgba, vec![2; 4]);
        assert!(!Arc::ptr_eq(&first, &changed));
        std::fs::remove_file(&path).expect("remove owned source");
        assert!(matches!(load(), Err(ImageDecodeError::Open(_))));
        decoded_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("missing decode");
        drop(loader);
        worker.join().expect("cache worker exits");
        assert_eq!(Arc::strong_count(&changed), 1);
        std::fs::remove_dir(root).expect("remove empty owned directory");
    }

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
