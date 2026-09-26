use super::*;
use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use towavue_core::{EditOperation, EditTimeline, TimeRange, TimelineEdit};

fn time(ms: i64) -> MediaTime {
    MediaTime::from_nanoseconds(ms * 1_000_000)
}

fn samples(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect()
}

fn generate_quantized_aac(ffmpeg: &Path, source: &Path, gap: bool) {
    // OBS-style millisecond quantization can survive in MP4's 1/48000 time base.
    // These packets alternate 1008/1056 ticks while each AAC frame has 1024 samples.
    let generated = crate::hidden_test_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "color=size=32x24:rate=10:duration=4",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.15*sin(2*PI*997*t)|0.15*cos(2*PI*701*t):s=48000:d=4",
            "-filter_complex",
            if gap {
                r"[1:a]asetnsamples=n=1024:p=0,asetpts=round(PTS*TB*1000)/1000/TB+gte(N\,24576)*0.1/TB,asplit[a][b]"
            } else {
                "[1:a]asetnsamples=n=1024:p=0,asetpts=round(PTS*TB*1000)/1000/TB,asplit[a][b]"
            },
            "-map",
            "0:v",
            "-map",
            "[a]",
            "-map",
            "[b]",
            "-c:v",
            "mpeg4",
            "-q:v",
            "5",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
        ])
        .arg(source)
        .output()
        .expect("generate quantized AAC");
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
}

#[test]
fn quantized_aac_timestamps_do_not_insert_silence_into_mix_edits_or_exports() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-aac-jitter-{unique}"));
    fs::create_dir(&root).expect("owned fixture");
    let source = root.join("quantized.mp4");
    let ffmpeg = crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg");
    generate_quantized_aac(&ffmpeg, &source, false);
    let read = |path: &Path, stream: &str| {
        let result = crate::hidden_test_command(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-map", stream, "-c:a", "pcm_f32le", "-f", "f32le", "pipe:1"])
            .output()
            .expect("independent PCM decode");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        samples(&result.stdout)
    };
    let first = read(&source, "0:a:0");
    let second = read(&source, "0:a:1");
    let format = AudioFormat {
        sample_rate: 48000,
        channels: 2,
    };
    let tracks = [AudioTrackId::from_index(1), AudioTrackId::from_index(2)];
    let mut failures = Vec::new();
    let mut check = |label: &str, actual: Vec<f32>, expected: Vec<f32>| {
        // Stay away from encoder padding and the final partial AAC frame.
        let count = 2 * 48000 * 2;
        assert!(
            actual.len() >= count && expected.len() >= count,
            "{label}: missing PCM"
        );
        let mismatches = actual[..count]
            .iter()
            .zip(&expected[..count])
            .filter(|(a, b)| (*a - *b).abs() > 0.00005)
            .count();
        if mismatches != 0 {
            failures.push(format!(
                "{label}: {mismatches}/{count} discontinuous samples"
            ));
        }
    };
    let mut mixed = Vec::new();
    decode(
        &source,
        &tracks,
        MediaTime::ZERO,
        Some(time(4000)),
        format,
        AudioSeekPolicy::Exact,
        &|| false,
        |chunk| {
            mixed.extend(chunk.bytes);
            true
        },
    )
    .expect("mixed preview");
    check(
        "All preview",
        samples(&mixed),
        first.iter().zip(&second).map(|(a, b)| a + b).collect(),
    );
    let operations = vec![EditOperation::Timeline(TimelineEdit::SetVolume(
        TimeRange::new(time(0), time(4000)).expect("gain range"),
        0.5,
    ))];
    let plan = EditTimeline::from_operations(time(4000), &operations).expect("gain plan");
    let mut edited = Vec::new();
    crate::playback::timeline::decode_audio_source(
        &source,
        crate::playback::timeline::AudioSource::Track(Some(tracks[0])),
        &plan,
        MediaTime::ZERO,
        None,
        1.0,
        format,
        AudioSeekPolicy::Exact,
        &AtomicBool::new(false),
        |chunk| {
            edited.extend(chunk.bytes);
            true
        },
    )
    .expect("edited preview");
    check(
        "single-track gain preview",
        samples(&edited),
        first.iter().map(|v| v * 0.5).collect(),
    );
    let target = root.join("edited.avi");
    crate::export_media(&crate::ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: towavue_core::MediaKind::Video,
        operations,
        hardware_encode: false,
    })
    .expect("edited export");
    check(
        "saved gain track",
        read(&target, "0:a:0"),
        first.iter().map(|v| v * 0.5).collect(),
    );
    let mut sought = Vec::new();
    decode(
        &source,
        &tracks,
        time(1000),
        Some(time(4000)),
        format,
        AudioSeekPolicy::Exact,
        &|| false,
        |chunk| {
            sought.extend(chunk.bytes);
            true
        },
    )
    .expect("exact mixed seek");
    check(
        "All after exact seek",
        samples(&sought),
        first[96000..]
            .iter()
            .zip(&second[96000..])
            .map(|(a, b)| a + b)
            .collect(),
    );
    let operations = vec![
        EditOperation::SetTrimStart(time(500)),
        EditOperation::Timeline(TimelineEdit::SetVolume(
            TimeRange::new(time(0), time(3500)).expect("trimmed gain"),
            0.5,
        )),
    ];
    let plan = EditTimeline::from_operations(time(4000), &operations).expect("trimmed plan");
    let mut trimmed = Vec::new();
    crate::playback::timeline::decode_audio_source(
        &source,
        crate::playback::timeline::AudioSource::Track(Some(tracks[0])),
        &plan,
        MediaTime::ZERO,
        None,
        1.0,
        format,
        AudioSeekPolicy::Exact,
        &AtomicBool::new(false),
        |chunk| {
            trimmed.extend(chunk.bytes);
            true
        },
    )
    .expect("trimmed preview");
    check(
        "trimmed gain preview",
        samples(&trimmed),
        first[48000..].iter().map(|v| v * 0.5).collect(),
    );
    let target = root.join("trimmed.avi");
    crate::export_media(&crate::ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: towavue_core::MediaKind::Video,
        operations,
        hardware_encode: false,
    })
    .expect("trimmed export");
    check(
        "saved trim and gain",
        read(&target, "0:a:0"),
        first[48000..].iter().map(|v| v * 0.5).collect(),
    );
    assert!(
        failures.is_empty(),
        "{}; fixture retained at {}",
        failures.join("; "),
        root.display()
    );
    fs::remove_dir_all(root).expect("remove owned fixture");
}

