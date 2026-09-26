use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
use towavue_core::{MediaTime, TimeRange, TimelineEdit};

fn time(ms: i64) -> MediaTime {
    MediaTime::from_nanoseconds(ms * 1_000_000)
}
fn range(a: i64, b: i64) -> TimeRange {
    TimeRange::new(time(a), time(b)).expect("range")
}

#[test]
fn sample_precision_timestamps_preserve_a_single_missing_sample() {
    let root = super::super::audio_tests::root("single-sample-gap");
    let source = root.join("source.nut");
    super::super::audio_tests::ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=size=64x48:rate=20:duration=0.1",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.25|0.25:s=48000:d=0.02",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.5|0.5:s=48000:d=0.02",
            "-filter_complex",
            "[1:a]asetnsamples=n=1:p=0,aselect='not(eq(n,100))'[gap]",
            "-map",
            "0:v",
            "-map",
            "[gap]",
            "-map",
            "2:a",
            "-c:v",
            "ffv1",
            "-c:a",
            "pcm_f32le",
        ],
        &source,
    );
    let tracks = probe(&source).expect("sample precision source");
    assert_eq!(tracks[0].tolerance, 0.0);
    let export = request(&source, root.join("output.avi"), vec![]);
    export_media(&export).expect("single-sample gap export");
    let tracks = probe(&export.target).expect("both output tracks");
    assert_eq!(tracks.len(), 2);
    for (ordinal, amplitude) in [(0, 0.25), (1, 0.5)] {
        let values = samples(
            &export.target,
            AudioTrackId::from_index(tracks[ordinal].stream.0),
        );
        assert_eq!(values.len(), 960);
        for (index, actual) in values.into_iter().enumerate() {
            let expected = if ordinal == 0 && index == 100 {
                0.0
            } else {
                amplitude
            };
            assert!(
                (actual - expected).abs() < 0.0001,
                "track {ordinal}, sample {index}: {actual} vs {expected}"
            );
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("towavue-multi-export-{name}-{unique}"));
    fs::create_dir(&root).expect("owned fixture");
    let source = root.join("source.mkv");
    let ffmpeg = crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg");
    let output = crate::hidden_test_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "color=size=64x48:rate=20:duration=3",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.125*sin(2*PI*n/32)|0.125*sin(2*PI*n/32):s=44100:d=2.5",
            "-itsoffset",
            "0.5",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.5*sin(2*PI*n/64)|0.5*sin(2*PI*n/64):s=48000:d=1.5",
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
            "-metadata:s:a:0",
            "title=Main",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:1",
            "title=Commentary",
            "-metadata:s:a:1",
            "language=jpn",
            "-disposition:a:0",
            "0",
            "-disposition:a:1",
            "default",
        ])
        .arg(&source)
        .output()
        .expect("source generation");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (root, source)
}

fn request(source: &Path, target: PathBuf, operations: Vec<EditOperation>) -> ExportRequest {
    ExportRequest {
        source: source.to_owned(),
        target,
        kind: MediaKind::Video,
        operations,
        hardware_encode: false,
    }
}

fn samples(path: &Path, id: AudioTrackId) -> Vec<f32> {
    let mut values = Vec::new();
    crate::decode::decode_audio_track_cancellable(
        path,
        Some(id),
        MediaTime::ZERO,
        None,
        &|| false,
        |output| {
            let crate::decode::ParallelSoftwareDecodeOutput::Item(
                crate::decode::DecodeOutput::Audio(chunk),
            ) = output
            else {
                return true;
            };
            assert_eq!(chunk.format.channels, 2);
            let start = usize::try_from(
                (i128::from(chunk.presentation_time.as_nanoseconds())
                    * i128::from(chunk.format.sample_rate)
                    + 500_000_000)
                    / 1_000_000_000,
            )
            .expect("nonnegative sample axis");
            if start > values.len() {
                values.resize(start, 0.0);
            }
            values.extend(
                chunk
                    .bytes
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|frame| f32::from_le_bytes(frame[..4].try_into().expect("sample"))),
            );
            true
        },
    )
    .expect("decode output track");
    values
}

