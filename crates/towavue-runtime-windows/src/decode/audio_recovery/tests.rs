use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: std::path::PathBuf,
    clean: std::path::PathBuf,
    packets: Vec<(usize, usize)>,
}

impl Fixture {
    fn new() -> Self {
        ffmpeg::init().expect("FFmpeg");
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-aac-corruption-{unique}"));
        fs::create_dir(&root).expect("exclusive fixture directory");
        let clean = root.join("clean.m4a");
        let tool = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg helper");
        assert!(
            crate::hidden_test_command(tool)
                .args([
                    "-v",
                    "error",
                    "-n",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000:duration=2",
                    "-ac",
                    "2",
                    "-c:a",
                    "aac",
                    // Disable stochastic noise substitution for exact later PCM comparison.
                    "-aac_pns",
                    "0"
                ])
                .arg(&clean)
                .status()
                .expect("generate AAC fixture")
                .success()
        );
        let mut input = format::input(&clean).expect("probe clean fixture");
        let packets = input
            .packets()
            .map(|(_, packet)| {
                assert!(packet.position() >= 0);
                (packet.position() as usize, packet.size())
            })
            .collect();
        Self {
            root,
            clean,
            packets,
        }
    }

    fn corrupt(&self, first: usize, count: usize) -> std::path::PathBuf {
        let path = self.root.join(format!("bad-{first}-{count}.m4a"));
        let mut bytes = fs::read(&self.clean).expect("read owned fixture");
        for &(offset, length) in &self.packets[first..first + count] {
            bytes[offset..offset + length].fill(0xA5);
        }
        fs::write(&path, bytes).expect("write damaged fixture");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("release fixture handles");
    }
}

type Captured = (Vec<AudioChunk>, Vec<(MediaTime, MediaTime)>);

fn collect(input: &mut PlaybackAudioInput, target: MediaTime) -> Result<Captured, DecodeError> {
    let mut chunks = Vec::new();
    let mut skips = Vec::new();
    input.decode(
        target,
        None,
        AudioSeekPolicy::Playback,
        &|| false,
        |output| {
            match output {
                ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) => {
                    chunks.push(chunk)
                }
                ParallelSoftwareDecodeOutput::AudioSkipped(start, end) => skips.push((start, end)),
                _ => {}
            }
            true
        },
    )?;
    Ok((chunks, skips))
}

#[test]
fn localized_corruption_skips_only_damaged_samples_and_preserves_strict_decode() {
    let fixture = Fixture::new();
    let path = fixture.corrupt(30, 2);
    let mut clean =
        PlaybackAudioInput::open(&fixture.clean, None, &|| false).expect("open clean playback");
    let (expected, clean_skips) =
        collect(&mut clean, MediaTime::ZERO).expect("decode clean reference");
    assert!(clean_skips.is_empty());
    // Exercise both the ordinary inline feed and the parallel fallback.
    for inline in [true, false] {
        audio_inline::ENABLED.set(inline);
        let mut input =
            PlaybackAudioInput::open(&path, None, &|| false).expect("open damaged playback");
        let (actual, skips) =
            collect(&mut input, MediaTime::ZERO).expect("recover localized corruption");
        assert_eq!(
            actual.iter().map(|c| c.frames).sum::<usize>() + 2048,
            expected.iter().map(|c| c.frames).sum::<usize>()
        );
        assert_eq!(skips.len(), 2);
        assert_eq!(actual.len() + 2, expected.len());
        for a in &actual {
            let b = expected
                .iter()
                .find(|b| b.presentation_time == a.presentation_time)
                .expect("original source timestamp");
            assert_eq!(a.frames, b.frames);
            if a.presentation_time.as_nanoseconds() >= 900_000_000 {
                assert!(a.bytes == b.bytes, "later clean PCM");
            }
            assert!(!skips.iter().any(|(start, end)| a.presentation_time >= *start && a.presentation_time < *end));
        }
        let target = MediaTime::from_nanoseconds(630_000_000);
        let (after_seek, clipped_skips) =
            collect(&mut input, target).expect("reused seek through damage");
        audio_inline::ENABLED.set(true);
        assert_eq!(clipped_skips.first().expect("clipped skip").0, target);
        assert!(!after_seek.is_empty());
        assert!(
            after_seek
                .last()
                .expect("audio resumes after seek")
                .presentation_time
                .as_nanoseconds()
                > 1_900_000_000
        );
    }
    let mut strict = ParallelInput::open(&path, &|| false).expect("open strict decoder");
    assert!(matches!(
        strict.decode_software(
            MediaTime::ZERO,
            None,
            Some(DecodeStream::Audio),
            &|| false,
            |_| true
        ),
        Err(DecodeError::Ffmpeg(ffmpeg::Error::InvalidData))
    ));
}

