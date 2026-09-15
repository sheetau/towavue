use super::*;
use std::time::Instant;

// Only the session opt-in changes the global default to reach playback threads.
static DEFER_PREROLL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
thread_local! {
    static LOCAL_DEFER_PREROLL: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    pub(super) static PREROLL_OBSERVED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn defer_preroll() -> bool {
    LOCAL_DEFER_PREROLL
        .get()
        .unwrap_or_else(|| DEFER_PREROLL.load(std::sync::atomic::Ordering::Relaxed))
}

fn deferred<T>(enabled: bool, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<bool>);
    impl Drop for Restore {
        fn drop(&mut self) {
            LOCAL_DEFER_PREROLL.set(self.0);
        }
    }
    let _restore = Restore(LOCAL_DEFER_PREROLL.replace(Some(enabled)));
    run()
}

fn session_deferred<T>(enabled: bool, run: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            DEFER_PREROLL.store(self.0, std::sync::atomic::Ordering::Relaxed);
        }
    }
    let _restore = Restore(DEFER_PREROLL.swap(enabled, std::sync::atomic::Ordering::Relaxed));
    run()
}

fn fixture(root: &Path, size: &str, gop: u32) -> std::path::PathBuf {
    let path = root.join(format!("video-{size}-{gop}.mp4"));
    let executable =
        std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
    let generated = std::process::Command::new(executable)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc2=size={size}:rate=30:duration=6"),
            "-c:v",
            "mpeg4",
            "-q:v",
            "5",
            "-g",
            &gop.to_string(),
            "-bf",
            "2",
            "-an",
        ])
        .arg(&path)
        .output()
        .expect("generate owned fixture");
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    path
}

fn root(label: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("towavue-{label}-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&path).expect("owned directory");
    path
}

fn equal_frames(actual: &[VideoFrame], expected: &[VideoFrame]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.presentation_time, expected.presentation_time);
        assert_eq!(actual.duration, expected.duration);
        assert_eq!(
            (actual.width, actual.height),
            (expected.width, expected.height)
        );
        assert_eq!(actual.pixel_aspect, expected.pixel_aspect);
        assert_eq!(actual.orientation, expected.orientation);
        assert!(actual.rgba == expected.rgba, "exact selected RGBA");
    }
}

#[test]
fn preroll_worker_test_controls_are_thread_local() {
    deferred(false, || {
        assert!(!defer_preroll());
        PREROLL_OBSERVED.set(5);
        std::thread::spawn(|| {
            assert!(defer_preroll());
            assert_eq!(PREROLL_OBSERVED.get(), 0);
        })
        .join()
        .expect("independent test thread");
        assert_eq!(PREROLL_OBSERVED.replace(0), 5);
    });
    assert!(defer_preroll());
}

