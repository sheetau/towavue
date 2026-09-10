use std::collections::hash_map::DefaultHasher;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

use thiserror::Error;
use towavue_core::MediaKind;

use crate::Cancellation;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CACHE_LIMIT_BYTES: u64 = 64 * 1024 * 1024;
const MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const IMAGE_PREVIEW_VARIANT: &str = "filmstrip-image-v4";
#[cfg(test)]
#[path = "preview_sharing_tests.rs"]
mod sharing_tests;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

type InFlight = Arc<(Mutex<HashSet<String>>, Condvar)>;

struct GenerationLease {
    pending: InFlight,
    key: String,
}

impl Drop for GenerationLease {
    fn drop(&mut self) {
        let (pending, ready) = &*self.pending;
        pending
            .lock()
            .expect("preview generation")
            .remove(&self.key);
        ready.notify_all();
    }
}

#[derive(Default)]
struct PreviewMemory {
    entries: VecDeque<PreviewEntry>,
    bytes: usize,
    durations: VecDeque<(String, Duration)>,
}

struct PreviewEntry {
    key: String,
    image: PreviewImage,
    source_size: Option<(u32, u32)>,
}

pub struct CachedImagePreview {
    pub image: PreviewImage,
    pub source_size: (u32, u32),
}

impl PreviewMemory {
    fn get(&mut self, key: &str) -> Option<PreviewImage> {
        let index = self.entries.iter().position(|entry| entry.key == key)?;
        let entry = self.entries.remove(index)?;
        let image = entry.image.clone();
        self.entries.push_back(entry);
        Some(image)
    }

