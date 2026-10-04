use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, PartialEq)]
struct Samples {
    first: Option<MediaTime>,
    bytes: Vec<u8>,
}

fn collect(input: &mut PlaybackAudioInput, target: MediaTime, policy: AudioSeekPolicy) -> Samples {
    let mut samples = Samples {
        first: None,
        bytes: Vec::new(),
    };
    input
        .decode(
            target,
            Some(target.saturating_add(Duration::from_millis(190))),
            policy,
            &|| false,
            |output| {
                if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) = output {
                    samples.first.get_or_insert(chunk.presentation_time);
                    samples.bytes.extend_from_slice(&chunk.bytes);
                }
                true
            },
        )
        .expect("decode audio interval");
    samples
}

fn policies(input: &PlaybackAudioInput) -> Vec<ffmpeg::ffi::AVDiscard> {
    // SAFETY: the input owns these streams, with no live decode workers.
    input
        .0
        .input
        .streams()
        .map(|s| unsafe { (*s.as_ptr()).discard })
        .collect()
}

#[test]
fn retained_audio_matches_fresh_pcm_across_seeks_rewind_eof_and_cancellation() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-retained-audio-{unique}"));
    fs::create_dir(&root).expect("exclusive fixture");
    let ffmpeg = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
    for (codec, extension, video) in [
        ("pcm_s16le", "wav", false),
        ("flac", "flac", false),
        ("libmp3lame", "mp3", false),
        ("aac", "m4a", false),
        ("libopus", "webm", true),
        ("libvorbis", "mkv", true),
        ("pcm_s16le", "mkv", true),
    ] {
        let path = root.join(format!("{codec}.{extension}"));
        let mut command = crate::hidden_test_command(&ffmpeg);
        command.args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "anoisesrc=sample_rate=48000:duration=2.2:seed=175",
        ]);
        if video {
            command.args([
                "-f",
                "lavfi",
                "-i",
                "color=size=32x24:rate=10:duration=2.2",
                "-map",
                "1:v",
                "-map",
                "0:a",
                "-c:v",
                "libvpx-vp9",
            ]);
        }
        let output = command
            .args(["-ac", "2", "-c:a", codec])
            .arg(&path)
            .output()
            .expect("generate fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        for track in [None, Some(AudioTrackId::from_index(usize::from(video)))] {
            let mut reused =
                PlaybackAudioInput::open(&path, track, &|| false).expect("retained input");
            let original_policies = policies(&reused);
            assert!(reused.matches_track(track));
            assert!(!reused.matches_track(Some(AudioTrackId::from_index(99))));
            for policy in [AudioSeekPolicy::Playback, AudioSeekPolicy::Exact] {
                for ms in [1137, 213, 1751, 0, 2500, 637] {
                    let target = MediaTime::from_nanoseconds(ms * 1_000_000);
                    let mut fresh =
                        PlaybackAudioInput::open(&path, track, &|| false).expect("fresh input");
                    let actual = collect(&mut reused, target, policy);
                    let expected = collect(&mut fresh, target, policy);
                    assert!(
                        actual == expected,
                        "{codec} {track:?} {policy:?} at {ms}: origin {:?}/{:?}, bytes {}/{}",
                        actual.first,
                        expected.first,
                        actual.bytes.len(),
                        expected.bytes.len()
                    );
                    assert_eq!(
                        policies(&reused),
                        original_policies,
                        "seek anchor policies restored"
                    );
                }
            }
            let aborted = reused.decode(
                MediaTime::ZERO,
                None,
                AudioSeekPolicy::Playback,
                &|| false,
                |_| false,
            );
            assert!(matches!(aborted, Err(DecodeError::ConsumerClosed)));
            assert_eq!(policies(&reused), original_policies);
            assert!(matches!(
                reused.decode(
                    MediaTime::ZERO,
                    None,
                    AudioSeekPolicy::Exact,
                    &|| true,
                    |_| panic!("cancelled input must not emit")
                ),
                Err(DecodeError::ConsumerClosed)
            ));
            assert_eq!(policies(&reused), original_policies);
            let target = MediaTime::from_nanoseconds(837_000_000);
            let mut fresh = PlaybackAudioInput::open(&path, track, &|| false)
                .expect("fresh after cancellation");
            assert!(
                collect(&mut reused, target, AudioSeekPolicy::Playback)
                    == collect(&mut fresh, target, AudioSeekPolicy::Playback),
                "{codec} after cancellation"
            );
        }
    }
    fs::remove_dir_all(root).expect("remove generated fixture");
}
