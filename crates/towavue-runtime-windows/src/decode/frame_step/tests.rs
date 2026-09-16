use super::*;
use towavue_core::{PlaybackRange, TimeRange, TimelineEdit};

fn time(ns: i64) -> MediaTime {
    MediaTime::from_nanoseconds(ns)
}

#[test]
#[ignore = "Release 100 backward queries on explicit read-only TOWAVUE_SEEK_REFERENCE_SOURCE"]
fn reference_cache_reports_hundred_backward_steps() {
    if cfg!(debug_assertions) {
        panic!("Release timing only");
    }
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit reference"),
    );
    let stamp = || {
        crate::export::frame::SourceLease::open(&path)
            .expect("identity")
            .source
    };
    let original = stamp();
    let mut expected = None;
    for cached in [false, true, true, false] {
        let mut cache = FrameStepCache::default();
        let mut target = time(60_550_000_000);
        let mut selected = Vec::new();
        let mut costs = Vec::new();
        for _ in 0..100 {
            let start = std::time::Instant::now();
            let next = if cached {
                cache.adjacent(&path, target, false, None, &|| false)
            } else {
                adjacent_video_frame(&path, target, false, None, &|| false)
            }
            .expect("adjacent query")
            .expect("reference contains enough frames");
            costs.push(start.elapsed().as_secs_f64() * 1000.0);
            assert!(next < target);
            selected.push(next);
            target = next;
        }
        if let Some(expected) = &expected {
            assert_eq!(&selected, expected, "all 100 distinct PTS match");
        } else {
            expected = Some(selected);
        }
        let total: f64 = costs.iter().sum();
        costs.sort_by(f64::total_cmp);
        println!(
            "FRAME_CACHE_BURST cached={cached} queries=100 scans={} total_ms={total:.3} median_ms={:.3} p95_ms={:.3} max_ms={:.3}",
            if cached { cache.misses } else { 100 },
            costs[50],
            costs[94],
            costs[99]
        );
        assert_eq!(stamp(), original, "source remains unchanged");
    }
}

fn with_mode<T>(mode: u8, run: impl FnOnce() -> T) -> T {
    struct Restore(u8);
    impl Drop for Restore {
        fn drop(&mut self) {
            PIXEL_MODE.set(self.0);
        }
    }
    let _restore = Restore(PIXEL_MODE.replace(mode));
    run()
}

#[test]
fn h264_hevc_probe_modes_preserve_full_decode_pts() {
    for file in ["h264-aac.mp4", "hevc-aac.mkv"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/generated/m1")
            .join(file);
        let mut reference = Vec::new();
        decode_file(&path, |item| {
            if let DecodeOutput::Video(frame) = item {
                reference.push(frame.presentation_time);
            }
            true
        })
        .expect("unmodified full decoder");
        assert!(reference.len() > 20);
        let mut plan = EditTimeline::new(
            reference
                .last()
                .expect("last PTS")
                .saturating_add(Duration::from_secs(1)),
            PlaybackRange::default(),
        )
        .expect("plan");
        assert!(plan.apply(TimelineEdit::Delete(
            TimeRange::new(time(350_000_000), time(910_000_000)).expect("cut")
        )));
        assert!(plan.apply(TimelineEdit::Stretch(
            TimeRange::new(time(200_000_000), time(700_000_000)).expect("stretch"),
            time(800_000_000)
        )));
        let edited: Vec<_> = reference
            .iter()
            .filter_map(|source| plan.edited_time(*source))
            .collect();
        for mode in [0, 1, 2] {
            with_mode(mode, || verify(&path, &reference, None));
            with_mode(mode, || verify(&path, &edited, Some(&plan)));
        }
    }
}

