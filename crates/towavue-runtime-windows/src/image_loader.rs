use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::SystemTime;

use crate::image::{IMAGE_BYTE_LIMIT, decode_image_cancellable, decode_image_for_prefetch};
use crate::{DecodedImage, ImageDecodeError, LatestTask};

const CACHE_BYTE_LIMIT: usize = 256 * 1024 * 1024;

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

impl Default for ImageCache {
    fn default() -> Self {
        Self::new(CACHE_BYTE_LIMIT)
    }
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
        if let Some(index) = self.entries.iter().position(|entry| entry.0 == path) {
            let (_, _, removed) = self.entries.remove(index).expect("existing entry");
            self.bytes -= removed.retained_bytes();
        }
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
    cache: Arc<Mutex<ImageCache>>,
}

/// Latest-only foreground decoder with independent, bounded speculative decoding.
pub struct ImageLoader {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    prefetch_worker: LatestTask,
}

impl ImageLoader {
    pub fn new(notify: impl Fn() + Send + 'static) -> Result<Self, std::io::Error> {
        let prefetch_worker = LatestTask::new("towavue-image-prefetch")?;
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("towavue-images".into())
            .spawn(move || {
                run_worker(worker_shared, notify, |path, budget, current| {
                    decode_image_cancellable(path, budget, current)
                });
            })?;
        Ok(Self {
            shared,
            prefetch_worker,
        })
    }

    pub fn request(&self, paths: Vec<PathBuf>) -> u64 {
        self.prefetch_worker.clear();
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("image mailbox");
        mailbox.generation = mailbox.generation.wrapping_add(1);
        mailbox.pending = (!paths.is_empty()).then_some(paths);
        mailbox.completed = None;
        ready.notify_one();
        mailbox.generation
    }

    pub fn prefetch(&self, path: PathBuf) {
        self.prefetch_with_decode(path, |path, budget, current| {
            decode_image_for_prefetch(path, budget, current)
        });
    }

