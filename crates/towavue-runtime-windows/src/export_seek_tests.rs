use super::*;
use std::os::windows::process::CommandExt;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use towavue_core::{MediaTime, TimeRange, TimelineEdit};

fn run(executable: &Path, arguments: &[String]) -> Duration {
    let started = Instant::now();
    let output = Command::new(executable)
        .creation_flags(0x0800_0000)
        .args(arguments)
        .output()
        .expect("owned FFmpeg process");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    started.elapsed()
}

fn content(path: &Path) -> (Vec<Vec<u8>>, Vec<u8>) {
    let (mut video, mut audio) = (Vec::new(), Vec::new());
    crate::decode::decode_file(path, |output| {
        match output {
            crate::DecodeOutput::Video(frame) => video.push(frame.rgba),
            crate::DecodeOutput::Audio(chunk) => audio.extend(chunk.bytes),
        }
        true
    })
    .expect("decode output");
    (video, audio)
}

#[test]
fn video_seek_only_changes_trimmed_video_inputs_and_keeps_microsecond_bounds() {
    let mut request = ExportRequest {
        source: PathBuf::from("source.mkv"),
        target: PathBuf::from("target.mkv"),
        kind: MediaKind::Video,
        operations: Vec::new(),
        hardware_encode: false,
    };
    let mut streams = ExportStreams {
        video: Some((3, ffmpeg::Rational(1, 1000))),
        audio: Some((5, ffmpeg::Rational(1, 48000))),
        ..Default::default()
    };
    for (ns, expected) in [
        (0, None),
        (999_999_999, None),
        (1_000_000_999, None),
        (1_000_001_001, Some(1000)),
        (8_137_000_999, Some(7_137_000_000)),
    ] {
        request.operations = vec![EditOperation::SetTrimStart(MediaTime::from_nanoseconds(ns))];
        assert_eq!(
            video_input_seek(&request, &streams).map(MediaTime::as_nanoseconds),
            expected
        );
    }
    let args = ffmpeg_arguments(&request, false, &streams);
    assert!(args.windows(2).any(|pair| pair == ["-map", "1:3"]));
    assert!(args.windows(2).any(|pair| pair == ["-map", "0:5"]));
    assert!(args.windows(2).any(|pair| pair == ["-ss", "7.137000"]));
    assert_eq!(args.iter().filter(|arg| *arg == "-i").count(), 2);
    for kind in [MediaKind::Image, MediaKind::Audio] {
        request.kind = kind;
        assert!(video_input_seek(&request, &streams).is_none());
    }
    request.kind = MediaKind::Video;
    streams.video = None;
    assert!(video_input_seek(&request, &streams).is_none());
    streams.video = Some((3, ffmpeg::Rational(1, 1000)));
    request.operations = vec![
        EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(9_000_000_000)),
        EditOperation::FlipHorizontal,
    ];
    assert!(video_input_seek(&request, &streams).is_none());
}

#[test]
fn video_input_seek_preserves_trimmed_frames_audio_and_nonzero_origins() {
    trial(false);
}

#[test]
#[ignore = "generated late-trim comparison; run alone without concurrent builds"]
fn late_video_trim_reports_sequential_and_input_seek_cost() {
    trial(true);
}

