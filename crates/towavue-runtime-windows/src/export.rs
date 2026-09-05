use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use thiserror::Error;
use towavue_core::{EditOperation, EditState, MediaKind};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug)]
pub struct ExportRequest {
    pub source: PathBuf,
    pub target: PathBuf,
    pub kind: MediaKind,
    pub operations: Vec<EditOperation>,
    pub hardware_encode: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportOutcome {
    pub used_hardware_encoder: bool,
}

#[derive(Debug)]
pub enum ExportEvent {
    Progress(Duration),
    Finished(Result<ExportOutcome, ExportError>),
}

/// Owns one export worker; dropping it cancels and reaps its FFmpeg process.
pub struct ExportJob {
    cancelled: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ExportJob {
    pub fn start(
        request: ExportRequest,
        notify: impl Fn(ExportEvent) + Send + Sync + 'static,
    ) -> Result<Self, ExportError> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let thread = thread::Builder::new()
            .name("towavue-export".into())
            .spawn(move || {
                let result = export_cancellable(&request, &worker_cancelled, &|time| {
                    notify(ExportEvent::Progress(time));
                });
                notify(ExportEvent::Finished(result));
            })
            .map_err(ExportError::Start)?;
        Ok(Self {
            cancelled,
            thread: Some(thread),
        })
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Drop for ExportJob {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("export target must differ from the source path")]
    SameAsSource,
    #[error("trim start must be earlier than trim end")]
    InvalidTrim,
    #[error("could not start FFmpeg export: {0}")]
    Start(#[source] std::io::Error),
    #[error("FFmpeg export failed: {0}")]
    Failed(String),
    #[error("export cancelled; existing files were not changed")]
    Cancelled,
    #[error("could not prepare or publish export: {0}")]
    Output(#[source] std::io::Error),
}

pub fn export_media(request: &ExportRequest) -> Result<ExportOutcome, ExportError> {
    export_cancellable(request, &AtomicBool::new(false), &|_| {})
}

fn export_cancellable(
    request: &ExportRequest,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
) -> Result<ExportOutcome, ExportError> {
    if same_path(&request.source, &request.target) {
        return Err(ExportError::SameAsSource);
    }
    let state = EditState::from_operations(&request.operations);
    if state.trim_start.is_some() && state.trim_end.is_some() && state.valid_trim().is_none() {
        return Err(ExportError::InvalidTrim);
    }
    check_cancelled(cancelled)?;
    let staging = StagedExport::new(&request.target)?;
    let staged_request = ExportRequest {
        target: staging.output.clone(),
        ..request.clone()
    };
    let executable = std::env::var_os("FFMPEG_DIR")
        .map(PathBuf::from)
        .map(|directory| directory.join("bin").join("ffmpeg.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ffmpeg.exe"));
    if request.hardware_encode && hardware_encode_supported_target(request) {
        let hardware = run_ffmpeg(
            &executable,
            ffmpeg_arguments(&staged_request, true),
            cancelled,
            progress,
        )?;
        if hardware.status.success() {
            staging.publish(&request.target, cancelled)?;
            return Ok(ExportOutcome {
                used_hardware_encoder: true,
            });
        }
    }
    let output = run_ffmpeg(
        &executable,
        ffmpeg_arguments(&staged_request, false),
        cancelled,
        progress,
    )?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(ExportError::Failed(if message.is_empty() {
            format!("process exited with {}", output.status)
        } else {
            message
        }));
    }
    staging.publish(&request.target, cancelled)?;
    Ok(ExportOutcome {
        used_hardware_encoder: false,
    })
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), ExportError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(ExportError::Cancelled)
    } else {
        Ok(())
    }
}

struct StagedExport {
    directory: PathBuf,
    output: PathBuf,
}

impl StagedExport {
    fn new(target: &Path) -> Result<Self, ExportError> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let target = std::path::absolute(target).map_err(ExportError::Output)?;
        let parent = target
            .parent()
            .ok_or_else(|| ExportError::Failed("export target has no parent directory".into()))?;
        loop {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let directory = parent.join(format!(".towavue-export-{}-{id}", std::process::id()));
            match fs::create_dir(&directory) {
                Ok(()) => {
                    // A fixed basename prevents image-sequence expansion in user filenames.
                    let mut output = directory.join("output");
                    if let Some(extension) = target.extension() {
                        output.set_extension(extension);
                    }
                    return Ok(Self { directory, output });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(ExportError::Output(error)),
            }
        }
    }

