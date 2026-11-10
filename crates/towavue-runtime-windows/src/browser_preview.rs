//! Gallery/Filmstrip persistence, independent of seek and foreground image caches.
use super::*;
use std::cell::RefCell;

const DISK_LIMIT: u64 = 256 * 1024 * 1024;
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const PIXEL_LIMIT: usize = 1024 * 1024 * 4;
const HEADER: usize = 32;
const MAGIC: &[u8; 8] = b"TOWABR01";

pub(crate) struct BrowserCache {
    source: PreviewCache,
    root: PathBuf,
    memory: RefCell<VecDeque<(String, MediaPreview)>>,
    deferred: bool,
    pending: RefCell<Option<PendingPublication>>,
}

struct PendingPublication {
    path: PathBuf,
    key: String,
    preview: MediaPreview,
    cancellation: Cancellation,
    image: Option<ImagePublication>,
}

impl BrowserCache {
    pub(crate) fn new(source: PreviewCache) -> Self {
        Self {
            root: source.root.join("browser-v1"),
            source,
            memory: RefCell::default(),
            deferred: false,
            pending: RefCell::default(),
        }
    }

    pub(crate) fn deferred(mut self) -> Self {
        self.deferred = true;
        self
    }

    pub(crate) fn flush(&self) {
        if let Some(pending) = self.pending.borrow_mut().take() {
            self.publish(pending);
        }
    }

    fn publish(&self, pending: PendingPublication) {
        if !pending.cancellation.is_cancelled()
            && self.key(&pending.path).as_deref() == Some(&pending.key)
        {
            if let Some(image) = pending.image {
                self.source
                    .cancellable(pending.cancellation.clone())
                    .publish_browser_image(&pending.path, image);
            }
            if !pending.cancellation.is_cancelled()
                && self.key(&pending.path).as_deref() == Some(&pending.key)
            {
                self.store(&pending.key, &pending.preview);
            }
        }
    }

    fn persist(
        &self,
        path: &Path,
        key: &str,
        preview: &MediaPreview,
        cancellation: &Cancellation,
        image: Option<ImagePublication>,
    ) {
        let pending = PendingPublication {
            path: path.to_owned(),
            key: key.to_owned(),
            preview: preview.clone(),
            cancellation: cancellation.clone(),
            image,
        };
        if self.deferred {
            // One shared thumbnail allocation survives UI notification. Memory
            // hits do not retry failed persistence; cancellation can discard it.
            *self.pending.borrow_mut() = Some(pending);
        } else {
            self.publish(pending);
        }
    }

    pub(crate) fn key(&self, path: &Path) -> Option<String> {
        cache_key(path, "browser-v1").ok()
    }

    pub(crate) fn memory(&self, key: &str) -> Option<MediaPreview> {
        let mut memory = self.memory.borrow_mut();
        let index = memory.iter().position(|(stored, _)| stored == key)?;
        let entry = memory.remove(index)?;
        let preview = entry.1.clone();
        memory.push_back(entry);
        Some(preview)
    }

    fn remember(&self, key: String, preview: &MediaPreview) {
        let mut memory = self.memory.borrow_mut();
        memory.retain(|(stored, _)| stored != &key);
        let mut bytes = memory
            .iter()
            .map(|(_, p)| p.image.rgba.len())
            .sum::<usize>();
        while memory.len() >= 256 || bytes + preview.image.rgba.len() > MEMORY_LIMIT {
            let Some((_, old)) = memory.pop_front() else {
                return;
            };
            bytes -= old.image.rgba.len();
        }
        memory.push_back((key, preview.clone()));
    }