    fn prefetch_with_decode(
        &self,
        path: PathBuf,
        decode: impl FnOnce(
            &Path,
            usize,
            &dyn Fn() -> bool,
        ) -> Result<Option<DecodedImage>, ImageDecodeError>
        + Send
        + 'static,
    ) {
        let shared = Arc::clone(&self.shared);
        let (generation, cache) = {
            let mailbox = shared.0.lock().expect("image mailbox");
            (mailbox.generation, Arc::clone(&mailbox.cache))
        };
        self.prefetch_worker.submit(move |cancellation| {
            let Some(stamp) = ImageStamp::read(&path) else {
                return;
            };
            {
                let mailbox = shared.0.lock().expect("image mailbox");
                if mailbox.closed || mailbox.generation != generation || cancellation.is_cancelled()
                {
                    return;
                }
            }
            if cache
                .lock()
                .expect("image cache")
                .entries
                .iter()
                .any(|entry| entry.0 == path && entry.1 == stamp)
            {
                return;
            }
            let current = || !cancellation.is_cancelled();
            let Ok(Some(image)) = decode(&path, CACHE_BYTE_LIMIT, &current) else {
                return;
            };
            if image.is_animated() || ImageStamp::read(&path) != Some(stamp) {
                return;
            }
            let mut cache = cache.lock().expect("image cache");
            if current() {
                cache.insert(path, stamp, Arc::new(image));
            }
        });
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
    // Cache eviction may free large pixel allocations; never hold the UI mailbox lock there.
    let cache = Arc::clone(&mutex.lock().expect("image mailbox").cache);
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
            let cached = cache
                .lock()
                .expect("image cache")
                .take(&path, stamp, remaining);
            let result =
                cached.unwrap_or_else(|| decode(&path, remaining, &is_current).map(Arc::new));
            if let Ok(image) = &result {
                remaining -= image.retained_bytes();
                if is_current()
                    && let Some(stamp) = stamp
                    && ImageStamp::read(&path) == Some(stamp)
                {
                    cache.lock().expect("image cache").insert(
                        path.clone(),
                        stamp,
                        Arc::clone(image),
                    );
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
    fn prefetch_does_not_block_foreground_and_reuses_only_current_results() {
        let root = std::env::temp_dir().join(format!("towavue-prefetch-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("fixture directory");
        let first = root.join("first.png");
        let second = root.join("second.png");
        std::fs::write(&first, [1]).expect("first fixture");
        std::fs::write(&second, [2]).expect("second fixture");
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: shared.clone(),
            prefetch_worker: LatestTask::new("prefetch-priority-test").expect("prefetch worker"),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let (decoded_tx, decoded_rx) = mpsc::channel();
        let foreground = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    let _ = ready_tx.send(());
                },
                |_, _, _| {
                    decoded_tx.send(()).expect("foreground decode");
                    Ok((*pixel(9)).clone())
                },
            )
        });
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (cancelled_tx, cancelled_rx) = mpsc::channel();
        loader.prefetch_with_decode(first.clone(), move |_, budget, current| {
            assert_eq!(budget, CACHE_BYTE_LIMIT);
            started_tx.send(()).expect("prefetch started");
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("release slow codec");
            cancelled_tx
                .send(!current())
                .expect("cancellation observed");
            Ok(Some((*pixel(1)).clone()))
        });
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("background started");
        let cache = loader.shared.0.lock().expect("mailbox").cache.clone();
        let generation = thread::scope(|scope| {
            let held = cache
                .lock()
                .expect("hold cache as an expensive eviction would");
            let (sent, received) = mpsc::channel();
            let loader = &loader;
            let path = second.clone();
            scope.spawn(move || sent.send(loader.request(vec![path])).expect("request"));
            let result = received.recv_timeout(Duration::from_secs(5));
            drop(held);
            result.expect("UI request must not wait for the cache lock")
        });
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("foreground must not wait for background codec");
        let completed = loader.take_completed().expect("foreground result");
        assert_eq!(completed.generation, generation);
        assert_eq!(completed.images[0].0, second);
        decoded_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first foreground decode");
        release_tx.send(()).expect("finish cancelled prefetch");
        assert!(
            cancelled_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("cancelled")
        );
        loader.prefetch_with_decode(first.clone(), |_, _, _| Ok(Some((*pixel(2)).clone())));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let prefetched = loop {
            if let Some(image) = loader
                .shared
                .0
                .lock()
                .expect("mailbox")
                .cache
                .lock()
                .expect("image cache")
                .entries
                .iter()
                .find(|entry| entry.0 == first)
                .map(|entry| entry.2.clone())
            {
                break image;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "prefetch did not populate the cache"
            );
            thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(
            prefetched.frames[0].rgba,
            vec![2; 4],
            "cancelled pixels cannot replace the new prefetch"
        );
        assert!(
            loader.take_completed().is_none(),
            "prefetch never publishes a visible image"
        );
        loader.request(vec![first.clone()]);
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("cached foreground");
        let hit = loader
            .take_completed()
            .expect("cache result")
            .images
            .remove(0)
            .1
            .expect("cached image");
        assert!(Arc::ptr_eq(&hit, &prefetched));
        assert!(decoded_rx.try_recv().is_err());
        std::fs::write(&first, [3, 4]).expect("changed prefetched file");
        loader.request(vec![first.clone()]);
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("changed foreground");
        decoded_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("changed file must decode again");
        let changed = loader
            .take_completed()
            .expect("changed result")
            .images
            .remove(0)
            .1
            .expect("new image");
        assert!(!Arc::ptr_eq(&changed, &prefetched));
        drop(loader);
        foreground.join().expect("foreground exits");
        std::fs::remove_file(first).expect("remove owned fixture");
        std::fs::remove_file(second).expect("remove owned fixture");
        std::fs::remove_dir(root).expect("remove empty owned fixture directory");
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
            prefetch_worker: LatestTask::new("image-cache-test-prefetch").expect("prefetch worker"),
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
            prefetch_worker: LatestTask::new("image-limit-test-prefetch").expect("prefetch worker"),
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
            prefetch_worker: LatestTask::new("image-cancel-test-prefetch")
                .expect("prefetch worker"),
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