#[test]
fn quantized_aac_keeps_real_gaps_during_preview_seek_and_export() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-aac-real-gap-{unique}"));
    fs::create_dir(&root).expect("owned fixture");
    let source = root.join("gap.mp4");
    let ffmpeg = crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg");
    generate_quantized_aac(&ffmpeg, &source, true);
    let input = ffmpeg_next::format::input(&source).expect("input");
    for stream in input
        .streams()
        .filter(|s| s.parameters().medium() == ffmpeg_next::media::Type::Audio)
    {
        assert!(
            crate::audio_timestamps::quantized_aac(&stream),
            "fixture must exercise the rounded path, including its real gap"
        );
    }
    drop(input);
    let tracks = [AudioTrackId::from_index(1), AudioTrackId::from_index(2)];
    let format = AudioFormat {
        sample_rate: 48000,
        channels: 2,
    };
    let read = |start| {
        let mut pcm = Vec::new();
        decode(
            &source,
            &tracks,
            start,
            Some(time(2000)),
            format,
            AudioSeekPolicy::Exact,
            &|| false,
            |chunk| {
                pcm.extend(chunk.bytes);
                true
            },
        )
        .expect("mixed decode");
        samples(&pcm)
    };
    let assert_gap = |pcm: &[f32]| {
        let peak = |start: usize, end: usize| {
            pcm[start * 96..end * 96]
                .iter()
                .map(|v| v.abs())
                .fold(0.0_f32, f32::max)
        };
        assert!(peak(300, 400) > 0.05, "signal before gap");
        assert!(peak(550, 580) < 0.000001, "real gap must stay silent");
        assert!(peak(800, 900) > 0.05, "signal after gap");
    };
    let sequential = read(MediaTime::ZERO);
    assert_gap(&sequential);
    let sought = read(time(800));
    assert!(sought.len() >= 48000);
    assert!(
        sought[..48000]
            .iter()
            .zip(&sequential[800 * 96..])
            .all(|(a, b)| (a - b).abs() < 0.00005),
        "exact seek must retain the gap's offset"
    );
    let target = root.join("gap.avi");
    crate::export_media(&crate::ExportRequest {
        source,
        target: target.clone(),
        kind: towavue_core::MediaKind::Video,
        operations: vec![EditOperation::SetVolume(0.5)],
        hardware_encode: false,
    })
    .expect("gap export");
    let exported = crate::hidden_test_command(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&target)
        .args([
            "-map",
            "0:a:0",
            "-c:a",
            "pcm_f32le",
            "-f",
            "f32le",
            "pipe:1",
        ])
        .output()
        .expect("export readback");
    assert!(exported.status.success());
    assert_gap(&samples(&exported.stdout));
    fs::remove_dir_all(root).expect("remove owned fixture");
}
#[test]
#[ignore = "read-only owner-authorized two-track source; set reference path and start milliseconds"]
fn reference_track_mix_and_gain_preserve_the_independent_pcm_sequence() {
    let path = PathBuf::from(
        std::env::var_os("TOWAVUE_AUDIO_TRACK_REFERENCE").expect("authorized source"),
    );
    let start: i64 = std::env::var("TOWAVUE_AUDIO_REFERENCE_START_MS")
        .expect("start milliseconds")
        .parse()
        .expect("integer start");
    let before = fs::metadata(&path).expect("source metadata");
    let input = ffmpeg_next::format::input(&path).expect("input");
    let tracks: Vec<_> = input
        .streams()
        .filter(|s| s.parameters().medium() == ffmpeg_next::media::Type::Audio)
        .map(|stream| {
            eprintln!(
                "track {}: quantized={}",
                stream.index(),
                crate::audio_timestamps::quantized_aac(&stream)
            );
            AudioTrackId::from_index(stream.index())
        })
        .collect();
    assert_eq!(tracks.len(), 2, "reference control expects two tracks");
    drop(input);
    let format = decode::probe_audio_track_format(&path, Some(tracks[0]))
        .expect("probe")
        .expect("audio");
    let mut independent = Vec::new();
    for track in &tracks {
        let mut bytes = Vec::new();
        decode::decode_audio_track_cancellable(
            &path,
            Some(*track),
            time(start),
            Some(time(start + 4000)),
            &|| false,
            |output| {
                if let decode::ParallelSoftwareDecodeOutput::Item(decode::DecodeOutput::Audio(
                    chunk,
                )) = output
                {
                    bytes.extend(chunk.bytes);
                }
                true
            },
        )
        .expect("selected PCM");
        independent.push(samples(&bytes));
    }
    let mut mixed = Vec::new();
    decode(
        &path,
        &tracks,
        time(start),
        Some(time(start + 4000)),
        format,
        AudioSeekPolicy::Exact,
        &|| false,
        |chunk| {
            mixed.extend(chunk.bytes);
            true
        },
    )
    .expect("All PCM");
    let mixed = samples(&mixed);
    let mut gain = Vec::new();
    let plan = EditTimeline::from_operations(
        time(start + 4000),
        &[
            EditOperation::SetTrimStart(time(start)),
            EditOperation::Timeline(TimelineEdit::SetVolume(
                TimeRange::new(time(0), time(4000)).expect("range"),
                0.5,
            )),
        ],
    )
    .expect("trim/gain plan");
    crate::playback::timeline::decode_audio_source(
        &path,
        crate::playback::timeline::AudioSource::Track(Some(tracks[0])),
        &plan,
        MediaTime::ZERO,
        None,
        1.0,
        format,
        AudioSeekPolicy::Exact,
        &AtomicBool::new(false),
        |chunk| {
            gain.extend(chunk.bytes);
            true
        },
    )
    .expect("trim/gain PCM");
    let gain = samples(&gain);
    let count = format.sample_rate as usize * 2 * 4;
    for sequence in [&independent[0], &independent[1], &mixed, &gain] {
        assert_eq!(sequence.len(), count, "four complete seconds");
    }
    let mut mix_error = 0.0_f32;
    let mut gain_error = 0.0_f32;
    let mut signal = 0.0_f32;
    for index in 0..count {
        signal = signal
            .max(independent[0][index].abs())
            .max(independent[1][index].abs());
        mix_error =
            mix_error.max((mixed[index] - independent[0][index] - independent[1][index]).abs());
        gain_error = gain_error.max((gain[index] - independent[0][index] * 0.5).abs());
    }
    eprintln!(
        "frames={}, signal_peak={signal}, mix_error={mix_error}, gain_error={gain_error}",
        count / 2
    );
    assert!(signal > 0.0001, "audible reference segment");
    assert!(
        mix_error < 0.000001 && gain_error < 0.000001,
        "no PCM displacement or inserted silence"
    );
    let after = fs::metadata(&path).expect("unchanged source");
    assert_eq!(before.len(), after.len());
    assert_eq!(
        before.modified().expect("before time"),
        after.modified().expect("after time")
    );
}

