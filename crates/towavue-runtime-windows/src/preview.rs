use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use thiserror::Error;
use towavue_core::MediaKind;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CACHE_LIMIT_BYTES: u64 = 64 * 1024 * 1024;

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
    #[error("preview seek preparation failed: {0}")]
    Seek(#[from] crate::DecodeError),
    #[error("FFprobe returned an invalid duration")]
    InvalidDuration,
    #[error("LOCALAPPDATA is unavailable")]
    NoLocalAppData,
}

#[derive(Clone)]
pub struct PreviewCache {
    root: PathBuf,
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
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn thumbnail(
        &self,
        source: &Path,
        position: Duration,
        width: u32,
    ) -> Result<PreviewImage, PreviewError> {
        let key = cache_key(
            source,
            &format!("thumbnail-v2-{}-{width}", position.as_millis()),
        )?;
        self.load_or_generate(key, || {
            let mut arguments = preview_input_arguments(source, position)?;
            arguments.extend([
                "-map".into(),
                "0:v:0".into(),
                "-vf".into(),
                format!("scale={width}:-2:force_original_aspect_ratio=decrease"),
                "-frames:v".into(),
                "1".into(),
            ]);
            run_ffmpeg(&arguments)
        })
    }

    pub fn filmstrip(&self, source: &Path, kind: MediaKind) -> Result<MediaPreview, PreviewError> {
        let duration = (kind != MediaKind::Image)
            .then(|| self.duration(source).ok())
            .flatten();
        let image = if kind == MediaKind::Audio {
            self.waveform(source, 240, 160)?
        } else {
            let key = cache_key(source, "filmstrip-v2")?;
            self.load_or_generate(key, || {
                let position = duration.unwrap_or_default().mul_f64(0.1);
                let mut arguments = preview_input_arguments(source, position)?;
                arguments.extend([
                    "-map".into(),
                    "0:v:0".into(),
                    "-vf".into(),
                    "scale=240:160:force_original_aspect_ratio=decrease:reset_sar=1,pad=240:160:(ow-iw)/2:(oh-ih)/2".into(),
                    "-frames:v".into(),
                    "1".into(),
                ]);
                run_ffmpeg(&arguments)
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
        let key = cache_key(source, &format!("waveform-{width}-{height}"))?;
        self.load_or_generate(key, || {
            run_ffmpeg(&[
                "-i".into(),
                source.display().to_string(),
                "-filter_complex".into(),
                format!(
                    "[0:a:0]aformat=channel_layouts=mono,showwavespic=s={width}x{height}:colors=white[wave]"
                ),
                "-map".into(),
                "[wave]".into(),
                "-frames:v".into(),
                "1".into(),
            ])
        })
    }

    pub fn duration(&self, source: &Path) -> Result<Duration, PreviewError> {
        let executable = tool_path("ffprobe.exe");
        let output = hidden_command(
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
        )
        .output()
        .map_err(|source| PreviewError::Start {
            program: "FFprobe",
            source,
        })?;
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
        let path = self.root.join(format!("{key}.png"));
        if let Ok(bytes) = fs::read(&path)
            && let Ok(image) = decode_png(&bytes)
        {
            return Ok(image);
        }
        let bytes = generate()?;
        let image = decode_png(&bytes)?;
        let temporary = self.root.join(format!("{key}.{}.tmp", std::process::id()));
        fs::write(&temporary, &bytes)?;
        if let Err(error) = fs::rename(&temporary, &path) {
            let _ = fs::remove_file(&temporary);
            if !path.exists() {
                return Err(error.into());
            }
        }
        self.prune()?;
        Ok(image)
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

fn run_ffmpeg(arguments: &[String]) -> Result<Vec<u8>, PreviewError> {
    let executable = tool_path("ffmpeg.exe");
    let output = hidden_command(
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
    )
    .output()
    .map_err(|source| PreviewError::Start {
        program: "FFmpeg",
        source,
    })?;
    if !output.status.success() || output.stdout.is_empty() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(PreviewError::Generate(if message.is_empty() {
            format!("process exited with {}", output.status)
        } else {
            message
        }));
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

fn tool_path(name: &str) -> PathBuf {
    std::env::var_os("FFMPEG_DIR")
        .map(PathBuf::from)
        .map(|directory| directory.join("bin").join(name))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn preview_input_arguments(source: &Path, position: Duration) -> Result<Vec<String>, PreviewError> {
    let start = if position.is_zero() || MediaKind::from_path(source) == Some(MediaKind::Image) {
        position
    } else {
        crate::decode::preview_seek_start(source, position)?
    };
    let mut arguments = vec![
        "-ss".into(),
        format!("{:.6}", start.as_secs_f64()),
        "-i".into(),
        source.display().to_string(),
    ];
    if start < position {
        arguments.extend([
            "-ss".into(),
            format!("{:.6}", (position - start).as_secs_f64()),
        ]);
    }
    Ok(arguments)
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{ImageFormat, Rgb, RgbImage};

    use super::*;

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
                let output = hidden_command(&tool_path("ffmpeg.exe"), ["-v", "error", "-i"])
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
        let cached = cache
            .thumbnail(&source, Duration::ZERO, 16)
            .expect("read cached thumbnail");

        assert_eq!(generated, cached);
        assert_eq!((generated.width, generated.height), (16, 8));
        let card = cache
            .filmstrip(&source, MediaKind::Image)
            .expect("landscape card");
        assert_eq!((card.image.width, card.image.height), (240, 160));
        assert!(card.duration.is_none());
        RgbImage::from_pixel(16, 1024, Rgb([200, 30, 10]))
            .save_with_format(&source, ImageFormat::Png)
            .expect("replace with portrait");
        let portrait = cache
            .filmstrip(&source, MediaKind::Image)
            .expect("portrait card");
        assert_eq!((portrait.image.width, portrait.image.height), (240, 160));
        assert_ne!(
            card.image, portrait.image,
            "changed metadata invalidates disk preview"
        );
        assert_eq!(&portrait.image.rgba[..4], &[0, 0, 0, 255]);
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
            &tool_path("ffmpeg.exe"),
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
}
