use super::*;
use std::fs;
use std::os::windows::process::CommandExt;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use towavue_core::{EditOperation, TimeRange, TimelineEdit};

fn time(ms: i64) -> MediaTime {
    MediaTime::from_nanoseconds(ms * 1_000_000)
}
fn range(a: i64, b: i64) -> TimeRange {
    TimeRange::new(time(a), time(b)).expect("range")
}
fn operations() -> Vec<EditOperation> {
    vec![
        EditOperation::Timeline(TimelineEdit::Delete(range(500, 1000))),
        EditOperation::Timeline(TimelineEdit::Stretch(range(500, 1000), time(1000))),
        EditOperation::Timeline(TimelineEdit::SetVolume(range(200, 400), 0.5)),
    ]
}
fn plan() -> EditTimeline {
    EditTimeline::from_operations(time(2000), &operations()).expect("plan")
}

fn fixture(audio: bool) -> (PathBuf, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("towavue-timeline-playback-{unique}"));
    fs::create_dir(&directory).expect("owned fixture directory");
    let path = directory.join("source.mkv");
    let executable =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe");
    let mut command = std::process::Command::new(executable);
    command.creation_flags(0x0800_0000).args(["-v", "error", "-f", "lavfi", "-i",
        "nullsrc=size=160x96:rate=20:duration=2,geq=r='mod(N*37,256)':g='mod(N*67,256)':b='mod(N*97,256)'"]);
    if audio {
        command.args([
            "-f",
            "lavfi",
            "-i",
            "sine=sample_rate=48000:duration=2",
            "-c:a",
            "pcm_s16le",
        ]);
    }
    let output = command
        .args(["-c:v", "ffv1"])
        .arg(&path)
        .output()
        .expect("generate fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (directory, path)
}

