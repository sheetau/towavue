use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
#[ignore = "requires an explicit read-only seek reference source"]
fn reference_audio_and_video_seek() {
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("reference source"),
    );
    for ms in [0, 30000, 10000, 100000] {
        let target = MediaTime::from_nanoseconds(ms * 1_000_000);
        for stream in [DecodeStream::Audio, DecodeStream::Video] {
            let mut input = ParallelInput::open(&path, &|| false).expect("reference input");
            let result = input.decode_software(
                target,
                Some(target.saturating_add(Duration::from_millis(190))),
                Some(stream),
                &|| false,
                |_| true,
            );
            let summary = result.expect("seek reference stream");
            assert_eq!(summary.video_frames, 0, "album art is not playback video");
            if stream == DecodeStream::Audio {
                assert!(summary.audio_frames > 0);
            }
        }
    }
}

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
        .input
        .as_ref()
        .expect("active input")
        .input
        .streams()
        .map(|s| unsafe { (*s.as_ptr()).discard })
        .collect()
}

#[test]
fn verified_pcm_reopens_match_fresh_and_release_file_locks() {
    use std::os::windows::fs::OpenOptionsExt;
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-pcm-reopen-{unique}"));
    fs::create_dir(&root).expect("exclusive fixture");
    let ffmpeg = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
    let generate = |path: &Path, codec: &str, seed: &str| {
        let status = crate::hidden_test_command(&ffmpeg)
            .args([
                "-v",
                "error",
                "-n",
                "-f",
                "lavfi",
                "-i",
                &format!("anoisesrc=sample_rate=48000:duration=2.2:seed={seed}"),
                "-ac",
                "2",
                "-c:a",
                codec,
            ])
            .arg(path)
            .status()
            .expect("fixture generation");
        assert!(status.success());
    };
    for codec in ["pcm_s16le", "pcm_s24le", "pcm_f32le", "adpcm_ima_wav"] {
        let path = root.join(format!("{codec}.wav"));
        generate(&path, codec, "175");
        for track in [None, Some(AudioTrackId::from_index(0))] {
            let mut reused = PlaybackAudioInput::open(&path, track, &|| false).expect("full probe");
            assert_eq!(reused.verified.is_some(), codec == "pcm_s16le");
            if codec != "pcm_s16le" {
                assert!(reused.release_completed().is_none());
                continue;
            }
            assert!(!reused.verified.as_ref().expect("verified PCM proof").reused);
            for policy in [AudioSeekPolicy::Playback, AudioSeekPolicy::Exact] {
                for ms in [1751, 0, 2500, 637, 213] {
                    reused = reused.release_completed().expect("scalar proof retained");
                    assert!(reused.input.is_none());
                    // A completed run must not extend the old reader's lifetime.
                    drop(
                        fs::OpenOptions::new()
                            .read(true)
                            .write(true)
                            .share_mode(0)
                            .open(&path)
                            .expect("no remaining native or lease handle"),
                    );
                    let target = MediaTime::from_nanoseconds(ms * 1_000_000);
                    let mut fresh =
                        PlaybackAudioInput::open(&path, track, &|| false).expect("fresh");
                    assert_eq!(
                        collect(&mut reused, target, policy),
                        collect(&mut fresh, target, policy)
                    );
                    assert!(
                        reused.verified.as_ref().expect("verified PCM proof").reused,
                        "bounded repeat probe was used"
                    );
                }
            }
        }
    }
    let path = root.join("pcm_s16le.wav");
    let original_stamp = path
        .metadata()
        .expect("fixture metadata")
        .modified()
        .expect("fixture write time");
    let input = PlaybackAudioInput::open(&path, None, &|| false).expect("open PCM fixture");
    let mut dormant = input.release_completed().expect("completed PCM proof");
    let replacement = root.join("replacement.wav");
    generate(&replacement, "pcm_s16le", "276");
    assert_eq!(
        path.metadata().expect("fixture metadata").len(),
        replacement.metadata().expect("fixture metadata").len()
    );
    fs::File::options()
        .write(true)
        .open(&replacement)
        .expect("open replacement for timestamp")
        .set_times(fs::FileTimes::new().set_modified(original_stamp))
        .expect("restore replacement write time");
    fs::rename(&path, root.join("original.wav")).expect("rename after completion");
    fs::rename(&replacement, &path).expect("replace same path");
    let target = MediaTime::from_nanoseconds(637_000_000);
    let mut fresh = PlaybackAudioInput::open(&path, None, &|| false).expect("open PCM fixture");
    assert_eq!(
        collect(&mut dormant, target, AudioSeekPolicy::Exact),
        collect(&mut fresh, target, AudioSeekPolicy::Exact)
    );
    assert!(
        !dormant
            .verified
            .as_ref()
            .expect("verified PCM proof")
            .reused,
        "new identity needs full probe"
    );
    drop(fresh);
    dormant = dormant.release_completed().expect("completed PCM proof");
    let same_file_stamp = path
        .metadata()
        .expect("fixture metadata")
        .modified()
        .expect("fixture write time");
    let modified = root.join("modified.wav");
    generate(&modified, "pcm_s16le", "377");
    fs::write(&path, fs::read(&modified).expect("read PCM fixture"))
        .expect("modify same file identity");
    fs::File::options()
        .write(true)
        .open(&path)
        .expect("open changed file for timestamp")
        .set_times(fs::FileTimes::new().set_modified(same_file_stamp))
        .expect("restore changed file write time");
    let mut fresh = PlaybackAudioInput::open(&path, None, &|| false).expect("open PCM fixture");
    assert_eq!(
        collect(&mut dormant, target, AudioSeekPolicy::Exact),
        collect(&mut fresh, target, AudioSeekPolicy::Exact)
    );
    assert!(
        !dormant
            .verified
            .as_ref()
            .expect("verified PCM proof")
            .reused,
        "change time survives restored write time"
    );
    drop((dormant, fresh));

    let input = PlaybackAudioInput::open(&path, None, &|| false).expect("full probe before writer");
    let mut dormant = input
        .release_completed()
        .expect("completed proof before writer");
    let writer = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("existing shared writer");
    let mut fresh = PlaybackAudioInput::open(&path, None, &|| false).expect("ordinary shared read");
    assert_eq!(
        collect(&mut dormant, target, AudioSeekPolicy::Exact),
        collect(&mut fresh, target, AudioSeekPolicy::Exact)
    );
    assert!(
        dormant.verified.is_none(),
        "lease denial uses ordinary probing without proof"
    );
    drop((dormant, fresh, writer));

    let input =
        PlaybackAudioInput::open(&path, None, &|| false).expect("probe before cancellation");
    let mut dormant = input
        .release_completed()
        .expect("completed proof before cancellation");
    assert!(matches!(
        dormant.decode(
            MediaTime::ZERO,
            None,
            AudioSeekPolicy::Exact,
            &|| true,
            |_| panic!("cancelled reopen must not emit")
        ),
        Err(DecodeError::ConsumerClosed)
    ));
    assert!(dormant.input.is_none());
    let mut fresh =
        PlaybackAudioInput::open(&path, None, &|| false).expect("fresh after cancelled reopen");
    assert_eq!(
        collect(&mut dormant, target, AudioSeekPolicy::Exact),
        collect(&mut fresh, target, AudioSeekPolicy::Exact)
    );
    assert!(!dormant.verified.as_ref().expect("new proof").reused);
    drop((dormant, fresh));

    // A PCM declaration does not prove that the payload is PCM. Preserve the
    // original full probe's detection of a compressed DTS-in-WAV payload.
    let dts = root.join("hidden.dts");
    let status = crate::hidden_test_command(&ffmpeg)
        .args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "sine=sample_rate=48000:duration=2.2",
            "-ac",
            "2",
            "-c:a",
            "dca",
            "-strict",
            "-2",
            "-b:a",
            "768k",
            "-f",
            "dts",
        ])
        .arg(&dts)
        .status()
        .expect("generate DTS fixture");
    assert!(status.success());
    let payload = fs::read(&dts).expect("read DTS payload");
    let mut wave = Vec::new();
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(36 + payload.len() as u32).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16_u32.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes());
    wave.extend_from_slice(&2_u16.to_le_bytes());
    wave.extend_from_slice(&48000_u32.to_le_bytes());
    wave.extend_from_slice(&192000_u32.to_le_bytes());
    wave.extend_from_slice(&4_u16.to_le_bytes());
    wave.extend_from_slice(&16_u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    wave.extend_from_slice(&payload);
    let hidden = root.join("hidden.wav");
    fs::write(&hidden, wave).expect("write disguised WAV");
    let compressed =
        PlaybackAudioInput::open(&hidden, None, &|| false).expect("probe disguised DTS");
    assert_eq!(
        compressed
            .input
            .as_ref()
            .expect("active compressed input")
            .input
            .stream(0)
            .expect("compressed audio stream")
            .parameters()
            .id(),
        ffmpeg::codec::Id::DTS
    );
    assert!(compressed.verified.is_none());
    assert!(compressed.release_completed().is_none());

    // Preserve existing demux semantics for metadata and multiple data chunks,
    // including seeks beyond a short first chunk. Compare against full probing;
    // do not assume a canonical 44-byte WAV header or a single data payload.
    let encoded = fs::read(&modified).expect("read PCM fixture");
    let mut chunks = Vec::new();
    let mut offset = 12;
    let mut audio = Vec::new();
    while offset + 8 <= encoded.len() {
        let size = u32::from_le_bytes(
            encoded[offset + 4..offset + 8]
                .try_into()
                .expect("four-byte RIFF size"),
        ) as usize;
        if &encoded[offset..offset + 4] == b"data" {
            audio = encoded[offset + 8..offset + 8 + size].to_vec();
        } else {
            chunks.extend_from_slice(&encoded[offset..offset + 8 + size + (size & 1)]);
        }
        offset += 8 + size + (size & 1);
    }
    assert!(audio.len() > 4000);
    for (name, split) in [("metadata", false), ("multiple-data", true)] {
        let mut body = chunks.clone();
        let split_at = if split { 4000 } else { audio.len() };
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(split_at as u32).to_le_bytes());
        body.extend_from_slice(&audio[..split_at]);
        body.extend_from_slice(b"JUNK\x07\x00\x00\x00padding\x00");
        if split {
            body.extend_from_slice(b"data");
            body.extend_from_slice(&((audio.len() - split_at) as u32).to_le_bytes());
            body.extend_from_slice(&audio[split_at..]);
        }
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(&body);
        let path = root.join(format!("{name}.wav"));
        fs::write(&path, bytes).expect("write chunked WAV");
        let mut reused =
            PlaybackAudioInput::open(&path, None, &|| false).expect("open PCM fixture");
        for ms in [0, 637, 1751, 213] {
            reused = reused.release_completed().expect("PCM proof");
            let mut fresh =
                PlaybackAudioInput::open(&path, None, &|| false).expect("open PCM fixture");
            let target = MediaTime::from_nanoseconds(ms * 1_000_000);
            assert_eq!(
                collect(&mut reused, target, AudioSeekPolicy::Exact),
                collect(&mut fresh, target, AudioSeekPolicy::Exact),
                "{name} at {ms}"
            );
        }
    }
    fs::remove_dir_all(root).expect("remove generated fixtures");
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

