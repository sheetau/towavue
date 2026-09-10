use super::*;
use std::os::windows::process::CommandExt;
use std::time::{SystemTime, UNIX_EPOCH};
use towavue_core::{
    MediaTime, PixelCrop, ResampleFilter, TimeRange, TimelineEdit, VideoResize, VideoRotation,
};

fn root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-audio-export-{label}-{unique}"));
    fs::create_dir(&root).expect("owned fixture");
    root
}

fn ffmpeg(args: &[&str], target: &Path) {
    let output = Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg"))
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-v", "error", "-nostdin", "-y"])
        .args(args)
        .arg(target)
        .output()
        .expect("fixture FFmpeg");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture(path: &Path) {
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=10:duration=1.2",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo:d=1.2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=431:sample_rate=48000:duration=1.2",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-c:v",
            "ffv1",
            "-c:a",
            "pcm_s16le",
            "-disposition:a:0",
            "0",
            "-disposition:a:1",
            "default",
            "-metadata",
            "title=Audio derivative fixture",
        ],
        path,
    );
}

fn pcm(path: &Path) -> Vec<u8> {
    let output = Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg"))
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-f",
            "f32le",
            "-c:a",
            "pcm_f32le",
            "pipe:1",
        ])
        .output()
        .expect("decode only; never played");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn time(ms: i64) -> MediaTime {
    MediaTime::from_nanoseconds(ms * 1_000_000)
}
fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(time(start), time(end)).expect("range")
}

#[test]
fn audio_only_exports_best_stream_all_formats_and_ignores_only_spatial_edits() {
    let root = root("formats");
    let source = root.join("source 日本語 & multi.mkv");
    fixture(&source);
    let original = fs::read(&source).expect("source");
    let operations = vec![
        EditOperation::Crop(PixelCrop {
            x: 2,
            y: 2,
            width: 32,
            height: 24,
        }),
        EditOperation::RotateClockwise,
        // Deliberately stale geometry: an audio derivative does not render or validate video pixels.
        EditOperation::RotateVideo(VideoRotation::new(317, (64, 48), 2.0).expect("rotation")),
        EditOperation::ResizeVideo(
            VideoResize::new((96, 64), ResampleFilter::Lanczos, (64, 48), 1.0).expect("resize"),
        ),
        EditOperation::FlipVertical,
    ];
    let reference = root.join("reference.wav");
    ffmpeg(
        &[
            "-i",
            source.to_str().expect("path"),
            "-map",
            "0:2",
            "-vn",
            "-c:a",
            "pcm_s16le",
        ],
        &reference,
    );
    let expected = pcm(&reference);
    assert!(
        expected.iter().any(|byte| *byte != 0),
        "selected stream is not the silent first audio"
    );
    for extension in ["wav", "flac", "mp3", "m4a", "aac", "ogg", "opus"] {
        let request = ExportRequest {
            source: source.clone(),
            target: root.join(format!("audio 日本語 & %.{extension}")),
            kind: MediaKind::Video,
            operations: operations.clone(),
            hardware_encode: true,
        };
        let outcome =
            export_media_with_output(&request, ExportOutput::AudioOnly).expect("audio export");
        assert!(!outcome.used_hardware_encoder);
        let input = ffmpeg::format::input(&request.target).expect("reopen derivative");
        assert_eq!(input.streams().len(), 1);
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .expect("audio only");
        let decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .expect("context")
            .decoder()
            .audio()
            .expect("audio");
        assert_eq!(decoder.channels(), 1, "best audio selected");
        let actual = pcm(&request.target);
        assert!(!actual.is_empty());
        if matches!(extension, "wav" | "flac") {
            assert_eq!(actual, expected, "lossless PCM");
            assert_eq!(
                input.metadata().get("title"),
                Some("Audio derivative fixture")
            );
        }
        assert_eq!(request.kind, MediaKind::Video);
        assert_eq!(request.operations, operations);
    }
    assert_eq!(fs::read(&source).expect("source retained"), original);
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}