    fn insert(&mut self, key: String, image: PreviewImage) {
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            let previous = self.entries.remove(index).expect("existing preview");
            self.bytes -= previous.image.rgba.len();
        }
        let bytes = image.rgba.len();
        if bytes > MEMORY_LIMIT_BYTES {
            return;
        }
        while self.entries.len() >= 64 || self.bytes + bytes > MEMORY_LIMIT_BYTES {
            let oldest = self.entries.pop_front().expect("preview to evict");
            self.bytes -= oldest.image.rgba.len();
        }
        self.bytes += bytes;
        self.entries.push_back(PreviewEntry {
            key,
            image,
            source_size: None,
        });
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct MediaPreview {
    pub image: PreviewImage,
    pub duration: Option<Duration>,
}

#[derive(Debug, Error)]
pub enum PreviewError {
    #[error("preview cancelled")]
    Cancelled,
    #[error("preview cache I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not decode generated preview: {0}")]
    Decode(#[from] image::ImageError),
    #[error("could not start {program}: {source}")]
    Start {
        program: &'static str,
        source: std::io::Error,
    },
    #[error("FFmpeg preview generation failed: {0}")]
    Generate(String),
    #[error("no video frame at the requested preview position")]
    NoFrame,
    #[error("preview input preparation failed: {0}")]
    Seek(#[from] crate::DecodeError),
    #[error("FFprobe returned an invalid duration")]
    InvalidDuration,
    #[error("LOCALAPPDATA is unavailable")]
    NoLocalAppData,
}

#[derive(Clone)]
pub struct PreviewCache {
    root: PathBuf,
    cancellation: Option<Cancellation>,
    memory: Arc<Mutex<PreviewMemory>>,
    in_flight: InFlight,
}

impl PreviewCache {
    pub fn local() -> Result<Self, PreviewError> {
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or(PreviewError::NoLocalAppData)?
            .join("towavue")
            .join("preview-cache");
        Self::new(root)
    }

    pub fn new(root: PathBuf) -> Result<Self, PreviewError> {
        if let Err(error) = fs::create_dir_all(&root) {
            eprintln!("towavue: could not create preview cache: {error}");
        }
        Ok(Self {
            root,
            cancellation: None,
            memory: Arc::default(),
            in_flight: Arc::default(),
        })
    }

    pub fn cancellable(&self, cancellation: Cancellation) -> Self {
        Self {
            root: self.root.clone(),
            cancellation: Some(cancellation),
            memory: Arc::clone(&self.memory),
            in_flight: Arc::clone(&self.in_flight),
        }
    }

    fn check_cancelled(&self) -> Result<(), PreviewError> {
        check_cancelled(self.cancellation.as_ref())
    }

    pub fn thumbnail(
        &self,
        source: &Path,
        position: Duration,
        width: u32,
    ) -> Result<PreviewImage, PreviewError> {
        self.check_cancelled()?;
        let key = cache_key(
            source,
            &format!("thumbnail-v3-{}-{width}", position.as_millis()),
        )?;
        self.load_or_generate(key, || {
            frame_preview(
                source,
                position,
                &format!("scale={width}:-2:force_original_aspect_ratio=decrease"),
                self.cancellation.as_ref(),
            )
        })
    }

    pub fn filmstrip(&self, source: &Path, kind: MediaKind) -> Result<MediaPreview, PreviewError> {
        self.check_cancelled()?;
        let duration = (kind != MediaKind::Image)
            .then(|| self.duration(source).ok())
            .flatten();
        let image = if kind == MediaKind::Audio {
            self.waveform(source, 240, 160)?
        } else {
            let (version, filter) = if kind == MediaKind::Image {
                (
                    IMAGE_PREVIEW_VARIANT,
                    "scale=240:160:force_original_aspect_ratio=decrease:reset_sar=1",
                )
            } else {
                (
                    "filmstrip-v3",
                    "scale=240:160:force_original_aspect_ratio=decrease:reset_sar=1,pad=240:160:(ow-iw)/2:(oh-ih)/2",
                )
            };
            let key = cache_key(source, version)?;
            self.load_or_generate(key, || {
                let position = duration.unwrap_or_default().mul_f64(0.1);
                frame_preview(source, position, filter, self.cancellation.as_ref())
            })?
        };
        Ok(MediaPreview { image, duration })
    }

    pub fn waveform(
        &self,
        source: &Path,
        width: u32,
        height: u32,
    ) -> Result<PreviewImage, PreviewError> {
        self.check_cancelled()?;
        if width == 0 || height == 0 {
            return Err(PreviewError::Generate("invalid waveform dimensions".into()));
        }
        let key = cache_key(source, &format!("waveform-v3-{width}-{height}"))?;
        self.load_or_generate(key, || {
            let (_, stream) = crate::decode::preview_input(
                source,
                ffmpeg_next::media::Type::Audio,
                Duration::ZERO,
                &|| {
                    self.cancellation
                        .as_ref()
                        .is_some_and(Cancellation::is_cancelled)
                },
            )?;
            let command = hidden_command(
                &tool_path("ffmpeg.exe")?,
                [
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-i",
                    &source.display().to_string(),
                    "-map",
                    &format!("0:{stream}"),
                    "-ac",
                    "1",
                    "-c:a",
                    "pcm_s16le",
                    "-f",
                    "s16le",
                    "pipe:1",
                ],
            );
            let cancellation = self.cancellation.clone().unwrap_or_default();
            let result = cancellation.read_output(command, move |reader| {
                Ok(crate::waveform::read(reader, width, height))
            });
            self.check_cancelled()?;
            let (status, image, diagnostics) = result.map_err(|source| PreviewError::Start {
                program: "FFmpeg",
                source,
            })?;
            if !status.success() {
                return Err(PreviewError::Generate(
                    String::from_utf8_lossy(&diagnostics).trim().into(),
                ));
            }
            let image = image.map_err(|error| PreviewError::Generate(error.to_string()))?;
            let mut png = std::io::Cursor::new(Vec::new());
            image.write_to(&mut png, image::ImageFormat::Png)?;
            Ok(png.into_inner())
        })
    }

    pub fn duration(&self, source: &Path) -> Result<Duration, PreviewError> {
        self.check_cancelled()?;
        self.duration_with(cache_key(source, "duration-v1")?, || {
            self.probe_duration(source)
        })
    }

    fn duration_with(
        &self,
        key: String,
        probe: impl FnOnce() -> Result<Duration, PreviewError>,
    ) -> Result<Duration, PreviewError> {
        let _lease = self.claim_generation(&key)?;
        {
            let mut memory = self.memory.lock().expect("preview memory");
            if let Some(index) = memory
                .durations
                .iter()
                .position(|(stored, _)| stored == &key)
            {
                let entry = memory.durations.remove(index).expect("duration");
                let duration = entry.1;
                memory.durations.push_back(entry);
                return Ok(duration);
            }
        }
        let duration = probe()?;
        self.check_cancelled()?;
        let mut memory = self.memory.lock().expect("preview memory");
        if memory.durations.len() == 64 {
            memory.durations.pop_front();
        }
        memory.durations.push_back((key, duration));
        Ok(duration)
    }

    fn probe_duration(&self, source: &Path) -> Result<Duration, PreviewError> {
        let executable = tool_path("ffprobe.exe")?;
        let command = hidden_command(
            &executable,
            [
                "-v",
                "error",
                "-show_entries",
                "format=format_name,start_time,duration",
                "-of",
                "default=noprint_wrappers=1",
                &source.display().to_string(),
            ],
        );
        let output = run_command(command, self.cancellation.as_ref(), "FFprobe")?;
        if !output.status.success() {
            return Err(PreviewError::InvalidDuration);
        }
        probed_duration(&String::from_utf8_lossy(&output.stdout))
    }

    fn load_or_generate(
        &self,
        key: String,
        generate: impl FnOnce() -> Result<Vec<u8>, PreviewError>,
    ) -> Result<PreviewImage, PreviewError> {
        self.check_cancelled()?;
        if let Some(image) = self.memory.lock().expect("preview memory").get(&key) {
            self.check_cancelled()?;
            return Ok(image);
        }
        // Coalesce only this key. Never hold a cache mutex during decode or disk I/O.
        let _lease = self.claim_generation(&key)?;
        self.check_cancelled()?;
        let cached = self.memory.lock().expect("preview memory").get(&key);
        if let Some(image) = cached {
            self.check_cancelled()?;
            return Ok(image);
        }
        let path = self.root.join(format!("{key}.png"));
        if let Ok(bytes) = fs::read(&path)
            && let Ok(image) = decode_png(&bytes)
        {
            self.check_cancelled()?;
            self.memory
                .lock()
                .expect("preview memory")
                .insert(key, image.clone());
            return Ok(image);
        }
        let bytes = generate()?;
        let image = decode_png(&bytes)?;
        self.check_cancelled()?;
        let temporary = self.root.join(format!(
            "{key}.{}.{}.tmp",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        // Disk persistence is optional once valid pixels are available.
        let stored = fs::create_dir_all(&self.root)
            .and_then(|()| fs::write(&temporary, &bytes))
            .and_then(|()| {
                fs::rename(&temporary, &path).inspect_err(|_| {
                    let _ = fs::remove_file(&temporary);
                })
            });
        if let Err(error) = &stored {
            eprintln!("towavue: could not store preview cache: {error}");
        }
        if let Err(error) = self.prune() {
            eprintln!("towavue: could not prune preview cache: {error}");
        }
        self.check_cancelled()?;
        if stored.is_ok() {
            self.memory
                .lock()
                .expect("preview memory")
                .insert(key, image.clone());
        }
        Ok(image)
    }

    fn claim_generation(&self, key: &str) -> Result<GenerationLease, PreviewError> {
        let (pending, ready) = &*self.in_flight;
        let mut pending = pending.lock().expect("preview generation");
        loop {
            self.check_cancelled()?;
            if pending.insert(key.to_owned()) {
                return Ok(GenerationLease {
                    pending: Arc::clone(&self.in_flight),
                    key: key.to_owned(),
                });
            }
            pending = ready
                .wait_timeout(pending, Duration::from_millis(10))
                .expect("preview generation")
                .0;
        }
    }

    pub(crate) fn remember_image(
        &self,
        path: &Path,
        decoded: &crate::DecodedImage,
        current: &dyn Fn() -> bool,
    ) {
        if let Some(frame) = decoded.frames.first() {
            self.remember_pixels(path, frame.width, frame.height, &frame.rgba, current);
        }
    }

    pub(crate) fn remember_pixels(
        &self,
        path: &Path,
        source_width: u32,
        source_height: u32,
        pixels: &[u8],
        current: &dyn Fn() -> bool,
    ) {
        if !current() {
            return;
        }
        let Ok(key) = cache_key(path, IMAGE_PREVIEW_VARIANT) else {
            return;
        };
        {
            let mut memory = self.memory.lock().expect("preview memory");
            if let Some(entry) = memory.entries.iter_mut().find(|entry| entry.key == key) {
                entry.source_size = Some((source_width, source_height));
                return;
            }
        }
        let scale = (240.0 / f64::from(source_width))
            .min(160.0 / f64::from(source_height))
            .min(1.0);
        let width = (f64::from(source_width) * scale).round().max(1.0) as u32;
        let height = (f64::from(source_height) * scale).round().max(1.0) as u32;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let source_y = u64::from(y) * u64::from(source_height) / u64::from(height);
            for x in 0..width {
                let source_x = u64::from(x) * u64::from(source_width) / u64::from(width);
                let index = ((source_y * u64::from(source_width) + source_x) * 4) as usize;
                rgba.extend_from_slice(&pixels[index..index + 4]);
            }
        }
        if current() && cache_key(path, IMAGE_PREVIEW_VARIANT).ok().as_ref() == Some(&key) {
            let mut memory = self.memory.lock().expect("preview memory");
            memory.insert(
                key,
                PreviewImage {
                    width,
                    height,
                    rgba,
                },
            );
            memory
                .entries
                .back_mut()
                .expect("bounded image preview")
                .source_size = Some((source_width, source_height));
        }
    }

    pub fn cached_image(&self, path: &Path) -> Result<Option<CachedImagePreview>, PreviewError> {
        self.check_cancelled()?;
        let key = cache_key(path, IMAGE_PREVIEW_VARIANT)?;
        let preview = {
            let mut memory = self.memory.lock().expect("preview memory");
            let size = memory
                .entries
                .iter()
                .find(|entry| entry.key == key)
                .and_then(|entry| entry.source_size);
            size.and_then(|source_size| {
                memory
                    .get(&key)
                    .map(|image| CachedImagePreview { image, source_size })
            })
        };
        self.check_cancelled()?;
        Ok(preview)
    }

    fn prune(&self) -> Result<(), PreviewError> {
        let mut entries = fs::read_dir(&self.root)?
            .filter_map(Result::ok)
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
            .collect::<Vec<_>>();
        let mut total = entries.iter().map(|(_, length, _)| length).sum::<u64>();
        entries.sort_by_key(|(_, _, modified)| *modified);
        for (path, length, _) in entries {
            if total <= CACHE_LIMIT_BYTES {
                break;
            }
            if fs::remove_file(path).is_ok() {
                total = total.saturating_sub(length);
            }
        }
        Ok(())
    }
}

fn cache_key(source: &Path, variant: &str) -> Result<String, PreviewError> {
    let metadata = source.metadata()?;
    let modified = metadata
        .modified()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut hasher = DefaultHasher::new();
    source
        .canonicalize()
        .unwrap_or_else(|_| source.to_owned())
        .to_string_lossy()
        .to_lowercase()
        .hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    modified.hash(&mut hasher);
    variant.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn frame_preview(
    source: &Path,
    position: Duration,
    filter: &str,
    cancellation: Option<&Cancellation>,
) -> Result<Vec<u8>, PreviewError> {
    let generate = |position| {
        let mut arguments = preview_input_arguments(source, position, cancellation)?;
        arguments.extend(["-vf".into(), filter.into(), "-frames:v".into(), "1".into()]);
        run_ffmpeg(&arguments, cancellation)
    };
    match generate(position) {
        Err(PreviewError::NoFrame)
            if !position.is_zero() && MediaKind::from_path(source) == Some(MediaKind::Video) =>
        {
            let last = crate::decode::preview_last_video_time(source, position, &|| {
                cancellation.is_some_and(Cancellation::is_cancelled)
            });
            check_cancelled(cancellation)?;
            let last = last?
                .filter(|last| *last < position)
                .ok_or(PreviewError::NoFrame)?;
            // CLI seek times have microsecond precision; rounding must not exclude the last frame.
            generate(last.saturating_sub(Duration::from_micros(1)))
        }
        result => result,
    }
}

fn run_ffmpeg(
    arguments: &[String],
    cancellation: Option<&Cancellation>,
) -> Result<Vec<u8>, PreviewError> {
    let executable = tool_path("ffmpeg.exe")?;
    let command = hidden_command(
        &executable,
        ["-hide_banner", "-loglevel", "error"]
            .into_iter()
            .map(String::from)
            .chain(arguments.iter().cloned())
            .chain([
                "-f".into(),
                "image2pipe".into(),
                "-vcodec".into(),
                "png".into(),
                "pipe:1".into(),
            ]),
    );
    let output = run_command(command, cancellation, "FFmpeg")?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(PreviewError::Generate(if message.is_empty() {
            format!("process exited with {}", output.status)
        } else {
            message
        }));
    }
    if output.stdout.is_empty() {
        return Err(PreviewError::NoFrame);
    }
    Ok(output.stdout)
}

fn hidden_command(
    executable: &Path,
    arguments: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
) -> Command {
    let mut command = Command::new(executable);
    command.args(arguments);
    <Command as std::os::windows::process::CommandExt>::creation_flags(
        &mut command,
        CREATE_NO_WINDOW,
    );
    command
}

fn tool_path(name: &'static str) -> Result<PathBuf, PreviewError> {
    crate::media_tools::tool_path(name).map_err(|source| PreviewError::Start {
        program: name,
        source,
    })
}

fn preview_input_arguments(
    source: &Path,
    position: Duration,
    cancellation: Option<&Cancellation>,
) -> Result<Vec<String>, PreviewError> {
    check_cancelled(cancellation)?;
    let (start, stream) = if MediaKind::from_path(source) == Some(MediaKind::Image) {
        (position, 0)
    } else {
        crate::decode::preview_input(source, ffmpeg_next::media::Type::Video, position, &|| {
            cancellation.is_some_and(Cancellation::is_cancelled)
        })?
    };
    let mut arguments = vec![
        "-ss".into(),
        format!("{:.6}", start.as_secs_f64()),
        "-i".into(),
        source.display().to_string(),
        "-map".into(),
        format!("0:{stream}"),
    ];
    if start < position {
        arguments.extend([
            "-ss".into(),
            format!("{:.6}", (position - start).as_secs_f64()),
        ]);
    }
    Ok(arguments)
}

fn check_cancelled(cancellation: Option<&Cancellation>) -> Result<(), PreviewError> {
    if cancellation.is_some_and(Cancellation::is_cancelled) {
        Err(PreviewError::Cancelled)
    } else {
        Ok(())
    }
}

fn run_command(
    mut command: Command,
    cancellation: Option<&Cancellation>,
    program: &'static str,
) -> Result<std::process::Output, PreviewError> {
    check_cancelled(cancellation)?;
    let output = match cancellation {
        Some(cancellation) => cancellation.output(command),
        None => command.output(),
    };
    check_cancelled(cancellation)?;
    output.map_err(|source| PreviewError::Start { program, source })
}

fn probed_duration(text: &str) -> Result<Duration, PreviewError> {
    let field = |prefix| text.lines().find_map(|line| line.strip_prefix(prefix));
    let mut seconds = field("duration=")
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or(PreviewError::InvalidDuration)?;
    if field("format_name=").is_some_and(|name| {
        name.split(',')
            .any(|format| matches!(format, "matroska" | "webm"))
    }) {
        // The pinned Matroska demuxer exposes the segment end, not end minus start.
        if let Some(start) = field("start_time=").filter(|value| *value != "N/A") {
            seconds -= start
                .parse::<f64>()
                .map_err(|_| PreviewError::InvalidDuration)?;
        }
    }
    Duration::try_from_secs_f64(seconds).map_err(|_| PreviewError::InvalidDuration)
}

fn decode_png(bytes: &[u8]) -> Result<PreviewImage, image::ImageError> {
    let rgba = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.to_rgba8();
    Ok(PreviewImage {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn preview_memory_bounds_pixels_and_entries_and_keeps_recent_hits() {
        use super::*;

        let small = PreviewImage {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        };
        let mut memory = PreviewMemory::default();
        for index in 0..64 {
            memory.insert(index.to_string(), small.clone());
        }
        assert_eq!(memory.bytes, 256);
        assert_eq!(memory.get("0"), Some(small.clone()));
        memory.insert("64".into(), small.clone());
        assert!(memory.get("1").is_none());
        assert!(memory.get("0").is_some());
        memory.insert("0".into(), small);
        assert_eq!(memory.bytes, 256);
        let large = PreviewImage {
            width: 1024,
            height: 1024,
            rgba: vec![127; 4 * 1024 * 1024],
        };
        for index in 0..5 {
            memory.insert(format!("large-{index}"), large.clone());
        }
        assert_eq!(memory.entries.len(), 4);
        assert_eq!(memory.bytes, MEMORY_LIMIT_BYTES);
        assert!(memory.get("large-0").is_none());
        assert!(memory.get("large-1").is_some());
        memory.insert(
            "too-large".into(),
            PreviewImage {
                width: 4096,
                height: 2048,
                rgba: vec![0; 32 * 1024 * 1024],
            },
        );
        assert_eq!(memory.bytes, MEMORY_LIMIT_BYTES);
        assert!(memory.get("too-large").is_none());
    }

    #[test]
    fn foreground_pixels_seed_shared_previews_without_decoding_again() {
        use super::*;
        use std::sync::mpsc;
        use std::time::Instant;

        let root =
            std::env::temp_dir().join(format!("towavue-shared-preview-{}", std::process::id()));
        fs::create_dir_all(&root).expect("owned fixture directory");
        let path = root.join("portrait.png");
        let pixels = image::RgbaImage::from_fn(600, 800, |x, y| {
            image::Rgba([x as u8, y as u8, 123, (x + y) as u8])
        });
        pixels.save(&path).expect("owned PNG");
        let cache = PreviewCache::new(root.join("cache")).expect("cache");
        let (sent, ready) = mpsc::channel();
        let loader = crate::ImageLoader::new(cache.clone(), move || {
            let _ = sent.send(());
        })
        .expect("loader");
        let generation = loader.request(vec![path.clone()]);
        ready
            .recv_timeout(Duration::from_secs(5))
            .expect("foreground published");
        let loaded = loader.take_completed().expect("foreground image");
        assert_eq!(loaded.generation, generation);
        assert_eq!(
            loaded.images[0].1.as_ref().expect("decoded").frames[0].rgba,
            pixels.as_raw().as_slice()
        );
        let key = cache_key(&path, IMAGE_PREVIEW_VARIANT).expect("image key");
        let deadline = Instant::now() + Duration::from_secs(5);
        while cache.memory.lock().expect("memory").get(&key).is_none() {
            assert!(
                Instant::now() < deadline,
                "preview follows foreground publication"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let preview = cache
            .clone()
            .filmstrip(&path, MediaKind::Image)
            .expect("shared preview");
        assert_eq!((preview.image.width, preview.image.height), (120, 160));
        let loading = cache
            .cached_image(&path)
            .expect("metadata lookup")
            .expect("source-sized preview");
        assert_eq!(loading.source_size, (600, 800));
        assert_eq!(loading.image, preview.image);
        for y in 0..160_u32 {
            for x in 0..120_u32 {
                let offset = ((y * 120 + x) * 4) as usize;
                assert_eq!(
                    &preview.image.rgba[offset..offset + 4],
                    &pixels.get_pixel(x * 5, y * 5).0
                );
            }
        }
        assert!(
            fs::read_dir(&cache.root)
                .expect("cache directory")
                .next()
                .is_none(),
            "no PNG encoding or companion process for seeded previews"
        );
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert!(matches!(
            cache
                .cancellable(cancellation)
                .filmstrip(&path, MediaKind::Image),
            Err(PreviewError::Cancelled)
        ));
        let rejected =
            PreviewCache::new(root.join("rejected-cache")).expect("separate empty cache");
        let checks = std::cell::Cell::new(0);
        let decoded = loaded.images[0].1.as_ref().expect("foreground pixels");
        rejected.remember_image(&path, decoded, &|| {
            checks.set(checks.get() + 1);
            checks.get() == 1
        });
        assert_eq!(
            checks.get(),
            2,
            "cancellation checked again before publication"
        );
        assert!(rejected.memory.lock().expect("memory").entries.is_empty());
        checks.set(0);
        rejected.remember_image(&path, decoded, &|| {
            checks.set(checks.get() + 1);
            if checks.get() == 2 {
                fs::write(&path, b"changed source")
                    .expect("change metadata during preview sampling");
            }
            true
        });
        assert!(rejected.memory.lock().expect("memory").entries.is_empty());
        fs::remove_dir(&rejected.root).expect("remove empty rejection cache");
        let changed = cache_key(&path, IMAGE_PREVIEW_VARIANT).expect("changed key");
        assert_ne!(key, changed);
        assert!(
            cache
                .cached_image(&path)
                .expect("changed file lookup")
                .is_none()
        );
        assert!(matches!(
            cache.load_or_generate(changed, || Err(PreviewError::NoFrame)),
            Err(PreviewError::NoFrame)
        ));
        drop(loader);
        fs::remove_file(&path).expect("remove owned source");
        assert!(matches!(
            cache.filmstrip(&path, MediaKind::Image),
            Err(PreviewError::Io(_))
        ));
        fs::remove_dir(&cache.root).expect("remove empty cache");
        fs::remove_dir(root).expect("remove empty fixture directory");
    }

    #[test]
    fn generated_preview_memory_is_shared_and_variants_do_not_collide() {
        use super::*;

        let root =
            std::env::temp_dir().join(format!("towavue-preview-memory-{}", std::process::id()));
        let cache = PreviewCache::new(root.clone()).expect("cache");
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 1, image::Rgba([12, 34, 56, 78]))
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("PNG bytes");
        let expected = cache
            .load_or_generate("first".into(), || Ok(png.into_inner()))
            .expect("generate preview");
        fs::remove_file(root.join("first.png"))
            .expect("remove owned disk entry to prove memory reuse");
        let shared = cache.cancellable(Cancellation::default());
        assert_eq!(
            shared
                .load_or_generate("first".into(), || panic!("must use shared pixels"))
                .expect("memory hit"),
            expected
        );
        assert!(matches!(
            shared.load_or_generate("other-variant".into(), || Err(PreviewError::NoFrame)),
            Err(PreviewError::NoFrame)
        ));
        fs::remove_dir(root).expect("remove empty owned cache");
    }

    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{ImageFormat, Rgb, RgbImage};

    use super::*;

    #[test]
    fn an_unavailable_cache_directory_does_not_block_preview_generation() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-preview-cache-directory-{unique}"));
        fs::create_dir(&root).expect("isolated fixture directory");
        let cache_path = root.join("cache");
        fs::write(&cache_path, b"preserve this file").expect("cache path collision");
        let cache = PreviewCache::new(cache_path.clone()).expect("optional cache storage");
        let source = root.join("source.png");
        RgbImage::from_pixel(32, 16, Rgb([20, 40, 60]))
            .save_with_format(&source, ImageFormat::Png)
            .expect("source fixture");
        let source_bytes = fs::read(&source).expect("original source");
        let preview = cache
            .filmstrip(&source, MediaKind::Image)
            .expect("generate without disk cache");
        assert_eq!((preview.image.width, preview.image.height), (240, 120));
        assert_eq!(
            fs::read(&cache_path).expect("collision"),
            b"preserve this file"
        );
        fs::remove_file(&cache_path).expect("remove own collision fixture");
        assert_eq!(
            cache
                .filmstrip(&source, MediaKind::Image)
                .expect("retry after cache path is available")
                .image,
            preview.image
        );
        let key = cache_key(&source, "filmstrip-image-v4").expect("cache key");
        assert!(cache_path.join(format!("{key}.png")).is_file());
        assert_eq!(fs::read(&source).expect("unchanged source"), source_bytes);
        fs::remove_dir_all(root).expect("remove isolated fixture");
    }

    #[test]
    fn a_busy_cache_file_does_not_discard_a_generated_preview() {
        use std::os::windows::fs::OpenOptionsExt;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-preview-busy-cache-{unique}"));
        let cache = PreviewCache::new(root.clone()).expect("cache");
        let mut png = std::io::Cursor::new(Vec::new());
        RgbImage::from_pixel(2, 2, Rgb([24, 48, 96]))
            .write_to(&mut png, ImageFormat::Png)
            .expect("fixture PNG");
        let bytes = png.into_inner();
        let expected = decode_png(&bytes).expect("fixture pixels");
        let temporary = root.join("busy.png");
        fs::write(&temporary, b"held cache file").expect("temporary fixture");
        let lock = fs::OpenOptions::new()
            .read(true)
            // A failed replacement must preserve the existing cache file.
            .share_mode(1)
            .open(&temporary)
            .expect("deny replacement while permitting read");
        let result = cache.load_or_generate("busy".into(), || Ok(bytes.clone()));
        assert_eq!(
            result.expect("cache storage is optional after successful generation"),
            expected
        );
        assert_eq!(fs::read(&temporary).expect("held file"), b"held cache file");
        drop(lock);
        fs::remove_file(&temporary).expect("release temporary fixture");
        assert_eq!(
            cache
                .load_or_generate("busy".into(), || Ok(bytes.clone()))
                .expect("retry cache storage"),
            expected
        );
        assert_eq!(
            cache
                .load_or_generate("busy".into(), || panic!("valid cache must be reused"))
                .expect("cache hit after retry"),
            expected
        );
        assert!(matches!(
            cache.load_or_generate("failed".into(), || Err(PreviewError::NoFrame)),
            Err(PreviewError::NoFrame)
        ));
        assert!(matches!(
            cache.load_or_generate("invalid".into(), || Ok(vec![0; 8])),
            Err(PreviewError::Decode(_))
        ));
        let cancellation = Cancellation::default();
        assert!(matches!(
            cache
                .cancellable(cancellation.clone())
                .load_or_generate("cancelled".into(), || {
                    cancellation.cancel();
                    Ok(bytes.clone())
                }),
            Err(PreviewError::Cancelled)
        ));
        assert!(!root.join("cancelled.png").exists());
        fs::remove_dir_all(root).expect("remove isolated cache fixture");
        assert_eq!(
            cache
                .load_or_generate("missing-root".into(), || Ok(bytes))
                .expect("a removed cache directory does not discard pixels"),
            expected
        );
        fs::remove_dir_all(&cache.root).expect("remove recreated cache fixture");
    }

    #[test]
    fn video_previews_use_the_last_selected_frame_beyond_its_end() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-preview-tail-{unique}"));
        let cache = PreviewCache::new(root.join("cache")).expect("cache");
        let source = root.join("tail.mp4");
        assert!(
            hidden_command(
                &tool_path("ffmpeg.exe").expect("fixed FFmpeg"),
                [
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=blue:s=160x96:r=5:d=12",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=s=160x96:r=5:d=1",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=sample_rate=48000:duration=12",
                    "-map",
                    "0:v",
                    "-map",
                    "1:v",
                    "-map",
                    "2:a",
                    "-c:v",
                    "libopenh264",
                    "-c:a",
                    "aac",
                    "-disposition:v:0",
                    "0",
                    "-disposition:v:1",
                    "default",
                ]
            )
            .arg(&source)
            .status()
            .expect("tail fixture")
            .success()
        );
        let last = cache
            .thumbnail(&source, Duration::from_millis(799), 160)
            .expect("last frame");
        for position in [Duration::from_secs(1), Duration::from_secs(10)] {
            let preview = cache
                .thumbnail(&source, position, 160)
                .expect("tail thumbnail");
            assert_eq!(preview, last);
            assert_eq!(
                cache
                    .thumbnail(&source, position, 160)
                    .expect("cached tail"),
                last
            );
        }
        let card = cache
            .filmstrip(&source, MediaKind::Video)
            .expect("tail filmstrip");
        assert_eq!((card.image.width, card.image.height), (240, 160));
        let mut args =
            preview_input_arguments(&source, Duration::from_millis(799), None).expect("input");
        args.extend(["-vf".into(), "scale=240:160:force_original_aspect_ratio=decrease:reset_sar=1,pad=240:160:(ow-iw)/2:(oh-ih)/2".into(), "-frames:v".into(), "1".into()]);
        assert_eq!(
            card.image,
            decode_png(&run_ffmpeg(&args, None).expect("reference card")).expect("PNG")
        );
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert!(matches!(
            cache
                .cancellable(cancellation)
                .thumbnail(&source, Duration::from_secs(11), 160),
            Err(PreviewError::Cancelled)
        ));
        let checks = std::sync::atomic::AtomicUsize::new(0);
        assert!(matches!(
            crate::decode::preview_last_video_time(&source, Duration::from_secs(10), &|| {
                checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 8
            }),
            Err(crate::DecodeError::ConsumerClosed)
        ));
        assert_eq!(checks.load(std::sync::atomic::Ordering::Relaxed), 9);
        fs::remove_dir_all(root).expect("remove preview fixture");
    }

    #[test]
    fn previews_use_the_playback_streams_not_the_first_streams() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-preview-streams-{unique}"));
        let cache = PreviewCache::new(root.join("cache")).expect("cache");
        let source = root.join("streams.mkv");
        let status = hidden_command(
            &tool_path("ffmpeg.exe").expect("fixed FFmpeg"),
            [
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=red:s=320x240:r=10:d=1",
                "-f",
                "lavfi",
                "-i",
                "color=blue:s=160x96:r=10:d=1",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=stereo:d=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:sample_rate=48000:duration=1",
                "-map",
                "0:v",
                "-map",
                "1:v",
                "-map",
                "2:a",
                "-map",
                "3:a",
                "-c:v",
                "libopenh264",
                "-c:a",
                "aac",
                "-disposition:v:0",
                "0",
                "-disposition:v:1",
                "default",
                "-disposition:a:0",
                "0",
                "-disposition:a:1",
                "default",
            ],
        )
        .arg(&source)
        .status()
        .expect("multistream fixture");
        assert!(status.success());
        let mut video_seen = false;
        let mut audible = false;
        crate::decode_file(&source, |output| {
            match output {
                crate::DecodeOutput::Video(frame) => {
                    assert_eq!((frame.width, frame.height), (160, 96));
                    video_seen = true;
                }
                crate::DecodeOutput::Audio(chunk) => {
                    audible |= chunk
                        .bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|bytes| f32::from_le_bytes(*bytes).abs() > 0.01);
                }
            }
            true
        })
        .expect("playback streams");
        assert!(video_seen && audible);
        let thumbnail = cache
            .thumbnail(&source, Duration::ZERO, 160)
            .expect("thumbnail");
        let card = cache
            .filmstrip(&source, MediaKind::Video)
            .expect("filmstrip");
        let waveform = cache.waveform(&source, 160, 96).expect("waveform");
        let blue = |image: &PreviewImage| {
            let center = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
            image.rgba[center] < 10 && image.rgba[center + 2] > 240
        };
        assert!(
            blue(&thumbnail),
            "thumbnail must match the blue playback stream"
        );
        assert_eq!((thumbnail.width, thumbnail.height), (160, 96));
        assert!(
            blue(&card.image),
            "filmstrip must match the blue playback stream"
        );
        assert!(
            waveform
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0),
            "waveform must match the audible playback stream"
        );
        fs::remove_dir_all(root).expect("remove owned multistream fixture");
    }

    #[test]
    fn streaming_waveform_matches_showwavespic_with_bounded_column_error() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-stream-waveform-{unique}"));
        let cache = PreviewCache::new(root.join("cache")).expect("cache");
        for (duration, width) in [("1.37", 127), ("61.37", 127)] {
            let source = root.join(format!("{duration}.wav"));
            let status = hidden_command(
                &tool_path("ffmpeg.exe").expect("fixed FFmpeg"),
                ["-v", "error", "-f", "lavfi", "-i"],
            )
            .arg(format!(
                "aevalsrc=0.7*sin(2*PI*440*t)*(0.5+0.5*sin(2*PI*0.37*t)):s=44100:d={duration}"
            ))
            .args(["-ac", "2", "-c:a", "pcm_s16le"])
            .arg(&source)
            .status()
            .expect("audio fixture");
            assert!(status.success());
            let filter =
                format!("aformat=channel_layouts=mono,showwavespic=s={width}x96:colors=white");
            let reference = run_ffmpeg(
                &[
                    "-i".into(),
                    source.display().to_string(),
                    "-filter_complex".into(),
                    filter,
                    "-frames:v".into(),
                    "1".into(),
                ],
                None,
            )
            .expect("reference waveform");
            let reference = decode_png(&reference).expect("reference PNG");
            let actual = cache
                .waveform(&source, width, 96)
                .expect("streaming waveform");
            assert_eq!((actual.width, actual.height), (width, 96));
            if duration == "1.37" {
                assert_eq!(actual.rgba, reference.rgba);
            }
            for x in 0..width as usize {
                let bar = |image: &PreviewImage| {
                    (0..96)
                        .filter(|y| image.rgba[(y * width as usize + x) * 4 + 3] != 0)
                        .count()
                };
                assert!(
                    bar(&actual).abs_diff(bar(&reference)) <= 1,
                    "duration {duration}, column {x}"
                );
            }
            assert_eq!(
                cache.waveform(&source, width, 96).expect("cache hit"),
                actual
            );
        }
        fs::remove_dir_all(root).expect("remove owned waveform fixtures");
    }

