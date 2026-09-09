use super::*;
use towavue_core::{PlaybackRange, TimeRange, TimelineEdit};

fn time(ns: i64) -> MediaTime {
    MediaTime::from_nanoseconds(ns)
}

#[test]
fn adjacent_pts_match_full_decode_for_vfr_b_frames_and_transport_origins() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-frame-step-{unique}"));
    std::fs::create_dir(&root).expect("owned fixtures");
    let ffmpeg = std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
        .join("bin/ffmpeg.exe");
    for (extension, codec) in [("mp4", "mpeg4"), ("mkv", "ffv1"), ("ts", "mpeg2video")] {
        let path = root.join(format!("vfr.{extension}"));
        let mut command = std::process::Command::new(&ffmpeg);
        command.args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x96:rate=25:duration=2",
            "-vf",
            "select='not(eq(mod(n,3),1))'",
            "-fps_mode",
            "vfr",
            "-c:v",
            codec,
            "-g",
            "8",
        ]);
        if extension != "mkv" {
            command.args(["-bf", "2"]);
        }
        assert!(command.arg(&path).status().expect("generate").success());
        if extension == "ts" {
            let input = format::input(&path).expect("TS metadata");
            assert!(input_origin(&input) > 0, "fixture has a nonzero start PTS");
        }
        if extension != "mkv" {
            let probe = std::process::Command::new(ffmpeg.with_file_name("ffprobe.exe"))
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-show_entries",
                    "frame=pict_type",
                    "-of",
                    "csv=p=0",
                ])
                .arg(&path)
                .output()
                .expect("B-frame evidence");
            assert!(probe.status.success());
            assert!(
                String::from_utf8_lossy(&probe.stdout)
                    .lines()
                    .any(|line| line.starts_with('B')),
                "fixture actually contains B frames"
            );
        }
        let mut reference = Vec::new();
        decode_file(&path, |output| {
            if let DecodeOutput::Video(frame) = output {
                reference.push(frame.presentation_time);
            }
            true
        })
        .expect("reference full decode");
        assert!(reference.len() > 20);
        assert!(
            reference
                .windows(3)
                .any(|w| w[1].as_nanoseconds() - w[0].as_nanoseconds()
                    != w[2].as_nanoseconds() - w[1].as_nanoseconds()),
            "fixture is actually VFR"
        );
        verify(&path, &reference, None);
        let mut plan =
            EditTimeline::new(time(2_000_000_000), PlaybackRange::default()).expect("plan");
        assert!(plan.apply(TimelineEdit::Delete(
            TimeRange::new(time(350_000_000), time(910_000_000)).expect("deleted interval")
        )));
        assert!(plan.apply(TimelineEdit::Stretch(
            TimeRange::new(time(200_000_000), time(700_000_000)).expect("stretch across join"),
            time(800_000_000)
        )));
        let edited = reference
            .iter()
            .filter_map(|source| plan.edited_time(*source))
            .collect::<Vec<_>>();
        verify(&path, &edited, Some(&plan));
    }
    std::fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn frame_query_uses_the_selected_stream_and_handles_empty_edits() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("towavue-frame-streams-{unique}.mkv"));
    let ffmpeg = std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
        .join("bin/ffmpeg.exe");
    assert!(
        std::process::Command::new(ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x96:rate=7:duration=2",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x96:rate=11:duration=2",
                "-map",
                "0:v",
                "-map",
                "1:v",
                "-c:v",
                "ffv1",
                "-disposition:v:0",
                "0",
                "-disposition:v:1",
                "default"
            ])
            .arg(&path)
            .status()
            .expect("generate selected-stream fixture")
            .success()
    );
    let input = format::input(&path).expect("input");
    assert_eq!(
        best_stream_config(&input, Type::Video)
            .expect("selected stream")
            .index,
        1
    );
    drop(input);
    let mut reference = Vec::new();
    let mut pixels = Vec::new();
    decode_file(&path, |output| {
        if let DecodeOutput::Video(frame) = output {
            reference.push(frame.presentation_time);
            pixels.push(frame.rgba);
        }
        true
    })
    .expect("selected-stream reference");
    assert_eq!(reference.len(), 22);
    verify(&path, &reference, None);
    for target in [time(90_000_000), time(630_000_000), time(1_100_000_000)] {
        let mut frames = Vec::new();
        decode_file_parallel(&path, target, None, Some(DecodeStream::Video), |output| {
            if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) = output {
                frames.push((frame.presentation_time, frame.rgba));
            }
            true
        })
        .expect("playback uses the same selected-stream seek");
        let expected = reference
            .iter()
            .copied()
            .zip(pixels.iter().cloned())
            .filter(|(pts, _)| *pts >= target)
            .collect::<Vec<_>>();
        assert_eq!(
            frames, expected,
            "selected-stream seek preserves exact decoded pixels"
        );
    }
    let range = TimeRange::new(MediaTime::ZERO, time(2_000_000_000)).expect("whole");
    let mut empty = EditTimeline::new(range.end(), PlaybackRange::default()).expect("plan");
    assert!(empty.apply(TimelineEdit::Delete(range)));
    for forward in [false, true] {
        assert_eq!(
            adjacent_video_frame(&path, MediaTime::ZERO, forward, Some(&empty), &|| false)
                .expect("empty plan"),
            None
        );
    }
    std::fs::remove_file(path).expect("remove owned fixture");
}

fn verify(path: &Path, reference: &[MediaTime], plan: Option<&EditTimeline>) {
    let mut targets = vec![MediaTime::ZERO, time(1_999_999_999)];
    for (index, pts) in reference.iter().enumerate() {
        if index % 3 == 0 || index + 1 == reference.len() {
            targets.extend([*pts, time(pts.as_nanoseconds() + 1)]);
        }
    }
    if let Some(plan) = plan {
        let mut boundary = 0_i64;
        for span in plan.spans() {
            boundary += span.duration().as_nanoseconds();
            targets.extend([time(boundary), time(boundary - 1), time(boundary + 1)]);
        }
    }
    for target in targets {
        for forward in [false, true] {
            let expected = if forward {
                reference.iter().copied().find(|pts| *pts > target)
            } else {
                reference.iter().copied().rev().find(|pts| *pts < target)
            };
            assert_eq!(
                adjacent_video_frame(path, target, forward, plan, &|| false)
                    .expect("adjacent frame"),
                expected,
                "{}: target={target:?} forward={forward}",
                path.display()
            );
        }
    }
}

#[test]
fn frame_query_cancels_before_io_and_during_preroll_without_poisoning_later_queries() {
    assert!(matches!(
        adjacent_video_frame(
            Path::new("absent.mp4"),
            MediaTime::ZERO,
            true,
            None,
            &|| true
        ),
        Err(DecodeError::ConsumerClosed)
    ));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    let calls = std::sync::atomic::AtomicUsize::new(0);
    assert!(matches!(
        adjacent_video_frame(&path, time(1_500_000_000), false, None, &|| calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            >= 6),
        Err(DecodeError::ConsumerClosed)
    ));
    assert!(
        adjacent_video_frame(&path, MediaTime::ZERO, true, None, &|| false)
            .expect("fresh query")
            .is_some()
    );
}
