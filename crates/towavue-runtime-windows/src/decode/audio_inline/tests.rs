use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

struct Mode(bool);
impl Mode {
    fn set(value: bool) -> Self {
        Self(ENABLED.replace(value))
    }
}
impl Drop for Mode {
    fn drop(&mut self) {
        ENABLED.set(self.0);
    }
}

fn fixtures() -> Vec<std::path::PathBuf> {
    let paths: Vec<_> = std::env::split_paths(
        &std::env::var_os("TOWAVUE_INLINE_AUDIO_FIXTURES").expect("fixed audio fixtures"),
    )
    .collect();
    assert_eq!(paths.len(), 9);
    assert!(paths.iter().all(|p| p.is_file()));
    paths
}

#[derive(Debug, PartialEq)]
struct Chunk {
    pts: MediaTime,
    frames: usize,
    format: AudioFormat,
    bytes: Vec<u8>,
}

fn collect(
    input: &mut PlaybackAudioInput,
    minimum: MediaTime,
    maximum: Option<MediaTime>,
    policy: AudioSeekPolicy,
) -> (DecodeSummary, Vec<Chunk>, usize) {
    let mut chunks = Vec::new();
    let mut finished = 0;
    let summary = input
        .decode(minimum, maximum, policy, &|| false, |output| {
            match output {
                ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(c)) => chunks.push(Chunk {
                    pts: c.presentation_time,
                    frames: c.frames,
                    format: c.format,
                    bytes: c.bytes,
                }),
                ParallelSoftwareDecodeOutput::AudioFinished => finished += 1,
                _ => panic!("unexpected audio-only output"),
            }
            true
        })
        .expect("fixed interval");
    assert_eq!(finished, 1);
    (summary, chunks, finished)
}

#[test]
#[ignore = "explicit fixed fixtures; original and direct PCM/count/range/cancel comparison"]
fn native_direct_audio_matches_parallel_pcm_ranges_counts_cancel_and_recovery() {
    let _restore = Mode::set(false);
    for (case, path) in fixtures().iter().enumerate() {
        let mut old = PlaybackAudioInput::open(path, None, &|| false).expect("parallel input");
        let mut new = PlaybackAudioInput::open(path, None, &|| false).expect("direct input");
        for policy in [AudioSeekPolicy::Playback, AudioSeekPolicy::Exact] {
            for ns in [
                8_416_666_667,
                2_416_666_667,
                9_616_666_667,
                3_616_666_667,
                7_216_666_667,
                1_216_666_667,
                0,
                13_000_000_000,
            ] {
                let target = MediaTime::from_nanoseconds(ns);
                for maximum in [
                    Some(target.saturating_add(Duration::from_millis(100))),
                    Some(target),
                    Some(target.saturating_sub(Duration::from_millis(100))),
                ] {
                    ENABLED.set(false);
                    let expected = collect(&mut old, target, maximum, policy);
                    ENABLED.set(true);
                    assert_eq!(
                        collect(&mut new, target, maximum, policy),
                        expected,
                        "case={case} target={ns} max={maximum:?} policy={policy:?}"
                    );
                }
            }
        }
        let target = MediaTime::from_nanoseconds(2_416_666_667);
        for enabled in [false, true] {
            ENABLED.set(enabled);
            let mut input =
                PlaybackAudioInput::open(path, None, &|| false).expect("cancellation input");
            let cancelled = AtomicBool::new(false);
            let mut seen = 0;
            let result = input.decode(
                target,
                None,
                AudioSeekPolicy::Playback,
                &|| cancelled.load(Ordering::Relaxed),
                |output| {
                    if matches!(
                        output,
                        ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(_))
                    ) {
                        seen += 1;
                        cancelled.store(true, Ordering::Relaxed);
                    }
                    true
                },
            );
            assert!(
                matches!(result, Err(DecodeError::ConsumerClosed)),
                "{result:?}"
            );
            assert_eq!(seen, 1);
            let result = input.decode(target, None, AudioSeekPolicy::Playback, &|| false, |_| {
                false
            });
            assert!(
                matches!(result, Err(DecodeError::ConsumerClosed)),
                "{result:?}"
            );
            let mut reference = PlaybackAudioInput::open(path, None, &|| false).expect("reference");
            ENABLED.set(false);
            let expected = collect(
                &mut reference,
                target,
                Some(target.saturating_add(Duration::from_millis(100))),
                AudioSeekPolicy::Playback,
            );
            ENABLED.set(enabled);
            assert_eq!(
                collect(
                    &mut input,
                    target,
                    Some(target.saturating_add(Duration::from_millis(100))),
                    AudioSeekPolicy::Playback
                ),
                expected
            );
        }
        if case < 5 {
            let mut reference =
                PlaybackAudioInput::open(path, None, &|| false).expect("full reference");
            let mut direct = PlaybackAudioInput::open(path, None, &|| false).expect("full direct");
            ENABLED.set(false);
            let expected = collect(
                &mut reference,
                MediaTime::ZERO,
                None,
                AudioSeekPolicy::Exact,
            );
            ENABLED.set(true);
            assert_eq!(
                collect(&mut direct, MediaTime::ZERO, None, AudioSeekPolicy::Exact),
                expected
            );
        }
        println!(
            "AUDIO_INLINE_NATIVE case={case} policies=2 targets=8 ranges=3 cancel=2 refusal=2 recovery=2 exact_pcm=1 counts=1"
        );
    }
}

#[test]
#[ignore = "fixed component timing; no WASAPI, no desktop, not an ordinary ranking"]
fn component_first_selected_pcm_fixed_paired_blocks() {
    let _restore = Mode::set(false);
    for (case, path) in fixtures().iter().take(5).enumerate() {
        let mut inputs = [
            PlaybackAudioInput::open(path, None, &|| false).expect("old"),
            PlaybackAudioInput::open(path, None, &|| false).expect("new"),
        ];
        for block in -1_i32..12 {
            let modes = if (block + case as i32) % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            };
            for enabled in modes {
                ENABLED.set(enabled);
                for (step, percent) in [70, 20, 80, 30, 60, 10].into_iter().enumerate() {
                    let target = MediaTime::from_nanoseconds(percent * 120_000_000 + 16_666_667);
                    let cancelled = AtomicBool::new(false);
                    let start = Instant::now();
                    let mut observed = None;
                    let result = inputs[usize::from(enabled)].decode(
                        target,
                        None,
                        AudioSeekPolicy::Playback,
                        &|| cancelled.load(Ordering::Relaxed),
                        |output| {
                            if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(c)) =
                                output
                            {
                                assert!(observed.is_none());
                                observed = Some((
                                    start.elapsed().as_secs_f64() * 1000.0,
                                    c.presentation_time.as_nanoseconds(),
                                    c.frames,
                                ));
                                cancelled.store(true, Ordering::Relaxed);
                            }
                            true
                        },
                    );
                    assert!(
                        matches!(result, Err(DecodeError::ConsumerClosed)),
                        "{result:?}"
                    );
                    let (ms, pts, frames) = observed.expect("first selected PCM");
                    println!(
                        "AUDIO_INLINE_COST case={case} block={block} direct={} step={step} percent={percent} ms={ms:.6} pts={pts} frames={frames}",
                        u8::from(enabled)
                    );
                }
            }
        }
    }
}