#[test]
fn all_tracks_match_independent_mix_and_keep_offsets_tails_edits_and_cancellation() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-audio-mix-{unique}"));
    fs::create_dir(&root).expect("owned fixture");
    let path = root.join("tracks.nut");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let output = crate::hidden_test_command(&ffmpeg)
        .args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "color=size=32x24:rate=10:duration=2",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.125|0.125:s=44100:d=1.25",
            "-itsoffset",
            "0.5",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.25|0.25:s=48000:d=1",
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
    let original = fs::read(&path).expect("original source");
    let tracks = [AudioTrackId::from_index(1), AudioTrackId::from_index(2)];
    for rate in [44100, 48000] {
        let format = AudioFormat {
            sample_rate: rate,
            channels: 2,
        };
        // Resample each payload, then align its known source origin. first_pts=0
        // would instead filter artificial lead-in silence and change the initial
        // signal samples at the delayed track's boundary.
        let reference = crate::hidden_test_command(&ffmpeg).args(["-v", "error", "-i"]).arg(&path)
            .args(["-filter_complex", &format!("[0:a:0]aresample={rate}[a0];[0:a:1]asetpts=PTS-STARTPTS,aresample={rate},adelay=500:all=1[a1];[a0][a1]amix=inputs=2:duration=longest:normalize=0:dropout_transition=0,aformat=sample_fmts=flt:sample_rates={rate}:channel_layouts=stereo[out]"),
                "-map", "[out]", "-f", "f32le", "pipe:1"])
            .output().expect("independent FFmpeg mix");
        assert!(
            reference.status.success(),
            "{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let reference = samples(&reference.stdout);
        for target in [0, 123, 900] {
            let mut bytes = Vec::new();
            let mut previous = None;
            decode(
                &path,
                &tracks,
                time(target),
                None,
                format,
                AudioSeekPolicy::Exact,
                &|| false,
                |chunk| {
                    assert!(chunk.frames > 0 && chunk.frames <= BLOCK);
                    assert_eq!(chunk.format, format);
                    assert!(previous.is_none_or(|previous| chunk.presentation_time > previous));
                    previous = Some(chunk.presentation_time);
                    bytes.extend(chunk.bytes);
                    true
                },
            )
            .expect("mixed audio");
            let got = samples(&bytes);
            let first = sample_ceil(time(target), rate) as usize * 2;
            let want = &reference[first..];
            assert!(
                got.len().abs_diff(want.len()) <= 2,
                "sample count at {rate}/{target}: {}/{}",
                got.len(),
                want.len()
            );
            // Fresh resampling can have one sample of phase/edge history at a
            // seek. The whole source must match; interior constant amplitudes
            // must remain exact after seeks, with no gain normalization/drift.
            let margin = if target == 0 { 0 } else { rate as usize / 100 };
            let error = got
                .iter()
                .zip(want)
                .enumerate()
                .skip(margin)
                .take(got.len().saturating_sub(margin * 2))
                .filter(|(index, _)| {
                    target == 0
                        || [500, 1250, 1500].iter().all(|ms| {
                            ((first + index) / 2).abs_diff(sample_nearest(time(*ms), rate) as usize)
                                > rate as usize / 500
                        })
                })
                .map(|(_, (a, b))| (*a - *b).abs())
                .fold(0_f32, f32::max);
            if error >= 0.00001 {
                fs::write(root.join(format!("native-{rate}-{target}.f32")), &bytes)
                    .expect("retain failed native PCM");
                fs::write(
                    root.join(format!("reference-{rate}-{target}.f32")),
                    want.iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .expect("retain failed reference PCM");
            }
            assert!(error < 0.00001, "mix differs at {rate}/{target}: {error}");
        }
        let plan = EditTimeline::from_operations(
            time(2000),
            &[
                EditOperation::Timeline(TimelineEdit::Delete(
                    TimeRange::new(time(250), time(750)).expect("delete"),
                )),
                EditOperation::Timeline(TimelineEdit::SetVolume(
                    TimeRange::new(time(500), time(1000)).expect("gain"),
                    0.5,
                )),
            ],
        )
        .expect("plan");
        let mut bytes = Vec::new();
        crate::playback::timeline::decode_audio_source(
            &path,
            crate::playback::timeline::AudioSource::All(&tracks),
            &plan,
            MediaTime::ZERO,
            None,
            1.0,
            format,
            AudioSeekPolicy::Playback,
            &AtomicBool::new(false),
            |chunk| {
                bytes.extend(chunk.bytes);
                true
            },
        )
        .expect("edited mix");
        let got = samples(&bytes);
        assert_eq!(got.len(), rate as usize * 3);
        for (ms, expected) in [
            (100, 0.125),
            (300, 0.375),
            (600, 0.1875),
            (800, 0.125),
            (1100, 0.0),
        ] {
            let index = sample_nearest(time(ms), rate) as usize * 2;
            assert!(
                (got[index] - expected).abs() < 0.00001,
                "edited sample {rate}/{ms}: {} / {expected}",
                got[index]
            );
        }
        for speed in [0.5, 2.0] {
            let mut frames = 0;
            crate::playback::timeline::decode_audio_source(
                &path,
                crate::playback::timeline::AudioSource::All(&tracks),
                &plan,
                time(100),
                Some(time(1000)),
                speed,
                format,
                AudioSeekPolicy::Playback,
                &AtomicBool::new(false),
                |chunk| {
                    assert!(
                        chunk
                            .bytes
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|x| f32::from_le_bytes(*x).is_finite())
                    );
                    frames += chunk.frames;
                    true
                },
            )
            .expect("retimed mixed selection");
            assert_eq!(
                frames as u64,
                crate::tempo::output_sample_boundary(1_000_000_000, rate, speed)
                    - crate::tempo::output_sample_boundary(100_000_000, rate, speed)
            );
        }
        let started = Instant::now();
        assert!(matches!(
            decode(
                &path,
                &tracks,
                MediaTime::ZERO,
                None,
                format,
                AudioSeekPolicy::Playback,
                &|| false,
                |_| false
            ),
            Err(DecodeError::ConsumerClosed)
        ));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "consumer rejection must join blocked producers"
        );
        let cancelled = AtomicBool::new(false);
        assert!(matches!(
            decode(
                &path,
                &tracks,
                MediaTime::ZERO,
                None,
                format,
                AudioSeekPolicy::Playback,
                &|| cancelled.load(Ordering::Relaxed),
                |_| {
                    cancelled.store(true, Ordering::Relaxed);
                    true
                }
            ),
            Err(DecodeError::ConsumerClosed)
        ));
        assert!(matches!(
            decode(
                &path,
                &[tracks[0], AudioTrackId::from_index(0)],
                MediaTime::ZERO,
                None,
                format,
                AudioSeekPolicy::Exact,
                &|| false,
                |_| true
            ),
            Err(DecodeError::AudioTrackUnavailable)
        ));
        assert!(matches!(
            decode(
                &path,
                &tracks,
                MediaTime::ZERO,
                None,
                format,
                AudioSeekPolicy::Exact,
                &|| true,
                |_| panic!("pre-cancelled mix must not emit")
            ),
            Err(DecodeError::ConsumerClosed)
        ));
    }
    assert_eq!(fs::read(&path).expect("source retained"), original);
    fs::remove_dir_all(root).expect("remove owned fixture");
}