#[test]
#[ignore = "Release explicit read-only TOWAVUE_SEEK_REFERENCE_SOURCE; timestamp-only, no pixel output"]
fn reference_probe_reports_pixel_work_cost() {
    if cfg!(debug_assertions) {
        panic!("use Release");
    }
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit source"),
    );
    let stamp = || {
        let m = std::fs::metadata(&path).expect("metadata");
        (m.len(), m.modified().expect("mtime"))
    };
    let original = stamp();
    for target_ms in [60_550, 64_550, 300_550] {
        for forward in [false, true] {
            let mut expected = None;
            for mode in [0, 1, 2, 2, 1, 0] {
                let start = std::time::Instant::now();
                let actual = with_mode(mode, || {
                    adjacent_video_frame(&path, time(target_ms * 1_000_000), forward, None, &|| {
                        false
                    })
                })
                .expect("reference PTS query");
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                assert!(actual.is_some());
                if let Some(expected) = expected {
                    assert_eq!(actual, expected);
                } else {
                    expected = Some(actual);
                }
                println!(
                    "FRAME_PROBE target_ms={target_ms} forward={forward} mode={mode} elapsed_ms={elapsed_ms:.4}"
                );
            }
        }
    }
    assert_eq!(stamp(), original);
}

#[test]
#[ignore = "Release 100 adjacent queries/direction on explicit read-only TOWAVUE_SEEK_REFERENCE_SOURCE"]
fn reference_probe_reports_hundred_consecutive_steps() {
    if cfg!(debug_assertions) {
        panic!("use Release");
    }
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit source"),
    );
    let stamp = || {
        let m = std::fs::metadata(&path).expect("metadata");
        (m.len(), m.modified().expect("mtime"))
    };
    let original = stamp();
    for forward in [true, false] {
        let mut expected = Vec::new();
        for mode in [0, 2] {
            let mut target = time(60_550_000_000);
            let mut selected = Vec::new();
            let mut costs = Vec::new();
            for _ in 0..100 {
                let start = std::time::Instant::now();
                let next = with_mode(mode, || {
                    adjacent_video_frame(&path, target, forward, None, &|| false)
                })
                .expect("adjacent query")
                .expect("reference contains enough frames");
                costs.push(start.elapsed().as_secs_f64() * 1000.0);
                assert!(if forward {
                    next > target
                } else {
                    next < target
                });
                selected.push(next);
                target = next;
            }
            if mode == 0 {
                expected = selected;
            } else {
                assert_eq!(selected, expected, "all 100 distinct PTS match");
            }
            let total: f64 = costs.iter().sum();
            costs.sort_by(f64::total_cmp);
            println!(
                "FRAME_PROBE_BURST forward={forward} mode={mode} queries=100 total_ms={total:.3} median_ms={:.3} p95_ms={:.3} max_ms={:.3}",
                costs[50], costs[94], costs[99]
            );
        }
    }
    assert_eq!(stamp(), original);
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
        let path = root.join(format!("{codec}.{extension}"));
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
        let mut pixels = Vec::new();
        decode_file(&path, |output| {
            if let DecodeOutput::Video(frame) = output {
                reference.push(frame.presentation_time);
                pixels.push(frame.rgba.to_vec());
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
        let mut playback = ParallelInput::open(&path, &|| false).expect("playback input");
        for (index, target) in reference.iter().enumerate() {
            let mut first = None;
            let mut first_pixels = None;
            playback
                .decode_software(
                    *target,
                    None,
                    Some(DecodeStream::Video),
                    &|| false,
                    |output| {
                        if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) =
                            output
                        {
                            first.get_or_insert(frame.presentation_time);
                            first_pixels.get_or_insert_with(|| frame.rgba.to_vec());
                        }
                        true
                    },
                )
                .expect("playback seek");
            assert_eq!(
                first,
                Some(*target),
                "{extension} playback exact target {target:?}"
            );
            assert_eq!(
                first_pixels.as_deref(),
                Some(pixels[index].as_slice()),
                "{codec} exact seek pixels at {target:?}"
            );
        }
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
    let mut cache = FrameStepCache::default();
    let mut targets = vec![MediaTime::ZERO, time(1_999_999_999)];
    for pts in reference {
        targets.extend([*pts, time(pts.as_nanoseconds() + 1)]);
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
            assert_eq!(
                cache
                    .adjacent(path, target, forward, plan, &|| false)
                    .expect("cached adjacent frame"),
                expected,
                "cache target={target:?} forward={forward}"
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
