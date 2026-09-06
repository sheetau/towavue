use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use thiserror::Error;
use towavue_core::MediaKind;

use crate::Cancellation;

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
        Ok(Self {
            root,
            cancellation: None,
        })
    }

    pub fn cancellable(&self, cancellation: Cancellation) -> Self {
        Self {
            root: self.root.clone(),
            cancellation: Some(cancellation),
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
            let key = cache_key(source, "filmstrip-v3")?;
            self.load_or_generate(key, || {
                let position = duration.unwrap_or_default().mul_f64(0.1);
                frame_preview(source, position,
                    "scale=240:160:force_original_aspect_ratio=decrease:reset_sar=1,pad=240:160:(ow-iw)/2:(oh-ih)/2",
                    self.cancellation.as_ref())
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
                &tool_path("ffmpeg.exe"),
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
        let executable = tool_path("ffprobe.exe");
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
        let path = self.root.join(format!("{key}.png"));
        if let Ok(bytes) = fs::read(&path)
            && let Ok(image) = decode_png(&bytes)
        {
            self.check_cancelled()?;
            return Ok(image);
        }
        let bytes = generate()?;
        let image = decode_png(&bytes)?;
        self.check_cancelled()?;
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
    let executable = tool_path("ffmpeg.exe");
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

fn tool_path(name: &str) -> PathBuf {
    std::env::var_os("FFMPEG_DIR")
        .map(PathBuf::from)
        .map(|directory| directory.join("bin").join(name))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{ImageFormat, Rgb, RgbImage};

    use super::*;

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
                &tool_path("ffmpeg.exe"),
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
            &tool_path("ffmpeg.exe"),
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
                &tool_path("ffmpeg.exe"),
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
            root: PathBuf::from("must-not-create-cache"),
            cancellation: Some(cancellation),
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