#[test]
fn deferred_worker_preserves_bounds_eof_counts_and_cancelled_input_reuse() {
    let root = root("deferred-video-guards");
    let path = fixture(&root, "160x96", 180);
    let bytes = std::fs::read(&path).expect("bytes");
    let mut input = ParallelInput::open(&path, &|| false).expect("input");
    let initial_preroll = PREROLL_OBSERVED.get();
    input
        .decode_software(
            MediaTime::from_nanoseconds(550_000_000),
            Some(MediaTime::from_nanoseconds(650_000_000)),
            Some(DecodeStream::Video),
            &|| false,
            |_| true,
        )
        .expect("default construction, without enabling the comparison override");
    assert!(PREROLL_OBSERVED.get() > initial_preroll);
    for (start_ms, end_ms) in [
        (0, None),
        (550, None),
        (4550, None),
        (5980, None),
        (6000, None),
        (7000, None),
        (1000, Some(1000)),
        (550, Some(650)),
        (5950, Some(5980)),
        (0, Some(0)),
    ] {
        let target = MediaTime::from_nanoseconds(start_ms * 1_000_000);
        let end = end_ms.map(|ms| MediaTime::from_nanoseconds(ms * 1_000_000));
        let mut expected: Option<(Vec<VideoFrame>, u64)> = None;
        for mode in [false, true, true, false] {
            let mut frames = Vec::new();
            let mut eof = 0;
            let summary = deferred(mode, || {
                input.decode_software(target, end, Some(DecodeStream::Video), &|| false, |item| {
                    match item {
                        ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) => {
                            frames.push(frame)
                        }
                        ParallelSoftwareDecodeOutput::VideoFinished => eof += 1,
                        _ => panic!("video-only output"),
                    }
                    true
                })
            })
            .expect("complete reused-input decode");
            assert_eq!(eof, 1);
            if let Some((reference, count)) = &expected {
                equal_frames(&frames, reference);
                assert_eq!(summary.video_frames, *count, "preroll counts preserved");
            } else {
                expected = Some((frames, summary.video_frames));
            }
        }
    }
    for _ in 0..12 {
        PREROLL_OBSERVED.set(0);
        let start = Instant::now();
        let result = deferred(true, || {
            input.decode_software(
                MediaTime::from_nanoseconds(4_550_000_000),
                None,
                Some(DecodeStream::Video),
                &|| PREROLL_OBSERVED.get() >= 5,
                |_| panic!("cancel must precede target publication"),
            )
        });
        assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
        assert_eq!(PREROLL_OBSERVED.get(), 5);
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "bounded shutdown, not a latency target"
        );
    }
    let mut recovered = Vec::new();
    deferred(true, || {
        input.decode_software(
            MediaTime::ZERO,
            None,
            Some(DecodeStream::Video),
            &|| false,
            |item| {
                if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) = item {
                    recovered.push(frame);
                }
                true
            },
        )
    })
    .expect("same cancelled input rewinds and drains");
    assert_eq!(recovered.len(), 180);
    drop(input);
    assert_eq!(std::fs::read(&path).expect("unchanged source"), bytes);
    std::fs::remove_file(path).expect("owned file cleanup");
    std::fs::remove_dir(root).expect("empty directory");
}

#[test]
#[ignore = "Release actual software worker first-result/cleanup comparison; no concurrent timing"]
fn deferred_worker_reports_first_frame_and_cleanup_cost() {
    if cfg!(debug_assertions) {
        panic!("use Release");
    }
    let root = root("deferred-video-cost");
    for gop in [30, 180] {
        let path = fixture(&root, "1920x1080", gop);
        let bytes = std::fs::read(&path).expect("source bytes");
        let stamp = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        for milliseconds in [550, 4550] {
            let target = MediaTime::from_nanoseconds(milliseconds * 1_000_000);
            let (expected, _, _) = first_frame(&path, target, false);
            for mode in [false, true, true, false] {
                let mut first = Vec::new();
                let mut complete = Vec::new();
                for _ in 0..3 {
                    let start = Instant::now();
                    let mut actual = None;
                    let result = deferred(mode, || {
                        decode_file_parallel(
                            &path,
                            target,
                            None,
                            Some(DecodeStream::Video),
                            |item| {
                                if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(
                                    frame,
                                )) = item
                                {
                                    first.push(start.elapsed().as_secs_f64() * 1000.0);
                                    actual = Some(frame);
                                    false
                                } else {
                                    true
                                }
                            },
                        )
                    });
                    complete.push(start.elapsed().as_secs_f64() * 1000.0);
                    assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
                    equal_frames(
                        std::slice::from_ref(&actual.expect("selected frame")),
                        std::slice::from_ref(&expected),
                    );
                }
                let raw_first = first.clone();
                let raw_complete = complete.clone();
                first.sort_by(f64::total_cmp);
                complete.sort_by(f64::total_cmp);
                println!(
                    "SEEK_WORKER gop={gop} target_ms={milliseconds} deferred={mode} first_ms={:.4} complete_ms={:.4} raw_first={raw_first:?} raw_complete={raw_complete:?}",
                    first[1], complete[1]
                );
            }
        }
        assert_eq!(std::fs::read(&path).expect("source bytes"), bytes);
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .modified()
                .expect("mtime"),
            stamp
        );
        std::fs::remove_file(path).expect("owned fixture cleanup");
    }
    std::fs::remove_dir(root).expect("empty owned directory");
    println!("SEEK_WORKER_CHECKS completed=48 exact_selected_frames=true source_bytes_stamps=true");
}

