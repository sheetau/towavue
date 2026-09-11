use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::SystemTime;

use crate::image::{
    IMAGE_BYTE_LIMIT, ImagePreviewCallback, decode_image_for_prefetch, decode_image_with_preview,
};
use crate::{CachedImagePreview, DecodedImage, ImageDecodeError, LatestTask, PreviewCache};

const CACHE_BYTE_LIMIT: usize = 256 * 1024 * 1024;
const CACHE_ENTRY_LIMIT: usize = 10;

pub struct LoadedImages {
    pub generation: u64,
    pub first_index: usize,
    pub total: usize,
    pub images: Vec<(PathBuf, Result<Arc<DecodedImage>, ImageDecodeError>)>,
}

pub struct LoadedImagePreview {
    pub generation: u64,
    pub path: PathBuf,
    pub preview: CachedImagePreview,
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
        while self.entries.len() >= CACHE_ENTRY_LIMIT || self.bytes + bytes > self.byte_limit {
            let (_, _, removed) = self.entries.pop_front().expect("cached image to evict");
            self.bytes -= removed.retained_bytes();
        }
        self.bytes += bytes;
        self.entries.push_back((path, stamp, image));
    }
}

struct PrefetchWork {
    id: Arc<()>,
    path: PathBuf,
    generation: u64,
    decoding: bool,
    cancellation: crate::Cancellation,
}

struct PrefetchLease {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    id: Arc<()>,
}

impl Drop for PrefetchLease {
    fn drop(&mut self) {
        let mut mailbox = self.shared.0.lock().expect("image mailbox");
        if mailbox
            .prefetch
            .as_ref()
            .is_some_and(|work| Arc::ptr_eq(&work.id, &self.id))
        {
            mailbox.prefetch = None;
        }
        self.shared.1.notify_all();
    }
}

#[derive(Default)]
struct Mailbox {
    generation: u64,
    pending: Option<(Vec<PathBuf>, usize)>,
    completed: Option<LoadedImages>,
    preview_ready: Option<LoadedImagePreview>,
    closed: bool,
    cache: Arc<Mutex<ImageCache>>,
    previews: Option<PreviewCache>,
    prefetch: Option<PrefetchWork>,
}

/// Latest-only foreground decoder with independent, bounded speculative decoding.
pub struct ImageLoader {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    prefetch_worker: LatestTask,
}

impl ImageLoader {
    pub fn new(
        previews: PreviewCache,
        notify: impl Fn() + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        let prefetch_worker = LatestTask::new("towavue-image-prefetch")?;
        let shared = Arc::new((
            Mutex::new(Mailbox {
                previews: Some(previews),
                ..Default::default()
            }),
            Condvar::new(),
        ));
        let worker_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("towavue-images".into())
            .spawn(move || {
                run_worker(worker_shared, notify, |path, budget, current, preview| {
                    decode_image_with_preview(path, budget, current, preview)
                });
            })?;
        Ok(Self {
            shared,
            prefetch_worker,
        })
    }

    pub fn request(&self, paths: Vec<PathBuf>) -> u64 {
        self.request_with_retained_bytes(paths, 0)
    }

    /// Decode missing pages within the same budget as the caller's already retained pages.
    pub fn request_with_retained_bytes(&self, paths: Vec<PathBuf>, retained_bytes: usize) -> u64 {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("image mailbox");
        mailbox.generation = mailbox.generation.wrapping_add(1);
        let generation = mailbox.generation;
        if let Some(work) = &mut mailbox.prefetch {
            if paths.contains(&work.path) && work.decoding {
                work.generation = generation;
            } else {
                // Invalidate the job without dropping a queued lease under this lock.
                work.cancellation.cancel();
                mailbox.prefetch = None;
            }
        }
        mailbox.pending =
            (!paths.is_empty()).then_some((paths, IMAGE_BYTE_LIMIT.saturating_sub(retained_bytes)));
        mailbox.completed = None;
        mailbox.preview_ready = None;
        ready.notify_all();
        mailbox.generation
    }

    pub fn prefetch(&self, path: PathBuf) {
        self.prefetch_paths(vec![path]);
    }

    /// Prepare a bounded group in priority order on the existing speculative worker.
    pub fn prefetch_paths(&self, paths: Vec<PathBuf>) {
        self.prefetch_with_decode(paths, |path, budget, current| {
            decode_image_for_prefetch(path, budget, current)
        });
    }