#[test]
fn compressed_tracks_keep_the_decoded_source_axis_and_samples() {
    let (root, source) = fixture("compressed");
    let compressed = root.join("compressed.mkv");
    super::super::audio_tests::ffmpeg(
        &[
            "-i",
            source.to_str().expect("path"),
            "-map",
            "0",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
        ],
        &compressed,
    );
    let source_tracks = probe(&compressed).expect("AAC source tracks");
    assert_eq!(source_tracks.len(), 2);
    let export = request(&compressed, root.join("decoded.avi"), vec![]);
    export_media(&export).expect("independent compressed tracks");
    let output_tracks = probe(&export.target).expect("output tracks");
    assert_eq!(output_tracks.len(), source_tracks.len());
    for (original, output) in source_tracks.iter().zip(output_tracks) {
        let expected = samples(&compressed, AudioTrackId::from_index(original.stream.0));
        let actual = samples(&export.target, AudioTrackId::from_index(output.stream.0));
        assert_eq!(actual.len(), expected.len(), "decoded source-axis length");
        let error = actual
            .iter()
            .zip(expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0_f32, f32::max);
        assert!(error <= 1.0 / 32768.0, "PCM16 rounding only: {error}");
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn all_and_selected_exports_preserve_independent_sample_axes_edits_and_silent_exclusion() {
    let (root, source) = fixture("axes");
    let original = fs::read(&source).expect("source");
    for (name, edits) in [
        ("plain", vec![]),
        (
            "trim",
            vec![
                EditOperation::SetTrimStart(time(250)),
                EditOperation::SetTrimEnd(time(2250)),
            ],
        ),
        (
            "timeline",
            vec![
                EditOperation::Timeline(TimelineEdit::SetVolume(range(1000, 1500), 0.5)),
                EditOperation::Timeline(TimelineEdit::Delete(range(250, 750))),
            ],
        ),
    ] {
        let export = request(&source, root.join(format!("{name}.avi")), edits);
        export_media_with_options(&export, ExportOptions::default())
            .expect("all tracks by default");
        let catalog = crate::probe_audio_tracks(&export.target).expect("output catalog");
        assert_eq!(catalog.tracks.len(), 2);
        for (ordinal, rate, amplitude, period, start, end) in [
            (0, 44100usize, 0.125, 32.0, 0, 2500),
            (1, 48000, 0.5, 64.0, 500, 2000),
        ] {
            let sample = |ms: usize| ms * rate / 1000;
            let mut expected = vec![0_f32; sample(if name == "timeline" { 3000 } else { end })];
            for (index, value) in expected[sample(start)..sample(end)].iter_mut().enumerate() {
                *value =
                    (amplitude * (2.0 * std::f64::consts::PI * index as f64 / period).sin()) as f32;
            }
            if name == "timeline" {
                for value in &mut expected[sample(1000)..sample(1500)] {
                    *value *= 0.5;
                }
                expected.drain(sample(250)..sample(750));
            } else if name == "trim" {
                expected.truncate(sample(2250));
                expected.drain(..sample(250));
            }
            let got = samples(&export.target, catalog.tracks[ordinal].id);
            assert_eq!(got.len(), expected.len(), "{name} track {ordinal} length");
            let maximum = got
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0_f32, f32::max);
            assert!(
                maximum < 0.0001,
                "{name} track {ordinal} PCM differs by {maximum}"
            );
        }
    }
    for (name, selected, expected_count) in [
        ("second", vec![AudioTrackId::from_index(2)], 1),
        ("silent", vec![], 0),
    ] {
        let export = request(&source, root.join(format!("{name}.mkv")), vec![]);
        export_media_with_options(
            &export,
            ExportOptions {
                audio_tracks: AudioTrackRetention::Selected(selected),
                ..Default::default()
            },
        )
        .expect("selected tracks");
        let catalog = crate::probe_audio_tracks(&export.target).expect("output catalog");
        assert_eq!(catalog.tracks.len(), expected_count);
        if expected_count == 1 {
            assert_eq!(catalog.tracks[0].title.as_deref(), Some("Commentary"));
            assert_eq!(catalog.tracks[0].language.as_deref(), Some("jpn"));
        }
    }
    let target = root.join("protected.mkv");
    fs::write(&target, b"existing destination").expect("sentinel");
    let export = request(&source, target.clone(), vec![]);
    let options = ExportOptions {
        audio_tracks: AudioTrackRetention::Selected(vec![AudioTrackId::from_index(99)]),
        ..Default::default()
    };
    assert!(export_media_with_options(&export, options).is_err());
    assert_eq!(
        fs::read(target).expect("destination"),
        b"existing destination"
    );
    assert_eq!(fs::read(source).expect("unchanged source"), original);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn short_source_gaps_survive_multi_track_export_without_shifting_following_samples() {
    let (root, source) = fixture("gaps");
    let gapped = root.join("gapped.mkv");
    let ffmpeg = crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg");
    let output = crate::hidden_test_command(ffmpeg)
        .args(["-v", "error", "-n", "-i"])
        .arg(&source)
        .args([
            "-filter_complex",
            "[0:a:0]aselect='not(between(t,1,1.04))'[a]",
            "-map",
            "0:v",
            "-map",
            "[a]",
            "-map",
            "0:a:1",
            "-c:v",
            "copy",
            "-c:a",
            "pcm_f32le",
        ])
        .arg(&gapped)
        .output()
        .expect("gap fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = samples(&gapped, AudioTrackId::from_index(1));
    assert!(
        expected[45_056..45_824].iter().all(|sample| *sample == 0.0),
        "fixture must contain a real gap"
    );
    let export = request(&gapped, root.join("gapped.avi"), vec![]);
    export_media_with_options(&export, ExportOptions::default()).expect("gap export");
    let got = samples(&export.target, AudioTrackId::from_index(1));
    assert_eq!(got.len(), expected.len());
    let maximum = got
        .iter()
        .zip(expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0_f32, f32::max);
    assert!(maximum < 0.0001, "gap sample axis differs: {maximum}");
    fs::remove_dir_all(root).expect("owned cleanup");
}

#[test]
fn each_retained_track_keeps_its_rate_and_receives_independent_normalization() {
    let (root, source) = fixture("processing");
    for rate in [0.5, 2.0] {
        let export = request(
            &source,
            root.join(format!("rate-{rate}.avi")),
            vec![EditOperation::SetRate(rate)],
        );
        export_media_with_options(&export, ExportOptions::default()).expect("retimed tracks");
        let tracks = probe(&export.target).expect("output formats");
        assert_eq!(
            tracks
                .iter()
                .map(|track| track.stream.1.denominator())
                .collect::<Vec<_>>(),
            [44100, 48000]
        );
        for (ordinal, before) in [(0, 110250usize), (1, 96000)] {
            let values = samples(
                &export.target,
                AudioTrackId::from_index(tracks[ordinal].stream.0),
            );
            assert_eq!(
                values.len(),
                (before as f64 / f64::from(rate)).ceil() as usize
            );
            assert!(values.iter().all(|value| value.is_finite()));
            assert!(values.iter().any(|value| value.abs() > 0.05));
        }
    }
    for normalization in [
        AudioNormalization::Peak,
        AudioNormalization::Loudness(LoudnessTarget::default()),
    ] {
        let export = request(
            &source,
            root.join(format!(
                "{}.avi",
                if normalization == AudioNormalization::Peak {
                    "peak"
                } else {
                    "loudness"
                }
            )),
            vec![],
        );
        export_media_with_options(
            &export,
            ExportOptions {
                audio: AudioExportOptions {
                    normalization,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .expect("each track normalization and candidate verification");
        let tracks = probe(&export.target).expect("retained tracks");
        assert_eq!(tracks.len(), 2);
        for track in tracks {
            let values = samples(&export.target, AudioTrackId::from_index(track.stream.0));
            let peak = values.iter().map(|value| value.abs()).fold(0_f32, f32::max);
            if normalization == AudioNormalization::Peak {
                assert!(
                    (peak - 10_f32.powf(-1.0 / 20.0)).abs() < 0.0001,
                    "independent peak: {peak}"
                );
            } else {
                assert!(peak > 0.05 && peak < 0.8913);
            }
        }
    }
    let protected = root.join("cancelled.mkv");
    fs::write(&protected, b"existing").expect("sentinel");
    let export = request(&source, protected.clone(), vec![]);
    assert!(matches!(
        export_options_cancellable(
            &export,
            ExportOptions::default(),
            &AtomicBool::new(true),
            &|_| {},
            &|_| {}
        ),
        Err(ExportError::Cancelled)
    ));
    assert_eq!(
        fs::read(protected).expect("unchanged destination"),
        b"existing"
    );
    fs::remove_dir_all(root).expect("owned cleanup");
}