#[test]
#[ignore = "Release paused-session rapid seeks with WARP software fallback; run alone"]
fn deferred_session_reports_rapid_seek_replacement_cost() {
    use crate::PlaybackSession;
    use towavue_core::PlaybackRange;
    if cfg!(debug_assertions) {
        panic!("use Release");
    }
    let root = root("deferred-session-cost");
    let path = fixture(&root, "1920x1080", 180);
    let bytes = std::fs::read(&path).expect("source bytes");
    let stamp = std::fs::metadata(&path)
        .expect("metadata")
        .modified()
        .expect("mtime");
    let targets: Vec<_> = [
        550, 1550, 2550, 3550, 4550, 5550, 4550, 3550, 2550, 1550, 550, 5550,
    ]
    .map(|ms| MediaTime::from_nanoseconds(ms * 1_000_000))
    .into();
    let expected: Vec<_> = targets
        .iter()
        .map(|&target| first_frame(&path, target, false).0.presentation_time)
        .collect();
    let device =
        GraphicsDevice::warp_for_test().expect("owned WARP device, forces software fallback");
    for mode in [false, true, true, false] {
        let mut final_times = Vec::new();
        let mut blocking = Vec::new();
        let mut total_times = Vec::new();
        let mut presented = Vec::new();
        for _ in 0..3 {
            session_deferred(mode, || {
                let (notify, events) = mpsc::channel();
                let mut session = PlaybackSession::open(
                    &path,
                    device.clone(),
                    0.0,
                    1.0,
                    PlaybackRange::default(),
                    move |event| {
                        let _ = notify.send(event);
                    },
                )
                .expect("session");
                session.set_paused(true).expect("pause");
                let start = Instant::now();
                let mut maximum_block = Duration::ZERO;
                let mut final_start = start;
                let mut selected = 0;
                for (index, &target) in targets.iter().enumerate() {
                    let generation = session.generation();
                    let command = Instant::now();
                    session.seek(target).expect("superseding seek");
                    maximum_block = maximum_block.max(command.elapsed());
                    assert_ne!(generation, session.generation());
                    assert!(
                        session.video_refresh_pending(),
                        "held frame is not the new result"
                    );
                    if index + 1 == targets.len() {
                        final_start = command;
                        break;
                    }
                    let deadline = Duration::from_millis(33 * (index as u64 + 1));
                    while start.elapsed() < deadline {
                        if session.video_refresh_pending()
                            && let Some(time) = session.pending_video_time()
                        {
                            assert_eq!(time, expected[index], "no stale target accepted");
                            assert!(session.advance_pending());
                            selected += 1;
                        }
                        let _ = events.recv_timeout(
                            deadline
                                .saturating_sub(start.elapsed())
                                .min(Duration::from_millis(5)),
                        );
                    }
                }
                loop {
                    if let Some(time) = session.pending_video_time() {
                        assert_eq!(Some(&time), expected.last());
                        final_times.push(final_start.elapsed().as_secs_f64() * 1000.0);
                        total_times.push(start.elapsed().as_secs_f64() * 1000.0);
                        assert!(session.advance_pending());
                        selected += 1;
                        assert_eq!(session.video_geometry(), Some((1920, 1080, 1.0)));
                        assert!(!session.video_refresh_pending());
                        assert_eq!(session.metrics().hardware_frame_count, 0);
                        assert!(session.metrics().cpu_transfer_count > 0);
                        break;
                    }
                    assert!(
                        final_start.elapsed() < Duration::from_secs(10),
                        "final replacement completes"
                    );
                    let _ = events.recv_timeout(Duration::from_millis(5));
                }
                blocking.push(maximum_block.as_secs_f64() * 1000.0);
                presented.push(selected);
                drop(session);
            });
        }
        let raw_final = final_times.clone();
        let raw_total = total_times.clone();
        let raw_blocking = blocking.clone();
        final_times.sort_by(f64::total_cmp);
        total_times.sort_by(f64::total_cmp);
        blocking.sort_by(f64::total_cmp);
        println!(
            "SEEK_SESSION deferred={mode} commands=12 interval_ms=33 final_ms={:.4} total_ms={:.4} max_command_ms={:.4} selected={presented:?} raw_final={raw_final:?} raw_total={raw_total:?} raw_blocking={raw_blocking:?}",
            final_times[1], total_times[1], blocking[1]
        );
    }
    drop(device);
    assert_eq!(std::fs::read(&path).expect("source bytes"), bytes);
    assert_eq!(
        std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        stamp
    );
    std::fs::remove_file(path).expect("owned file cleanup");
    std::fs::remove_dir(root).expect("empty directory");
    println!(
        "SEEK_SESSION_CHECKS commands=144 final_targets=12 superseded_targets_rejected=true software_fallback=true source_bytes_stamps=true"
    );
}