#[test]
fn audio_only_keeps_trim_rate_volume_and_edited_timeline_samples() {
    let root = root("edits");
    let source = root.join("source.mkv");
    fixture(&source);
    let original = fs::read(&source).expect("source");
    let mut request = ExportRequest {
        source: source.clone(),
        target: root.join("trim.wav"),
        kind: MediaKind::Video,
        operations: vec![
            EditOperation::SetTrimStart(time(200)),
            EditOperation::SetTrimEnd(time(1000)),
            EditOperation::SetRate(1.25),
            EditOperation::SetVolume(0.5),
            EditOperation::FlipHorizontal,
        ],
        hardware_encode: true,
    };
    export_media_with_output(&request, ExportOutput::AudioOnly).expect("trimmed derivative");
    let reference = root.join("reference.wav");
    ffmpeg(
        &[
            "-i",
            source.to_str().expect("path"),
            "-copyts",
            "-start_at_zero",
            "-map",
            "0:2",
            "-af",
            "atrim=start_pts=9600:end_pts=48000,asetpts=PTS-STARTPTS,atempo=1.25,volume=0.5",
            "-c:a",
            "pcm_s16le",
        ],
        &reference,
    );
    assert_eq!(pcm(&request.target), pcm(&reference));
    request.target = root.join("timeline.wav");
    request.operations = vec![
        EditOperation::Timeline(TimelineEdit::Delete(range(400, 800))),
        EditOperation::Timeline(TimelineEdit::Stretch(range(0, 400), time(800))),
        EditOperation::Timeline(TimelineEdit::SetVolume(range(0, 800), 0.25)),
        EditOperation::SetRate(2.0),
        EditOperation::SetVolume(0.5),
        EditOperation::RotateClockwise,
    ];
    export_media_with_output(&request, ExportOutput::AudioOnly).expect("edited derivative");
    let graph = "[0:2]asplit=2[a][b];[a]atrim=start_pts=0:end_pts=19200,asetpts=PTS-STARTPTS,aformat=sample_fmts=flt,volume=0.125,apad=whole_len=19200,atrim=end_sample=19200[x];[b]atrim=start_pts=38400:end_pts=57600,asetpts=PTS-STARTPTS,aformat=sample_fmts=flt,atempo=2,volume=0.5,apad=whole_len=9600,atrim=end_sample=9600[y];[x][y]concat=n=2:v=0:a=1[out]";
    ffmpeg(
        &[
            "-i",
            source.to_str().expect("path"),
            "-copyts",
            "-start_at_zero",
            "-filter_complex",
            graph,
            "-map",
            "[out]",
            "-c:a",
            "pcm_s16le",
        ],
        &reference,
    );
    let actual = pcm(&request.target);
    assert_eq!(actual, pcm(&reference));
    assert_eq!(actual.len(), 28_800 * 4, "edited 0.6 seconds of mono f32");
    assert_eq!(fs::read(&source).expect("source retained"), original);
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}

#[test]
fn audio_only_refuses_absent_audio_invalid_targets_empty_trim_and_cancellation_without_replacement()
{
    let root = root("protection");
    let source = root.join("source.mkv");
    fixture(&source);
    let target = root.join("existing.wav");
    let existing = b"existing target must survive";
    fs::write(&target, existing).expect("owned target");
    let mut request = ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: MediaKind::Video,
        operations: vec![],
        hardware_encode: false,
    };
    assert!(matches!(
        export_output_cancellable(
            &request,
            ExportOutput::AudioOnly,
            &AtomicBool::new(true),
            &|_| {}
        ),
        Err(ExportError::Cancelled)
    ));
    let cancelled = AtomicBool::new(false);
    let progressed = AtomicBool::new(false);
    assert!(matches!(
        export_output_cancellable(&request, ExportOutput::AudioOnly, &cancelled, &|time| {
            if time > Duration::ZERO {
                progressed.store(true, Ordering::Relaxed);
                cancelled.store(true, Ordering::Relaxed);
            }
        }),
        Err(ExportError::Cancelled)
    ));
    assert!(
        progressed.load(Ordering::Relaxed),
        "cancel after encoded audio progress"
    );
    assert_eq!(
        fs::read(&target).expect("target after active cancellation"),
        existing
    );
    request.operations = vec![EditOperation::SetTrimStart(time(2000))];
    assert!(export_media_with_output(&request, ExportOutput::AudioOnly).is_err());
    request.operations = vec![EditOperation::Timeline(TimelineEdit::Delete(range(
        0, 1200,
    )))];
    assert!(matches!(
        export_media_with_output(&request, ExportOutput::AudioOnly),
        Err(ExportError::InvalidTimeline)
    ));
    request.operations.clear();
    request.kind = MediaKind::Image;
    assert!(export_media_with_output(&request, ExportOutput::AudioOnly).is_err());
    request.kind = MediaKind::Video;
    let no_audio = root.join("no-audio.mkv");
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=size=64x48:rate=4:duration=1",
            "-c:v",
            "ffv1",
        ],
        &no_audio,
    );
    request.source = no_audio;
    assert!(
        export_media_with_output(&request, ExportOutput::AudioOnly)
            .expect_err("absent audio refused")
            .to_string()
            .contains("no audio stream")
    );
    request.source = source.clone();
    request.target = root.join("existing.mp4");
    fs::write(&request.target, existing).expect("unsupported target");
    assert!(export_media_with_output(&request, ExportOutput::AudioOnly).is_err());
    assert_eq!(
        fs::read(&request.target).expect("unsupported retained"),
        existing
    );
    request.target = source.clone();
    let original = fs::read(&source).expect("original");
    assert!(matches!(
        export_media_with_output(&request, ExportOutput::AudioOnly),
        Err(ExportError::SameAsSource)
    ));
    assert_eq!(fs::read(&source).expect("original retained"), original);
    assert_eq!(fs::read(target).expect("target retained"), existing);
    assert!(!fs::read_dir(&root).expect("root").any(|entry| {
        entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".towavue-export-")
    }));
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}
