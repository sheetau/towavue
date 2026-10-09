use super::*;
use std::fs;
use std::os::windows::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, Ordering};

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
        &std::env::var_os("TOWAVUE_REPROBE_FIXTURES").expect("fixed reprobe fixtures"),
    )
    .collect();
    assert!(paths.len() >= 5 && paths.iter().all(|p| p.is_file()));
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
    target: MediaTime,
    end: Option<MediaTime>,
    policy: AudioSeekPolicy,
) -> (DecodeSummary, Vec<Chunk>, usize) {
    let mut chunks = Vec::new();
    let mut finished = 0;
    let summary = input
        .decode(target, end, policy, &|| false, |output| {
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
        .expect("audio interval");
    assert_eq!(finished, 1);
    (summary, chunks, finished)
}

fn fresh(path: &Path, track: Option<AudioTrackId>) -> PlaybackAudioInput {
    let _ordinary = Mode::set(false);
    let input = PlaybackAudioInput::open(path, track, &|| false).expect("ordinary input");
    assert!(input.verified_video.is_none());
    input
}

#[test]
#[ignore = "fixed native fixtures; full PCM, timestamps, reopen, cancellation and EOF"]
fn native_reopen_preserves_pcm_ranges_counts_cancel_and_recovery() {
    let _restore = Mode::set(true);
    for (case, path) in fixtures().iter().enumerate() {
        for track in [None, Some(AudioTrackId::from_index(1))] {
            let mut input =
                PlaybackAudioInput::open(path, track, &|| false).expect("first full probe");
            let eligible = input.verified_video.is_some();
            println!("REPROBE_CASE case={case} track={track:?} eligible={eligible}");
            if path.file_name().is_some_and(|n| n == "h264-multitrack.mp4") {
                assert!(
                    !eligible,
                    "unsupported stream topology keeps ordinary probing"
                );
            }
            if case < 5 {
                assert_eq!(eligible, matches!(case, 0 | 1 | 3), "case={case}");
            }
            for policy in [AudioSeekPolicy::Playback, AudioSeekPolicy::Exact] {
                for ns in [
                    8_416_666_667,
                    2_416_666_667,
                    9_616_666_667,
                    3_616_666_667,
                    7_216_666_667,
                    1_216_666_667,
                    0,
                    11_999_999_999,
                    13_000_000_000,
                ] {
                    let target = MediaTime::from_nanoseconds(ns);
                    for end in [
                        Some(target.saturating_add(Duration::from_millis(100))),
                        Some(target),
                        Some(target.saturating_sub(Duration::from_millis(100))),
                    ] {
                        input = input.release_completed().unwrap_or_else(|| {
                            PlaybackAudioInput::open(path, track, &|| false)
                                .expect("unsupported ordinary open")
                        });
                        let expected = collect(&mut fresh(path, track), target, end, policy);
                        let actual = collect(&mut input, target, end, policy);
                        assert!(
                            actual == expected,
                            "case={case} target={ns} end={end:?} policy={policy:?}; actual summary={:?} first={:?}, expected summary={:?} first={:?}",
                            actual.0,
                            actual.1.first().map(|c| c.pts),
                            expected.0,
                            expected.1.first().map(|c| c.pts)
                        );
                        if eligible && case < 5 {
                            assert!(
                                input.verified_video.as_ref().is_some_and(|p| p.reused),
                                "qualified source must actually use verified probe case={case}"
                            );
                        }
                    }
                }
                // A full source comparison exercises actual natural completion.
                input = input
                    .release_completed()
                    .unwrap_or_else(|| fresh(path, track));
                assert_eq!(
                    collect(&mut input, MediaTime::ZERO, None, policy),
                    collect(&mut fresh(path, track), MediaTime::ZERO, None, policy),
                    "full case={case}"
                );
            }
            for ns in [9_616_666_667, 3_616_666_667, 0] {
                let target = MediaTime::from_nanoseconds(ns);
                assert_eq!(
                    collect(&mut input, target, None, AudioSeekPolicy::Playback),
                    collect(
                        &mut fresh(path, track),
                        target,
                        None,
                        AudioSeekPolicy::Playback
                    )
                );
            }
            let cancelled = AtomicBool::new(false);
            let mut seen = 0;
            let result = input.decode(
                MediaTime::ZERO,
                None,
                AudioSeekPolicy::Playback,
                &|| cancelled.load(Ordering::Relaxed),
                |item| {
                    if matches!(
                        item,
                        ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(_))
                    ) {
                        seen += 1;
                        cancelled.store(true, Ordering::Relaxed);
                    }
                    true
                },
            );
            assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
            assert_eq!(seen, 1);
            assert!(matches!(
                input.decode(
                    MediaTime::ZERO,
                    None,
                    AudioSeekPolicy::Playback,
                    &|| false,
                    |_| false
                ),
                Err(DecodeError::ConsumerClosed)
            ));
            let target = MediaTime::from_nanoseconds(637_000_000);
            assert_eq!(
                collect(&mut input, target, None, AudioSeekPolicy::Exact),
                collect(
                    &mut fresh(path, track),
                    target,
                    None,
                    AudioSeekPolicy::Exact
                )
            );
        }
    }
}

fn dormant(path: &Path) -> PlaybackAudioInput {
    let input = PlaybackAudioInput::open(path, None, &|| false).expect("full probe");
    assert!(input.verified_video.is_some());
    input
        .release_completed()
        .expect("proof without native input")
}

#[test]
#[ignore = "owned video fixtures; handles, writers, file identity and signature fallback"]
fn native_reopen_releases_handles_and_rejects_stale_or_invalid_proofs() {
    let _restore = Mode::set(true);
    let root = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_REPROBE_GUARD_DIR").expect("exclusive fixture directory"),
    );
    fs::create_dir(&root).expect("new guard directory");
    let source = fixtures().remove(0);
    let path = root.join("guard.mp4");
    fs::copy(&source, &path).expect("owned fixture copy");
    let target = MediaTime::from_nanoseconds(637_000_000);
    let end = Some(target.saturating_add(Duration::from_millis(190)));
    let compare = |input: &mut PlaybackAudioInput| {
        assert_eq!(
            collect(input, target, end, AudioSeekPolicy::Exact),
            collect(&mut fresh(&path, None), target, end, AudioSeekPolicy::Exact)
        );
    };
    let mut input = dormant(&path);
    assert!(input.input.is_none());
    drop(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .expect("no retained file handle"),
    );
    compare(&mut input);
    assert!(input.verified_video.as_ref().expect("proof").reused);
    drop(input);
    // A forged final signature must force a completely fresh ordinary input.
    let mut input = dormant(&path);
    input.verified_video.as_mut().expect("proof").complete[0] ^= 1;
    compare(&mut input);
    assert!(input.verified_video.is_none());
    drop(input);
    let mut input = dormant(&path);
    input.verified_video.as_mut().expect("proof").header[0] ^= 1;
    compare(&mut input);
    assert!(
        !input
            .verified_video
            .as_ref()
            .expect("new full proof")
            .reused
    );
    drop(input);
    let mut input = dormant(&path);
    let writer = fs::OpenOptions::new()
        .write(true)
        .read(true)
        .open(&path)
        .expect("shared writer");
    compare(&mut input);
    assert!(input.verified_video.is_none());
    drop((input, writer));
    let mut input = dormant(&path);
    let stamp = path
        .metadata()
        .expect("metadata")
        .modified()
        .expect("write stamp");
    fs::rename(&path, root.join("old.mp4")).expect("rename completed source");
    fs::copy(&source, &path).expect("replacement with same bytes and length");
    fs::File::options()
        .write(true)
        .open(&path)
        .expect("set stamp")
        .set_times(fs::FileTimes::new().set_modified(stamp))
        .expect("restore timestamp");
    compare(&mut input);
    assert!(
        !input
            .verified_video
            .as_ref()
            .expect("new identity proof")
            .reused
    );
    drop(input);
    let mut input = dormant(&path);
    let stamp = path
        .metadata()
        .expect("metadata")
        .modified()
        .expect("write stamp");
    let mut changed = fs::read(&path).expect("fixture bytes");
    let tag = changed
        .windows(4)
        .position(|b| b == b"Lavf")
        .expect("encoder metadata");
    changed[tag + 3] = b'F';
    fs::write(&path, changed).expect("same-length metadata change on same file identity");
    fs::File::options()
        .write(true)
        .open(&path)
        .expect("set stamp")
        .set_times(fs::FileTimes::new().set_modified(stamp))
        .expect("restore timestamp");
    compare(&mut input);
    assert!(
        !input
            .verified_video
            .as_ref()
            .expect("changed identity proof")
            .reused
    );
    drop(input);
    let mut input = dormant(&path);
    assert!(matches!(
        input.decode(
            MediaTime::ZERO,
            None,
            AudioSeekPolicy::Exact,
            &|| true,
            |_| panic!("cancelled reopen")
        ),
        Err(DecodeError::ConsumerClosed)
    ));
    assert!(input.input.is_none());
    compare(&mut input);
    assert!(!input.verified_video.as_ref().expect("new proof").reused);
    drop(input);
    let mut input = dormant(&path);
    fs::rename(&path, root.join("removed.mp4")).expect("remove completed path");
    assert!(input.reopen(&|| false).is_err());
    assert!(input.input.is_none());
}