#[test]
#[ignore = "read-only explicit TOWAVUE_SEEK_REFERENCE_SOURCE; native metadata and software equality only"]
fn reference_video_reports_native_seek_path_and_software_preroll_equality() {
    if cfg!(debug_assertions) {
        panic!("use Release");
    }
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit source"),
    );
    let source_stamp = || {
        let metadata = std::fs::metadata(&path).expect("source metadata");
        (metadata.len(), metadata.modified().expect("mtime"))
    };
    let stamp = source_stamp();
    let device = GraphicsDevice::hardware_for_test().expect("owned windowless hardware device");
    let mut input = ParallelInput::open(&path, &|| false).expect("read-only video input");
    for ms in [
        60550, 61550, 62550, 63550, 64550, 65550, 64550, 63550, 62550, 61550, 300550, 301550,
    ] {
        let target = MediaTime::from_nanoseconds(ms * 1_000_000);
        let start = Instant::now();
        let mut selected = None;
        let result = input.decode_hardware(&device, target, None, &|| false, |item| {
            if let ParallelRuntimeDecodeOutput::Item(RuntimeDecodeOutput::Video(frame)) = item {
                selected = Some((
                    frame.presentation_time,
                    frame.width,
                    frame.height,
                    start.elapsed(),
                ));
                false
            } else {
                true
            }
        });
        let mut hardware = true;
        if matches!(result, Err(DecodeError::HardwareUnavailable(_))) {
            assert!(selected.is_none());
            hardware = false;
            let fallback =
                input.decode_software(target, None, Some(DecodeStream::Video), &|| false, |item| {
                    if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) = item {
                        selected = Some((
                            frame.presentation_time,
                            frame.width,
                            frame.height,
                            start.elapsed(),
                        ));
                        false
                    } else {
                        true
                    }
                });
            assert!(matches!(fallback, Err(DecodeError::ConsumerClosed)));
        } else {
            assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
        }
        let complete = start.elapsed();
        let (time, width, height, ready) = selected.expect("selected video frame");
        assert!(time >= target);
        assert_eq!((width, height), (1920, 1080));
        println!(
            "REFERENCE_SEEK target_ms={ms} hardware={hardware} ready_ms={:.4} complete_ms={:.4}",
            ready.as_secs_f64() * 1000.0,
            complete.as_secs_f64() * 1000.0
        );
    }
    drop(input);
    drop(device);
    for ms in [60550, 300550] {
        let target = MediaTime::from_nanoseconds(ms * 1_000_000);
        let mut expected = None;
        for mode in [false, true] {
            let mut selected = None;
            let result = deferred(mode, || {
                decode_file_parallel(&path, target, None, Some(DecodeStream::Video), |item| {
                    if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) = item {
                        selected = Some(frame);
                        false
                    } else {
                        true
                    }
                })
            });
            assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
            let actual = selected.expect("software target");
            if let Some(reference) = &expected {
                equal_frames(
                    std::slice::from_ref(&actual),
                    std::slice::from_ref(reference),
                );
            } else {
                expected = Some(actual);
            }
        }
    }
    assert_eq!(source_stamp(), stamp);
    println!(
        "REFERENCE_SEEK_CHECKS targets=12 software_pairs=2 exact_software_pixels=true source_stamps=true no_pixel_output=true"
    );
}