#[test]
fn persistent_corruption_stops_and_consumer_cancellation_is_preserved() {
    let fixture = Fixture::new();
    let path = fixture.corrupt(30, MAX_CONSECUTIVE + 1);
    let mut input =
        PlaybackAudioInput::open(&path, None, &|| false).expect("open persistent corruption");
    assert!(matches!(
        collect(&mut input, MediaTime::ZERO),
        Err(DecodeError::Ffmpeg(ffmpeg::Error::InvalidData))
    ));
    let path = fixture.corrupt(30, 1);
    let mut input =
        PlaybackAudioInput::open(&path, None, &|| false).expect("open cancellation fixture");
    let result = input.decode(
        MediaTime::ZERO,
        None,
        AudioSeekPolicy::Exact,
        &|| false,
        |output| !matches!(output, ParallelSoftwareDecodeOutput::AudioSkipped(..)),
    );
    assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
}

#[test]
fn recovery_rejects_ambiguous_timing_and_non_data_errors() {
    let fixture = Fixture::new();
    let mut input = format::input(&fixture.clean).expect("probe clean fixture");
    let config = best_stream_config(&input, Type::Audio).expect("audio stream");
    let mut recovery = Recovery::new(true, &config);
    let mut pipeline = create_audio_pipeline_from(config).expect("audio decoder");
    let (_, original) = input.packets().nth(30).expect("owned audio packet");
    let mut emit = |_| panic!("invalid recovery must not emit audio");
    for case in 0..4 {
        let mut packet = original.clone();
        let error = if case == 3 {
            DecodeError::ConsumerClosed
        } else {
            ffmpeg::Error::InvalidData.into()
        };
        match case {
            0 => packet.set_pts(None),
            1 => packet.set_duration(0),
            2 => packet.set_duration(i64::MAX),
            _ => {}
        }
        assert!(
            recovery
                .replace(error, &packet, &mut pipeline, &mut emit)
                .is_err()
        );
    }
}

#[test]
fn missing_access_units_emit_one_skip_with_a_total_duration_limit() {
    let fixture = Fixture::new();
    let mut input = format::input(&fixture.clean).expect("probe clean fixture");
    let config = best_stream_config(&input, Type::Audio).expect("audio stream");
    let mut recovery = Recovery::new(true, &config);
    let mut pipeline = create_audio_pipeline_from(config).expect("audio decoder");
    let (_, mut packet) = input.packets().nth(30).expect("owned audio packet");
    packet.set_duration(84_992);
    let mut chunks = Vec::new();
    recovery
        .replace(
            ffmpeg::Error::InvalidData.into(),
            &packet,
            &mut pipeline,
            &mut |output| {
                if let ParallelDecodeOutput::AudioSkipped(start, end) = output {
                    chunks.push((start, end));
                }
                true
            },
        )
        .expect("replace missing access units");
    assert_eq!(chunks.len(), 1);
    let (start, end) = chunks[0];
    assert!((end.as_nanoseconds() - start.as_nanoseconds() - 1_770_666_667).abs() <= 1);
    packet.set_pts(Some(packet.pts().expect("packet timestamp") + 84_992));
    packet.set_duration(48_000 * 4);
    assert!(
        recovery
            .replace(
                ffmpeg::Error::InvalidData.into(),
                &packet,
                &mut pipeline,
                &mut |_| panic!("over budget"),
            )
            .is_err()
    );
    assert!(
        recovery.finish().is_err(),
        "skipping alone does not establish recovery"
    );
}