    pub(crate) fn disk(
        &self,
        path: &Path,
        key: &str,
        cancellation: &Cancellation,
    ) -> Option<MediaPreview> {
        if cancellation.is_cancelled() {
            return None;
        }
        // Exact lookup: cache hits never enumerate the cache or open a media decoder.
        let file = fs::File::open(self.root.join(format!("{key}.rgba"))).ok()?;
        let mut bytes = Vec::new();
        file.take((HEADER + PIXEL_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .ok()?;
        let preview = decode(&bytes)?;
        if cancellation.is_cancelled() || self.key(path).as_deref() != Some(key) {
            return None;
        }
        self.remember(key.to_owned(), &preview);
        Some(preview)
    }

    pub(crate) fn import(
        &self,
        path: &Path,
        key: &str,
        preview: MediaPreview,
        cancellation: &Cancellation,
    ) -> Option<MediaPreview> {
        if cancellation.is_cancelled() || self.key(path).as_deref() != Some(key) {
            return None;
        }
        self.remember(key.to_owned(), &preview);
        self.persist(path, key, &preview, cancellation, None);
        Some(preview)
    }

    // Import old image entries only after the new exact lookup missed.
    pub(crate) fn legacy_image(
        &self,
        path: &Path,
        kind: MediaKind,
        cancellation: &Cancellation,
    ) -> Option<MediaPreview> {
        if kind != MediaKind::Image || cancellation.is_cancelled() {
            return None;
        }
        let key = self.key(path)?;
        let image = self
            .source
            .cancellable(cancellation.clone())
            .cached_image(path)
            .ok()??;
        if cancellation.is_cancelled() || self.key(path).as_deref() != Some(&key) {
            return None;
        }
        let preview = MediaPreview {
            image: image.image,
            duration: None,
        };
        self.remember(key.clone(), &preview);
        self.persist(path, &key, &preview, cancellation, None);
        Some(preview)
    }

    pub(crate) fn generate(
        &self,
        path: &Path,
        kind: MediaKind,
        cancellation: &Cancellation,
    ) -> Result<MediaPreview, String> {
        if cancellation.is_cancelled() {
            return Err("Preview request was cancelled".into());
        }
        let key = self.key(path).ok_or("Could not identify preview source")?;
        if let Some(preview) = self
            .memory(&key)
            .or_else(|| self.disk(path, &key, cancellation))
        {
            return Ok(preview);
        }
        let source = self.source.cancellable(cancellation.clone());
        let (preview, image) = if kind == MediaKind::Image {
            source.browser_image(path)
        } else {
            source.filmstrip(path, kind).map(|preview| (preview, None))
        }
        .map_err(|error| error.to_string())?;
        if cancellation.is_cancelled() || self.key(path).as_deref() != Some(&key) {
            return Err("Preview source changed or request was cancelled".into());
        }
        self.remember(key.clone(), &preview);
        // Persistence is optional and never holds a cache lock during generation.
        self.persist(path, &key, &preview, cancellation, image);
        Ok(preview)
    }

    fn store(&self, key: &str, preview: &MediaPreview) {
        let Some(bytes) = encode(preview) else { return };
        if fs::create_dir_all(&self.root).is_err() {
            return;
        }
        let Ok(lock) = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(PUBLICATION_LOCK))
        else {
            return;
        };
        if lock.try_lock().is_err() {
            return;
        }
        let temporary = self.root.join(format!(
            "{key}.{}.{}.tmp",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let path = self.root.join(format!("{key}.rgba"));
        if fs::write(&temporary, bytes)
            .and_then(|()| fs::rename(&temporary, &path))
            .is_err()
        {
            let _ = fs::remove_file(temporary);
            return;
        }
        self.prune(DISK_LIMIT);
    }

    // Caller holds the publication lock. Maintenance never runs on a cache hit.
    fn prune(&self, limit: u64) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        let mut entries: Vec<_> = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "rgba" || ext == "tmp")
            })
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                metadata.is_file().then(|| {
                    (
                        entry.path(),
                        metadata.len(),
                        metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    )
                })
            })
            .collect();
        let mut size: u64 = entries.iter().map(|entry| entry.1).sum();
        entries.sort_by_key(|entry| entry.2);
        for (path, bytes, _) in entries {
            if size <= limit {
                break;
            }
            if fs::remove_file(path).is_ok() {
                size = size.saturating_sub(bytes);
            }
        }
    }
}

fn checksum(bytes: &[u8]) -> u64 {
    let mut hash = DefaultHasher::new();
    bytes[..24].hash(&mut hash);
    bytes[HEADER..].hash(&mut hash);
    hash.finish()
}

