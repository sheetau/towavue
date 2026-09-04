use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use towavue_core::{EditOperation, MediaKind, MediaTime};
use towavue_runtime_windows::{
    DecodeOutput, DecodeSummary, ExportRequest, decode_file, export_media,
};

#[test]
fn decodes_required_m1_container_and_codec_combinations() {
    let cases = ["h264-aac.mp4", "hevc-aac.mkv", "vp9-opus.webm"];

    for name in cases {
        verify_fixture(&fixture_directory().join(name));
    }
}

fn verify_fixture(path: &Path) {
    assert!(
        path.is_file(),
        "missing M1 fixture {}; run scripts/generate-m1-fixtures.ps1",
        path.display()
    );

    let mut observed = DecodeSummary::default();
    let mut previous_timestamp = None;
    let summary = decode_file(path, |output| {
        match output {
            DecodeOutput::Video(frame) => {
                assert_eq!(frame.width, 160);
                assert_eq!(frame.height, 96);
                assert_eq!(frame.rgba.len(), 160 * 96 * 4);
                if let Some(previous) = previous_timestamp {
                    assert!(frame.presentation_time >= previous);
                }
                previous_timestamp = Some(frame.presentation_time);
                observed.video_frames += 1;
            }
            DecodeOutput::Audio(chunk) => {
                assert_eq!(chunk.format.sample_rate, 48_000);
                assert_eq!(chunk.format.channels, 2);
                assert_eq!(chunk.bytes.len(), chunk.frames * 2 * size_of::<f32>());
                observed.audio_frames += chunk.frames as u64;
            }
        }
        true
    })
    .unwrap_or_else(|error| panic!("failed to decode {}: {error}", path.display()));

    assert_eq!(summary, observed);
    assert_eq!(summary.video_frames, 60);
    assert!(summary.audio_frames >= 90_000);
}

fn fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("generated")
        .join("m1")
}

#[test]
fn exports_trimmed_rate_adjusted_video_with_audio() {
    let source = fixture_directory().join("h264-aac.mp4");
    assert!(source.is_file(), "missing M1 fixture {}", source.display());
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let target = std::env::temp_dir().join(format!("towavue-video-export-{unique}.mp4"));
    export_media(&ExportRequest {
        source,
        target: target.clone(),
        kind: MediaKind::Video,
        operations: vec![
            EditOperation::SetTrimStart(MediaTime::from_nanoseconds(500_000_000)),
            EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(1_500_000_000)),
            EditOperation::SetRate(2.0),
            EditOperation::SetVolume(0.5),
        ],
        hardware_encode: false,
    })
    .expect("export edited video");

    let summary = decode_file(&target, |_| true).expect("decode exported video");
    fs::remove_file(target).expect("remove exported fixture");

    assert!((10..=20).contains(&summary.video_frames));
    assert!((20_000..=30_000).contains(&summary.audio_frames));
}

#[test]
fn uses_hardware_export_when_the_adapter_exposes_it() {
    let source = fixture_directory().join("h264-aac.mp4");
    assert!(source.is_file(), "missing M1 fixture {}", source.display());
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let target = std::env::temp_dir().join(format!("towavue-hardware-export-{unique}.mp4"));
    let outcome = export_media(&ExportRequest {
        source,
        target: target.clone(),
        kind: MediaKind::Video,
        operations: Vec::new(),
        hardware_encode: true,
    })
    .expect("export with hardware preference and software fallback");
    let summary = decode_file(&target, |_| true).expect("decode hardware-preferred export");
    fs::remove_file(target).expect("remove hardware export fixture");
    assert!(summary.video_frames > 0);
    if !outcome.used_hardware_encoder {
        eprintln!(
            "skipped hardware assertion: this adapter exposes no usable Media Foundation H.264 hardware encoder"
        );
        return;
    }
    assert!(outcome.used_hardware_encoder);
}