    fn publish(&self, target: &Path, cancelled: &AtomicBool) -> Result<(), ExportError> {
        check_cancelled(cancelled)?;
        let output = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.output)
            .map_err(ExportError::Output)?;
        if output.metadata().map_err(ExportError::Output)?.len() == 0 {
            return Err(ExportError::Failed("encoder produced an empty file".into()));
        }
        output.sync_all().map_err(ExportError::Output)?;
        drop(output);
        check_cancelled(cancelled)?;
        // Both paths are on the same filesystem; replacement happens only after encoding succeeds.
        fs::rename(&self.output, target).map_err(ExportError::Output)
    }
}

impl Drop for StagedExport {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.output);
        let _ = fs::remove_dir(&self.directory);
    }
}

fn hardware_encode_supported_target(request: &ExportRequest) -> bool {
    request.kind == MediaKind::Video
        && request
            .target
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("webm"))
}

fn run_ffmpeg(
    executable: &Path,
    arguments: Vec<String>,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
) -> Result<std::process::Output, ExportError> {
    check_cancelled(cancelled)?;
    progress(Duration::ZERO);
    let mut child = <Command as std::os::windows::process::CommandExt>::creation_flags(
        Command::new(executable).args(arguments),
        CREATE_NO_WINDOW,
    )
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(ExportError::Start)?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    thread::scope(|scope| {
        scope.spawn(|| {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(time) = line
                    .strip_prefix("out_time_us=")
                    .and_then(|value| value.parse::<u64>().ok())
                {
                    progress(Duration::from_micros(time));
                }
            }
        });
        let diagnostics = scope.spawn(|| {
            let mut stderr = stderr;
            let mut tail = Vec::new();
            let mut buffer = [0; 4096];
            while let Ok(count) = stderr.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                tail.extend_from_slice(&buffer[..count]);
                if tail.len() > 16_384 {
                    tail.drain(..tail.len() - 16_384);
                }
            }
            tail
        });
        let status = loop {
            if let Err(error) = check_cancelled(cancelled) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ExportError::Start(error));
                }
            }
        };
        Ok(std::process::Output {
            status,
            stdout: Vec::new(),
            stderr: diagnostics.join().expect("diagnostic reader completed"),
        })
    })
}

fn ffmpeg_arguments(request: &ExportRequest, hardware: bool) -> Vec<String> {
    let state = EditState::from_operations(&request.operations);
    let mut arguments = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-nostdin".into(),
        "-nostats".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-i".into(),
        request.source.display().to_string(),
        "-map_metadata".into(),
        "0".into(),
    ];
    let visual = visual_filters(&request.operations);
    match request.kind {
        MediaKind::Image => {
            if !visual.is_empty() {
                arguments.extend(["-vf".into(), visual.join(",")]);
            }
            arguments.extend(["-frames:v".into(), "1".into()]);
        }
        MediaKind::Video => {
            let video = video_filters(visual, &state);
            if !video.is_empty() {
                arguments.extend(["-vf".into(), video.join(",")]);
            }
            let audio = audio_filters(&state);
            if !audio.is_empty() {
                arguments.extend(["-af".into(), audio.join(",")]);
            }
        }
        MediaKind::Audio => {
            let audio = audio_filters(&state);
            if !audio.is_empty() {
                arguments.extend(["-af".into(), audio.join(",")]);
            }
        }
    }
    arguments.extend(codec_arguments(request, hardware));
    arguments.push(request.target.display().to_string());
    arguments
}