fn trial(long: bool) {
    let root = std::env::temp_dir().join(format!(
        "towavue-export-seek-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("owned fixture directory");
    let executable = crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg");
    let mut failures = Vec::new();
    for (name, video_codec, audio_codec, container) in [
        ("lossless", "ffv1", "pcm_s16le", "mkv"),
        ("shifted", "ffv1", "pcm_s16le", "mkv"),
        ("video-only", "mpeg4", "aac", "mp4"),
        ("bframes", "mpeg4", "aac", "mp4"),
        ("coarse-aac", "mpeg4", "aac", "mkv"),
    ] {
        if long && name != "bframes" {
            continue;
        }
        let seconds = if long { 180 } else { 12 };
        let size = if long { "640x360" } else { "160x96" };
        let video_fixture = format!("testsrc2=size={size}:rate=25:duration={seconds}");
        let audio_fixture = format!("aevalsrc=sin(2*PI*(317*t+71*t*t)):s=48000:d={seconds}");
        let source = root.join(format!("{name}.{container}"));
        let mut args: Vec<String> = [
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &video_fixture,
            "-f",
            "lavfi",
            "-i",
            &audio_fixture,
            "-c:v",
            video_codec,
            "-g",
            "100",
            "-c:a",
            audio_codec,
        ]
        .map(String::from)
        .to_vec();
        if video_codec == "mpeg4" {
            args.extend(["-bf".into(), "2".into()]);
        }
        if name == "shifted" {
            args.extend(["-output_ts_offset".into(), "5".into()]);
        }
        if name == "video-only" {
            args.push("-an".into());
        }
        args.push(source.display().to_string());
        run(&executable, &args);
        let bytes = fs::read(&source).expect("source bytes");
        for mode in 0..3 {
            let timeline = mode != 0;
            let start_ns = if long { 176_137_000_000 } else { 8_137_000_000 };
            let start = MediaTime::from_nanoseconds(start_ns);
            let end = MediaTime::from_nanoseconds(start_ns + 2_046_000_000);
            let mut request = ExportRequest {
                source: source.clone(),
                target: root.join("sequential.mkv"),
                kind: MediaKind::Video,
                operations: if timeline {
                    vec![EditOperation::Timeline(TimelineEdit::Keep(
                        TimeRange::new(start, end).expect("range"),
                    ))]
                } else {
                    vec![
                        EditOperation::SetTrimStart(start),
                        EditOperation::SetTrimEnd(end),
                    ]
                },
                hardware_encode: false,
            };
            if mode == 2 {
                request.operations.extend([
                    EditOperation::Timeline(TimelineEdit::Delete(
                        TimeRange::new(
                            MediaTime::from_nanoseconds(500_000_000),
                            MediaTime::from_nanoseconds(1_100_000_000),
                        )
                        .expect("gap"),
                    )),
                    EditOperation::SetRate(1.5),
                    EditOperation::FlipHorizontal,
                ]);
            }
            let mut streams = ExportStreams::probe(&request).expect("streams");
            if timeline {
                streams.timeline = towavue_core::EditTimeline::from_operations(
                    streams.duration.expect("duration"),
                    &request.operations,
                );
            }
            let mut args = ffmpeg_arguments(&request, false, &streams);
            // Both routes use the same lossless output to expose source selection
            // and audio alignment rather than lossy encoder differences.
            for index in 0..args.len() - 1 {
                if args[index] == "-c:v" {
                    args[index + 1] = "ffv1".into();
                }
                if args[index] == "-c:a" {
                    args[index + 1] = "pcm_s16le".into();
                }
            }
            assert!(args.iter().any(|arg| arg == "-ss"));
            assert_eq!(
                args.iter().filter(|arg| *arg == "-i").count(),
                if name == "video-only" { 1 } else { 2 }
            );
            let baseline_args = sequential(args.clone());
            let baseline_ms = run(&executable, &baseline_args).as_secs_f64() * 1000.0;
            let baseline = content(&request.target);
            assert!(!baseline.0.is_empty());
            assert_eq!(baseline.1.is_empty(), name == "video-only");
            let candidate = root.join("candidate.mkv");
            *args.last_mut().expect("output") = candidate.display().to_string();
            let seek_ms = run(&executable, &args).as_secs_f64() * 1000.0;
            let actual = content(&candidate);
            let video_equal = actual.0 == baseline.0;
            let audio_equal = actual.1 == baseline.1;
            eprintln!(
                "SEEK_CANDIDATE {name} mode={mode} video_equal={video_equal} audio_equal={audio_equal} frames={}/{} audio_bytes={}/{} sequential_ms={baseline_ms:.3} seek_ms={seek_ms:.3}",
                actual.0.len(),
                baseline.0.len(),
                actual.1.len(),
                baseline.1.len()
            );
            if !video_equal || !audio_equal {
                failures.push(format!("{name} mode={mode}"));
            }
            if !long && name == "bframes" && mode == 1 {
                // Exercise the public staged-export path with ordinary production
                // codecs too, rather than only the lossless argument-level control.
                run(
                    &executable,
                    &sequential(ffmpeg_arguments(&request, false, &streams)),
                );
                let expected = content(&request.target);
                let published = root.join("published.mkv");
                fs::write(&published, b"previous target").expect("existing target");
                export_media(&ExportRequest {
                    target: published.clone(),
                    ..request.clone()
                })
                .expect("public trimmed export");
                let result = content(&published);
                assert!(
                    result.0 == expected.0,
                    "published video matches sequential export"
                );
                assert!(
                    result.1 == expected.1,
                    "published audio matches sequential export"
                );
            }
        }
        assert_eq!(fs::read(&source).expect("unchanged source"), bytes);
    }
    // This directory contains only this test's generated inputs and outputs.
    fs::remove_dir_all(&root).expect("remove owned fixture");
    assert!(
        failures.is_empty(),
        "candidate mismatches: {}",
        failures.join(", ")
    );
}

// Reconstruct the old single-input route without changing filters or codecs.
fn sequential(mut args: Vec<String>) -> Vec<String> {
    let begin = args
        .iter()
        .position(|arg| arg == "pipe:1")
        .expect("progress")
        + 1;
    let end = args
        .iter()
        .position(|arg| arg == "-map_metadata")
        .expect("metadata");
    let source = args[end - 1].clone();
    args.splice(begin..end, ["-i".into(), source]);
    for index in 1..args.len() {
        if args[index - 1] == "-map" && args[index].starts_with("1:") {
            args[index].replace_range(..1, "0");
        }
        if args[index - 1] == "-filter_complex" {
            args[index] = args[index].replace("[1:", "[0:");
        }
    }
    args
}