#[test]
fn converter_preserves_large_gaps_without_buffering_silence_and_flushes_rates() {
    for rate in [44100, 48000, 96000] {
        let mut converter = Converter::new(48000);
        let mut blocks = Vec::new();
        let mut send = |chunk| {
            blocks.push(chunk);
            Ok(())
        };
        for second in [0, 3600] {
            converter
                .push(
                    AudioChunk {
                        presentation_time: time(second * 1000),
                        format: AudioFormat {
                            sample_rate: rate,
                            channels: 2,
                        },
                        frames: rate as usize / 10,
                        bytes: 0.125_f32.to_le_bytes().repeat(rate as usize / 5),
                    },
                    &mut send,
                )
                .expect("convert");
        }
        converter.finish(&mut send).expect("flush");
        assert_eq!(
            blocks
                .iter()
                .map(|block| block.bytes.len() / 8)
                .sum::<usize>(),
            9600
        );
        assert!(blocks.iter().all(|block| block.bytes.len() <= BLOCK * 8));
        assert_eq!(
            blocks.last().expect("second part").end(),
            3600 * 48000 + 4800
        );
        assert!(blocks.iter().any(|block| block.start == 3600 * 48000));
    }
}

#[test]
#[ignore = "read-only owner-authorized multi-track source; run alone without other builds or benchmarks"]
fn reference_all_track_seeks_report_readiness_and_sample_counts() {
    let path = PathBuf::from(
        std::env::var_os("TOWAVUE_AUDIO_TRACK_REFERENCE").expect("authorized reference path"),
    );
    let metadata = fs::metadata(&path).expect("reference metadata");
    let stamp = (metadata.len(), metadata.modified().expect("mtime"));
    let catalog = decode::probe_audio_tracks(&path).expect("catalog");
    assert!(catalog.tracks.len() >= 2, "multi-track reference");
    let tracks: Vec<_> = catalog.tracks.iter().map(|track| track.id).collect();
    let format = decode::probe_audio_track_format(&path, None)
        .expect("format")
        .expect("audio");
    for ms in [0, 950_000, 1_800_000] {
        let target = time(ms);
        let end = Some(time(ms + 1000));
        let started = Instant::now();
        let mut first = None;
        let mut frames = 0;
        decode(
            &path,
            &tracks,
            target,
            end,
            format,
            AudioSeekPolicy::Playback,
            &|| false,
            |chunk| {
                first.get_or_insert(started.elapsed());
                frames += chunk.frames;
                assert!(
                    chunk
                        .bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|value| f32::from_le_bytes(*value).is_finite())
                );
                true
            },
        )
        .expect("reference mixed listening");
        assert_eq!(frames, format.sample_rate as usize);
        eprintln!(
            "ALL_TRACKS target_ms={ms} tracks={} rate={} first_ms={:.3} total_ms={:.3} frames={frames}",
            tracks.len(),
            format.sample_rate,
            first.expect("PCM").as_secs_f64() * 1000.0,
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    let metadata = fs::metadata(&path).expect("unchanged reference");
    assert_eq!(stamp, (metadata.len(), metadata.modified().expect("mtime")));
}

#[test]
fn resampled_stereo_chirps_match_ffmpeg_across_irregular_input_chunks() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-mix-resample-{unique}"));
    fs::create_dir(&root).expect("owned resample fixture");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    for rate in [32000, 44100, 96000] {
        let frames = rate as usize / 3;
        let bytes: Vec<_> = (0..frames)
            .flat_map(|index| {
                let t = index as f64 / f64::from(rate);
                [
                    (std::f64::consts::TAU * (150.0 * t + 2200.0 * t * t)).sin() as f32 * 0.6,
                    (std::f64::consts::TAU * (530.0 * t + 1100.0 * t * t)).cos() as f32 * 0.4,
                ]
                .into_iter()
                .flat_map(f32::to_le_bytes)
            })
            .collect();
        let path = root.join(format!("input-{rate}.f32"));
        fs::write(&path, &bytes).expect("stereo source");
        let reference = crate::hidden_test_command(&ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "f32le",
                "-ar",
                &rate.to_string(),
                "-ac",
                "2",
                "-i",
            ])
            .arg(&path)
            .args(["-af", "aresample=48000", "-f", "f32le", "pipe:1"])
            .output()
            .expect("independent resampler");
        assert!(
            reference.status.success(),
            "{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let mut converter = Converter::new(48000);
        let mut converted = Vec::new();
        let mut next = 0;
        let mut send = |chunk: Samples| {
            assert_eq!(chunk.start, next);
            assert!(chunk.bytes.len() <= BLOCK * 8);
            next = chunk.end();
            converted.extend(chunk.bytes);
            Ok(())
        };
        let mut position = 0;
        for count in [1, 31, 1024, 2037].into_iter().cycle() {
            if position == frames {
                break;
            }
            let count = count.min(frames - position);
            converter
                .push(
                    AudioChunk {
                        presentation_time: sample_time(position as i64, rate),
                        format: AudioFormat {
                            sample_rate: rate,
                            channels: 2,
                        },
                        frames: count,
                        bytes: bytes[position * 8..(position + count) * 8].to_vec(),
                    },
                    &mut send,
                )
                .expect("stream resampling");
            position += count;
        }
        converter.finish(&mut send).expect("resampler tail");
        assert_eq!(
            converted.len(),
            reference.stdout.len(),
            "sample count at {rate}"
        );
        let error = samples(&converted)
            .iter()
            .zip(samples(&reference.stdout))
            .map(|(a, b)| (a - b).abs())
            .fold(0_f32, f32::max);
        assert!(
            error < 0.000001,
            "resampling changed signal at {rate}: {error}"
        );
    }
    fs::remove_dir_all(root).expect("remove owned resample fixture");
}
