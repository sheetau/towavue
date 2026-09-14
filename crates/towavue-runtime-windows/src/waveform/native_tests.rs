use super::*;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn ffmpeg() -> Command {
    let mut command = Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"));
    command.creation_flags(0x0800_0000).args(["-v", "error"]);
    command
}

#[test]
fn native_overview_matches_cli_across_formats_layouts_and_short_tails() {
    let root = std::env::temp_dir().join(format!(
        "towavue-native-waveform-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos(),
    ));
    std::fs::create_dir(&root).expect("owned fixtures");
    let cases = [
        ("pcm_u8", "wav", "mono", 8000),
        ("pcm_s24le", "wav", "stereo", 44100),
        ("pcm_f32le", "wav", "5.1", 48000),
        ("flac", "flac", "5.1", 44100),
        ("libmp3lame", "mp3", "stereo", 32000),
        ("libvorbis", "ogg", "stereo", 44100),
        ("libopus", "opus", "stereo", 48000),
        ("aac", "m4a", "stereo", 48000),
        ("aac", "m4a", "5.1", 48000),
        ("ac3", "ac3", "5.1", 48000),
        ("eac3", "eac3", "5.1", 48000),
        ("alac", "m4a", "stereo", 44100),
    ];
    for (codec, extension, layout, rate) in cases {
        for duration in ["0.017", "1.37"] {
            let source = root.join(format!("{codec}-{layout}-{duration}.{extension}"));
            let channels = match layout {
                "mono" => 1,
                "stereo" => 2,
                _ => 6,
            };
            let expressions = (0..channels)
                .map(|channel| {
                    format!(
                        "0.7*sin(2*PI*{}*t)*(0.5+0.5*sin(2*PI*13*t))",
                        211 + channel * 173
                    )
                })
                .collect::<Vec<_>>()
                .join("|");
            let output = ffmpeg()
                .args(["-f", "lavfi", "-i"])
                .arg(format!(
                    "aevalsrc={expressions}:s={rate}:c={layout}:d={duration}"
                ))
                .args(["-c:a", codec])
                .arg(&source)
                .output()
                .expect("generate");
            assert!(
                output.status.success(),
                "{codec}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            compare(&source, "0:a:0", codec);
        }
    }
    let selected = root.join("selected.mka");
    let output = ffmpeg()
        .args([
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo:d=1.37",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000:duration=1.37",
            "-map",
            "0:a",
            "-map",
            "1:a",
            "-c:a",
            "flac",
            "-disposition:a:0",
            "0",
            "-disposition:a:1",
            "default",
        ])
        .arg(&selected)
        .output()
        .expect("selected-stream fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    compare(&selected, "0:a:1", "selected stream");
    let checks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let progress = checks.clone();
    assert!(matches!(
        decode_cancellable(&selected, 127, 96, move || {
            progress.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 16
        }),
        Err(PreviewError::Cancelled)
    ));
    assert!(checks.load(std::sync::atomic::Ordering::Relaxed) > 16);
    compare(&selected, "0:a:1", "after cancellation");
    let changing = root.join("changing.aac");
    let mut parts = Vec::new();
    for (index, (rate, channels)) in [(44100, 1), (48000, 2), (32000, 1), (48000, 2)]
        .into_iter()
        .enumerate()
    {
        let part = root.join(format!("part-{index}.aac"));
        let output = ffmpeg()
            .args(["-f", "lavfi", "-i"])
            .arg(format!(
                "sine=frequency=880:sample_rate={rate}:duration=0.37"
            ))
            .args(["-ac", &channels.to_string(), "-c:a", "aac"])
            .arg(&part)
            .output()
            .expect("changing-format part");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        parts.extend(std::fs::read(part).expect("owned part"));
    }
    std::fs::write(&changing, parts).expect("owned changing-format fixture");
    compare(&changing, "0:a:0", "changing format");
    std::fs::remove_dir_all(root).expect("remove owned fixtures");
}

fn compare(source: &Path, stream: &str, label: &str) {
    let stamp = std::fs::metadata(source).expect("source stamp");
    let output = ffmpeg()
        .arg("-i")
        .arg(source)
        .args([
            "-map",
            stream,
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            "-f",
            "s16le",
            "pipe:1",
        ])
        .output()
        .expect("CLI reference");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (width, height) in [(127, 96), (257, 2048), (640, 96)] {
        let expected = crate::waveform::read(&mut output.stdout.as_slice(), width, height);
        let actual = decode(source, width, height, &Cancellation::default());
        match (expected, actual) {
            (Ok(expected), Ok(actual)) => assert!(
                expected == actual,
                "{label} {width}x{height}: pixels differ"
            ),
            (Err(_), Err(_)) => assert!(
                output.stdout.len() / 2 < width as usize,
                "only too-short sources may fail"
            ),
            (expected, actual) => panic!(
                "{label} {width}x{height}: success differs, reference={}, native={}",
                expected.is_ok(),
                actual.is_ok()
            ),
        }
    }
    let after = std::fs::metadata(source).expect("unchanged source");
    assert_eq!(stamp.len(), after.len());
    assert_eq!(
        stamp.modified().expect("mtime"),
        after.modified().expect("mtime")
    );
}

#[test]
fn native_overview_rejects_cancelled_missing_and_invalid_requests() {
    let missing = Path::new("must-not-open-waveform.wav");
    let cancellation = Cancellation::default();
    cancellation.cancel();
    assert!(matches!(
        decode(missing, 127, 96, &cancellation),
        Err(PreviewError::Cancelled)
    ));
    assert!(decode(missing, 0, 96, &Cancellation::default()).is_err());
    assert!(decode(missing, 127, 96, &Cancellation::default()).is_err());
}

#[test]
fn audio_only_waveforms_skip_unselected_video_and_audio_payloads() {
    let root = std::env::temp_dir().join(format!(
        "towavue-waveform-demux-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos(),
    ));
    std::fs::create_dir(&root).expect("owned fixtures");
    let source = root.join("mixed.mov");
    let output = ffmpeg()
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=30:duration=2",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000:duration=2",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-c:v",
            "rawvideo",
            "-pix_fmt",
            "uyvy422",
            "-c:a",
            "pcm_s16le",
            "-disposition:a:0",
            "0",
            "-disposition:a:1",
            "default",
        ])
        .arg(&source)
        .output()
        .expect("mixed-stream fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    ffmpeg_next::init().expect("FFmpeg");
    let scan = |discard: bool| {
        let mut input = format::input(&source).expect("input");
        let selected = input
            .streams()
            .best(media::Type::Audio)
            .expect("audio")
            .index();
        assert_eq!(selected, 2, "the default second audio track must win");
        if discard {
            discard_other_streams(&mut input, selected);
        }
        let mut audio = Vec::new();
        let mut other_bytes = 0;
        loop {
            let mut packet = ffmpeg_next::Packet::empty();
            match packet.read(&mut input) {
                Ok(()) if packet.stream() == selected => {
                    audio.push((
                        packet.pts(),
                        packet.dts(),
                        packet.data().expect("payload").to_vec(),
                    ));
                }
                Ok(()) => other_bytes += packet.size(),
                Err(Error::Eof) => break,
                Err(error) => panic!("read: {error}"),
            }
        }
        // This local file input owns a live AVIOContext; no native pointer escapes
        // the immutable borrow, and decoding/closing cannot run concurrently.
        let bytes_read = unsafe {
            let io = (*input.as_ptr()).pb;
            assert!(!io.is_null());
            (*io).bytes_read
        };
        (audio, other_bytes, bytes_read)
    };
    let baseline = scan(false);
    let selected = scan(true);
    assert!(
        baseline.0 == selected.0,
        "selected audio packets/timestamps changed"
    );
    assert!(
        baseline.1 > 6_000_000,
        "fixture must carry substantial video"
    );
    eprintln!(
        "WAVEFORM_DEMUX baseline_bytes={} selected_bytes={} baseline_other={} selected_other={}",
        baseline.2, selected.2, baseline.1, selected.1
    );
    // Stream probing happens before selecting the best audio. Its already
    // buffered packets must not be flushed (that would lose initial audio).
    assert!(
        selected.1 * 20 < baseline.1,
        "only probe-buffered unwanted packets should remain"
    );
    assert!(
        selected.2 * 3 < baseline.2,
        "demuxing must skip video payload reads, including initial probing"
    );
    compare(&source, "0:a:1", "mixed video and default audio");
    for extension in ["mov", "mp4", "mkv", "ts"] {
        let mixed = root.join(format!("mixed.{extension}"));
        if extension != "mov" {
            let output = ffmpeg()
                .arg("-i")
                .arg(&source)
                .args(["-map", "0", "-c:v", "mpeg4", "-c:a", "aac"])
                .arg(&mixed)
                .output()
                .expect("compressed mixed-stream fixture");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        compare_timeline_audio(&mixed);
    }
    std::fs::remove_dir_all(root).expect("remove owned fixtures");
}

fn compare_timeline_audio(source: &Path) {
    use crate::decode::{self, DecodeOutput, DecodeStream, ParallelSoftwareDecodeOutput};
    use towavue_core::{MediaTime, TimeRange};
    let stamp = std::fs::metadata(source).expect("source stamp");
    for start_ms in [0, 333, 1500] {
        let start = MediaTime::from_nanoseconds(start_ms * 1_000_000);
        let end = MediaTime::from_nanoseconds(1_900_000_000);
        let mut baseline = Vec::new();
        decode::decode_file_parallel(
            source,
            start,
            Some(end),
            Some(DecodeStream::Audio),
            |output| {
                if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) = output {
                    baseline.push((chunk.presentation_time, chunk.frames, chunk.bytes));
                }
                true
            },
        )
        .expect("unrestricted demux baseline");
        let mut selected = Vec::new();
        decode::decode_audio_intervals_cancellable(
            source,
            start,
            end,
            &[TimeRange::new(start, end).expect("interval")],
            &|| false,
            |chunk| {
                selected.push((chunk.presentation_time, chunk.frames, chunk.bytes));
                true
            },
        )
        .expect("audio-only timeline demux");
        assert!(!baseline.is_empty(), "fixture must emit audio");
        assert!(
            baseline == selected,
            "timeline audio bytes/timestamps changed at {start_ms}ms: {:?}, baseline chunks={}, frames={}, first={:?}; selected chunks={}, frames={}, first={:?}",
            source.extension(),
            baseline.len(),
            baseline.iter().map(|chunk| chunk.1).sum::<usize>(),
            baseline.first().map(|chunk| (chunk.0, chunk.1)),
            selected.len(),
            selected.iter().map(|chunk| chunk.1).sum::<usize>(),
            selected.first().map(|chunk| (chunk.0, chunk.1))
        );
    }
    let after = std::fs::metadata(source).expect("unchanged source");
    assert_eq!(stamp.len(), after.len());
    assert_eq!(
        stamp.modified().expect("mtime"),
        after.modified().expect("mtime")
    );
}
