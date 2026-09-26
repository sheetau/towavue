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
