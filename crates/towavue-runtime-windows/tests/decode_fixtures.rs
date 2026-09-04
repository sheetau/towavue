use std::path::{Path, PathBuf};

use towavue_runtime_windows::{DecodeOutput, DecodeSummary, decode_file};

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