fn drain_video(session: &mut PlaybackSession) -> Vec<(MediaTime, Vec<u8>)> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut frames = Vec::new();
    loop {
        if session.pending_video_time().is_some() {
            assert!(session.advance_pending());
            let Some(PresentationFrame::Software(frame)) = &session.current_video else {
                panic!("WARP software frame");
            };
            frames.push((frame.presentation_time, frame.rgba.to_vec()));
        } else if session.decode_finished() {
            return frames;
        }
        assert!(Instant::now() < deadline, "video timeline deadline");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn edited_video_retimes_original_frames_across_seek_visibility_recovery_and_empty_undo() {
    let (directory, path) = fixture(false);
    let plan = plan();
    let mut expected = Vec::new();
    decode::decode_file(&path, |output| {
        if let DecodeOutput::Video(frame) = output
            && let Some(time) = plan.edited_time(frame.presentation_time)
        {
            expected.push((time, frame.rgba.to_vec()));
        }
        true
    })
    .expect("reference source frames");
    assert_eq!(expected.len(), 30);
    let (notify, events) = mpsc::channel();
    let mut session = PlaybackSession::open(
        &path,
        GraphicsDevice::warp_for_test().expect("WARP"),
        0.0,
        1.0,
        PlaybackRange::default(),
        move |event| {
            let _ = notify.send(event);
        },
    )
    .expect("session");
    let original_generation = session.video_generation;
    session
        .seek_with_timeline(MediaTime::ZERO, 1.0, plan.clone(), true)
        .expect("install timeline");
    assert!(!session.accepts_event(&PlaybackEvent::VideoReady(original_generation)));
    assert!(
        drain_video(&mut session) == expected,
        "complete edited frame sequence"
    );
    for target in [time(750), MediaTime::ZERO] {
        session.seek(target).expect("edited seek");
        assert!(
            drain_video(&mut session)
                == expected
                    .iter()
                    .filter(|(time, _)| *time >= target)
                    .cloned()
                    .collect::<Vec<_>>(),
            "edited seek frame sequence"
        );
    }
    session.seek(time(2000)).expect("edited EOF seek");
    assert!(
        drain_video(&mut session) == vec![expected.last().expect("terminal frame").clone()],
        "terminal edited frame"
    );
    session
        .set_video_visible(false, time(1950))
        .expect("hide video");
    let frames = session.metrics().cpu_transfer_count;
    thread::sleep(Duration::from_millis(25));
    assert_eq!(session.metrics().cpu_transfer_count, frames);
    session
        .set_video_visible(true, time(1950))
        .expect("restore edited video");
    assert!(
        drain_video(&mut session) == vec![expected.last().expect("last").clone()],
        "restored terminal frame"
    );
    session
        .replace_graphics_device(
            GraphicsDevice::warp_for_test().expect("new WARP"),
            MediaTime::ZERO,
        )
        .expect("recover timeline");
    assert_eq!(session.timeline(), Some(&plan));
    assert!(
        drain_video(&mut session) == expected,
        "recovered edited frames"
    );
    let mut empty = plan.clone();
    let mut bounded = plan.clone();
    assert!(bounded.apply(TimelineEdit::Keep(range(0, 1500))));
    session
        .seek_with_timeline(time(1500), 1.0, bounded, true)
        .expect("terminal frame before a deleted tail");
    assert!(
        drain_video(&mut session)
            == vec![
                expected
                    .iter()
                    .rfind(|(position, _)| *position < time(1500))
                    .expect("bounded terminal")
                    .clone()
            ],
        "must not show a deleted tail frame"
    );
    assert!(empty.apply(TimelineEdit::Delete(range(0, 2000))));
    session
        .seek_with_timeline(time(1000), 1.0, empty, true)
        .expect("empty timeline");
    assert_eq!(session.target(), MediaTime::ZERO);
    session
        .set_video_visible(false, MediaTime::ZERO)
        .expect("hide empty");
    session
        .set_video_visible(true, MediaTime::ZERO)
        .expect("show empty");
    assert!(!session.video_refresh_pending());
    assert!(
        session.decode_finished() && !session.has_audio() && session.video_geometry().is_none()
    );
    session
        .seek_with_timeline(MediaTime::ZERO, 1.0, plan, true)
        .expect("undo empty timeline");
    assert!(
        drain_video(&mut session) == expected,
        "undo empty edited frames"
    );
    assert!(!events.try_iter().any(|event| matches!(
        event,
        PlaybackEvent::Failed(..) | PlaybackEvent::VideoFailed(..)
    )));
    session
        .seek_with_edits(MediaTime::ZERO, 1.0, PlaybackRange::default(), true)
        .expect("undo timeline mode");
    assert!(session.timeline().is_none());
    assert_eq!(drain_video(&mut session).len(), 40);
    drop(session);
    fs::remove_dir_all(directory).expect("remove owned fixture");
}

#[test]
#[ignore = "requires a live Windows shared-mode audio endpoint; generated media plays muted"]
fn timeline_joins_keep_one_audio_output_and_an_edited_clock_while_video_is_hidden() {
    let (directory, path) = fixture(true);
    let mut session = match PlaybackSession::open(
        &path,
        GraphicsDevice::warp_for_test().expect("WARP"),
        0.0,
        1.0,
        PlaybackRange::default(),
        |_| {},
    ) {
        Ok(session) => session,
        Err(PlaybackError::Audio(error)) => {
            eprintln!("SKIP: shared-mode audio endpoint unavailable: {error}");
            fs::remove_dir_all(directory).expect("remove skipped fixture");
            return;
        }
        Err(error) => panic!("session: {error}"),
    };
    session
        .seek_with_timeline(MediaTime::ZERO, 2.0, plan(), true)
        .expect("install edited audio");
    let worker = session.audio.as_ref().expect("output").worker_id();
    let feed = session.audio_thread.as_ref().expect("feed").thread().id();
    let generation = session.generation();
    session
        .set_video_visible(false, MediaTime::ZERO)
        .expect("hide video");
    session.set_paused(false).expect("play edited timeline");
    thread::sleep(Duration::from_millis(300));
    let position = session.audio_position().expect("edited clock");
    assert!(
        (time(350)..time(900)).contains(&position),
        "master rate must apply once: {position:?}"
    );
    session.set_paused(true).expect("pause");
    thread::sleep(Duration::from_millis(50));
    let paused = session.audio_position().expect("paused edited clock");
    thread::sleep(Duration::from_millis(70));
    assert_eq!(session.audio_position(), Some(paused));
    assert_eq!(session.audio.as_ref().expect("output").worker_id(), worker);
    assert_eq!(
        session.audio_thread.as_ref().expect("feed").thread().id(),
        feed
    );
    assert_eq!(session.generation(), generation);
    session.set_paused(false).expect("resume");
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        match session.try_audio_event() {
            Some(AudioOutputEvent::Drained) => break,
            Some(event) => panic!("unexpected endpoint event: {event:?}"),
            None => {}
        }
        assert!(Instant::now() < deadline, "edited audio must drain");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(session.audio_position().expect("final clock") >= time(1900));
    assert_eq!(
        session.audio.as_ref().expect("same output").worker_id(),
        worker
    );
    assert_eq!(session.generation(), generation);
    session
        .replace_graphics_device(
            GraphicsDevice::warp_for_test().expect("recovery WARP"),
            time(750),
        )
        .expect("recover edited session");
    session.set_paused(true).expect("pause recovered output");
    assert_eq!(session.timeline(), Some(&plan()));
    session
        .set_video_visible(true, time(750))
        .expect("show recovered timeline");
    let deadline = Instant::now() + Duration::from_secs(4);
    while session.pending_video_time().is_none() {
        assert!(Instant::now() < deadline, "recovered edited frame");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(session.pending_video_time().expect("frame") >= time(750));
    drop(session);
    fs::remove_dir_all(directory).expect("remove owned live fixture");
    eprintln!(
        "PASS: edited audio joins preserve worker identities, pause and master clock; hidden drain and recovery succeed"
    );
}

#[test]
fn edited_audio_has_export_equivalent_samples_bounded_chunks_and_cancellation() {
    let (directory, path) = fixture(true);
    let plan = plan();
    let format = AudioFormat {
        sample_rate: 48000,
        channels: 2,
    };
    for rate in [1.0, 0.25, 4.0] {
        let mut samples = Vec::new();
        let mut previous = None;
        timeline::decode_audio(
            &path,
            &plan,
            MediaTime::ZERO,
            rate,
            format,
            &AtomicBool::new(false),
            |chunk| {
                assert!(chunk.frames <= 1024);
                assert_eq!(chunk.bytes.len(), chunk.frames * 8);
                assert!(previous.is_none_or(|previous| previous < chunk.presentation_time));
                previous = Some(chunk.presentation_time);
                samples.extend(chunk.bytes);
                true
            },
        )
        .expect("timeline audio");
        assert_eq!(samples.len(), (96000.0 / rate) as usize * 8);
        let target = directory.join("result.wav");
        let mut edits = operations();
        edits.push(EditOperation::SetRate(rate));
        crate::export_media(&crate::ExportRequest {
            source: path.clone(),
            target: target.clone(),
            kind: towavue_core::MediaKind::Audio,
            operations: edits,
            hardware_encode: false,
        })
        .expect("export reference");
        let mut exported = Vec::new();
        decode::decode_file(&target, |output| {
            if let DecodeOutput::Audio(chunk) = output {
                exported.extend(chunk.bytes);
            }
            true
        })
        .expect("decode export");
        assert_eq!(samples.len(), exported.len());
        let max_error = samples
            .as_chunks::<4>()
            .0
            .iter()
            .zip(exported.as_chunks::<4>().0)
            .map(|(a, b)| (f32::from_le_bytes(*a) - f32::from_le_bytes(*b)).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            max_error <= 1.0 / 32768.0,
            "sample mismatch at rate={rate}: {max_error}"
        );
    }
    let cancelled = AtomicBool::new(false);
    for target in [time(750), time(1500), time(2000)] {
        let mut frames = 0;
        let mut first = None;
        timeline::decode_audio(
            &path,
            &plan,
            target,
            1.0,
            format,
            &AtomicBool::new(false),
            |chunk| {
                first.get_or_insert(chunk.presentation_time);
                frames += chunk.frames;
                true
            },
        )
        .expect("edited audio seek");
        assert_eq!(
            frames,
            ((plan.duration().as_nanoseconds() - target.as_nanoseconds()) * 48000 / 1_000_000_000)
                as usize
        );
        assert_eq!(first, (frames > 0).then_some(target));
    }
    let mut chunks = 0;
    let result = timeline::decode_audio(&path, &plan, time(750), 1.0, format, &cancelled, |_| {
        chunks += 1;
        cancelled.store(true, Ordering::Relaxed);
        true
    });
    assert!(matches!(result, Err(decode::DecodeError::ConsumerClosed)));
    assert_eq!(chunks, 1);
    fs::remove_dir_all(directory).expect("remove owned fixture");
}
