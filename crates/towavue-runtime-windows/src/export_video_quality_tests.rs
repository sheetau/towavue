use super::*;
use std::os::windows::process::CommandExt;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn software_h264_quality_is_explicit_and_does_not_change_other_codecs() {
    for (extension, kind, hardware, expected) in [
        ("mp4", MediaKind::Video, false, true),
        ("mkv", MediaKind::Video, false, true),
        ("mov", MediaKind::Video, false, true),
        ("mp4", MediaKind::Video, true, false),
        ("webm", MediaKind::Video, false, false),
        ("avi", MediaKind::Video, false, false),
        ("wmv", MediaKind::Video, false, false),
        ("m4a", MediaKind::Audio, false, false),
        ("png", MediaKind::Image, false, false),
    ] {
        let arguments = codec_arguments(
            &ExportRequest {
                source: "source.mkv".into(),
                target: format!("output.{extension}").into(),
                kind,
                operations: Vec::new(),
                hardware_encode: hardware,
            },
            hardware,
        );
        for pair in [["-profile:v", "high"], ["-qmin:v", "1"], ["-qmax:v", "20"]] {
            assert_eq!(arguments.windows(2).any(|actual| actual == pair), expected);
        }
    }
}

fn run(tool: &str, arguments: &[&str]) -> std::process::Output {
    let output = Command::new(crate::media_tools::tool_path(tool).expect("media helper"))
        .creation_flags(0x0800_0000)
        .args(arguments)
        .output()
        .expect("owned media helper process");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn psnr(source: &str, encoded: &str) -> f64 {
    let output = run(
        "ffmpeg.exe",
        &[
            "-hide_banner",
            "-i",
            source,
            "-i",
            encoded,
            "-filter_complex",
            "[0:v]hflip[reference];[reference][1:v]psnr",
            "-an",
            "-f",
            "null",
            "-",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = stderr
        .lines()
        .find(|line| line.contains("PSNR y:"))
        .expect("PSNR summary");
    summary
        .split("average:")
        .nth(1)
        .expect("average field")
        .split_whitespace()
        .next()
        .expect("average value")
        .parse()
        .expect("finite PSNR")
}

#[test]
fn edited_1080p_export_preserves_detail_beyond_the_default_bitrate_budget() {
    let root = std::env::temp_dir().join(format!(
        "towavue-export-quality-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("owned fixture directory");
    for (name, noise) in [
        ("edges", ""),
        ("texture", ",noise=alls=12:allf=t+u:all_seed=73"),
    ] {
        let source = root.join(format!("{name}.mkv"));
        let baseline = root.join(format!("{name}-default.mkv"));
        let target = root.join(format!("{name}-export.mkv"));
        let source_text = source.to_str().expect("source path");
        let baseline_text = baseline.to_str().expect("baseline path");
        let target_text = target.to_str().expect("target path");
        let fixture = format!("testsrc2=size=1920x1080:rate=24:duration=1{noise}");
        run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &fixture,
                "-c:v",
                "ffv1",
                source_text,
            ],
        );
        let source_bytes = fs::read(&source).expect("source bytes");
        let source_modified = fs::metadata(&source)
            .expect("source metadata")
            .modified()
            .expect("mtime");
        // Independent historical encoder control, not the production argument builder.
        run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-i",
                source_text,
                "-vf",
                "hflip,copy",
                "-an",
                "-c:v",
                "libopenh264",
                baseline_text,
            ],
        );
        fs::write(&target, b"previous target").expect("existing output");
        export_media(&ExportRequest {
            source: source.clone(),
            target: target.clone(),
            kind: MediaKind::Video,
            operations: vec![EditOperation::FlipHorizontal],
            hardware_encode: false,
        })
        .expect("public edited export");
        let old_quality = psnr(source_text, baseline_text);
        let new_quality = psnr(source_text, target_text);
        eprintln!(
            "EXPORT_QUALITY {name} old_psnr={old_quality:.3} new_psnr={new_quality:.3} old_bytes={} new_bytes={}",
            fs::metadata(&baseline).expect("baseline metadata").len(),
            fs::metadata(&target).expect("target metadata").len()
        );
        assert!(
            new_quality > old_quality + 4.0 && new_quality >= 40.0,
            "{name}: {old_quality} -> {new_quality}"
        );
        let probe = run(
            "ffprobe.exe",
            &[
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-count_frames",
                "-show_entries",
                "stream=profile,width,height,pix_fmt,nb_read_frames",
                "-of",
                "default=noprint_wrappers=1",
                target_text,
            ],
        );
        let metadata = String::from_utf8_lossy(&probe.stdout);
        for field in [
            "profile=High",
            "width=1920",
            "height=1080",
            "pix_fmt=yuv420p",
            "nb_read_frames=24",
        ] {
            assert!(
                metadata.lines().any(|line| line == field),
                "missing {field}: {metadata}"
            );
        }
        assert!(fs::read(&source).expect("unchanged source") == source_bytes);
        assert_eq!(
            fs::metadata(&source)
                .expect("source metadata")
                .modified()
                .expect("mtime"),
            source_modified
        );
    }
    // Only this test's uniquely named generated-media directory is removed.
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

fn depth_directory() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "towavue-video-depth-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("owned fixtures");
    root
}

fn depth_fixture(path: &Path, pixel: &str, size: &str, colors: &[&str]) {
    let filter = format!(
        "nullsrc=size={size}:rate=8:duration=2,format={pixel},geq=lum='64+mod(X+3*Y+13*N,876)':cb='400+mod(X+N,200)':cr='400+mod(Y+N,200)'"
    );
    let mut args = vec![
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        &filter,
        "-f",
        "lavfi",
        "-i",
        "sine=sample_rate=48000:duration=2",
        "-c:v",
        "libaom-av1",
        "-crf",
        "0",
        "-cpu-used",
        "6",
        "-row-mt",
        "1",
        "-threads:v",
        "4",
        "-c:a",
        "pcm_s16le",
    ];
    let color_filter = format!(
        "setparams=range={}:colorspace={}:color_trc={}:color_primaries={}",
        colors[1], colors[3], colors[5], colors[7]
    );
    args.extend_from_slice(&["-vf", &color_filter]);
    args.extend_from_slice(colors);
    args.push(path.to_str().expect("path"));
    run("ffmpeg.exe", &args);
}

fn assert_video_fields(path: &Path, fields: &[&str]) {
    let probe = run(
        "ffprobe.exe",
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=codec_name,pix_fmt,width,height,nb_read_frames,color_range,color_space,color_transfer,color_primaries",
            "-of",
            "default=noprint_wrappers=1",
            path.to_str().expect("path"),
        ],
    );
    let output = String::from_utf8_lossy(&probe.stdout);
    for field in fields {
        assert!(
            output.lines().any(|line| line == *field),
            "missing {field}: {output}"
        );
    }
}