    #[test]
    fn cancelled_previews_stop_before_file_access_or_process_start() {
        let cancellation = Cancellation::default();
        cancellation.cancel();
        let cache = PreviewCache {
            in_flight: Arc::default(),
            root: PathBuf::from("must-not-create-cache"),
            cancellation: Some(cancellation),
            memory: Arc::default(),
        };
        let missing = Path::new("must-not-open-media.mp4");
        assert!(matches!(
            cache.duration(missing),
            Err(PreviewError::Cancelled)
        ));
        assert!(matches!(
            cache.thumbnail(missing, Duration::from_secs(1), 240),
            Err(PreviewError::Cancelled)
        ));
        assert!(matches!(
            cache.waveform(missing, 640, 96),
            Err(PreviewError::Cancelled)
        ));
        assert!(matches!(
            cache.filmstrip(missing, MediaKind::Video),
            Err(PreviewError::Cancelled)
        ));
    }

    #[test]
    fn duration_metadata_rejects_invalid_and_unrepresentable_values() {
        for text in [
            "",
            "duration=N/A",
            "duration=NaN",
            "duration=inf",
            "duration=-1",
            "duration=1e100",
            "format_name=matroska,webm\nstart_time=3\nduration=2",
            "format_name=matroska,webm\nstart_time=NaN\nduration=2",
        ] {
            assert!(probed_duration(text).is_err(), "{text}");
        }
        for text in [
            "format_name=matroska,webm\nstart_time=5\nduration=7",
            "format_name=matroska,webm\nstart_time=N/A\nduration=2",
            "format_name=mpegts\nstart_time=11.4\nduration=2",
            "format_name=mov,mp4,m4a,3gp,3g2,mj2\nstart_time=5\nduration=2",
        ] {
            assert_eq!(
                probed_duration(text).expect("valid duration"),
                Duration::from_secs(2)
            );
        }
    }