#[test]
#[ignore = "same-source regression for negative Matroska rewind in both decode and probe modes"]
fn native_negative_timestamp_rewind_modes_preserve_pcm() {
    let _restore = Mode::set(true);
    let old_inline = audio_inline::ENABLED.get();
    let path = fixtures().last().expect("negative-start fixture").clone();
    let fingerprint = |value: &(DecodeSummary, Vec<Chunk>, usize)| {
        let mut hash = crc32fast::Hasher::new();
        let mut frames = 0;
        for c in &value.1 {
            hash.update(&c.bytes);
            frames += c.frames;
        }
        (
            value.0.audio_frames,
            frames,
            value.1.first().map(|c| c.pts),
            hash.finalize(),
        )
    };
    let mut compared = 0;
    for inline in [false, true] {
        audio_inline::ENABLED.set(inline);
        for optimized in [false, true] {
            ENABLED.set(optimized);
            let mut input =
                PlaybackAudioInput::open(&path, None, &|| false).expect("diagnostic input");
            let _ = collect(&mut input, MediaTime::ZERO, None, AudioSeekPolicy::Exact);
            for ns in [9_616_666_667, 3_616_666_667, 0, 637_000_000] {
                let target = MediaTime::from_nanoseconds(ns);
                let actual = collect(&mut input, target, None, AudioSeekPolicy::Playback);
                let expected = collect(
                    &mut fresh(&path, None),
                    target,
                    None,
                    AudioSeekPolicy::Playback,
                );
                println!(
                    "NEGATIVE_REWIND inline={inline} reprobe={optimized} target={ns} equal={} actual={:?} expected={:?}",
                    actual == expected,
                    fingerprint(&actual),
                    fingerprint(&expected)
                );
                assert!(
                    actual == expected,
                    "negative rewind inline={inline} reprobe={optimized} target={ns}"
                );
                compared += 1;
            }
        }
    }
    audio_inline::ENABLED.set(old_inline);
    assert_eq!(compared, 16);
}