#[test]
fn attached_album_art_does_not_become_a_playback_seek_target() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-cover-seek-{unique}"));
    fs::create_dir(&root).expect("exclusive fixture");
    let ffmpeg = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
    for (extension, codec) in [("mp3", "libmp3lame"), ("m4a", "aac")] {
        let path = root.join(format!("album.{extension}"));
        let status = crate::hidden_test_command(&ffmpeg)
            .args([
                "-v",
                "error",
                "-n",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=2.2:sample_rate=48000",
                "-f",
                "lavfi",
                "-i",
                "color=red:size=64x64:duration=0.04",
                "-map",
                "0:a",
                "-map",
                "1:v",
                "-c:a",
                codec,
                "-c:v",
                "mjpeg",
                "-disposition:v",
                "attached_pic",
            ])
            .arg(&path)
            .status()
            .expect("cover fixture");
        assert!(status.success());
        let mut video = ParallelInput::open(&path, &|| false).expect("input");
        assert!(
            best_stream_config(&video.input, Type::Video).is_some(),
            "artwork remains extractable"
        );
        let mut audio = PlaybackAudioInput::open(&path, None, &|| false).expect("audio input");
        for ms in [1200, 200, 0, 1700] {
            let target = MediaTime::from_nanoseconds(ms * 1_000_000);
            let summary = video
                .decode_software(
                    target,
                    Some(target.saturating_add(Duration::from_millis(190))),
                    Some(DecodeStream::Video),
                    &|| false,
                    |_| true,
                )
                .expect("no seek into artwork");
            assert_eq!(summary.video_frames, 0);
            let samples = collect(&mut audio, target, AudioSeekPolicy::Playback);
            let mut fresh = PlaybackAudioInput::open(&path, None, &|| false).expect("fresh audio");
            assert!(!samples.bytes.is_empty());
            assert_eq!(
                samples,
                collect(&mut fresh, target, AudioSeekPolicy::Playback)
            );
        }
    }
    fs::remove_dir_all(root).expect("fixture cleanup");
}