fn codec_arguments(request: &ExportRequest, hardware: bool) -> Vec<String> {
    let extension = request
        .target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let codecs: &[&str] = match (request.kind, extension.as_str(), hardware) {
        (MediaKind::Video, _, true) => &["-c:v", "h264_mf", "-hw_encoding", "1", "-c:a", "aac"],
        (MediaKind::Video, "webm", false) => &["-c:v", "libvpx-vp9", "-c:a", "libopus"],
        (MediaKind::Video, "avi", false) => &["-c:v", "mpeg4", "-c:a", "pcm_s16le"],
        (MediaKind::Video, "wmv", false) => &["-c:v", "wmv2", "-c:a", "wmav2"],
        (MediaKind::Video, _, false) => &["-c:v", "libopenh264", "-c:a", "aac"],
        (MediaKind::Audio, "mp3", _) => &["-c:a", "libmp3lame"],
        (MediaKind::Audio, "ogg" | "opus", _) => &["-c:a", "libopus"],
        (MediaKind::Audio, "wav", _) => &["-c:a", "pcm_s16le"],
        (MediaKind::Audio, "flac", _) => &["-c:a", "flac"],
        (MediaKind::Audio, _, _) => &["-c:a", "aac"],
        (MediaKind::Image, "avif", _) => &["-c:v", "libaom-av1", "-still-picture", "1"],
        (MediaKind::Image, "webp", _) => &["-c:v", "libwebp"],
        (MediaKind::Image, "jpg" | "jpeg", _) => &["-c:v", "mjpeg"],
        (MediaKind::Image, "png", _) => &["-c:v", "png"],
        (MediaKind::Image, "gif", _) => &["-c:v", "gif"],
        (MediaKind::Image, "tif" | "tiff", _) => &["-c:v", "tiff"],
        (MediaKind::Image, "bmp", _) => &["-c:v", "bmp"],
        (MediaKind::Image, _, _) => &[],
    };
    codecs.iter().map(|argument| (*argument).into()).collect()
}

fn visual_filters(operations: &[EditOperation]) -> Vec<String> {
    operations
        .iter()
        .filter_map(|operation| match *operation {
            EditOperation::Crop(region) => Some(format!(
                "crop=iw*{:.8}:ih*{:.8}:iw*{:.8}:ih*{:.8}",
                region.width(),
                region.height(),
                region.min.x,
                region.min.y
            )),
            EditOperation::RotateClockwise => Some("transpose=clock".into()),
            EditOperation::RotateCounterclockwise => Some("transpose=cclock".into()),
            EditOperation::FlipHorizontal => Some("hflip".into()),
            EditOperation::FlipVertical => Some("vflip".into()),
            EditOperation::SetTrimStart(_)
            | EditOperation::SetTrimEnd(_)
            | EditOperation::SetVolume(_)
            | EditOperation::SetRate(_) => None,
        })
        .collect()
}

fn video_filters(mut filters: Vec<String>, state: &EditState) -> Vec<String> {
    if let Some(trim) = trim_filter("trim", state) {
        filters.push(trim);
    }
    if state.trim_start.is_some() || state.trim_end.is_some() || state.rate != 1.0 {
        filters.push(if state.rate == 1.0 {
            "setpts=PTS-STARTPTS".into()
        } else {
            format!("setpts=(PTS-STARTPTS)/{:.4}", state.rate)
        });
    }
    filters
}

fn audio_filters(state: &EditState) -> Vec<String> {
    let mut filters = Vec::new();
    if let Some(trim) = trim_filter("atrim", state) {
        filters.push(trim);
        filters.push("asetpts=PTS-STARTPTS".into());
    }
    if state.rate != 1.0 {
        filters.extend(atempo_filters(state.rate));
    }
    if state.volume != 1.0 {
        filters.push(format!("volume={:.4}", state.volume));
    }
    filters
}

fn trim_filter(name: &str, state: &EditState) -> Option<String> {
    match (state.trim_start, state.trim_end) {
        (Some(start), Some(end)) if start < end => Some(format!(
            "{name}=start={:.6}:end={:.6}",
            start.as_seconds_f64(),
            end.as_seconds_f64()
        )),
        (Some(start), None) => Some(format!("{name}=start={:.6}", start.as_seconds_f64())),
        (None, Some(end)) => Some(format!("{name}=end={:.6}", end.as_seconds_f64())),
        _ => None,
    }
}

fn atempo_filters(mut rate: f32) -> Vec<String> {
    let mut filters = Vec::new();
    while rate < 0.5 {
        filters.push("atempo=0.5000".into());
        rate /= 0.5;
    }
    while rate > 2.0 {
        filters.push("atempo=2.0000".into());
        rate /= 2.0;
    }
    filters.push(format!("atempo={rate:.4}"));
    filters
}

