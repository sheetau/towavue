use super::*;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn pcm(
    chunks: &[AudioChunk],
    start: MediaTime,
    end: Option<MediaTime>,
) -> (Option<MediaTime>, Vec<u8>) {
    let mut first = None;
    let mut bytes = Vec::new();
    for chunk in chunks {
        let (range, time) = clip_audio_bounds(chunk, start, end);
        if !range.is_empty() {
            first.get_or_insert(time);
            bytes.extend_from_slice(&chunk.bytes[range.start * 8..range.end * 8]);
        }
    }
    (first, bytes)
}

fn collect(input: &mut ParallelInput, start: MediaTime, end: Option<MediaTime>) -> Vec<AudioChunk> {
    let mut chunks = Vec::new();
    input
        .decode_software(start, end, Some(DecodeStream::Audio), &|| false, |output| {
            if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) = output {
                chunks.push(chunk);
            }
            true
        })
        .expect("selected audio decode");
    chunks
}

#[test]
fn explicit_audio_tracks_preserve_pcm_formats_offsets_seeks_and_default_selection() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-audio-tracks-{unique}"));
    fs::create_dir(&root).expect("owned fixture directory");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe");
    for (codec, extension) in [("flac", "mkv"), ("aac", "mp4")] {
        let source = root.join(format!("source.{extension}"));
        let mut command = crate::hidden_test_command(&ffmpeg);
        command.args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "color=size=64x64:rate=30:duration=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=3",
            "-itsoffset",
            "0.25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000:duration=3",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-c:v",
            "libopenh264",
            "-c:a",
            codec,
            "-ac",
            "2",
            "-disposition:a:0",
            "0",
            "-disposition:a:1",
            "default",
            "-metadata:s:a:0",
            "title=Low",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:1",
            "title=日本語",
            "-metadata:s:a:1",
            "language=jpn",
        ]);
        // AAC noise substitution depends on decoder history. This control checks
        // exact selected-track samples; the separate AAC noise/phase controls
        // retain coverage of that codec behavior without claiming byte identity.
        if codec == "aac" {
            command.args(["-aac_pns", "0"]);
        }
        let generated = command
            .arg(&source)
            .output()
            .expect("generate two-track source");
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let original = fs::read(&source).expect("source bytes");
        let tracks = probe_audio_tracks(&source).expect("scalar tracks");
        assert_eq!(tracks.tracks.len(), 2);
        assert_eq!(tracks.preferred, Some(AudioTrackId::from_index(2)));
        assert_eq!(tracks.tracks[0].language.as_deref(), Some("eng"));
        assert_eq!(tracks.tracks[1].language.as_deref(), Some("jpn"));
        if extension == "mkv" {
            assert_eq!(tracks.tracks[0].title.as_deref(), Some("Low"));
            assert_eq!(tracks.tracks[1].title.as_deref(), Some("日本語"));
        }
        let mut all = Vec::new();
        for (ordinal, rate) in [44_100, 48_000].into_iter().enumerate() {
            let track = tracks.tracks[ordinal].id;
            assert_eq!(track.index(), ordinal + 1);
            assert_eq!(
                probe_audio_track_format(&source, Some(track))
                    .expect("selected format")
                    .expect("audio")
                    .sample_rate,
                rate
            );
            let mut input = ParallelInput::open_audio_track(&source, Some(track), &|| false)
                .expect("track-scoped input");
            let sequential = collect(&mut input, MediaTime::ZERO, None);
            assert!(
                sequential
                    .iter()
                    .all(|chunk| chunk.format.sample_rate == rate)
            );
            let actual = pcm(&sequential, MediaTime::ZERO, None);
            let reference = crate::hidden_test_command(&ffmpeg)
                .args(["-v", "error", "-i"])
                .arg(&source)
                .args([
                    "-map",
                    &format!("0:{}", track.index()),
                    "-vn",
                    "-c:a",
                    "pcm_f32le",
                    "-f",
                    "f32le",
                    "pipe:1",
                ])
                .output()
                .expect("independent selected-track PCM");
            assert!(
                reference.status.success(),
                "{}",
                String::from_utf8_lossy(&reference.stderr)
            );
            assert!(
                actual.1 == reference.stdout,
                "selected PCM differs from FFmpeg: {codec}/{ordinal}, native={} bytes, reference={} bytes, first difference={:?}",
                actual.1.len(),
                reference.stdout.len(),
                actual
                    .1
                    .iter()
                    .zip(&reference.stdout)
                    .position(|(a, b)| a != b)
            );
            if ordinal == 1 {
                let first = actual.0.expect("delayed track start").as_nanoseconds();
                assert!(
                    (200_000_000..=300_000_000).contains(&first),
                    "retain the delayed source origin: {first}"
                );
            }
            for ns in [1_234_567_000, 100_000_000, 2_750_000_000, 0] {
                let start = MediaTime::from_nanoseconds(ns);
                let end = start.saturating_add(Duration::from_millis(200));
                let expected = pcm(&sequential, start, Some(end));
                let selected = collect(&mut input, start, Some(end));
                let got = pcm(&selected, start, Some(end));
                assert!(
                    got == expected,
                    "exact track seek differs: {codec}/{ordinal}/{ns}; first={:?}/{:?}, bytes={}/{}, first difference={:?}",
                    got.0,
                    expected.0,
                    got.1.len(),
                    expected.1.len(),
                    got.1.iter().zip(&expected.1).position(|(a, b)| a != b)
                );
                let mut intervals = Vec::new();
                decode_audio_track_intervals_with_policy(
                    &source,
                    Some(track),
                    start,
                    end,
                    &[],
                    AudioSeekPolicy::Exact,
                    &|| false,
                    |chunk| {
                        intervals.push(chunk);
                        true
                    },
                )
                .expect("fresh selected interval");
                assert!(
                    pcm(&intervals, start, Some(end)) == expected,
                    "fresh track interval differs: {codec}/{ordinal}/{ns}"
                );
                let mut retimed_input = Vec::new();
                decode_audio_track_cancellable(
                    &source,
                    Some(track),
                    start,
                    Some(end),
                    &|| false,
                    |output| {
                        if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) =
                            output
                        {
                            retimed_input.push(chunk);
                        }
                        true
                    },
                )
                .expect("exact input for tempo-changing playback");
                assert!(
                    pcm(&retimed_input, start, Some(end)) == expected,
                    "tempo input differs: {codec}/{ordinal}/{ns}"
                );
                let mut listening = Vec::new();
                decode_playback_audio_track_cancellable(
                    &source,
                    Some(track),
                    start,
                    Some(end),
                    &|| false,
                    |output| {
                        if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) =
                            output
                        {
                            listening.push(chunk);
                        }
                        true
                    },
                )
                .expect("selected listening track");
                assert!(
                    listening
                        .iter()
                        .all(|chunk| chunk.format.sample_rate == rate)
                );
                assert!(
                    pcm(&listening, start, Some(end)) == expected,
                    "noise-free selected listening samples differ: {codec}/{ordinal}/{ns}"
                );
            }
            assert!(matches!(
                input.decode_software(
                    MediaTime::ZERO,
                    None,
                    Some(DecodeStream::Audio),
                    &|| true,
                    |_| true
                ),
                Err(DecodeError::ConsumerClosed)
            ));
            assert!(
                pcm(
                    &collect(&mut input, MediaTime::ZERO, None),
                    MediaTime::ZERO,
                    None
                ) == actual,
                "cancelled input retains track identity"
            );
            all.push(actual);
        }
        assert!(
            all[0].1 != all[1].1,
            "fixture detects choosing the wrong track"
        );
        let mut preferred = ParallelInput::open(&source, &|| false).expect("default input");
        assert!(
            pcm(
                &collect(&mut preferred, MediaTime::ZERO, None),
                MediaTime::ZERO,
                None
            ) == all[1],
            "default still selects the preferred track"
        );
        for index in [0, 3, usize::MAX] {
            let id = AudioTrackId::from_index(index);
            assert!(matches!(
                ParallelInput::open_audio_track(&source, Some(id), &|| false),
                Err(DecodeError::AudioTrackUnavailable)
            ));
            assert!(matches!(
                probe_audio_track_format(&source, Some(id)),
                Err(DecodeError::AudioTrackUnavailable)
            ));
        }
        assert_eq!(fs::read(&source).expect("source retained"), original);
    }
    let error = DecodeError::AudioTrackUnavailable;
    assert_eq!(
        error.message(towavue_core::localization::Language::English),
        error.to_string()
    );
    assert_ne!(
        error.message(towavue_core::localization::Language::Japanese),
        error.to_string()
    );
    fs::remove_dir_all(root).expect("remove owned generated fixtures");
}