#[test]
#[ignore = "requires an explicitly authorized read-only corrupt AAC source"]
fn reference_corrupt_aac_repeated_seeks_continue() {
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit reference source"),
    );
    let mut input =
        PlaybackAudioInput::open(&path, None, &|| false).expect("open reference playback");
    for ns in [871_963_802_229, 870_000_000_000, 871_963_802_229, 0] {
        let target = MediaTime::from_nanoseconds(ns);
        let end = target.saturating_add(Duration::from_secs(if ns == 0 { 900 } else { 3 }));
        let mut last = target;
        input
            .decode(
                target,
                Some(end),
                AudioSeekPolicy::Playback,
                &|| false,
                |output| {
                    if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) = output {
                        last = chunk.presentation_time;
                    }
                    true
                },
            )
            .expect("continue past invalid packets");
        assert!(last > end.saturating_sub(Duration::from_millis(100)));
    }
}

#[test]
#[ignore = "explicit read-only corrupt AAC source, windowless D3D11 and muted WASAPI"]
fn reference_corrupt_aac_native_playback_continues() {
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit reference source"),
    );
    let device = GraphicsDevice::hardware_for_test().expect("windowless hardware device");
    for rate in [1.0, 2.0] {
        let (sender, events) = mpsc::channel();
        let mut session = crate::PlaybackSession::open_paused(
            &path,
            device.clone(),
            0.0,
            rate,
            towavue_core::PlaybackRange::default(),
            move |event| {
                let _ = sender.send(event);
            },
        )
        .expect("muted reference session");
        for ns in [
            868_000_000_000,
            869_200_000_000,
            870_000_000_000,
            871_963_802_229,
        ] {
            let target = MediaTime::from_nanoseconds(ns);
            session.seek(target).expect("seek reference");
            session.set_paused(false).expect("resume muted playback");
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            let mut before_gap = false;
            loop {
                for event in events
                    .try_iter()
                    .filter(|event| session.accepts_event(event))
                {
                    assert!(
                        !matches!(
                            event,
                            crate::PlaybackEvent::Failed(..)
                                | crate::PlaybackEvent::VideoFailed(..)
                                | crate::PlaybackEvent::DeviceRemoved(..)
                        ),
                        "{event:?}"
                    );
                }
                assert!(!matches!(
                    session.try_audio_event(),
                    Some(crate::AudioOutputEvent::Failed(_))
                ));
                if session.pending_video_time().is_some() {
                    session.advance_pending();
                }
                before_gap |= session.audio_position().is_some_and(|position| {
                    position > target.saturating_add(Duration::from_millis(30))
                        && position.as_nanoseconds() < 869_100_000_000
                });
                if (869_141_333_333..870_912_000_000).contains(&ns)
                    && let Some(position) = session.audio_position()
                    && position > target.saturating_add(Duration::from_millis(30))
                {
                    assert!(
                        position.as_nanoseconds() >= 870_900_000_000,
                        "clock skips the gap without playing silence"
                    );
                }
                let past = target.saturating_add(Duration::from_secs(2));
                if session.audio_position().is_some_and(|time| time > past)
                    && session.current_video_time().is_some_and(|time| time > past)
                {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "audio and video continue past corruption"
                );
                thread::sleep(Duration::from_millis(2));
            }
            if ns == 868_000_000_000 {
                assert!(before_gap, "lookahead must not skip earlier valid playback");
            }
            session.set_paused(true).expect("pause muted playback");
        }
    }
}