    fn prefetch_with_decode(
        &self,
        paths: Vec<PathBuf>,
        mut decode: impl FnMut(
            &Path,
            usize,
            &dyn Fn() -> bool,
        ) -> Result<Option<DecodedImage>, ImageDecodeError>
        + Send
        + 'static,
    ) {
        let mut unique = Vec::new();
        for path in paths.into_iter().take(CACHE_ENTRY_LIMIT) {
            if !unique.contains(&path) {
                unique.push(path);
            }
        }
        let shared = Arc::clone(&self.shared);
        let id = Arc::new(());
        let work_cancellation = crate::Cancellation::default();
        let (generation, cache, previews) = {
            let mut mailbox = shared.0.lock().expect("image mailbox");
            if let Some(work) = &mailbox.prefetch {
                work.cancellation.cancel();
            }
            let Some(path) = unique.first() else {
                mailbox.prefetch = None;
                shared.1.notify_all();
                drop(mailbox);
                self.prefetch_worker.clear();
                return;
            };
            mailbox.prefetch = Some(PrefetchWork {
                id: Arc::clone(&id),
                path: path.clone(),
                generation: mailbox.generation,
                decoding: false,
                cancellation: work_cancellation.clone(),
            });
            shared.1.notify_all();
            (
                mailbox.generation,
                Arc::clone(&mailbox.cache),
                mailbox.previews.clone(),
            )
        };
        let lease = PrefetchLease {
            shared: Arc::clone(&shared),
            id: Arc::clone(&id),
        };
        self.prefetch_worker.submit(move |cancellation| {
            let _lease = lease;
            let mut remaining = cache.lock().expect("image cache").byte_limit;
            for path in unique {
                {
                    let mut mailbox = shared.0.lock().expect("image mailbox");
                    // An adopted job finishes its current page, not the obsolete batch tail.
                    if mailbox.generation != generation || remaining == 0 {
                        break;
                    }
                    if let Some(work) = &mut mailbox.prefetch
                        && Arc::ptr_eq(&work.id, &id)
                    {
                        work.path = path.clone();
                        work.decoding = true;
                    }
                    shared.1.notify_all();
                }
                let current = || {
                    let mailbox = shared.0.lock().expect("image mailbox");
                    !mailbox.closed
                        && !cancellation.is_cancelled()
                        && !work_cancellation.is_cancelled()
                        && mailbox.prefetch.as_ref().is_some_and(|work| {
                            Arc::ptr_eq(&work.id, &id) && work.generation == mailbox.generation
                        })
                };
                if !current() {
                    return;
                }
                let Some(stamp) = ImageStamp::read(&path) else {
                    continue;
                };
                let cached = cache
                    .lock()
                    .expect("image cache")
                    .entries
                    .iter()
                    .find(|entry| entry.0 == path && entry.1 == stamp)
                    .map(|entry| Arc::clone(&entry.2));
                let image = if let Some(image) = cached {
                    image
                } else {
                    let Ok(Some(image)) = decode(&path, remaining, &current) else {
                        continue;
                    };
                    if image.is_animated() || ImageStamp::read(&path) != Some(stamp) || !current() {
                        continue;
                    }
                    Arc::new(image)
                };
                if image.retained_bytes() > remaining || !current() {
                    continue;
                }
                remaining -= image.retained_bytes();
                {
                    let mut cache = cache.lock().expect("image cache");
                    if cancellation.is_cancelled() || work_cancellation.is_cancelled() {
                        return;
                    }
                    cache.insert(path.clone(), stamp, Arc::clone(&image));
                }
                {
                    let mut mailbox = shared.0.lock().expect("image mailbox");
                    if let Some(work) = &mut mailbox.prefetch
                        && Arc::ptr_eq(&work.id, &id)
                    {
                        // The original may proceed before optional thumbnail generation.
                        work.decoding = false;
                    }
                    shared.1.notify_all();
                }
                if let Some(previews) = &previews {
                    previews.remember_image(&path, &image, &|| {
                        current() && ImageStamp::read(&path) == Some(stamp)
                    });
                }
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

    pub fn take_preview(&self) -> Option<LoadedImagePreview> {
        self.shared
            .0
            .lock()
            .expect("image mailbox")
            .preview_ready
            .take()
    }
}

impl Drop for ImageLoader {
    fn drop(&mut self) {
        self.prefetch_worker.clear();
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("image mailbox");
        mailbox.closed = true;
        mailbox.pending = None;
        mailbox.completed = None;
        mailbox.preview_ready = None;
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
        &mut ImagePreviewCallback<'_>,
    ) -> Result<DecodedImage, ImageDecodeError>,
) {
    let (mutex, ready) = &*shared;
    // Cache eviction may free large pixel allocations; never hold the UI mailbox lock there.
    let cache = Arc::clone(&mutex.lock().expect("image mailbox").cache);
    let previews = mutex.lock().expect("image mailbox").previews.clone();
    loop {
        let (generation, (paths, mut remaining)) = {
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
        let total = paths.len();
        for (index, path) in paths.into_iter().enumerate() {
            if !is_current() {
                break;
            }
            let stamp = ImageStamp::read(&path);
            let cached = cache
                .lock()
                .expect("image cache")
                .take(&path, stamp, remaining);
            let preview_current =
                || is_current() && stamp.is_some() && ImageStamp::read(&path) == stamp;
            let publish_preview = |preview| {
                if !preview_current() {
                    return;
                }
                {
                    let mut mailbox = mutex.lock().expect("image mailbox");
                    if mailbox.closed || mailbox.generation != generation {
                        return;
                    }
                    // At most one in-progress image owns preview pixels; completed pages
                    // use their originals even if the UI has not consumed the wakeup yet.
                    mailbox.preview_ready = Some(LoadedImagePreview {
                        generation,
                        path: path.clone(),
                        preview,
                    });
                }
                notify();
            };
            if cached.is_none()
                && let Some(previews) = &previews
                && let Some(preview) =
                    previews.prepare_image_preview(&path, remaining, &preview_current)
            {
                publish_preview(preview);
            }
            let mut first_frame = |width, height, pixels: &[u8]| {
                let Some(previews) = &previews else { return };
                previews.remember_pixels(&path, width, height, pixels, &preview_current);
                if let Ok(Some(preview)) = previews.cached_image(&path) {
                    publish_preview(preview);
                }
            };
            let cached = cached.or_else(|| {
                let mailbox = ready
                    .wait_while(mutex.lock().expect("image mailbox"), |mailbox| {
                        !mailbox.closed
                            && mailbox.generation == generation
                            && mailbox.prefetch.as_ref().is_some_and(|work| {
                                work.decoding && work.generation == generation && work.path == path
                            })
                    })
                    .expect("image mailbox");
                drop(mailbox);
                if is_current() {
                    cache
                        .lock()
                        .expect("image cache")
                        .take(&path, stamp, remaining)
                } else {
                    None
                }
            });
            if !is_current() {
                break;
            }
            let result = cached.unwrap_or_else(|| {
                decode(&path, remaining, &is_current, &mut first_frame).map(Arc::new)
            });
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
            let preview = result.as_ref().ok().map(Arc::clone);
            {
                let mut mailbox = mutex.lock().expect("image mailbox");
                if mailbox.closed {
                    return;
                }
                if mailbox.generation != generation {
                    break;
                }
                mailbox.preview_ready = None;
                mailbox
                    .completed
                    .get_or_insert_with(|| LoadedImages {
                        generation,
                        first_index: index,
                        total,
                        images: Vec::new(),
                    })
                    .images
                    .push((path.clone(), result));
            }
            notify();
            if let Some(image) = preview
                && let Some(previews) = &previews
                && stamp.is_some()
            {
                previews.remember_image(&path, &image, &|| {
                    is_current() && ImageStamp::read(&path) == stamp
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn wait_for_prefetch(loader: &ImageLoader) {
        let (mailbox, timeout) = loader
            .shared
            .1
            .wait_timeout_while(
                loader.shared.0.lock().expect("mailbox"),
                Duration::from_secs(5),
                |mailbox| mailbox.prefetch.is_some(),
            )
            .expect("prefetch completion");
        assert!(!timeout.timed_out() && mailbox.prefetch.is_none());
    }

    #[test]
    fn prefetch_batch_preserves_priority_budget_and_skips_failures() {
        let root = std::env::temp_dir().join(format!(
            "towavue-prefetch-batch-budget-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("owned directory");
        let paths: Vec<_> = (0..11)
            .map(|index| root.join(format!("{index}.png")))
            .collect();
        for path in &paths {
            std::fs::write(path, [1]).expect("owned identity");
        }
        for budget in [8, 40] {
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let cache = Arc::clone(&shared.0.lock().expect("mailbox").cache);
            *cache.lock().expect("cache") = ImageCache::new(budget);
            let warm = pixel(42);
            {
                let mut cache = cache.lock().expect("cache");
                cache.insert(
                    paths[0].clone(),
                    ImageStamp::read(&paths[0]).expect("stamp"),
                    Arc::clone(&warm),
                );
                // The priority hit is older than unrelated entries and must be promoted.
                for index in 0..budget / 4 - 1 {
                    cache.insert(
                        root.join(format!("old-{index}")),
                        ImageStamp::read(&paths[0]).expect("stamp"),
                        pixel(99),
                    );
                }
            }
            let loader = ImageLoader {
                shared,
                prefetch_worker: LatestTask::new("batch-budget-test").expect("worker"),
            };
            let (sent, received) = mpsc::channel();
            let mut batch = paths.clone();
            batch.insert(1, paths[0].clone());
            loader.prefetch_with_decode(batch, move |path, remaining, _| {
                let index: u8 = path
                    .file_stem()
                    .expect("stem")
                    .to_str()
                    .expect("name")
                    .parse()
                    .expect("index");
                sent.send((index, remaining)).expect("decode observation");
                match index {
                    1 => Err(ImageDecodeError::TooLarge),
                    2 => Ok(None),
                    _ => Ok(Some((*pixel(index)).clone())),
                }
            });
            wait_for_prefetch(&loader);
            let calls: Vec<_> = received.try_iter().collect();
            let expected: Vec<_> = if budget == 8 {
                vec![(1, 4), (2, 4), (3, 4)]
            } else {
                (1_u8..=8)
                    .map(|index| (index, 36 - usize::from(index.saturating_sub(3)) * 4))
                    .collect()
            };
            assert_eq!(calls, expected, "budget={budget}");
            let mut cache = cache.lock().expect("cache");
            assert!(cache.bytes <= budget && cache.entries.len() <= 10);
            let retained = cache
                .take(&paths[0], ImageStamp::read(&paths[0]), budget)
                .expect("priority retained")
                .expect("warm image");
            assert!(Arc::ptr_eq(&warm, &retained));
            let successful = if budget == 8 { 3 } else { 8 };
            for (index, path) in paths.iter().enumerate().take(successful + 1).skip(3) {
                assert_eq!(
                    cache
                        .take(path, ImageStamp::read(path), budget)
                        .expect("batch page retained")
                        .expect("image")
                        .frames[0]
                        .rgba,
                    vec![index as u8; 4]
                );
            }
            assert!(
                cache
                    .entries
                    .iter()
                    .all(|entry| entry.0 != paths[9] && entry.0 != paths[10])
            );
        }
        std::fs::remove_dir_all(root).expect("remove owned fixtures");
    }

    #[test]
    fn ten_page_prefetch_reuses_exact_originals_and_seeds_every_preview() {
        let root =
            std::env::temp_dir().join(format!("towavue-prefetch-ten-pages-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("owned directory");
        let paths: Vec<_> = (0..10)
            .map(|index| root.join(format!("{index}.png")))
            .collect();
        for (index, path) in paths.iter().enumerate() {
            image::RgbaImage::from_pixel(30, 20, image::Rgba([index as u8, 37, 59, 127]))
                .save(path)
                .expect("owned PNG");
        }
        let previews = PreviewCache::new(root.join("cache")).expect("preview cache");
        let shared = Arc::new((
            Mutex::new(Mailbox {
                previews: Some(previews.clone()),
                ..Default::default()
            }),
            Condvar::new(),
        ));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
            prefetch_worker: LatestTask::new("ten-pages-test").expect("worker"),
        };
        loader.prefetch_paths(paths.clone());
        wait_for_prefetch(&loader);
        assert!(loader.take_completed().is_none() && loader.take_preview().is_none());
        let originals: Vec<_> = shared
            .0
            .lock()
            .expect("mailbox")
            .cache
            .lock()
            .expect("cache")
            .entries
            .iter()
            .map(|entry| Arc::clone(&entry.2))
            .collect();
        assert_eq!(originals.len(), 10);
        for (index, path) in paths.iter().enumerate() {
            let expected = [index as u8, 37, 59, 127];
            assert!(
                originals[index].frames[0]
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == expected)
            );
            let preview = previews
                .cached_image(path)
                .expect("lookup")
                .expect("seeded preview");
            assert_eq!(preview.source_size, (30, 20));
            assert!(
                preview
                    .image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == expected)
            );
        }
        let (sent, ready) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    let _ = sent.send(());
                },
                |_, _, _, _| panic!("prefetched spread must not decode again"),
            )
        });
        let generation = loader.request(paths.clone());
        let mut count = 0;
        while count < paths.len() {
            ready
                .recv_timeout(Duration::from_secs(5))
                .expect("foreground ready");
            let Some(result) = loader.take_completed() else {
                continue;
            };
            assert_eq!(result.generation, generation);
            assert_eq!(result.first_index, count);
            assert_eq!(result.total, 10);
            for (path, image) in result.images {
                assert_eq!(path, paths[count]);
                assert!(Arc::ptr_eq(&image.expect("image"), &originals[count]));
                count += 1;
            }
        }
        drop(loader);
        worker.join().expect("shutdown");
        std::fs::remove_dir_all(root).expect("remove owned fixtures");
    }

    #[test]
    fn later_prefetch_page_is_adopted_without_continuing_the_old_batch() {
        let root = std::env::temp_dir().join(format!(
            "towavue-prefetch-later-page-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("owned directory");
        let paths: Vec<_> = (0..3)
            .map(|index| root.join(format!("{index}.png")))
            .collect();
        for path in &paths {
            std::fs::write(path, [1]).expect("owned identity");
        }
        for cancel in [false, true] {
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let loader = ImageLoader {
                shared: Arc::clone(&shared),
                prefetch_worker: LatestTask::new("later-page-test").expect("worker"),
            };
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let (finished_tx, finished_rx) = mpsc::channel();
            let mut index = 0;
            loader.prefetch_with_decode(paths.clone(), move |_, _, current| {
                index += 1;
                assert!(index <= 2, "obsolete batch tail decoded");
                if index == 2 {
                    started_tx.send(()).expect("second started");
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release");
                    finished_tx.send(current()).expect("currency");
                }
                Ok(Some((*pixel(index)).clone()))
            });
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("second running");
            let (sent, ready) = mpsc::channel();
            let worker = thread::spawn(move || {
                run_worker(
                    shared,
                    || {
                        let _ = sent.send(());
                    },
                    |_, _, _, _| Ok((*pixel(99)).clone()),
                )
            });
            let generation = loader.request(paths.clone());
            if cancel {
                loader.prefetch_paths(vec![]);
            }
            if !cancel {
                release_tx.send(()).expect("resume adopted page");
            }
            let mut values = Vec::new();
            while values.len() < 3 {
                ready
                    .recv_timeout(Duration::from_secs(5))
                    .expect("foreground ready without cancelled job");
                if let Some(result) = loader.take_completed() {
                    assert_eq!(result.generation, generation);
                    assert_eq!(result.first_index, values.len());
                    values.extend(
                        result
                            .images
                            .into_iter()
                            .map(|(_, image)| image.expect("image").frames[0].rgba[0]),
                    );
                }
            }
            if cancel {
                release_tx.send(()).expect("finish cancelled page");
            }
            assert_eq!(
                finished_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("currency"),
                !cancel
            );
            assert_eq!(
                values,
                if cancel {
                    vec![1, 99, 99]
                } else {
                    vec![1, 2, 99]
                }
            );
            wait_for_prefetch(&loader);
            drop(loader);
            worker.join().expect("shutdown");
        }
        std::fs::remove_dir_all(root).expect("remove owned fixtures");
    }

    #[test]
    fn foreground_adopts_the_matching_inflight_prefetch() {
        let path = std::env::temp_dir().join(format!(
            "towavue-prefetch-handoff-{}.png",
            std::process::id()
        ));
        std::fs::write(&path, [1]).expect("owned source identity");
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
            prefetch_worker: LatestTask::new("handoff-test").expect("prefetch worker"),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    let _ = ready_tx.send(());
                },
                |_, _, _, _| Ok((*pixel(99)).clone()),
            )
        });
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (current_tx, current_rx) = mpsc::channel();
        loader.prefetch_with_decode(vec![path.clone()], move |_, _, current| {
            started_tx.send(()).expect("prefetch started");
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("release prefetch");
            current_tx.send(current()).expect("prefetch currency");
            Ok(Some((*pixel(42)).clone()))
        });
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("running prefetch");
        let previous = loader.request(vec![path.clone()]);
        let generation = loader.request(vec![path.clone()]);
        assert_ne!(generation, previous);
        release_tx.send(()).expect("finish prefetch");
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("foreground result");
        let result = loader.take_completed().expect("completed");
        assert_eq!(result.generation, generation);
        let still_current = current_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("currency result");
        drop(loader);
        worker.join().expect("foreground shutdown");
        std::fs::remove_file(path).expect("remove owned fixture");
        assert!(still_current, "matching prefetch was cancelled");
        assert_eq!(
            result.images[0].1.as_ref().expect("image").frames[0].rgba,
            vec![42; 4],
            "foreground repeated decoding"
        );
    }

    #[test]
    fn adopted_prefetch_preserves_budget_fallback_and_nonblocking_cancellation() {
        for mode in [
            "budget",
            "changed",
            "error",
            "unsupported",
            "animated",
            "cancel",
            "closed",
        ] {
            let root =
                std::env::temp_dir().join(format!("towavue-adopt-{mode}-{}", std::process::id()));
            std::fs::create_dir_all(&root).expect("owned directory");
            let path = root.join("source.png");
            let other = root.join("other.png");
            std::fs::write(&path, [1]).expect("source identity");
            std::fs::write(&other, [2]).expect("other identity");
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let mut loader = Some(ImageLoader {
                shared: Arc::clone(&shared),
                prefetch_worker: LatestTask::new("adopt-matrix").expect("prefetch worker"),
            });
            let (ready_tx, ready_rx) = mpsc::channel();
            let (fallback_tx, fallback_rx) = mpsc::channel();
            let (exit_tx, exit_rx) = mpsc::channel();
            let worker = thread::spawn(move || {
                run_worker(
                    shared,
                    || {
                        let _ = ready_tx.send(());
                    },
                    |path, _, _, _| {
                        fallback_tx
                            .send(path.to_owned())
                            .expect("fallback observation");
                        Ok((*pixel(99)).clone())
                    },
                );
                exit_tx.send(()).expect("worker exit");
            });
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let (finished_tx, finished_rx) = mpsc::channel();
            loader.as_ref().expect("loader").prefetch_with_decode(
                vec![path.clone()],
                move |_, _, current| {
                    started_tx.send(()).expect("started");
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release prefetch");
                    finished_tx.send(current()).expect("currency");
                    match mode {
                        "error" => Err(ImageDecodeError::TooLarge),
                        "unsupported" => Ok(None),
                        "animated" => {
                            let mut image = (*pixel(42)).clone();
                            image.frames.push(image.frames[0].clone());
                            Ok(Some(image))
                        }
                        _ => Ok(Some((*pixel(42)).clone())),
                    }
                },
            );
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("running prefetch");
            let generation = loader
                .as_ref()
                .expect("loader")
                .request_with_retained_bytes(
                    vec![path.clone()],
                    if mode == "budget" {
                        IMAGE_BYTE_LIMIT - 1
                    } else {
                        0
                    },
                );
            match mode {
                "changed" => std::fs::write(&path, [3, 4]).expect("replace owned source"),
                "cancel" => {
                    let next = loader
                        .as_ref()
                        .expect("loader")
                        .request(vec![other.clone()]);
                    ready_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("new image completes before old prefetch is released");
                    let result = loader
                        .as_ref()
                        .expect("loader")
                        .take_completed()
                        .expect("new result");
                    assert_eq!(result.generation, next);
                    assert_eq!(result.images[0].0, other);
                    assert_eq!(
                        fallback_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("new decode"),
                        other
                    );
                }
                "closed" => {
                    drop(loader.take());
                    exit_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("close does not wait for prefetch");
                }
                _ => {}
            }
            release_tx.send(()).expect("release old worker");
            let current = finished_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("prefetch finished");
            assert_eq!(current, !matches!(mode, "cancel" | "closed"), "{mode}");
            if !matches!(mode, "cancel" | "closed") {
                ready_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("adopted or fallback result");
                let result = loader
                    .as_ref()
                    .expect("loader")
                    .take_completed()
                    .expect("completion");
                assert_eq!(result.generation, generation);
                if mode == "budget" {
                    assert!(matches!(
                        result.images[0].1,
                        Err(ImageDecodeError::TooLarge)
                    ));
                    assert!(
                        fallback_rx.try_recv().is_err(),
                        "budget failure must not repeat decoding"
                    );
                } else {
                    assert_eq!(
                        fallback_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("fallback decode"),
                        path
                    );
                    assert_eq!(
                        result.images[0].1.as_ref().expect("fallback").frames[0].rgba,
                        vec![99; 4]
                    );
                }
            }
            drop(loader);
            worker.join().expect("foreground exit");
            std::fs::remove_dir_all(root).expect("remove owned fixtures");
        }
    }

    #[test]
    fn queued_prefetch_is_not_awaited_and_old_leases_do_not_clear_replacements() {
        for queued in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "towavue-queued-prefetch-{queued}-{}.png",
                std::process::id()
            ));
            std::fs::write(&path, [1]).expect("owned source");
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let loader = ImageLoader {
                shared: Arc::clone(&shared),
                prefetch_worker: LatestTask::new("queued-handoff").expect("worker"),
            };
            let (ready_tx, ready_rx) = mpsc::channel();
            let worker = thread::spawn(move || {
                run_worker(
                    shared,
                    || {
                        let _ = ready_tx.send(());
                    },
                    |_, _, _, _| Ok((*pixel(99)).clone()),
                )
            });
            let (old_started_tx, old_started_rx) = mpsc::channel();
            let (old_release_tx, old_release_rx) = mpsc::channel();
            let (old_finished_tx, old_finished_rx) = mpsc::channel();
            loader.prefetch_with_decode(vec![path.clone()], move |_, _, current| {
                old_started_tx.send(()).expect("old started");
                old_release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("old release");
                old_finished_tx.send(current()).expect("old finished");
                Ok(Some((*pixel(10)).clone()))
            });
            old_started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("old running");
            let (new_started_tx, new_started_rx) = mpsc::channel();
            let (new_release_tx, new_release_rx) = mpsc::channel();
            loader.prefetch_with_decode(vec![path.clone()], move |_, _, current| {
                new_started_tx.send(()).expect("replacement started");
                new_release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("replacement release");
                assert!(current(), "replacement lease was lost");
                Ok(Some((*pixel(42)).clone()))
            });
            if !queued {
                old_release_tx.send(()).expect("release old");
                assert!(
                    !old_finished_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("old invalidated")
                );
                new_started_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("replacement survives old lease drop");
            }
            let generation = loader.request(vec![path.clone()]);
            if !queued {
                new_release_tx.send(()).expect("complete replacement");
            }
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("no wait behind queued work");
            let result = loader.take_completed().expect("completion");
            assert_eq!(result.generation, generation);
            assert_eq!(
                result.images[0].1.as_ref().expect("image").frames[0].rgba,
                vec![if queued { 99 } else { 42 }; 4]
            );
            drop(loader);
            worker.join().expect("foreground stopped");
            if queued {
                old_release_tx
                    .send(())
                    .expect("release obsolete worker after foreground");
                assert!(
                    !old_finished_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("old cancelled")
                );
                assert!(
                    new_started_rx.recv_timeout(Duration::from_secs(5)).is_err(),
                    "queued work should never decode"
                );
            }
            std::fs::remove_file(path).expect("remove owned source");
        }
    }

    #[test]
    fn real_png_continues_from_its_prefetch_read_boundary() {
        let path = std::env::temp_dir().join(format!(
            "towavue-real-png-handoff-{}.png",
            std::process::id()
        ));
        image::RgbImage::from_fn(512, 256, |x, y| {
            let n = (x + y * 512).wrapping_mul(0x9e37_79b9);
            let n = (n ^ (n >> 16)).wrapping_mul(0x85eb_ca6b);
            image::Rgb([n as u8, (n >> 8) as u8, (n >> 16) as u8])
        })
        .save(&path)
        .expect("owned PNG");
        let expected = crate::decode_image(&path).expect("reference PNG");
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
            prefetch_worker: LatestTask::new("real-png-handoff").expect("prefetch"),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    let _ = ready_tx.send(());
                },
                |path, budget, current, preview| {
                    observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    decode_image_with_preview(path, budget, current, preview)
                },
            )
        });
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        loader.prefetch_with_decode(vec![path.clone()], move |path, budget, current| {
            let polls = std::cell::Cell::new(0);
            decode_image_for_prefetch(path, budget, &|| {
                polls.set(polls.get() + 1);
                if polls.get() == 32 {
                    started_tx.send(()).expect("mid-decode boundary");
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("resume same decoder");
                }
                current()
            })
        });
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("PNG is already decoding");
        let generation = loader.request(vec![path.clone()]);
        release_tx.send(()).expect("continue PNG");
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("PNG completion");
        let result = loader.take_completed().expect("completed PNG");
        assert_eq!(result.generation, generation);
        assert_eq!(
            result.images[0].1.as_ref().expect("PNG").as_ref(),
            &expected
        );
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "original decoder restarted"
        );
        drop(loader);
        worker.join().expect("worker stopped");
        std::fs::remove_file(path).expect("remove owned PNG");
    }

    #[test]
    fn obsolete_or_unsuitable_prefetches_do_not_seed_either_cache() {
        for mode in ["cancel", "changed", "closed", "error", "animated"] {
            let root = std::env::temp_dir().join(format!(
                "towavue-prefetch-reject-{}-{mode}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("owned fixture directory");
            let path = root.join("source.png");
            std::fs::write(&path, [1]).expect("source identity");
            let previews = PreviewCache::new(root.join("cache")).expect("preview cache");
            let shared = Arc::new((
                Mutex::new(Mailbox {
                    previews: Some(previews.clone()),
                    ..Default::default()
                }),
                Condvar::new(),
            ));
            let mut loader = Some(ImageLoader {
                shared: Arc::clone(&shared),
                prefetch_worker: LatestTask::new("prefetch-rejection-test").expect("worker"),
            });
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            loader.as_ref().expect("live loader").prefetch_with_decode(
                vec![path.clone()],
                move |_, _, _| {
                    started_tx.send(()).expect("started");
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release codec");
                    if mode == "error" {
                        return Err(ImageDecodeError::TooLarge);
                    }
                    let mut image = (*pixel(9)).clone();
                    if mode == "animated" {
                        image.frames.push(image.frames[0].clone());
                    }
                    Ok(Some(image))
                },
            );
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("prefetch started");
            match mode {
                "cancel" => {
                    loader.as_ref().expect("live loader").request(Vec::new());
                }
                "changed" => std::fs::write(&path, [2, 3]).expect("changed source"),
                "closed" => drop(loader.take()),
                _ => {}
            }
            release_tx.send(()).expect("finish codec");
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let retained = if loader.is_some() { 2 } else { 1 };
            while Arc::strong_count(&shared) != retained {
                assert!(
                    std::time::Instant::now() < deadline,
                    "prefetch closure did not finish"
                );
                thread::sleep(Duration::from_millis(1));
            }
            assert!(
                shared
                    .0
                    .lock()
                    .expect("mailbox")
                    .cache
                    .lock()
                    .expect("decoded cache")
                    .entries
                    .is_empty()
            );
            assert!(
                previews
                    .cached_image(&path)
                    .expect("preview lookup")
                    .is_none()
            );
            drop(loader);
            std::fs::remove_dir_all(root).expect("remove owned fixtures");
        }
    }

    #[test]
    fn prefetched_originals_seed_previews_and_reseed_without_decoding() {
        let root =
            std::env::temp_dir().join(format!("towavue-prefetch-preview-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("owned fixture directory");
        let path = root.join("unvisited.png");
        image::RgbaImage::from_pixel(600, 400, image::Rgba([17, 37, 59, 127]))
            .save(&path)
            .expect("owned PNG");
        let previews = PreviewCache::new(root.join("cache")).expect("preview cache");
        let (sent, ready) = mpsc::channel();
        let loader = ImageLoader::new(previews.clone(), move || {
            let _ = sent.send(());
        })
        .expect("image loader");
        let wait = |cache: &PreviewCache| {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(preview) = cache.cached_image(&path).expect("cache lookup") {
                    return preview;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "prefetch did not seed a preview"
                );
                thread::sleep(Duration::from_millis(1));
            }
        };
        loader.prefetch(path.clone());
        let first = wait(&previews);
        assert_eq!(first.source_size, (600, 400));
        assert_eq!((first.image.width, first.image.height), (240, 160));
        assert!(
            first
                .image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|rgba| rgba == &[17, 37, 59, 127])
        );
        assert!(loader.take_completed().is_none() && loader.take_preview().is_none());
        assert!(
            ready.try_recv().is_err(),
            "speculation must not wake or replace the visible image"
        );
        let original = loader
            .shared
            .0
            .lock()
            .expect("mailbox")
            .cache
            .lock()
            .expect("decoded cache")
            .entries[0]
            .2
            .clone();
        let filmstrip = previews
            .filmstrip(&path, towavue_core::MediaKind::Image)
            .expect("shared filmstrip");
        assert_eq!(filmstrip.image, first.image);
        assert_eq!(
            std::fs::read_dir(root.join("cache"))
                .expect("cache files")
                .count(),
            0,
            "seeded filmstrip does not generate a disk thumbnail"
        );

        // A decoded hit can outlive its small preview. Replace only that optional
        // cache in the fixture to exercise reseeding without another codec call.
        let empty = PreviewCache::new(root.join("empty-cache")).expect("empty preview cache");
        loader.shared.0.lock().expect("mailbox").previews = Some(empty.clone());
        loader.prefetch_with_decode(vec![path.clone()], |_, _, _| {
            panic!("decoded hit must not decode again")
        });
        let reseeded = wait(&empty);
        assert_eq!(reseeded.image, first.image);
        assert_eq!(reseeded.source_size, first.source_size);
        loader.request(vec![path.clone()]);
        ready
            .recv_timeout(Duration::from_secs(5))
            .expect("original ready");
        let loaded = loader
            .take_completed()
            .expect("original")
            .images
            .remove(0)
            .1
            .expect("decoded");
        assert!(Arc::ptr_eq(&loaded, &original));
        drop(loader);
        std::fs::remove_dir_all(root).expect("remove owned fixtures");
    }

    #[test]
    fn disk_thumbnail_precedes_the_original_after_memory_cache_restart() {
        let root =
            std::env::temp_dir().join(format!("towavue-disk-first-preview-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("owned directory");
        let path = root.join("source.png");
        image::RgbaImage::from_pixel(480, 320, image::Rgba([12, 34, 56, 127]))
            .save(&path)
            .expect("owned PNG");
        let original = crate::decode_image(&path).expect("reference original");
        let cache_root = root.join("cache");
        let seed = PreviewCache::new(cache_root.clone()).expect("seed cache");
        let expected = seed
            .filmstrip(&path, towavue_core::MediaKind::Image)
            .expect("persist thumbnail")
            .image;
        drop(seed);
        let shared = Arc::new((
            Mutex::new(Mailbox {
                previews: Some(PreviewCache::new(cache_root).expect("fresh memory cache")),
                ..Default::default()
            }),
            Condvar::new(),
        ));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
            prefetch_worker: LatestTask::new("disk-preview-test").expect("worker"),
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    let _ = ready_tx.send(());
                },
                |path, budget, current, preview| {
                    started_tx.send(()).expect("original started");
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release original");
                    decode_image_with_preview(path, budget, current, preview)
                },
            )
        });
        let generation = loader.request(vec![path.clone()]);
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("original blocked");
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("preview ready before original");
        assert!(loader.take_completed().is_none());
        let preview = loader.take_preview().expect("disk preview notification");
        assert_eq!(preview.generation, generation);
        assert_eq!(preview.path, path);
        assert_eq!(preview.preview.source_size, (480, 320));
        assert_eq!(preview.preview.image, expected);
        loader.shared.0.lock().expect("mailbox").preview_ready = Some(preview);
        release_tx.send(()).expect("resume original");
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("original ready");
        assert!(loader.take_preview().is_none());
        let completed = loader.take_completed().expect("original result");
        assert_eq!(completed.generation, generation);
        assert_eq!(
            completed.images[0].1.as_ref().expect("original").as_ref(),
            &original
        );
        drop(loader);
        worker.join().expect("shutdown");
        std::fs::remove_dir_all(root).expect("remove owned fixtures");
    }

    #[test]
    fn first_frame_preview_precedes_completion_and_respects_request_lifetime() {
        for mode in ["success", "failure", "cancel", "changed", "closed"] {
            let root = std::env::temp_dir().join(format!(
                "towavue-first-preview-{}-{mode}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("fixture directory");
            let path = root.join("source.gif");
            std::fs::write(&path, [1]).expect("source identity");
            let previews = PreviewCache::new(root.join("cache")).expect("preview cache");
            let shared = Arc::new((
                Mutex::new(Mailbox {
                    previews: Some(previews.clone()),
                    ..Default::default()
                }),
                Condvar::new(),
            ));
            let mut loader = Some(ImageLoader {
                shared: Arc::clone(&shared),
                prefetch_worker: LatestTask::new("first-preview-test").expect("prefetch worker"),
            });
            let (started_tx, started_rx) = mpsc::channel();
            let (begin_tx, begin_rx) = mpsc::channel();
            let (preview_tx, preview_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::channel();
            let worker = thread::spawn(move || {
                run_worker(
                    shared,
                    || {
                        let _ = ready_tx.send(());
                    },
                    |_, _, _, preview| {
                        started_tx.send(()).expect("started");
                        begin_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("begin first frame");
                        preview(600, 400, &vec![127; 600 * 400 * 4]);
                        preview_tx.send(()).expect("preview attempted");
                        release_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("finish remaining frames");
                        if mode == "failure" {
                            Err(ImageDecodeError::TooLarge)
                        } else {
                            Ok((*pixel(9)).clone())
                        }
                    },
                )
            });
            let generation = loader
                .as_ref()
                .expect("live loader")
                .request(vec![path.clone()]);
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("decode started");
            match mode {
                "cancel" => {
                    loader.as_ref().expect("live loader").request(Vec::new());
                }
                "changed" => std::fs::write(&path, [2, 3]).expect("replace source"),
                "closed" => drop(loader.take()),
                _ => {}
            }
            begin_tx.send(()).expect("decode first frame");
            preview_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("first frame attempted");
            if matches!(mode, "success" | "failure") {
                ready_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("preview wakeup before full decode");
                let loader = loader.as_ref().expect("live loader");
                assert!(loader.take_completed().is_none());
                let preview = loader
                    .take_preview()
                    .expect("cold preview without a completed original");
                assert_eq!(preview.generation, generation);
                assert_eq!(preview.path, path);
                assert_eq!(preview.preview.source_size, (600, 400));
                assert_eq!(
                    (preview.preview.image.width, preview.preview.image.height),
                    (240, 160)
                );
                assert_eq!(preview.preview.image.rgba, vec![127; 240 * 160 * 4]);
                assert_eq!(
                    previews
                        .cached_image(&path)
                        .expect("cache lookup")
                        .expect("seeded preview")
                        .image,
                    preview.preview.image
                );
                assert!(loader.take_preview().is_none());
                // Also exercise cleanup if the UI has not consumed the preview.
                loader.shared.0.lock().expect("mailbox").preview_ready = Some(preview);
            } else {
                assert!(
                    previews
                        .cached_image(&path)
                        .expect("cache lookup")
                        .is_none()
                );
                assert!(ready_rx.try_recv().is_err());
                if let Some(loader) = &loader {
                    assert!(loader.take_preview().is_none());
                }
            }
            release_tx.send(()).expect("complete decode");
            if matches!(mode, "success" | "failure" | "changed") {
                ready_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("original completion");
                let loader = loader.as_ref().expect("live loader");
                assert!(
                    loader.take_preview().is_none(),
                    "success and failure retire the placeholder"
                );
                let completed = loader.take_completed().expect("completed image");
                assert_eq!(completed.images[0].1.is_err(), mode == "failure");
            }
            drop(loader);
            worker.join().expect("worker shutdown");
            std::fs::remove_dir_all(root).expect("remove owned fixtures");
        }
    }

    #[test]
    fn cold_static_preview_is_published_before_original_and_retires_on_terminal_paths() {
        for (extension, mode) in ["jpg", "bmp"].into_iter().flat_map(|extension| {
            ["success", "failure", "cancel", "changed", "closed"].map(|mode| (extension, mode))
        }) {
            let root = std::env::temp_dir().join(format!(
                "towavue-static-first-{}-{extension}-{mode}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("owned fixture root");
            let path = root.join(format!("source.{extension}"));
            image::RgbImage::from_pixel(2560, 1920, image::Rgb([12, 80, 190]))
                .save(&path)
                .expect("large static image");
            let previews = PreviewCache::new(root.join("cache")).expect("preview cache");
            let shared = Arc::new((
                Mutex::new(Mailbox {
                    previews: Some(previews.clone()),
                    ..Default::default()
                }),
                Condvar::new(),
            ));
            let mut loader = Some(ImageLoader {
                shared: Arc::clone(&shared),
                prefetch_worker: LatestTask::new("static-preview-test").expect("prefetch worker"),
            });
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::channel();
            let worker = thread::spawn(move || {
                run_worker(
                    shared,
                    || {
                        let _ = ready_tx.send(());
                    },
                    |path, budget, current, preview| {
                        started_tx.send(()).expect("full decode entry");
                        release_rx
                            .recv_timeout(Duration::from_secs(10))
                            .expect("release original");
                        if mode == "failure" {
                            return Err(ImageDecodeError::TooLarge);
                        }
                        decode_image_with_preview(path, budget, current, preview)
                    },
                )
            });
            let generation = loader.as_ref().expect("loader").request(vec![path.clone()]);
            started_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("original starts after preview");
            ready_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("preview notification");
            let preview = loader
                .as_ref()
                .expect("loader")
                .take_preview()
                .expect("uncached static first display");
            assert_eq!(preview.generation, generation);
            assert_eq!(preview.path, path);
            assert_eq!(preview.preview.source_size, (2560, 1920));
            assert_eq!(
                (preview.preview.image.width, preview.preview.image.height),
                (213, 160)
            );
            assert!(loader.as_ref().expect("loader").take_completed().is_none());
            assert_eq!(
                previews
                    .cached_image(&path)
                    .expect("cache")
                    .expect("reusable preview")
                    .image,
                preview.preview.image
            );
            loader
                .as_ref()
                .expect("loader")
                .shared
                .0
                .lock()
                .expect("mailbox")
                .preview_ready = Some(preview);
            match mode {
                "cancel" => {
                    loader.as_ref().expect("loader").request(Vec::new());
                }
                "closed" => drop(loader.take()),
                "changed" => std::fs::write(&path, b"replaced image").expect("replace owned input"),
                _ => {}
            }
            release_tx.send(()).expect("release full decode");
            if matches!(mode, "success" | "failure" | "changed") {
                ready_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("original completion");
                let loader = loader.as_ref().expect("loader");
                assert!(loader.take_preview().is_none());
                let completed = loader.take_completed().expect("completion");
                let result = &completed.images[0].1;
                if mode == "success" {
                    let reference = crate::decode_image(&path).expect("independent original");
                    assert_eq!(result.as_ref().expect("original").as_ref(), &reference);
                } else {
                    assert!(result.is_err());
                }
            }
            if mode == "cancel" {
                let loader = loader.as_ref().expect("loader");
                assert!(loader.take_preview().is_none());
                assert!(loader.take_completed().is_none());
            }
            drop(loader);
            worker.join().expect("worker shutdown");
            if matches!(mode, "cancel" | "closed") {
                assert!(ready_rx.try_recv().is_err());
            }
            std::fs::remove_dir_all(root).expect("remove owned fixtures");
        }
    }

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
    fn resumed_pages_share_the_budget_with_already_retained_pixels() {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
            prefetch_worker: LatestTask::new("image-resume-budget").expect("prefetch worker"),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let (budget_tx, budget_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    ready_tx.send(()).expect("notify");
                },
                |_, budget, _, _| {
                    budget_tx.send(budget).expect("observed budget");
                    if budget < 4 {
                        Err(ImageDecodeError::TooLarge)
                    } else {
                        Ok((*pixel(1)).clone())
                    }
                },
            )
        });
        let paths = vec![
            PathBuf::from("missing-a"),
            PathBuf::from("missing-b"),
            PathBuf::from("missing-c"),
        ];
        let generation = loader.request_with_retained_bytes(paths, IMAGE_BYTE_LIMIT - 8);
        for expected in [8, 4, 0] {
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("page completion");
            assert_eq!(
                budget_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("budget"),
                expected
            );
        }
        let result = loader.take_completed().expect("pages");
        assert_eq!(result.generation, generation);
        assert_eq!(result.images.len(), 3);
        assert!(result.images[..2].iter().all(|(_, result)| result.is_ok()));
        assert!(matches!(
            &result.images[2].1,
            Err(ImageDecodeError::TooLarge)
        ));
        loader.request_with_retained_bytes(vec![PathBuf::from("over-budget")], usize::MAX);
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("over-budget completion");
        assert_eq!(
            budget_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("zero budget"),
            0
        );
        assert!(matches!(
            &loader.take_completed().expect("over-budget").images[0].1,
            Err(ImageDecodeError::TooLarge)
        ));
        loader.request(vec![PathBuf::from("fresh")]);
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("fresh completion");
        assert_eq!(
            budget_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("fresh budget"),
            IMAGE_BYTE_LIMIT
        );
        drop(loader);
        worker.join().expect("worker stopped");
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
                |_, _, _, _| {
                    decoded_tx.send(()).expect("foreground decode");
                    Ok((*pixel(9)).clone())
                },
            )
        });
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (cancelled_tx, cancelled_rx) = mpsc::channel();
        loader.prefetch_with_decode(vec![first.clone()], move |_, budget, current| {
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
        loader.prefetch_with_decode(vec![first.clone()], |_, _, _| Ok(Some((*pixel(2)).clone())));
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
        for index in 0..11 {
            cache.insert(index.to_string().into(), stamp, pixel(index));
        }
        assert_eq!(cache.entries.len(), 10);
        assert_eq!(cache.bytes, 40);
        assert!(cache.take(Path::new("0"), Some(stamp), 100).is_none());
        assert!(matches!(
            cache.take(Path::new("10"), Some(stamp), 3),
            Some(Err(ImageDecodeError::TooLarge))
        ));
        assert_eq!(cache.bytes, 36);
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
                |path, _, _, _| {
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
                |path, budget, _, _| {
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
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("second page result");
        let result = loader.take_completed().expect("pages");
        assert_eq!(result.first_index, 0);
        assert_eq!(result.total, 2);
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
    fn publishes_each_page_before_the_next_decode_and_cancels_partial_results() {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
            prefetch_worker: LatestTask::new("incremental-test-prefetch").expect("prefetch worker"),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    ready_tx.send(()).expect("notify");
                },
                |path, budget, _, _| {
                    if path == Path::new("second.png") {
                        assert_eq!(budget, IMAGE_BYTE_LIMIT - 4);
                        started_tx.send(()).expect("second started");
                        resume_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("release decode");
                    }
                    Ok((*pixel(1)).clone())
                },
            )
        });
        for cancel in [false, true] {
            let generation = loader.request(vec!["first.png".into(), "second.png".into()]);
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("second decode blocked");
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("first page already published");
            let first = loader.take_completed().expect("partial result");
            assert_eq!(first.generation, generation);
            assert_eq!(first.first_index, 0);
            assert_eq!(first.total, 2);
            assert_eq!(first.images.len(), 1);
            assert_eq!(first.images[0].0, Path::new("first.png"));
            let current = if cancel {
                loader.request(vec!["latest.png".into()])
            } else {
                generation
            };
            resume_tx.send(()).expect("release second");
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("next result");
            let next = loader.take_completed().expect("next chunk");
            assert_eq!(next.generation, current);
            assert_eq!(next.first_index, usize::from(!cancel));
            assert_eq!(next.total, if cancel { 1 } else { 2 });
            assert_eq!(next.images.len(), 1);
            assert_eq!(
                next.images[0].0,
                Path::new(if cancel { "latest.png" } else { "second.png" })
            );
        }
        drop(loader);
        worker.join().expect("worker exits");
        assert!(ready_rx.try_recv().is_err());
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
                |path, _, current, _| {
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