fn same_path(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        path.canonicalize()
            .unwrap_or_else(|_| path.to_owned())
            .to_string_lossy()
            .to_lowercase()
    };
    normalize(left) == normalize(right)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{ImageFormat, Rgb, RgbImage};
    use towavue_core::{MediaTime, UnitPoint, UnitRect};

    use super::*;

    #[test]
    fn image_export_preserves_operation_order() {
        let request = ExportRequest {
            source: "in.png".into(),
            target: "out.png".into(),
            kind: MediaKind::Image,
            operations: vec![
                EditOperation::Crop(UnitRect {
                    min: UnitPoint { x: 0.1, y: 0.2 },
                    max: UnitPoint { x: 0.6, y: 0.8 },
                }),
                EditOperation::RotateClockwise,
                EditOperation::FlipHorizontal,
            ],
            hardware_encode: false,
        };

        let arguments = ffmpeg_arguments(&request, false);
        let filter = arguments
            .windows(2)
            .find_map(|pair| (pair[0] == "-vf").then_some(pair[1].as_str()))
            .expect("video filter argument");

        assert_eq!(
            filter,
            "crop=iw*0.50000000:ih*0.60000002:iw*0.10000000:ih*0.20000000,transpose=clock,hflip"
        );
    }

    #[test]
    fn video_export_builds_trim_rate_and_volume_filters() {
        let request = ExportRequest {
            source: "in.mp4".into(),
            target: "out.mp4".into(),
            kind: MediaKind::Video,
            operations: vec![
                EditOperation::SetTrimStart(MediaTime::from_nanoseconds(2_000_000_000)),
                EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(5_000_000_000)),
                EditOperation::SetRate(4.0),
                EditOperation::SetVolume(0.5),
            ],
            hardware_encode: false,
        };

        let arguments = ffmpeg_arguments(&request, false).join(" ");

        assert!(
            arguments.contains("trim=start=2.000000:end=5.000000,setpts=(PTS-STARTPTS)/4.0000")
        );
        assert!(arguments.contains("atrim=start=2.000000:end=5.000000,asetpts=PTS-STARTPTS,atempo=2.0000,atempo=2.0000,volume=0.5000"));
    }

    #[test]
    fn hardware_request_forces_media_foundation_hardware_mode() {
        let request = ExportRequest {
            source: "in.mp4".into(),
            target: "out.mp4".into(),
            kind: MediaKind::Video,
            operations: Vec::new(),
            hardware_encode: true,
        };

        let arguments = ffmpeg_arguments(&request, true).join(" ");

        assert!(arguments.contains("-c:v h264_mf -hw_encoding 1"));
    }

    #[test]
    fn source_cannot_be_its_own_export_target() {
        let request = ExportRequest {
            source: "same.wav".into(),
            target: "same.wav".into(),
            kind: MediaKind::Audio,
            operations: Vec::new(),
            hardware_encode: false,
        };

        assert!(matches!(
            export_media(&request),
            Err(ExportError::SameAsSource)
        ));
    }

    #[test]
    fn failed_encode_preserves_existing_target() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("towavue-failed-export-{unique}"));
        fs::create_dir(&directory).expect("create fixture directory");
        let source = directory.join("source.png");
        let target = directory.join("target.png");
        RgbImage::from_pixel(40, 30, Rgb([20, 40, 60]))
            .save(&source)
            .expect("source");
        fs::write(&target, b"previous export").expect("existing target");
        let result = export_media(&ExportRequest {
            source,
            target: target.clone(),
            kind: MediaKind::Image,
            operations: vec![EditOperation::Crop(UnitRect {
                min: UnitPoint { x: 0.0, y: 0.0 },
                max: UnitPoint { x: 0.001, y: 0.001 },
            })],
            hardware_encode: false,
        });
        assert!(result.is_err(), "zero-pixel crop must fail");
        let contents = fs::read(&target).expect("target remains");
        fs::remove_dir_all(&directory).expect("remove fixture directory");
        assert_eq!(contents, b"previous export");
    }

    #[test]
    fn ffmpeg_exports_image_edits_without_changing_source() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let source = std::env::temp_dir().join(format!("towavue-export-{unique}-source.png"));
        let target = std::env::temp_dir().join(format!("towavue-export-{unique}-target.png"));
        RgbImage::from_pixel(40, 30, Rgb([20, 40, 60]))
            .save_with_format(&source, ImageFormat::Png)
            .expect("write export fixture");
        fs::write(&target, b"previous export").expect("existing target");
        let request = ExportRequest {
            source: source.clone(),
            target: target.clone(),
            kind: MediaKind::Image,
            operations: vec![
                EditOperation::Crop(UnitRect {
                    min: UnitPoint { x: 0.0, y: 0.0 },
                    max: UnitPoint { x: 0.5, y: 1.0 },
                }),
                EditOperation::RotateClockwise,
            ],
            hardware_encode: false,
        };

        export_media(&request).expect("export edited image");
        let exported = crate::decode_image(&target).expect("decode exported image");
        let original = crate::decode_image(&source).expect("decode original image");
        fs::remove_file(source).expect("remove source fixture");
        fs::remove_file(target).expect("remove target fixture");

        assert_eq!(original.dimensions(), (40, 30));
        assert_eq!(exported.dimensions(), (30, 20));
    }

    #[test]
    fn cancelling_background_export_keeps_files_and_cleans_staging() {
        use std::sync::{Mutex, mpsc};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("towavue-cancel-export-{unique}"));
        fs::create_dir(&directory).expect("fixture directory");
        let source = directory.join("source.png");
        let target = directory.join("target.png");
        RgbImage::from_pixel(40, 30, Rgb([20, 40, 60]))
            .save(&source)
            .expect("source");
        let original = fs::read(&source).expect("source bytes");
        fs::write(&target, b"previous export").expect("target");
        let (tx, rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let resume_rx = Mutex::new(resume_rx);
        let observed = AtomicBool::new(false);
        let job = ExportJob::start(
            ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind: MediaKind::Image,
                operations: vec![EditOperation::RotateClockwise],
                hardware_encode: false,
            },
            move |event| match event {
                ExportEvent::Progress(time)
                    if !time.is_zero() && !observed.swap(true, Ordering::Relaxed) =>
                {
                    tx.send(ExportEvent::Progress(time))
                        .expect("progress receiver");
                    resume_rx
                        .lock()
                        .expect("resume receiver")
                        .recv_timeout(Duration::from_secs(10))
                        .expect("resume progress callback");
                }
                ExportEvent::Finished(_) => {
                    tx.send(event).expect("completion receiver");
                }
                _ => {}
            },
        )
        .expect("start background export");
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(10)),
            Ok(ExportEvent::Progress(_))
        ));
        job.cancel();
        resume_tx.send(()).expect("resume progress callback");
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(10)),
            Ok(ExportEvent::Finished(Err(ExportError::Cancelled)))
        ));
        drop(job);
        assert_eq!(fs::read(&target).expect("target bytes"), b"previous export");
        assert_eq!(fs::read(&source).expect("source bytes"), original);
        assert_eq!(
            fs::read_dir(&directory).expect("fixture entries").count(),
            2
        );
        fs::remove_dir_all(directory).expect("remove fixture directory");
    }

    #[test]
    fn failed_publish_preserves_existing_target_and_removes_staging() {
        use std::os::windows::fs::OpenOptionsExt;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("towavue-locked-export-{unique}"));
        fs::create_dir(&directory).expect("fixture directory");
        let target = directory.join("target.png");
        fs::write(&target, b"previous export").expect("target");
        let staging = StagedExport::new(&target).expect("stage output");
        fs::write(&staging.output, b"completed encode").expect("encoded file");
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .expect("lock target");
        assert!(matches!(
            staging.publish(&target, &AtomicBool::new(false)),
            Err(ExportError::Output(_))
        ));
        drop(staging);
        drop(locked);
        assert_eq!(fs::read(&target).expect("target bytes"), b"previous export");
        assert_eq!(
            fs::read_dir(&directory).expect("fixture entries").count(),
            1
        );
        fs::remove_dir_all(directory).expect("remove fixture directory");
    }
}
