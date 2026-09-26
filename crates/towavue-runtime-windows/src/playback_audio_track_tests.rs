use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use towavue_core::{EditOperation, TimelineEdit};

fn time(ms: i64) -> MediaTime {
    MediaTime::from_nanoseconds(ms * 1_000_000)
}

fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(time(start), time(end)).expect("range")
}

#[test]
fn selected_audio_preserves_delays_edits_waveforms_and_session_restarts() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-selected-timeline-{unique}"));
    fs::create_dir(&root).expect("owned fixture");
    let path = root.join("tracks.nut");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let output = crate::hidden_test_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "color=size=32x24:rate=10:duration=3",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.125|0.125:s=44100:d=2.5",
            "-itsoffset",
            "0.5",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.5|0.5:s=48000:d=1.5",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-c:v",
            "ffv1",
            "-c:a",
            "pcm_f32le",
        ])
        .arg(&path)
        .output()
        .expect("generate tracks");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let original = fs::read(&path).expect("original");
    let plan = EditTimeline::from_operations(
        time(3000),
        &[
            EditOperation::Timeline(TimelineEdit::SetVolume(range(1000, 1500), 0.5)),
            EditOperation::Timeline(TimelineEdit::Delete(range(250, 750))),
        ],
    )
    .expect("edited plan");
    let collect = |track, rate, target, end, speed, policy| {
        let mut bytes = Vec::new();
        let mut first = None;
        timeline::decode_audio_with_policy(
            &path,
            Some(track),
            &plan,
            target,
            end,
            speed,
            AudioFormat {
                sample_rate: rate,
                channels: 2,
            },
            policy,
            &AtomicBool::new(false),
            |chunk| {
                first.get_or_insert(chunk.presentation_time);
                assert!(chunk.frames <= 1024);
                bytes.extend(chunk.bytes);
                true
            },
        )
        .expect("selected timeline");
        assert_eq!(first, Some(target));
        bytes
    };
    for (index, rate, amplitude, start_ms, end_ms) in
        [(1, 44100, 0.125_f32, 0, 2500), (2, 48000, 0.5, 500, 2000)]
    {
        let track = AudioTrackId::from_index(index);
        let sample = |ms: usize| ms * rate as usize / 1000;
        // Independent sample oracle: fixed generated amplitudes, source offsets,
        // gain before deletion, then a splice. No decoder/timeline output is reused.
        let mut expected = vec![0_f32; sample(3000)];
        expected[sample(start_ms)..sample(end_ms)].fill(amplitude);
        for value in &mut expected[sample(1000)..sample(1500)] {
            *value *= 0.5;
        }
        expected.drain(sample(250)..sample(750));
        for (target_ms, end_ms) in [(0, 2500), (100, 900), (900, 1700)] {
            let want: Vec<_> = expected[sample(target_ms)..sample(end_ms)]
                .iter()
                .flat_map(|value| value.to_le_bytes().repeat(2))
                .collect();
            for policy in [
                decode::AudioSeekPolicy::Exact,
                decode::AudioSeekPolicy::Playback,
            ] {
                let got = collect(
                    track,
                    rate,
                    time(target_ms as i64),
                    Some(time(end_ms as i64)),
                    1.0,
                    policy,
                );
                assert!(
                    got == want,
                    "track={index}, target={target_ms}, end={end_ms}, sizes={}/{}, first difference={:?}",
                    got.len(),
                    want.len(),
                    got.iter().zip(&want).position(|(a, b)| a != b)
                );
            }
        }
        let waveform = crate::timeline_audio_track_waveform(
            &path,
            Some(track),
            &plan,
            1.0,
            0.8,
            10,
            &crate::Cancellation::default(),
        )
        .expect("selected waveform");
        for (bin, value) in waveform.iter().enumerate() {
            let values = &expected[bin * expected.len() / 10..(bin + 1) * expected.len() / 10];
            let mean =
                values.iter().map(|v| f64::from(*v)).sum::<f64>() / values.len() as f64 * 0.8;
            assert!(
                (f64::from(*value) - mean).abs() < 0.000001,
                "track={index}, bin={bin}: {value}/{mean}"
            );
        }
        for speed in [0.5, 2.0] {
            assert!(
                collect(
                    track,
                    rate,
                    time(100),
                    None,
                    speed,
                    decode::AudioSeekPolicy::Exact
                ) == collect(
                    track,
                    rate,
                    time(100),
                    None,
                    speed,
                    decode::AudioSeekPolicy::Playback
                ),
                "retimed listening must retain exact samples"
            );
        }
    }
    let mut session = PlaybackSession::open_paused(
        &path,
        GraphicsDevice::warp_for_test().expect("WARP"),
        0.0,
        1.0,
        PlaybackRange::default(),
        |_| {},
    )
    .expect("paused hidden session");
    assert_eq!(session.audio_tracks().tracks.len(), 2);
    session
        .seek_with_timeline_selection(time(100), 1.0, plan.clone(), Some(range(100, 2000)), true)
        .expect("plan");
    for (index, rate) in [(2, 48000), (1, 44100)] {
        let track = AudioTrackId::from_index(index);
        let before = session.generation();
        session
            .set_audio_track_at(time(100), Some(track))
            .expect("switch track");
        assert_ne!(session.generation(), before);
        assert_eq!(session.audio_format.expect("format").sample_rate, rate);
        session
            .set_rate_at(time(100), 2.0, true)
            .expect("rate restart");
        session
            .replace_graphics_device(
                GraphicsDevice::warp_for_test().expect("replacement WARP"),
                time(100),
            )
            .expect("device restart");
        assert_eq!(session.audio_track(), Some(track));
        assert_eq!(session.timeline(), Some(&plan));
        assert_eq!(
            session.range(),
            PlaybackRange {
                start: time(100),
                end: Some(time(2000))
            }
        );
        assert_eq!(session.rate(), 2.0);
        assert!(session.paused);
        assert_eq!(session.volume, 0.0);
        let before = session.generation();
        assert!(
            session
                .set_audio_track_at(time(100), Some(AudioTrackId::from_index(0)))
                .is_err()
        );
        assert_eq!(session.generation(), before);
        assert_eq!(session.audio_track(), Some(track));
    }
    session
        .set_audio_selection_at(time(100), AudioTrackSelection::All)
        .expect("all tracks");
    session
        .set_rate_at(time(100), 0.5, true)
        .expect("mixed rate restart");
    session
        .replace_graphics_device(
            GraphicsDevice::warp_for_test().expect("mixed WARP"),
            time(100),
        )
        .expect("mixed device restart");
    assert_eq!(session.audio_selection(), AudioTrackSelection::All);
    assert_eq!(session.timeline(), Some(&plan));
    assert!(session.paused);
    session
        .set_audio_selection_at(time(100), AudioTrackSelection::Default)
        .expect("restore preferred");
    assert_eq!(session.audio_selection(), AudioTrackSelection::Default);
    drop(session);
    for selection in [
        AudioTrackSelection::Track(AudioTrackId::from_index(2)),
        AudioTrackSelection::All,
    ] {
        let session = PlaybackSession::open_input_with_audio(
            crate::MediaInput::new(path.clone()),
            GraphicsDevice::warp_for_test().expect("initial selected WARP"),
            0.0,
            1.0,
            PlaybackRange::default(),
            true,
            selection,
            |_| {},
        )
        .expect("initial audio choice");
        assert_eq!(
            session.generation(),
            PlaybackGeneration::INITIAL,
            "initial choice must not restart a default feed"
        );
        assert_eq!(session.audio_selection(), selection);
        if matches!(selection, AudioTrackSelection::Track(_)) {
            assert_eq!(
                session.audio_format.expect("selected format").sample_rate,
                48000
            );
        }
        assert!(session.input_owner.is_some());
        assert!(session.paused);
    }
    assert_eq!(fs::read(&path).expect("unchanged input"), original);
    fs::remove_dir_all(root).expect("remove owned fixture");
}