    #[test]
    fn duration_is_relative_to_input_origin_for_mkv_mp4_and_ts() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-origin-duration-{unique}"));
        let cache = PreviewCache::new(root.join("cache")).expect("cache");
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        for extension in ["mkv", "mp4", "ts"] {
            let mut durations = Vec::new();
            for offset in ["0", "5"] {
                let target = root.join(format!("offset-{offset}.{extension}"));
                let output = hidden_command(
                    &tool_path("ffmpeg.exe").expect("fixed FFmpeg"),
                    ["-v", "error", "-i"],
                )
                .arg(&source)
                .args(["-map", "0", "-c", "copy", "-output_ts_offset", offset])
                .arg(&target)
                .output()
                .expect("remux timestamp fixture");
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                durations.push(cache.duration(&target).expect("normalized duration"));
            }
            // MP4's zero-origin edit list can discard one 1,024-sample AAC priming packet.
            assert!(
                durations[0].abs_diff(durations[1]) < Duration::from_millis(22),
                "{extension}: {durations:?}"
            );
            assert!((2.0..2.1).contains(&durations[1].as_secs_f64()));
        }
        fs::remove_dir_all(root).expect("remove owned duration fixtures");
    }

    #[test]
    fn thumbnail_is_generated_and_reused_from_disk() {
        use std::os::windows::fs::OpenOptionsExt;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-preview-{unique}"));
        let source = root.join("source.png");
        fs::create_dir_all(&root).expect("create preview fixture directory");
        RgbImage::from_pixel(32, 16, Rgb([20, 40, 60]))
            .save_with_format(&source, ImageFormat::Png)
            .expect("write preview fixture");
        let cache = PreviewCache::new(root.join("cache")).expect("create cache");

        let generated = cache
            .thumbnail(&source, Duration::ZERO, 16)
            .expect("generate thumbnail");
        let cached = PreviewCache::new(cache.root.clone())
            .expect("fresh memory cache")
            .thumbnail(&source, Duration::ZERO, 16)
            .expect("read cached thumbnail");

        assert_eq!(generated, cached);
        assert_eq!((generated.width, generated.height), (16, 8));
        cache
            .load_or_generate(cache_key(&source, "filmstrip-v3").expect("old key"), || {
                frame_preview(&source, Duration::ZERO,
                    "scale=240:160:force_original_aspect_ratio=decrease:reset_sar=1,pad=240:160:(ow-iw)/2:(oh-ih)/2", None)
            })
            .expect("seed legacy padded cache");
        let key = cache_key(&source, "filmstrip-image-v4").expect("card key");
        let temporary = cache.root.join(format!("{key}.png"));
        fs::write(&temporary, b"held cache file").expect("temporary fixture");
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&temporary)
            .expect("deny cache writes and deletion");
        let card = cache
            .filmstrip(&source, MediaKind::Image)
            .expect("generated landscape card despite cache contention");
        assert_eq!(fs::read(&temporary).expect("held file"), b"held cache file");
        drop(lock);
        fs::remove_file(temporary).expect("release cache file");
        assert_eq!((card.image.width, card.image.height), (240, 120));
        assert!(card.duration.is_none());
        assert_eq!(
            card.image,
            cache
                .filmstrip(&source, MediaKind::Image)
                .expect("cache card after releasing contention")
                .image
        );
        assert!(
            cache
                .cached_image(&source)
                .expect("known thumbnail")
                .is_none(),
            "unknown source dimensions cannot be used as an original-size preview"
        );
        let decoded = crate::decode_image(&source).expect("original dimensions");
        cache.remember_image(&source, &decoded, &|| true);
        let upgraded = cache
            .cached_image(&source)
            .expect("lookup")
            .expect("upgraded source size");
        assert_eq!(upgraded.source_size, (32, 16));
        assert_eq!(
            upgraded.image, card.image,
            "reuse existing thumbnail pixels when adding dimensions"
        );
        assert!(
            card.image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[0] > 10 && pixel[3] == 255),
            "image card must not contain black padding"
        );
        RgbImage::from_pixel(16, 1024, Rgb([200, 30, 10]))
            .save_with_format(&source, ImageFormat::Png)
            .expect("replace with portrait");
        let portrait = cache
            .filmstrip(&source, MediaKind::Image)
            .expect("portrait card");
        assert_eq!((portrait.image.width, portrait.image.height), (3, 160));
        assert_ne!(
            card.image, portrait.image,
            "changed metadata invalidates disk preview"
        );
        assert_eq!(&portrait.image.rgba[..4], &[200, 30, 10, 255]);
        fs::remove_dir_all(root).expect("remove preview fixture");
    }

    #[test]
    fn waveform_and_duration_are_generated_for_audio() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-waveform-{unique}"));
        let source = root.join("source.wav");
        fs::create_dir_all(&root).expect("create waveform fixture directory");
        let status = hidden_command(
            &tool_path("ffmpeg.exe").expect("fixed FFmpeg"),
            [
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=0.25",
                &source.display().to_string(),
            ],
        )
        .status()
        .expect("start FFmpeg fixture generation");
        assert!(status.success(), "generate waveform fixture");
        let cache = PreviewCache::new(root.join("cache")).expect("create cache");

        let waveform = cache.waveform(&source, 128, 32).expect("generate waveform");
        let duration = cache.duration(&source).expect("probe duration");

        assert_eq!((waveform.width, waveform.height), (128, 32));
        assert!(duration >= Duration::from_millis(200));
        let card = cache
            .filmstrip(&source, MediaKind::Audio)
            .expect("audio card");
        assert_eq!((card.image.width, card.image.height), (240, 160));
        assert_eq!(card.duration, Some(duration));
        fs::remove_dir_all(root).expect("remove waveform fixture");
    }

    #[test]
    fn adjacent_helpers_work_without_development_paths_and_never_mix_locations() {
        const CHILD: &str = "TOWAVUE_ADJACENT_HELPER_FIXTURE";
        if let Some(mode) = std::env::var_os(CHILD) {
            let executable = std::env::current_exe().expect("child executable");
            let directory = executable.parent().expect("child directory");
            let cache =
                PreviewCache::new(directory.join(format!("cache-{}", mode.to_string_lossy())))
                    .expect("isolated preview cache");
            let source =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
            if mode == "missing-probe" {
                assert!(
                    cache.duration(&source).is_err(),
                    "must not use development FFprobe"
                );
                return;
            }
            let target = directory.join(format!("export-{}.mp4", mode.to_string_lossy()));
            let request = crate::ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind: MediaKind::Video,
                operations: Vec::new(),
                hardware_encode: false,
            };
            if mode == "missing-both" {
                assert!(
                    cache.duration(&source).is_err(),
                    "must not use PATH FFprobe"
                );
                assert!(
                    cache.waveform(&source, 64, 16).is_err(),
                    "must not use PATH FFmpeg"
                );
                fs::write(&target, b"existing user output").expect("existing target");
                assert!(
                    crate::export_media(&request).is_err(),
                    "export must not use PATH FFmpeg"
                );
                assert_eq!(
                    fs::read(&target).expect("preserved target"),
                    b"existing user output"
                );
                return;
            }
            assert!(cache.duration(&source).expect("adjacent FFprobe") > Duration::ZERO);
            assert!(
                cache
                    .thumbnail(&source, Duration::ZERO, 64)
                    .expect("adjacent thumbnail")
                    .width
                    > 0
            );
            let waveform = cache.waveform(&source, 64, 16).expect("adjacent waveform");
            assert_eq!((waveform.width, waveform.height), (64, 16));
            crate::export_media(&request).expect("adjacent export");
            crate::decode_file(&target, |_| true).expect("reopen exported media");
            return;
        }

        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-adjacent-{unique}"));
        let application = root.join("日本語 viewer & tools");
        let unrelated = root.join("unrelated working directory");
        let decoy = root.join("other FFmpeg");
        fs::create_dir_all(&application).expect("application directory");
        fs::create_dir_all(&unrelated).expect("unrelated directory");
        fs::create_dir_all(decoy.join("bin")).expect("decoy directory");
        for name in ["ffmpeg.exe", "ffprobe.exe"] {
            fs::write(decoy.join("bin").join(name), b"not an executable").expect("decoy helper");
        }
        let development = PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"));
        for entry in fs::read_dir(development.join("bin")).expect("runtime directory") {
            let entry = entry.expect("runtime file");
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
                || entry.file_name() == "ffmpeg.exe"
                || entry.file_name() == "ffprobe.exe"
            {
                fs::copy(&path, application.join(entry.file_name()))
                    .expect("isolated runtime copy");
            }
        }
        let executable = application.join("towavue-helper-fixture.exe");
        fs::copy(
            std::env::current_exe().expect("test executable"),
            &executable,
        )
        .expect("child copy");
        let system =
            PathBuf::from(std::env::var_os("SystemRoot").expect("Windows root")).join("System32");
        for mode in [
            "no-environment",
            "conflicting-environment",
            "missing-probe",
            "missing-both",
        ] {
            if mode == "missing-probe" {
                fs::rename(
                    application.join("ffprobe.exe"),
                    root.join("saved-ffprobe.exe"),
                )
                .expect("remove probe from fixture");
            } else if mode == "missing-both" {
                fs::rename(
                    application.join("ffmpeg.exe"),
                    root.join("saved-ffmpeg.exe"),
                )
                .expect("remove encoder from fixture");
            }
            let mut command = Command::new(&executable);
            command.args(["--exact", "preview::tests::adjacent_helpers_work_without_development_paths_and_never_mix_locations", "--nocapture"])
                .current_dir(&unrelated).env(CHILD, mode).env_remove("FFMPEG_DIR").env("PATH", &system);
            match mode {
                "conflicting-environment" => {
                    command.env("FFMPEG_DIR", &decoy);
                }
                "missing-probe" => {
                    command.env("FFMPEG_DIR", &development);
                }
                "missing-both" => {
                    command.env(
                        "PATH",
                        std::env::join_paths([development.join("bin"), system.clone()])
                            .expect("child PATH"),
                    );
                }
                _ => {}
            }
            <Command as std::os::windows::process::CommandExt>::creation_flags(
                &mut command,
                CREATE_NO_WINDOW,
            );
            let output = command.output().expect("start isolated helper fixture");
            assert!(
                output.status.success(),
                "{mode}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        fs::remove_dir_all(root).expect("remove isolated helper fixture");
    }
}
