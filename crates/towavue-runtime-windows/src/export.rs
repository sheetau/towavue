use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ffmpeg_next as ffmpeg;
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
    #[error("trim times must be non-negative and start must be earlier than end")]
    InvalidTrim,
    #[error("timeline edits require a known duration and valid nonempty source intervals")]
    InvalidTimeline,
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
    if request.kind != MediaKind::Image
        && request
            .operations
            .iter()
            .any(|operation| matches!(operation, EditOperation::RotateImage(_)))
    {
        return Err(ExportError::Failed(
            "Free rotation currently supports images only".into(),
        ));
    }
    let state = EditState::from_operations(&request.operations);
    if !state.trim_is_valid(None) {
        return Err(ExportError::InvalidTrim);
    }
    check_cancelled(cancelled)?;
    let mut streams = ExportStreams::probe(request)?;
    if request
        .operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::Timeline(_)))
    {
        if request.kind == MediaKind::Image {
            return Err(ExportError::InvalidTimeline);
        }
        streams.timeline = Some(
            towavue_core::EditTimeline::from_operations(
                streams.duration.ok_or(ExportError::InvalidTimeline)?,
                &request.operations,
            )
            .ok_or(ExportError::InvalidTimeline)?,
        );
        if streams
            .timeline
            .as_ref()
            .is_some_and(|timeline| timeline.spans().is_empty())
        {
            return Err(ExportError::InvalidTimeline);
        }
    }
    let trimmed_kind =
        (state.trim_start.is_some() || state.trim_end.is_some() || streams.timeline.is_some())
            .then_some(request.kind);
    let staging = StagedExport::new(&request.target)?;
    let staged_request = ExportRequest {
        target: staging.output.clone(),
        ..request.clone()
    };
    let executable = crate::media_tools::tool_path("ffmpeg.exe").map_err(ExportError::Start)?;
    if request.hardware_encode && hardware_encode_supported_target(request) {
        let hardware = run_ffmpeg(
            &executable,
            staging.arguments(&staged_request, true, &streams)?,
            cancelled,
            progress,
        )?;
        if hardware.status.success() {
            staging.publish(&request.target, cancelled, trimmed_kind)?;
            return Ok(ExportOutcome {
                used_hardware_encoder: true,
            });
        }
    }
    let output = run_ffmpeg(
        &executable,
        staging.arguments(&staged_request, false, &streams)?,
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
    staging.publish(&request.target, cancelled, trimmed_kind)?;
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
    fn arguments(
        &self,
        request: &ExportRequest,
        hardware: bool,
        streams: &ExportStreams,
    ) -> Result<Vec<String>, ExportError> {
        let mut arguments = ffmpeg_arguments(request, hardware, streams);
        if let Some(index) = arguments
            .iter()
            .position(|argument| argument == "-filter_complex")
        {
            // A selection history can exceed Windows' command-line limit; keep the graph in staging.
            let path = self.directory.join("timeline-filter.txt");
            fs::write(&path, &arguments[index + 1]).map_err(ExportError::Output)?;
            arguments[index] = "-/filter_complex".into();
            arguments[index + 1] = path.display().to_string();
        }
        Ok(arguments)
    }

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

    fn publish(
        &self,
        target: &Path,
        cancelled: &AtomicBool,
        trimmed_kind: Option<MediaKind>,
    ) -> Result<(), ExportError> {
        check_cancelled(cancelled)?;
        if let Some(kind) = trimmed_kind {
            let mut input = ffmpeg::format::input(&self.output).map_err(|error| {
                ExportError::Failed(format!("trim output contains no readable media: {error}"))
            })?;
            let expected = if kind == MediaKind::Audio {
                ffmpeg::media::Type::Audio
            } else {
                ffmpeg::media::Type::Video
            };
            let mut found = false;
            for (stream, packet) in input.packets() {
                check_cancelled(cancelled)?;
                if stream.parameters().medium() == expected && packet.size() > 0 {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(ExportError::Failed(format!(
                    "trim contains no {expected:?} frames; choose a wider range"
                )));
            }
        }
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
        let _ = fs::remove_file(self.directory.join("timeline-filter.txt"));
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

#[derive(Default)]
struct ExportStreams {
    video: Option<(usize, ffmpeg::Rational)>,
    audio: Option<(usize, ffmpeg::Rational)>,
    duration: Option<towavue_core::MediaTime>,
    timeline: Option<towavue_core::EditTimeline>,
}

impl ExportStreams {
    fn probe(request: &ExportRequest) -> Result<Self, ExportError> {
        if request.kind == MediaKind::Image {
            return Ok(Self::default());
        }
        let probe = || -> Result<Self, ffmpeg::Error> {
            ffmpeg::init()?;
            let input = ffmpeg::format::input(&request.source)?;
            let video = (request.kind == MediaKind::Video)
                .then(|| input.streams().best(ffmpeg::media::Type::Video))
                .flatten()
                .map(|stream| (stream.index(), stream.time_base()));
            let audio = input
                .streams()
                .best(ffmpeg::media::Type::Audio)
                .map(|stream| {
                    let decoder =
                        ffmpeg::codec::context::Context::from_parameters(stream.parameters())?
                            .decoder()
                            .audio()?;
                    Ok::<_, ffmpeg::Error>((
                        stream.index(),
                        ffmpeg::Rational(1, decoder.rate() as i32),
                    ))
                })
                .transpose()?;
            let duration = input.duration();
            let duration = if input
                .format()
                .name()
                .split(',')
                .any(|name| matches!(name, "matroska" | "webm"))
            {
                duration.checked_sub(crate::decode::input_origin(&input))
            } else {
                Some(duration)
            };
            let duration = duration
                .filter(|duration| *duration > 0)
                .and_then(|duration| duration.checked_mul(1000))
                .map(towavue_core::MediaTime::from_nanoseconds);
            Ok(Self {
                video,
                audio,
                duration,
                timeline: None,
            })
        };
        probe().map_err(|error| {
            ExportError::Failed(format!("could not inspect export streams: {error}"))
        })
    }
}

fn ffmpeg_arguments(
    request: &ExportRequest,
    hardware: bool,
    streams: &ExportStreams,
) -> Vec<String> {
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
    if let Some(timeline) = &streams.timeline {
        arguments.extend([
            "-copyts".into(),
            "-start_at_zero".into(),
            "-filter_complex".into(),
            timeline_filters(request, streams, timeline, hardware),
        ]);
        if streams.video.is_some() {
            arguments.extend(["-map".into(), "[outv]".into()]);
        }
        if streams.audio.is_some() {
            arguments.extend(["-map".into(), "[outa]".into()]);
        }
        arguments.extend(codec_arguments(request, hardware));
        arguments.push(request.target.display().to_string());
        return arguments;
    }
    if streams.video.is_some() || streams.audio.is_some() {
        if state.trim_start.is_some() || state.trim_end.is_some() {
            arguments.push("-copyts".into());
            arguments.push("-start_at_zero".into());
        }
        for (index, _) in streams.video.iter().chain(streams.audio.iter()) {
            arguments.extend(["-map".into(), format!("0:{index}")]);
        }
    }
    let visual = visual_filters(&request.operations);
    let codecs = codec_arguments(request, hardware);
    match request.kind {
        MediaKind::Image => {
            if !visual.is_empty() {
                arguments.extend(["-vf".into(), visual.join(",")]);
            }
            arguments.extend(["-frames:v".into(), "1".into()]);
        }
        MediaKind::Video => {
            let mut video = video_filters(
                visual,
                &state,
                streams.video.map(|(_, time_base)| time_base),
            );
            if codecs.iter().any(|codec| codec == "libopenh264") {
                // Autorotate/vflip can expose negative strides rejected by OpenH264.
                video.push("copy".into());
            }
            if !video.is_empty() {
                arguments.extend(["-vf".into(), video.join(",")]);
            }
            let audio = audio_filters(&state, streams.audio.map(|(_, time_base)| time_base));
            if !audio.is_empty() {
                arguments.extend(["-af".into(), audio.join(",")]);
            }
        }
        MediaKind::Audio => {
            let audio = audio_filters(&state, streams.audio.map(|(_, time_base)| time_base));
            if !audio.is_empty() {
                arguments.extend(["-af".into(), audio.join(",")]);
            }
        }
    }
    arguments.extend(codecs);
    arguments.push(request.target.display().to_string());
    arguments
}

fn timeline_filters(
    request: &ExportRequest,
    streams: &ExportStreams,
    timeline: &towavue_core::EditTimeline,
    hardware: bool,
) -> String {
    let master = EditState::from_operations(&request.operations);
    let count = timeline.spans().len();
    let mut filters = Vec::new();
    for (stream, prefix, split) in [
        (streams.video, "v", "split"),
        (streams.audio, "a", "asplit"),
    ] {
        if let Some((index, _)) = stream {
            let outputs = (0..count)
                .map(|part| format!("[{prefix}s{part}]"))
                .collect::<String>();
            filters.push(format!("[0:{index}]{split}={count}{outputs}"));
        }
    }
    let mut inputs = String::new();
    let mut edited_ns = 0_i64;
    for (index, span) in timeline.spans().iter().enumerate() {
        let state = EditState {
            trim_start: Some(span.source().start()),
            trim_end: Some(span.source().end()),
            ..Default::default()
        };
        if let Some((_, time_base)) = streams.video {
            let trim = trim_filter("trim", &state, time_base).expect("validated timeline interval");
            filters.push(format!(
                "[vs{index}]{trim},setpts=(PTS-STARTPTS)*{}/{}/{:.9}[v{index}]",
                span.duration().as_nanoseconds(),
                span.source().duration().as_nanoseconds(),
                master.rate
            ));
            inputs.push_str(&format!("[v{index}]"));
        }
        if let Some((_, time_base)) = streams.audio {
            let mut audio = vec![
                trim_filter("atrim", &state, time_base).expect("validated timeline interval"),
                "asetpts=PTS-STARTPTS".into(),
                "aformat=sample_fmts=flt".into(),
            ];
            let rate = span.rate() * f64::from(master.rate);
            if rate != 1.0 {
                audio.extend(tempo_filters(rate, 9));
            }
            let volume = f64::from(span.volume()) * f64::from(master.volume);
            if volume != 1.0 {
                audio.push(format!("volume={volume:.9}"));
            }
            // atempo may produce a short tail. Keep every join on the planned sample axis.
            let sample_at = |ns: i64| {
                (ns as f64 / 1_000_000_000.0 / f64::from(master.rate)
                    * f64::from(time_base.denominator()))
                .ceil() as u64
            };
            let samples =
                sample_at(edited_ns + span.duration().as_nanoseconds()) - sample_at(edited_ns);
            audio.push(format!("apad=whole_len={samples}"));
            audio.push(format!("atrim=end_sample={samples}"));
            filters.push(format!("[as{index}]{}[a{index}]", audio.join(",")));
            inputs.push_str(&format!("[a{index}]"));
        }
        edited_ns += span.duration().as_nanoseconds();
    }
    let video = usize::from(streams.video.is_some());
    let audio = usize::from(streams.audio.is_some());
    let outputs = format!(
        "{}{}",
        if video == 1 { "[joinedv]" } else { "" },
        if audio == 1 { "[outa]" } else { "" }
    );
    filters.push(format!(
        "{inputs}concat=n={count}:v={video}:a={audio}{outputs}"
    ));
    if video == 1 {
        let mut visual = visual_filters(&request.operations);
        if codec_arguments(request, hardware)
            .iter()
            .any(|codec| codec == "libopenh264")
        {
            visual.push("copy".into());
        }
        if visual.is_empty() {
            visual.push("null".into());
        }
        filters.push(format!("[joinedv]{}[outv]", visual.join(",")));
    }
    filters.join(";")
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

pub(crate) fn visual_filters(operations: &[EditOperation]) -> Vec<String> {
    operations
        .iter()
        .filter_map(|operation| match *operation {
            EditOperation::RotateImage(rotation) => Some(match rotation.tenths() {
                0 => "null".into(),
                900 => "transpose=clock".into(),
                -900 => "transpose=cclock".into(),
                -1800 | 1800 => "hflip,vflip".into(),
                tenths => {
                    let (width, height) = rotation.size();
                    // The pinned rotate filter supports GBRAP8, not GBRAP16.
                    // Premultiplication prevents hidden RGB from bleeding across alpha edges.
                    format!("format=gbrap,premultiply=inplace=1,rotate={tenths}*PI/1800:ow={width}:oh={height}:c=black@0:bilinear=1,unpremultiply=inplace=1,format=rgba")
                }
            }),
            EditOperation::Resize(resize) => {
                use towavue_core::ResampleFilter;
                let (width, height) = resize.size();
                let flags = match resize.filter {
                    ResampleFilter::Nearest => "neighbor",
                    ResampleFilter::Bilinear => "bilinear",
                    ResampleFilter::Bicubic => "bicubic",
                    ResampleFilter::Lanczos => "lanczos",
                };
                let scale = format!("scale={width}:{height}:flags={flags}");
                Some(if resize.filter == ResampleFilter::Nearest {
                    format!("format=rgba,{scale},format=rgba")
                } else {
                    format!("format=gbrap16le,premultiply=inplace=1,{scale},unpremultiply=inplace=1,format=rgba")
                })
            }
            EditOperation::Crop(region) => Some(format!(
                "crop={}:{}:{}:{}:exact=1",
                region.width, region.height, region.x, region.y
            )),
            EditOperation::RotateClockwise => Some("transpose=clock".into()),
            EditOperation::RotateCounterclockwise => Some("transpose=cclock".into()),
            EditOperation::FlipHorizontal => Some("hflip".into()),
            EditOperation::FlipVertical => Some("vflip".into()),
            EditOperation::SetTrimStart(_)
            | EditOperation::SetTrimEnd(_)
            | EditOperation::SetVolume(_)
            | EditOperation::SetRate(_)
            | EditOperation::Timeline(_) => None,
        })
        .collect()
}

fn video_filters(
    mut filters: Vec<String>,
    state: &EditState,
    time_base: Option<ffmpeg::Rational>,
) -> Vec<String> {
    if let Some(trim) = time_base.and_then(|time_base| trim_filter("trim", state, time_base)) {
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

fn audio_filters(state: &EditState, time_base: Option<ffmpeg::Rational>) -> Vec<String> {
    let mut filters = Vec::new();
    if let Some(trim) = time_base.and_then(|time_base| trim_filter("atrim", state, time_base)) {
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

fn trim_filter(name: &str, state: &EditState, time_base: ffmpeg::Rational) -> Option<String> {
    // A half-open source interval keeps the first tick/sample at or after each boundary.
    let ticks = |time: towavue_core::MediaTime| {
        let numerator = i128::from(time.as_nanoseconds()) * i128::from(time_base.denominator());
        let denominator = 1_000_000_000_i128 * i128::from(time_base.numerator());
        (numerator + denominator - 1) / denominator
    };
    match (state.trim_start, state.trim_end) {
        (Some(start), Some(end)) if start < end => Some(format!(
            "{name}=start_pts={}:end_pts={}",
            ticks(start),
            ticks(end)
        )),
        (Some(start), None) => Some(format!("{name}=start_pts={}", ticks(start))),
        (None, Some(end)) => Some(format!("{name}=start_pts=0:end_pts={}", ticks(end))),
        _ => None,
    }
}

fn atempo_filters(rate: f32) -> Vec<String> {
    tempo_filters(f64::from(rate), 4)
}

fn tempo_filters(rate: f64, precision: usize) -> Vec<String> {
    crate::tempo::filters(rate, precision)
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
#[path = "export_timeline_tests.rs"]
mod timeline_tests;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::DecodeOutput;
    use image::{ImageFormat, Rgb, RgbImage};
    use towavue_core::{MediaTime, PixelCrop};

    use super::*;

    #[test]
    fn export_preserves_playback_streams_with_and_without_trim() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("towavue-export-streams-{unique}"));
        fs::create_dir(&directory).expect("fixture directory");
        let source = directory.join("source.mkv");
        let executable = PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
        let generated = Command::new(executable)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=blue:s=160x96:r=30:d=1",
            ])
            .args(["-f", "lavfi", "-i", "color=red:s=320x240:r=30:d=1"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:sample_rate=48000:duration=1",
            ])
            .args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo:d=1"])
            .args(["-map", "0:v", "-map", "1:v", "-map", "2:a", "-map", "3:a"])
            .args([
                "-c:v",
                "libopenh264",
                "-c:a",
                "pcm_s16le",
                "-disposition",
                "0",
                "-disposition:a:1",
                "hearing_impaired",
            ])
            .arg(&source)
            .output()
            .expect("generate multistream fixture");
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let inspect = |path: &Path, dimensions: Option<(u32, u32)>| {
            let mut video_seen = false;
            let mut audible = false;
            crate::decode_file(path, |output| {
                match output {
                    DecodeOutput::Video(frame) => {
                        assert_eq!(
                            Some((frame.width, frame.height)),
                            dimensions,
                            "{}",
                            path.display()
                        );
                        assert!(
                            frame.rgba[0] < 10 && frame.rgba[2] > 240,
                            "export must retain the blue playback stream"
                        );
                        video_seen = true;
                    }
                    DecodeOutput::Audio(chunk) => {
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
            .expect("decode selected streams");
            assert_eq!(video_seen, dimensions.is_some());
            assert!(audible, "export must retain the audible playback stream");
        };
        inspect(&source, Some((160, 96)));
        for (name, kind, operations, dimensions) in [
            ("plain.mkv", MediaKind::Video, vec![], Some((160, 96))),
            (
                "crop.mkv",
                MediaKind::Video,
                vec![EditOperation::Crop(PixelCrop {
                    x: 0,
                    y: 0,
                    width: 32,
                    height: 32,
                })],
                Some((32, 32)),
            ),
            (
                "trim.mkv",
                MediaKind::Video,
                vec![EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(
                    500_000_000,
                ))],
                Some((160, 96)),
            ),
            ("audio.wav", MediaKind::Audio, vec![], None),
        ] {
            let target = directory.join(name);
            let request = ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind,
                operations,
                hardware_encode: false,
            };
            let streams = ExportStreams::probe(&request).expect("selected input streams");
            for hardware in [false, true] {
                let arguments = ffmpeg_arguments(&request, hardware, &streams).join(" ");
                assert!(arguments.contains("-map 0:2"));
                assert_eq!(arguments.contains("-map 0:0"), kind == MediaKind::Video);
                assert_eq!(
                    arguments.contains("-copyts -start_at_zero"),
                    name == "trim.mkv"
                );
            }
            export_media(&request).expect("export selected streams");
            inspect(&target, dimensions);
        }
        fs::remove_dir_all(directory).expect("remove owned export fixtures");
    }

    #[test]
    fn trim_export_preserves_source_boundaries_and_rejects_empty_media() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("towavue-trim-boundary-{unique}"));
        fs::create_dir(&directory).expect("fixture directory");
        let source = directory.join("source.mkv");
        let executable =
            PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg directory"))
                .join("bin/ffmpeg.exe");
        let generated = Command::new(&executable).args([
            "-v", "error", "-f", "lavfi", "-i",
            "nullsrc=size=160x96:rate=30:duration=2,geq=r='mod(N*37,256)':g='mod(N*67,256)':b='mod(N*97,256)'",
            "-f", "lavfi", "-i", "sine=sample_rate=48000:duration=2", "-c:v", "ffv1", "-c:a", "pcm_s16le",
        ]).arg(&source).output().expect("generate boundary fixture");
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        for (start_ns, end_ns) in [
            (33_400_000, 99_600_000),
            (67_000_001, 100_000_001),
            (1_067_000_001, 1_100_000_001),
        ] {
            let start = MediaTime::from_nanoseconds(start_ns);
            let end = MediaTime::from_nanoseconds(end_ns);
            let (mut expected_pixels, mut expected_samples) = (Vec::new(), 0);
            let mut expected_audio = Vec::new();
            crate::decode::decode_file(&source, |output| {
                match output {
                    DecodeOutput::Video(frame)
                        if frame.presentation_time >= start && frame.presentation_time < end =>
                    {
                        expected_pixels.push(frame.rgba[..3].to_vec())
                    }
                    DecodeOutput::Audio(mut chunk) => {
                        crate::decode::clip_audio_chunk(&mut chunk, start, Some(end));
                        expected_samples += chunk.frames;
                        expected_audio.extend(chunk.bytes);
                    }
                    _ => {}
                }
                true
            })
            .expect("source interval");
            let (mut live_frames, mut live_audio) = (0, Vec::new());
            crate::decode::decode_file_parallel(&source, start, Some(end), None, |output| {
                match output {
                    crate::decode::ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(_)) => {
                        live_frames += 1
                    }
                    crate::decode::ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(
                        chunk,
                    )) => live_audio.extend(chunk.bytes),
                    _ => {}
                }
                true
            })
            .expect("live interval");
            assert_eq!(live_frames, expected_pixels.len());
            assert_eq!(live_audio.len(), expected_audio.len());
            // Seeking into coarse PTS can lose the original sub-tick sample phase.
            // Compare exact live samples only when demux preroll still starts at zero.
            if start_ns < 100_000_000 {
                assert!(
                    live_audio == expected_audio,
                    "live source samples at {start_ns}..{end_ns}"
                );
            } else {
                let probe = &live_audio[48 * 8..112 * 8];
                let offset = expected_audio
                    .windows(probe.len())
                    .step_by(8)
                    .position(|samples| samples == probe)
                    .expect("matching source samples") as i64
                    - 48;
                assert!(
                    offset.abs() <= 24,
                    "seek phase exceeds half a millisecond: {offset} samples"
                );
                eprintln!(
                    "coarse PTS seek at {start_ns} ns: source phase offset {offset} samples ({:.3} ms)",
                    offset as f64 / 48.0
                );
            }
            let target = directory.join(format!("{start_ns}.avi"));
            export_media(&ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind: MediaKind::Video,
                operations: vec![
                    EditOperation::SetTrimStart(start),
                    EditOperation::SetTrimEnd(end),
                ],
                hardware_encode: false,
            })
            .expect("trim export");
            let (mut actual_pixels, mut actual_samples) = (Vec::new(), 0);
            let mut actual_audio = Vec::new();
            crate::decode::decode_file(&target, |output| {
                match output {
                    DecodeOutput::Video(frame) => actual_pixels.push(frame.rgba[..3].to_vec()),
                    DecodeOutput::Audio(chunk) => {
                        actual_samples += chunk.frames;
                        actual_audio.extend(chunk.bytes);
                    }
                }
                true
            })
            .expect("decode trimmed output");
            assert_eq!(
                actual_pixels.len(),
                expected_pixels.len(),
                "frame count at {start_ns}..{end_ns}"
            );
            for (actual, expected) in actual_pixels.iter().zip(&expected_pixels) {
                assert!(
                    actual
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| actual.abs_diff(*expected) <= 4),
                    "wrong source frame: {actual:?} != {expected:?}"
                );
            }
            assert_eq!(
                actual_samples, expected_samples,
                "sample count at {start_ns}..{end_ns}"
            );
            assert!(
                actual_audio == expected_audio,
                "export source samples at {start_ns}..{end_ns}"
            );
        }
        let shifted = directory.join("shifted.mkv");
        let remux = Command::new(&executable)
            .args(["-v", "error", "-i"])
            .arg(&source)
            .args(["-map", "0", "-c", "copy", "-output_ts_offset", "5"])
            .arg(&shifted)
            .output()
            .expect("offset fixture");
        assert!(
            remux.status.success(),
            "{}",
            String::from_utf8_lossy(&remux.stderr)
        );
        let shifted_target = directory.join("shifted.avi");
        let mut first_video = None;
        crate::decode::decode_file(&shifted, |output| {
            if let DecodeOutput::Video(frame) = output {
                first_video.get_or_insert(frame.presentation_time);
            }
            true
        })
        .expect("decode nonzero-origin source");
        assert_eq!(first_video, Some(MediaTime::ZERO));
        let (mut videos, mut samples) = (0, 0);
        crate::decode::decode_file_parallel(
            &shifted,
            MediaTime::from_nanoseconds(1_000_000_000),
            Some(MediaTime::from_nanoseconds(1_100_000_000)),
            None,
            |output| {
                match output {
                    crate::decode::ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(_)) => {
                        videos += 1
                    }
                    crate::decode::ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(
                        chunk,
                    )) => samples += chunk.frames,
                    _ => {}
                }
                true
            },
        )
        .expect("seek and trim relative to the source origin");
        assert_eq!((videos, samples), (3, 4_800));
        export_media(&ExportRequest {
            source: shifted,
            target: shifted_target.clone(),
            kind: MediaKind::Video,
            operations: vec![
                EditOperation::SetTrimStart(MediaTime::from_nanoseconds(67_000_001)),
                EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(100_000_001)),
            ],
            hardware_encode: false,
        })
        .expect("source-offset trim export");
        let mut decoded_outputs = Vec::new();
        for path in [directory.join("67000001.avi"), shifted_target] {
            let (mut pixels, mut audio) = (Vec::new(), Vec::new());
            crate::decode::decode_file(&path, |output| {
                match output {
                    DecodeOutput::Video(frame) => pixels.extend(frame.rgba),
                    DecodeOutput::Audio(chunk) => audio.extend(chunk.bytes),
                }
                true
            })
            .expect("decode offset comparison");
            decoded_outputs.push((pixels, audio));
        }
        assert!(
            decoded_outputs[0] == decoded_outputs[1],
            "source offset changed selected media"
        );
        for (kind, end_ns) in [
            (MediaKind::Video, 2),
            (MediaKind::Video, 100_000),
            (MediaKind::Audio, 2),
        ] {
            let target = directory.join(if kind == MediaKind::Video {
                "empty.avi"
            } else {
                "empty.wav"
            });
            fs::write(&target, b"previous export").expect("existing target");
            let result = export_media(&ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind,
                operations: vec![
                    EditOperation::SetTrimStart(MediaTime::from_nanoseconds(1)),
                    EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(end_ns)),
                ],
                hardware_encode: false,
            });
            assert!(result.is_err(), "empty {kind:?} must fail: {result:?}");
            assert_eq!(
                fs::read(target).expect("target remains"),
                b"previous export"
            );
        }
        fs::remove_dir_all(directory).expect("remove owned boundary fixtures");
    }

    #[test]
    fn image_export_preserves_operation_order() {
        let request = ExportRequest {
            source: "in.png".into(),
            target: "out.png".into(),
            kind: MediaKind::Image,
            operations: vec![
                EditOperation::Crop(PixelCrop {
                    x: 4,
                    y: 6,
                    width: 20,
                    height: 18,
                }),
                EditOperation::RotateClockwise,
                EditOperation::FlipHorizontal,
            ],
            hardware_encode: false,
        };

        let arguments = ffmpeg_arguments(&request, false, &ExportStreams::default());
        let filter = arguments
            .windows(2)
            .find_map(|pair| (pair[0] == "-vf").then_some(pair[1].as_str()))
            .expect("video filter argument");

        assert_eq!(filter, "crop=20:18:4:6:exact=1,transpose=clock,hflip");
    }

    #[test]
    fn trim_ticks_round_up_and_keep_implicit_source_start() {
        let mut state = EditState {
            trim_end: Some(MediaTime::from_nanoseconds(1)),
            ..EditState::default()
        };
        assert_eq!(
            trim_filter("trim", &state, ffmpeg::Rational(1, 1000)).as_deref(),
            Some("trim=start_pts=0:end_pts=1")
        );
        state.trim_start = Some(MediaTime::from_nanoseconds(33_333_334));
        state.trim_end = None;
        assert_eq!(
            trim_filter("trim", &state, ffmpeg::Rational(1, 30)).as_deref(),
            Some("trim=start_pts=2")
        );
        assert_eq!(
            trim_filter("atrim", &state, ffmpeg::Rational(1, 48000)).as_deref(),
            Some("atrim=start_pts=1601")
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

        let streams = ExportStreams {
            video: Some((0, ffmpeg::Rational(1, 1000))),
            audio: Some((1, ffmpeg::Rational(1, 48000))),
            ..Default::default()
        };
        let arguments = ffmpeg_arguments(&request, false, &streams).join(" ");

        assert!(
            arguments.contains("trim=start_pts=2000:end_pts=5000,setpts=(PTS-STARTPTS)/4.0000")
        );
        assert!(arguments.contains("atrim=start_pts=96000:end_pts=240000,asetpts=PTS-STARTPTS,atempo=2.0000,atempo=2.0000,volume=0.5000"));
        assert!(arguments.contains("-copyts -start_at_zero -map 0:0 -map 0:1"));
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

        let arguments = ffmpeg_arguments(&request, true, &ExportStreams::default()).join(" ");

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
    fn invalid_trim_is_rejected_before_preparing_output_or_starting_ffmpeg() {
        for operations in [
            vec![EditOperation::SetTrimEnd(MediaTime::ZERO)],
            vec![EditOperation::SetTrimStart(MediaTime::from_nanoseconds(-1))],
            vec![
                EditOperation::SetTrimStart(MediaTime::from_nanoseconds(2_000_000_000)),
                EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(1_000_000_000)),
            ],
        ] {
            let request = ExportRequest {
                source: "missing-source.mp4".into(),
                target: "missing-parent/output.mp4".into(),
                kind: MediaKind::Video,
                operations,
                hardware_encode: false,
            };
            assert!(matches!(
                export_media(&request),
                Err(ExportError::InvalidTrim)
            ));
        }
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
            operations: vec![EditOperation::Crop(PixelCrop {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
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
                EditOperation::Crop(PixelCrop {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 30,
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
    fn pixel_crops_export_one_pixel_odd_bounds_and_nested_rotations_exactly() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("towavue-pixel-crop-{unique}"));
        fs::create_dir(&directory).expect("fixture directory");
        let source = directory.join("source.png");
        let pixels = RgbImage::from_fn(8, 8, |x, y| Rgb([(x * 30) as u8, (y * 30) as u8, 70]));
        pixels.save(&source).expect("source image");
        let original = fs::read(&source).expect("source bytes");
        for crop in [
            PixelCrop {
                x: 3,
                y: 2,
                width: 1,
                height: 1,
            },
            PixelCrop {
                x: 1,
                y: 3,
                width: 5,
                height: 3,
            },
        ] {
            let target = directory.join("crop.png");
            export_media(&ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind: MediaKind::Image,
                operations: vec![EditOperation::Crop(crop)],
                hardware_encode: false,
            })
            .expect("pixel crop export");
            let exported = crate::decode_image(&target).expect("decode cropped image");
            assert_eq!(exported.dimensions(), (crop.width, crop.height));
            for y in 0..crop.height {
                for x in 0..crop.width {
                    let index = ((y * crop.width + x) * 4) as usize;
                    assert_eq!(
                        &exported.frames[0].rgba[index..index + 3],
                        &pixels.get_pixel(crop.x + x, crop.y + y).0
                    );
                }
            }
        }
        let target = directory.join("nested.png");
        export_media(&ExportRequest {
            source: source.clone(),
            target: target.clone(),
            kind: MediaKind::Image,
            operations: vec![
                EditOperation::Crop(PixelCrop {
                    x: 1,
                    y: 3,
                    width: 5,
                    height: 3,
                }),
                EditOperation::RotateClockwise,
                EditOperation::Crop(PixelCrop {
                    x: 1,
                    y: 2,
                    width: 1,
                    height: 2,
                }),
            ],
            hardware_encode: false,
        })
        .expect("nested crop export");
        let exported = crate::decode_image(&target).expect("decode nested crop");
        assert_eq!(exported.dimensions(), (1, 2));
        assert_eq!(&exported.frames[0].rgba[..3], &pixels.get_pixel(3, 4).0);
        assert_eq!(&exported.frames[0].rgba[4..7], &pixels.get_pixel(4, 4).0);
        assert_eq!(fs::read(&source).expect("source remains"), original);
        fs::remove_dir_all(directory).expect("remove crop fixtures");
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
            staging.publish(&target, &AtomicBool::new(false), None),
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