// Isolate avoidable preroll conversion before changing worker/lifetime contracts.
// This does not reuse decoder state or skip reference-picture decoding.
fn first_frame(
    path: &Path,
    target: MediaTime,
    skip_conversion: bool,
) -> (VideoFrame, usize, usize) {
    let mut input = format::input(path).expect("owned input");
    let origin = input_origin(&input);
    let mut video = create_video_pipeline(&input)
        .expect("pipeline")
        .expect("video");
    seek_input(&mut input, target, true, &|| false).expect("same GOP seek");
    let mut decoded_count = 0;
    let mut converted_count = 0;
    for (stream, mut packet) in input.packets() {
        if stream.index() != video.stream_index {
            continue;
        }
        normalize_packet_time(&mut packet, stream.time_base(), origin);
        video.decoder.send_packet(&packet).expect("packet");
        loop {
            let mut decoded = frame::Video::empty();
            match video.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    decoded_count += 1;
                    let time = timestamp_to_media_time(decoded.timestamp(), video.time_base);
                    if skip_conversion && time < target {
                        continue;
                    }
                    let mut rgba = frame::Video::empty();
                    video.scaler.run(&decoded, &mut rgba).expect("RGBA");
                    let output =
                        copy_video_frame(&decoded, &rgba, video.time_base, video.orientation)
                            .expect("owned frame");
                    converted_count += 1;
                    if time >= target {
                        return (output, decoded_count, converted_count);
                    }
                }
                Err(error) if decoder_is_drained(error) => break,
                Err(error) => panic!("decode: {error}"),
            }
        }
    }
    panic!("fixture target must precede decoder drain; not an EOF prototype");
}

#[test]
#[ignore = "Release-only generated short/long-GOP software preroll conversion comparison"]
fn preroll_conversion_reports_first_frame_cost() {
    if cfg!(debug_assertions) {
        panic!("use Release");
    }
    ffmpeg::init().expect("FFmpeg");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "towavue-seek-conversion-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&root).expect("owned fixtures");
    let executable =
        std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
            .join("bin/ffmpeg.exe");
    for gop in [30, 180] {
        let path = root.join(format!("gop-{gop}.mp4"));
        let generated = std::process::Command::new(&executable)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=1920x1080:rate=30:duration=6",
                "-c:v",
                "mpeg4",
                "-q:v",
                "5",
                "-g",
                &gop.to_string(),
                "-bf",
                "2",
                "-an",
            ])
            .arg(&path)
            .output()
            .expect("generate");
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let bytes = std::fs::read(&path).expect("owned bytes");
        let stamp = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        for milliseconds in [550, 4550] {
            let target = MediaTime::from_nanoseconds(milliseconds * 1_000_000);
            let mut expected = None;
            let result =
                decode_file_parallel(&path, target, None, Some(DecodeStream::Video), |output| {
                    if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) = output {
                        expected = Some(frame);
                        false
                    } else {
                        true
                    }
                });
            assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
            let expected = expected.expect("ordinary parallel path's target frame");
            let mut expected_decoded = None;
            for skip in [false, true, true, false] {
                let mut times = Vec::new();
                let mut converted = 0;
                let mut decoded = 0;
                for _ in 0..3 {
                    let start = Instant::now();
                    let (actual, received, scaled) = first_frame(&path, target, skip);
                    times.push(start.elapsed().as_secs_f64() * 1000.0);
                    assert_eq!(actual.presentation_time, expected.presentation_time);
                    assert_eq!(actual.duration, expected.duration);
                    assert_eq!(
                        (actual.width, actual.height),
                        (expected.width, expected.height)
                    );
                    assert_eq!(actual.pixel_aspect, expected.pixel_aspect);
                    assert_eq!(actual.orientation, expected.orientation);
                    assert!(actual.rgba == expected.rgba, "exact selected RGBA");
                    assert_eq!(*expected_decoded.get_or_insert(received), received);
                    assert_eq!(scaled, if skip { 1 } else { received });
                    (decoded, converted) = (received, scaled);
                }
                let raw = times.clone();
                times.sort_by(f64::total_cmp);
                println!(
                    "SEEK_CONVERSION gop={gop} target_ms={milliseconds} skip={skip} median_ms={:.4} decoded={decoded} converted={converted} raw={raw:?}",
                    times[1]
                );
            }
        }
        assert_eq!(std::fs::read(&path).expect("unchanged source"), bytes);
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .modified()
                .expect("mtime"),
            stamp
        );
        std::fs::remove_file(path).expect("owned fixture cleanup");
    }
    std::fs::remove_dir(root).expect("empty owned fixture directory");
    println!(
        "SEEK_CONVERSION_CHECKS cases=48 exact_selected_frames=true unchanged_decode_count=true source_bytes_stamps=true"
    );
}