#[test]
fn high_depth_exports_preserve_precision_edits_color_and_container_with_hardware_requested() {
    use towavue_core::{
        MediaTime, ResampleFilter, TimeRange, TimelineEdit, VideoResize, VideoRotation,
    };
    let root = depth_directory();
    let source = root.join("source.mkv");
    depth_fixture(
        &source,
        "yuv420p10le",
        "160x96",
        &[
            "-color_range",
            "tv",
            "-colorspace",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_primaries",
            "bt709",
        ],
    );
    let source_bytes = fs::read(&source).expect("source");
    let modified = fs::metadata(&source)
        .expect("source metadata")
        .modified()
        .expect("mtime");
    let rotation = VideoRotation::new(73, (160, 96), 1.0).expect("rotation");
    let cases = [
        ("flip", vec![EditOperation::FlipHorizontal], (160, 96), 16),
        (
            "trim",
            vec![
                EditOperation::SetTrimStart(MediaTime::from_nanoseconds(500_000_000)),
                EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(1_500_000_000)),
            ],
            (160, 96),
            8,
        ),
        (
            "timeline",
            vec![
                EditOperation::Timeline(TimelineEdit::Keep(
                    TimeRange::new(
                        MediaTime::from_nanoseconds(500_000_000),
                        MediaTime::from_nanoseconds(1_500_000_000),
                    )
                    .expect("range"),
                )),
                EditOperation::FlipVertical,
            ],
            (160, 96),
            8,
        ),
        (
            "resize",
            vec![EditOperation::ResizeVideo(
                VideoResize::new((128, 80), ResampleFilter::Lanczos, (160, 96), 1.0)
                    .expect("resize"),
            )],
            (128, 80),
            16,
        ),
        (
            "rotate",
            vec![EditOperation::RotateVideo(rotation)],
            rotation.size(),
            16,
        ),
    ];
    for extension in ["mp4", "mkv", "webm"] {
        for (name, operations, size, count) in &cases {
            let target = root.join(format!("{name}.{extension}"));
            fs::write(&target, b"previous output").expect("existing output");
            let outcome = export_media(&ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind: MediaKind::Video,
                operations: operations.clone(),
                hardware_encode: true,
            })
            .expect("high-depth public export");
            assert!(!outcome.used_hardware_encoder);
            assert_video_fields(
                &target,
                &[
                    "codec_name=av1",
                    "pix_fmt=yuv420p10le",
                    &format!("width={}", size.0),
                    &format!("height={}", size.1),
                    &format!("nb_read_frames={count}"),
                    "color_range=tv",
                    "color_space=bt709",
                    "color_transfer=bt709",
                    "color_primaries=bt709",
                ],
            );
            let audio = run(
                "ffprobe.exe",
                &[
                    "-v",
                    "error",
                    "-select_streams",
                    "a:0",
                    "-show_entries",
                    "stream=codec_name",
                    "-of",
                    "default=noprint_wrappers=1",
                    target.to_str().expect("path"),
                ],
            );
            assert!(
                String::from_utf8_lossy(&audio.stdout).contains(if extension == "webm" {
                    "codec_name=opus"
                } else {
                    "codec_name=aac"
                })
            );
        }
    }
    assert_eq!(fs::read(&source).expect("source"), source_bytes);
    assert_eq!(
        fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        modified
    );
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn high_depth_small_chroma_hdr_and_rejection_controls() {
    let root = depth_directory();
    for (pixel, size) in [
        ("yuv420p10le", "32x24"),
        ("yuv420p10le", "65x49"),
        ("yuv444p10le", "80x64"),
        ("yuv422p12le", "80x64"),
    ] {
        let source = root.join(format!("{pixel}-{size}.mkv"));
        depth_fixture(
            &source,
            pixel,
            size,
            &[
                "-color_range",
                "tv",
                "-colorspace",
                "bt2020nc",
                "-color_trc",
                "smpte2084",
                "-color_primaries",
                "bt2020",
            ],
        );
        let target = root.join(format!("{pixel}-{size}-export.mkv"));
        let mut request = ExportRequest {
            source: source.clone(),
            target: target.clone(),
            kind: MediaKind::Video,
            operations: vec![EditOperation::FlipVertical],
            hardware_encode: true,
        };
        export_media(&request).expect("AOM precision path");
        assert_video_fields(
            &target,
            &[
                "codec_name=av1",
                &format!("pix_fmt={pixel}"),
                "nb_read_frames=16",
                "color_space=bt2020nc",
                "color_transfer=smpte2084",
                "color_primaries=bt2020",
            ],
        );
        request.target = root.join("unsupported.avi");
        fs::write(&request.target, b"keep existing output").expect("previous output");
        let error = export_media(&request).expect_err("no implicit precision reduction");
        assert!(error.to_string().contains("requires MP4"), "{error}");
        assert_eq!(
            fs::read(&request.target).expect("existing output"),
            b"keep existing output"
        );
        request.target = target.clone();
        let previous = fs::read(&target).expect("encoded target");
        assert!(matches!(
            export_cancellable(&request, &AtomicBool::new(true), &|_| {}),
            Err(ExportError::Cancelled)
        ));
        assert_eq!(fs::read(&target).expect("unchanged output"), previous);
        let encoding = video_encoding::HighDepth::probe(&request)
            .expect("probe")
            .expect("depth plan");
        let invalid = root.join("invalid.mkv");
        fs::write(&invalid, b"invalid stage").expect("invalid output");
        assert!(
            encoding.verify(&invalid).is_err(),
            "an invalid stage cannot publish"
        );
    }
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

fn precision_psnr(reference: &Path, encoded: &Path) -> f64 {
    let output = run(
        "ffmpeg.exe",
        &[
            "-hide_banner",
            "-i",
            reference.to_str().expect("path"),
            "-i",
            encoded.to_str().expect("path"),
            "-filter_complex",
            "[0:v]format=yuv420p10le[r];[1:v]format=yuv420p10le[e];[r][e]psnr",
            "-an",
            "-f",
            "null",
            "-",
        ],
    );
    let text = String::from_utf8_lossy(&output.stderr);
    let summary = text
        .lines()
        .find(|line| line.contains("PSNR y:"))
        .expect("PSNR summary");
    summary
        .split("average:")
        .nth(1)
        .expect("average")
        .split_whitespace()
        .next()
        .expect("score")
        .parse()
        .expect("PSNR")
}

#[test]
fn high_depth_gradient_quality_survives_resize_without_an_eight_bit_intermediate() {
    use towavue_core::{ResampleFilter, VideoResize};
    let root = depth_directory();
    let source = root.join("source.mkv");
    depth_fixture(
        &source,
        "yuv420p10le",
        "512x256",
        &[
            "-color_range",
            "tv",
            "-colorspace",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_primaries",
            "bt709",
        ],
    );
    for (name, edits, filter) in [
        ("flip", vec![EditOperation::FlipHorizontal], "hflip"),
        (
            "resize",
            vec![EditOperation::ResizeVideo(
                VideoResize::new((384, 192), ResampleFilter::Lanczos, (512, 256), 1.0)
                    .expect("resize"),
            )],
            "format=gbrp16le,scale=384:192:flags=lanczos+full_chroma_inp,format=gbrp16le,setsar=1",
        ),
    ] {
        let reference = root.join(format!("{name}-reference.mkv"));
        let baseline = root.join(format!("{name}-old.mkv"));
        let actual = root.join(format!("{name}-new.mkv"));
        let reference_filter = format!("{filter},format=yuv420p10le,copy");
        run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-i",
                source.to_str().expect("path"),
                "-an",
                "-vf",
                &reference_filter,
                "-c:v",
                "ffv1",
                reference.to_str().expect("path"),
            ],
        );
        let old_filter = format!("{},copy", filter.replace("gbrp16le", "gbrp"));
        let mut old_args = vec![
            "-v",
            "error",
            "-i",
            source.to_str().expect("path"),
            "-an",
            "-vf",
            &old_filter,
            "-c:v",
            "libopenh264",
        ];
        old_args.extend_from_slice(SOFTWARE_H264_QUALITY);
        old_args.push(baseline.to_str().expect("path"));
        run("ffmpeg.exe", &old_args);
        export_media(&ExportRequest {
            source: source.clone(),
            target: actual.clone(),
            kind: MediaKind::Video,
            operations: edits,
            hardware_encode: false,
        })
        .expect("precision export");
        let old = precision_psnr(&reference, &baseline);
        let new = precision_psnr(&reference, &actual);
        eprintln!("EXPORT_DEPTH_QUALITY {name} old_psnr={old:.3} new_psnr={new:.3}");
        assert!(new >= 45.0 && new > old + 1.0, "{name}: {old} -> {new}");
    }
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
#[ignore = "requires an explicitly selected short 10-bit reference clip"]
fn reference_high_depth_public_export_preserves_precision_and_reports_quality() {
    let source = PathBuf::from(
        std::env::var_os("TOWAVUE_EXPORT_DEPTH_REFERENCE").expect("explicit short reference clip"),
    );
    let stamp = fs::metadata(&source).expect("source metadata");
    let modified = stamp.modified().expect("mtime");
    let root = depth_directory();
    let target = root.join("export.mkv");
    let outcome = export_media(&ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: MediaKind::Video,
        operations: Vec::new(),
        hardware_encode: true,
    })
    .expect("public export");
    assert!(!outcome.used_hardware_encoder);
    assert_video_fields(&target, &["codec_name=av1", "pix_fmt=yuv420p10le"]);
    let score = precision_psnr(&source, &target);
    eprintln!(
        "EXPORT_DEPTH_REFERENCE psnr={score:.3} bytes={}",
        fs::metadata(&target).expect("output").len()
    );
    assert!(score >= 45.0, "reference PSNR {score}");
    assert_eq!(fs::metadata(&source).expect("source").len(), stamp.len());
    assert_eq!(
        fs::metadata(&source)
            .expect("source")
            .modified()
            .expect("mtime"),
        modified
    );
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn live_high_depth_export_cancellation_preserves_targets_cleans_staging_and_allows_retry() {
    use std::collections::BTreeSet;
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::mpsc;
    use std::time::Instant;
    use towavue_core::MediaTime;

    let root = depth_directory();
    let entries = || {
        fs::read_dir(&root)
            .expect("owned export directory")
            .map(|entry| entry.expect("directory entry").file_name())
            .collect::<BTreeSet<_>>()
    };
    for (pixel, encoder) in [("yuv420p10le", "libsvtav1"), ("yuv444p10le", "libaom-av1")] {
        let source = root.join(format!("{pixel}.mkv"));
        // Video only: positive output time cannot come from an audio packet.
        // Temporal detail keeps these bounded fixtures encoding beyond the first
        // progress update on both encoders, without private media or mock helpers.
        let filter = format!(
            "nullsrc=size=160x96:rate=30:duration=120,format={pixel},geq=lum=mod(X*Y*17+N*91\\,877)+64:cb=mod(X*31+Y*7+N*23\\,897)+64:cr=mod(X*11+Y*41+N*43\\,897)+64"
        );
        run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &filter,
                "-c:v",
                "ffv1",
                source.to_str().expect("source path"),
            ],
        );
        let original = fs::read(&source).expect("source bytes");
        let modified = fs::metadata(&source)
            .expect("source metadata")
            .modified()
            .expect("source date");
        for extension in ["mp4", "mkv", "webm"] {
            for drop_to_cancel in [false, true] {
                let target = root.join(format!("{pixel}-{drop_to_cancel}.{extension}"));
                fs::write(&target, b"previous export must survive").expect("existing target");
                let target_date = fs::metadata(&target)
                    .expect("target metadata")
                    .modified()
                    .expect("target date");
                let before = entries();
                let request = ExportRequest {
                    source: source.clone(),
                    target: target.clone(),
                    kind: MediaKind::Video,
                    operations: vec![EditOperation::FlipVertical],
                    hardware_encode: true,
                };
                let plan = video_encoding::HighDepth::probe(&request)
                    .expect("source depth")
                    .expect("AV1 plan");
                assert!(
                    plan.arguments(&target)
                        .windows(2)
                        .any(|pair| pair == ["-c:v", encoder])
                );
                let (tx, rx) = mpsc::channel();
                let job = ExportJob::start(request, move |event| {
                    let _ = tx.send(event);
                })
                .expect("public export job");
                let deadline = Instant::now() + Duration::from_secs(20);
                let first = loop {
                    match rx
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .expect("live encoder progress")
                    {
                        ExportEvent::Progress(time) if !time.is_zero() => break time,
                        ExportEvent::Finished(result) => {
                            panic!("export finished before live cancellation: {result:?}")
                        }
                        _ => {}
                    }
                };
                assert!(
                    first < Duration::from_secs(119),
                    "cancel before final output time: {first:?}"
                );
                assert!(!job.thread.as_ref().expect("worker").is_finished());
                let stages: Vec<_> = entries().difference(&before).cloned().collect();
                assert_eq!(stages.len(), 1, "one owned live staging directory");
                let stage = root.join(&stages[0]).join(format!("output.{extension}"));
                // A sharing violation proves the staged file still has a live
                // encoder handle; merely seeing progress or an old file is weaker.
                let locked = fs::OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(&stage)
                    .expect_err("encoder still owns its output file");
                assert_eq!(
                    locked.raw_os_error(),
                    Some(32),
                    "live stage sharing violation: {locked}"
                );
                assert_eq!(
                    fs::read(&target).expect("target during encode"),
                    b"previous export must survive"
                );
                let cancelled_at = Instant::now();
                let mut job = Some(job);
                if drop_to_cancel {
                    drop(job.take());
                } else {
                    job.as_ref().expect("job").cancel();
                }
                loop {
                    match rx
                        .recv_timeout(Duration::from_secs(10))
                        .expect("cancel completion")
                    {
                        ExportEvent::Finished(result) => {
                            assert!(matches!(result, Err(ExportError::Cancelled)), "{result:?}");
                            break;
                        }
                        ExportEvent::Progress(_) => {}
                        ExportEvent::AnalyzingAudio(_) => {
                            panic!("video-only fixture cannot analyze audio")
                        }
                    }
                }
                drop(job);
                let elapsed = cancelled_at.elapsed();
                assert!(
                    elapsed < Duration::from_secs(10),
                    "bounded job/pipe cleanup: {elapsed:?}"
                );
                assert!(
                    matches!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected)),
                    "one terminal event and no retained callback"
                );
                assert_eq!(entries(), before, "remove all owned staging on cancel");
                assert_eq!(
                    fs::read(&target).expect("target after cancel"),
                    b"previous export must survive"
                );
                assert_eq!(
                    fs::metadata(&target)
                        .expect("target metadata")
                        .modified()
                        .expect("target date"),
                    target_date
                );
                assert_eq!(fs::read(&source).expect("unchanged source"), original);
                assert_eq!(
                    fs::metadata(&source)
                        .expect("source metadata")
                        .modified()
                        .expect("source date"),
                    modified
                );
                eprintln!(
                    "PASS live {encoder} {extension}: drop={drop_to_cancel}, first={first:?}, cleanup={elapsed:?}"
                );
            }
        }
        let retry = root.join(format!("{pixel}-retry.mkv"));
        export_media(&ExportRequest {
            source: source.clone(),
            target: retry.clone(),
            kind: MediaKind::Video,
            operations: vec![
                EditOperation::FlipVertical,
                EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(100_000_000)),
            ],
            hardware_encode: true,
        })
        .expect("successful export after cancellation");
        assert_video_fields(
            &retry,
            &[
                "codec_name=av1",
                &format!("pix_fmt={pixel}"),
                "nb_read_frames=3",
            ],
        );
    }
    fs::remove_dir_all(root).expect("remove owned cancellation fixtures");
}