fn encode(preview: &MediaPreview) -> Option<Vec<u8>> {
    let image = &preview.image;
    let size = image.width.checked_mul(image.height)?.checked_mul(4)? as usize;
    if size == 0
        || image.width > 1024
        || image.height > 1024
        || size > PIXEL_LIMIT
        || image.rgba.len() != size
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(HEADER + size);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&image.width.to_le_bytes());
    bytes.extend_from_slice(&image.height.to_le_bytes());
    let duration = preview
        .duration
        .map(|value| u64::try_from(value.as_micros()).unwrap_or(u64::MAX - 1))
        .unwrap_or(u64::MAX);
    bytes.extend_from_slice(&duration.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&image.rgba);
    let sum = checksum(&bytes);
    bytes[24..32].copy_from_slice(&sum.to_le_bytes());
    Some(bytes)
}

fn decode(bytes: &[u8]) -> Option<MediaPreview> {
    if bytes.len() < HEADER || bytes.len() > HEADER + PIXEL_LIMIT || &bytes[..8] != MAGIC {
        return None;
    }
    let width = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let height = u32::from_le_bytes(bytes[12..16].try_into().ok()?);
    let size = width.checked_mul(height)?.checked_mul(4)? as usize;
    if size == 0 || width > 1024 || height > 1024 || size != bytes.len() - HEADER {
        return None;
    }
    let duration = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
    let expected = u64::from_le_bytes(bytes[24..32].try_into().ok()?);
    if checksum(bytes) != expected {
        return None;
    }
    Some(MediaPreview {
        image: PreviewImage {
            width,
            height,
            rgba: bytes[HEADER..].to_vec().into(),
        },
        duration: (duration != u64::MAX).then(|| Duration::from_micros(duration)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_pixels_match_shared_generation_and_persist_after_publication() {
        let root = std::env::temp_dir().join(format!(
            "towavue-browser-pixels-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("fixture");
        let pixels = image::RgbaImage::from_fn(311, 187, |x, y| {
            image::Rgba([
                x as u8,
                y as u8,
                (x ^ y) as u8,
                if x % 5 == 0 { 0 } else { 255 },
            ])
        });
        for (index, format) in [
            image::ImageFormat::Png,
            image::ImageFormat::Jpeg,
            image::ImageFormat::Bmp,
            image::ImageFormat::Gif,
            image::ImageFormat::WebP,
            image::ImageFormat::Tiff,
        ]
        .into_iter()
        .enumerate()
        {
            for mislabeled in [false, true] {
                let extension = if mislabeled {
                    "data"
                } else {
                    format.extensions_str()[0]
                };
                let path = root.join(format!("{index}-{mislabeled}.{extension}"));
                let image = image::DynamicImage::ImageRgba8(pixels.clone());
                let image = if format == image::ImageFormat::Jpeg {
                    image::DynamicImage::ImageRgb8(image.into_rgb8())
                } else {
                    image
                };
                image.save_with_format(&path, format).expect("source");
                let legacy = PreviewCache::new(root.join(format!("old-{index}-{mislabeled}")))
                    .expect("old cache");
                let expected = legacy
                    .filmstrip(&path, MediaKind::Image)
                    .expect("reference pixels");
                let source = PreviewCache::new(root.join(format!("new-{index}-{mislabeled}")))
                    .expect("new cache");
                let browser = BrowserCache::new(source.clone()).deferred();
                let actual = browser
                    .generate(&path, MediaKind::Image, &Cancellation::default())
                    .expect("browser pixels");
                assert_eq!(actual.image, expected.image, "{format:?}/{mislabeled}");
                let key = browser.key(&path).expect("key");
                let record = browser.root.join(format!("{key}.rgba"));
                assert!(!record.exists(), "pixels are returned before disk work");
                assert!(Arc::ptr_eq(
                    &actual.image.rgba,
                    &browser.memory(&key).expect("published memory").image.rgba
                ));
                assert!(
                    !source
                        .root
                        .join(format!(
                            "{}.png",
                            cache_key(&path, IMAGE_PREVIEW_VARIANT).expect("test fixture")
                        ))
                        .exists(),
                    "no PNG encoding before pixel publication"
                );
                browser.flush();
                assert!(record.is_file());
                if format != image::ImageFormat::Bmp
                    && !(format == image::ImageFormat::Jpeg && mislabeled)
                {
                    let fresh_shared =
                        PreviewCache::new(source.root.clone()).expect("shared cache restart");
                    assert_eq!(
                        fresh_shared
                            .cached_image(&path)
                            .expect("shared read")
                            .expect("shared pixels")
                            .image,
                        expected.image
                    );
                }
                let fresh =
                    BrowserCache::new(PreviewCache::new(source.root.clone()).expect("restart"));
                assert_eq!(
                    fresh
                        .disk(&path, &key, &Cancellation::default())
                        .expect("test fixture")
                        .image,
                    expected.image
                );
            }
        }
        fs::remove_dir_all(root).expect("remove owned fixture");
    }

    #[test]
    fn deferred_browser_publication_drops_cancelled_and_changed_sources() {
        let root = std::env::temp_dir().join(format!(
            "towavue-browser-defer-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("fixture");
        for changed in [false, true] {
            let path = root.join(format!("{changed}.png"));
            image::RgbaImage::from_pixel(2, 1, image::Rgba([1, 2, 3, 255]))
                .save(&path)
                .expect("image");
            let browser = BrowserCache::new(
                PreviewCache::new(root.join(format!("cache-{changed}"))).expect("test fixture"),
            )
            .deferred();
            let cancellation = Cancellation::default();
            browser
                .generate(&path, MediaKind::Image, &cancellation)
                .expect("pixels");
            let key = browser.key(&path).expect("test fixture");
            if changed {
                fs::write(&path, b"changed source").expect("test fixture");
            } else {
                cancellation.cancel();
            }
            browser.flush();
            assert!(!browser.root.join(format!("{key}.rgba")).exists());
            assert!(browser.pending.borrow().is_none());
        }
        fs::remove_dir_all(root).expect("test fixture");
    }

    #[test]
    fn restarted_browser_reads_pixels_and_duration_without_opening_media() {
        let root = std::env::temp_dir().join(format!(
            "towavue-browser-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("owned fixture");
        let store = BrowserCache::new(PreviewCache::new(root.join("cache")).expect("cache"));
        for (name, kind) in [
            ("a.bmp", MediaKind::Image),
            ("b.mp4", MediaKind::Video),
            ("c.wav", MediaKind::Audio),
        ] {
            let path = root.join(name);
            // These are deliberately undecodable: success proves cache-only reuse.
            fs::write(&path, b"source identity only").expect("source");
            let key = store.key(&path).expect("key");
            let expected = MediaPreview {
                image: PreviewImage {
                    width: 2,
                    height: 1,
                    rgba: vec![1, 2, 3, 0, 4, 5, 6, 255].into(),
                },
                duration: (kind != MediaKind::Image).then(|| Duration::from_micros(12_345_678)),
            };
            store.store(&key, &expected);
            let fresh = BrowserCache::new(PreviewCache::new(root.join("cache")).expect("restart"));
            let actual = fresh
                .generate(&path, kind, &Cancellation::default())
                .expect("cache hit without decoder");
            assert_eq!(actual.image, expected.image);
            assert_eq!(actual.duration, expected.duration);
            assert!(Arc::ptr_eq(
                &actual.image.rgba,
                &fresh.memory(&key).expect("memory hit").image.rgba
            ));
            let cancelled = Cancellation::default();
            cancelled.cancel();
            assert!(fresh.disk(&path, &key, &cancelled).is_none());
            fs::write(&path, b"changed source identity").expect("change");
            assert!(fresh.disk(&path, &key, &Cancellation::default()).is_none());
        }
        fs::remove_dir_all(root).expect("remove owned fixture");
    }

    #[test]
    fn browser_loader_prioritizes_persisted_audio_and_video_and_palette_keeps_its_cache() {
        let root = std::env::temp_dir().join(format!(
            "towavue-browser-queue-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("owned fixture");
        let uncached = root.join("uncached.png");
        image::RgbaImage::from_pixel(2, 1, image::Rgba([1, 2, 3, 255]))
            .save(&uncached)
            .expect("owned image");
        let source = PreviewCache::new(root.join("cache")).expect("cache");
        let browser = BrowserCache::new(source.clone());
        let mut request = vec![(uncached.clone(), MediaKind::Image)];
        for (name, kind) in [
            ("video.mp4", MediaKind::Video),
            ("audio.wav", MediaKind::Audio),
        ] {
            let path = root.join(name);
            fs::write(&path, b"deliberately undecodable").expect("source identity");
            let preview = MediaPreview {
                image: PreviewImage {
                    width: 1,
                    height: 1,
                    rgba: vec![1, 2, 3, 255].into(),
                },
                duration: Some(Duration::from_secs(42)),
            };
            browser.store(&browser.key(&path).expect("key"), &preview);
            request.push((path, kind));
        }
        let (notify, ready) = std::sync::mpsc::channel();
        let loader = crate::PreviewLoader::new(source, move || {
            let _ = notify.send(());
        })
        .expect("worker");
        loader.request(request.clone());
        for _ in 0..3 {
            ready
                .recv_timeout(Duration::from_secs(10))
                .expect("preview");
        }
        let completed = loader.take_completed();
        assert_eq!(
            completed.iter().map(|item| &item.path).collect::<Vec<_>>(),
            [&request[1].0, &request[2].0, &request[0].0]
        );
        for item in &completed[..2] {
            assert_eq!(
                item.result.as_ref().expect("cache only").duration,
                Some(Duration::from_secs(42))
            );
        }
        assert!(completed[2].result.is_ok());
        drop(loader);
        let palette_root = root.join("palette");
        let (notify, ready) = std::sync::mpsc::channel();
        let palette = crate::PreviewLoader::new_shared(
            PreviewCache::new(palette_root.clone()).expect("palette cache"),
            move || {
                let _ = notify.send(());
            },
        )
        .expect("palette worker");
        palette.request(vec![(uncached, MediaKind::Image)]);
        ready
            .recv_timeout(Duration::from_secs(10))
            .expect("palette preview");
        assert!(palette.take_completed()[0].result.is_ok());
        assert!(!palette_root.join("browser-v1").exists());
        drop(palette);
        fs::remove_dir_all(root).expect("remove owned fixture");
    }

    #[test]
    fn persisted_browser_rejects_corruption_truncation_and_invalid_dimensions() {
        let preview = MediaPreview {
            image: PreviewImage {
                width: 2,
                height: 1,
                rgba: vec![0; 8].into(),
            },
            duration: None,
        };
        let bytes = encode(&preview).expect("encode");
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_none());
        }
        for index in 0..bytes.len() {
            let mut corrupt = bytes.clone();
            corrupt[index] ^= 1;
            assert!(decode(&corrupt).is_none(), "byte {index}");
        }
        let mut invalid = bytes.clone();
        invalid[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&invalid).is_none());
        let mut extra = bytes;
        extra.push(0);
        assert!(decode(&extra).is_none());
    }
    #[test]
    fn browser_pruning_preserves_recent_records_and_unrelated_files() {
        let root = std::env::temp_dir().join(format!(
            "towavue-browser-prune-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let store = BrowserCache::new(PreviewCache::new(root.clone()).expect("cache"));
        let preview = MediaPreview {
            image: PreviewImage {
                width: 1,
                height: 1,
                rgba: vec![0; 4].into(),
            },
            duration: None,
        };
        store.store("old", &preview);
        let old = store.root.join("old.rgba");
        fs::File::options()
            .write(true)
            .open(&old)
            .expect("old record")
            .set_times(
                fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
            )
            .expect("old timestamp");
        store.store("recent", &preview);
        fs::write(store.root.join("keep.txt"), b"unrelated").expect("extra file");
        let lock = fs::File::options()
            .read(true)
            .write(true)
            .open(store.root.join(PUBLICATION_LOCK))
            .expect("lock");
        lock.try_lock().expect("owned lock");
        store.prune((HEADER + 4) as u64);
        assert!(!old.exists());
        assert!(store.root.join("recent.rgba").exists());
        assert!(store.root.join("keep.txt").exists());
        assert!(store.root.join(PUBLICATION_LOCK).exists());
        drop(lock);
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