/// Opt-in real-source continuation. Retain uniquely owned outputs for a later
/// visual review; numeric quality and matching timestamps are not visual approval.
#[test]
#[ignore = "requires an explicitly selected 10-bit source and a trim start in seconds"]
fn reference_high_depth_minute_trim_matches_independent_frames_and_reports_quality() {
    let source = PathBuf::from(
        std::env::var_os("TOWAVUE_EXPORT_TRIM_REFERENCE").expect("explicit reference source"),
    );
    let start: i64 = std::env::var("TOWAVUE_EXPORT_TRIM_START_SECONDS")
        .expect("explicit trim start")
        .parse()
        .expect("whole seconds");
    assert!((0..=24 * 60 * 60).contains(&start), "bounded trim position");
    let end = start + 60;
    let root = depth_directory();
    eprintln!("EXPORT_MINUTE_ROOT={}", root.display());
    let stamp = fs::metadata(&source).expect("source metadata");
    let modified = stamp.modified().expect("source mtime");
    let reference = root.join("reference.mkv");
    let target = root.join("av1.mkv");
    let baseline = root.join("h264.mkv");
    // Independent input seek ten seconds earlier, retaining absolute source PTS
    // for trim. This does not call the app's seek/filter/encoder argument builder.
    let filter = format!("trim=start={start}:end={end},setpts=PTS-STARTPTS");
    run(
        "ffmpeg.exe",
        &[
            "-v",
            "error",
            "-copyts",
            "-ss",
            &(start - 10).max(0).to_string(),
            "-i",
            source.to_str().expect("source path"),
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-vf",
            &filter,
            "-fps_mode",
            "passthrough",
            "-enc_time_base",
            "demux",
            "-c:v",
            "ffv1",
            "-level",
            "3",
            reference.to_str().expect("reference path"),
        ],
    );
    fs::write(&target, b"previous target").expect("old target");
    let outcome = export_media(&ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: MediaKind::Video,
        operations: vec![
            EditOperation::SetTrimStart(towavue_core::MediaTime::from_nanoseconds(
                start * 1_000_000_000,
            )),
            EditOperation::SetTrimEnd(towavue_core::MediaTime::from_nanoseconds(
                end * 1_000_000_000,
            )),
        ],
        hardware_encode: true,
    })
    .expect("public minute trim export");
    assert!(!outcome.used_hardware_encoder);
    assert_video_fields(&target, &["codec_name=av1", "pix_fmt=yuv420p10le"]);
    run(
        "ffmpeg.exe",
        &[
            "-v",
            "error",
            "-i",
            reference.to_str().expect("reference path"),
            "-an",
            "-c:v",
            "libopenh264",
            "-profile:v",
            "high",
            "-qmin:v",
            "1",
            "-qmax:v",
            "20",
            baseline.to_str().expect("baseline path"),
        ],
    );
    let timestamps = |path: &Path| {
        let output = run(
            "ffprobe.exe",
            &[
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_frames",
                "-show_entries",
                "frame=best_effort_timestamp_time",
                "-of",
                "csv=p=0",
                path.to_str().expect("video path"),
            ],
        );
        String::from_utf8(output.stdout)
            .expect("timestamp output")
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                line.trim_end_matches(',')
                    .parse::<f64>()
                    .expect("timestamp")
            })
            .collect::<Vec<_>>()
    };
    let fields = |path: &Path| {
        run("ffprobe.exe", &[
        "-v", "error", "-select_streams", "v:0", "-show_entries",
        "stream=width,height,pix_fmt,color_range,color_space,color_transfer,color_primaries,sample_aspect_ratio",
        "-of", "default=noprint_wrappers=1", path.to_str().expect("video path"),
    ]).stdout
    };
    let reference_fields = fields(&reference);
    let target_fields = fields(&target);
    fs::write(root.join("reference-video.txt"), &reference_fields).expect("reference fields");
    fs::write(root.join("av1-video.txt"), &target_fields).expect("output fields");
    assert_eq!(
        target_fields, reference_fields,
        "precision, geometry and known color tags"
    );
    let expected = timestamps(&reference);
    assert!(!expected.is_empty() && expected[0] == 0.0);
    assert!(expected.last().expect("last frame") < &60.0);
    assert_eq!(
        timestamps(&target),
        expected,
        "every exported frame timestamp"
    );
    assert_eq!(
        timestamps(&baseline),
        expected,
        "historical comparison alignment"
    );
    // Native PSNR emits one record per paired frame; retain those records for
    // localized artifacts, instead of relying only on a whole-clip mean.
    let quality = |path: &Path, name: &str| {
        let output = Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
            .creation_flags(0x0800_0000).current_dir(&root)
            .args(["-hide_banner", "-i"]).arg(&reference).arg("-i").arg(path)
            .args(["-filter_complex", &format!("[0:v]format=yuv420p10le[r];[1:v]format=yuv420p10le[e];[r][e]psnr=stats_file={name}-frames.log"),
                "-an", "-f", "null", "-"])
            .output().expect("quality comparison");
        fs::write(root.join(format!("{name}-quality.log")), &output.stderr).expect("quality log");
        assert!(output.status.success(), "quality helper failed");
        let stats =
            fs::read_to_string(root.join(format!("{name}-frames.log"))).expect("frame scores");
        assert_eq!(stats.lines().count(), expected.len(), "one score per frame");
        let text = String::from_utf8_lossy(&output.stderr);
        let summary = text
            .lines()
            .find(|line| line.contains("PSNR y:"))
            .expect("PSNR summary");
        eprintln!("EXPORT_MINUTE_{name} {summary}");
        summary
            .split("average:")
            .nth(1)
            .expect("average")
            .split_whitespace()
            .next()
            .expect("score")
            .parse::<f64>()
            .expect("numeric score")
    };
    let old_score = quality(&baseline, "h264");
    let score = quality(&target, "av1");
    assert!(score.is_finite() && score >= 45.0, "reference PSNR {score}");
    eprintln!(
        "EXPORT_MINUTE start={start} end={end} frames={} av1_psnr={score:.3} h264_psnr={old_score:.3} av1_bytes={} h264_bytes={}",
        expected.len(),
        fs::metadata(&target).expect("output").len(),
        fs::metadata(&baseline).expect("baseline").len()
    );
    assert_eq!(fs::metadata(&source).expect("source").len(), stamp.len());
    assert_eq!(
        fs::metadata(&source)
            .expect("source")
            .modified()
            .expect("mtime"),
        modified
    );
    assert!(fs::read_dir(&root).expect("artifacts").all(|entry| {
        !entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".towavue-export-")
    }));
}
