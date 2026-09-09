use super::*;
use crate::DecodeOutput;
use std::os::windows::process::CommandExt;
use std::time::{SystemTime, UNIX_EPOCH};
use towavue_core::{MediaTime, TimeRange, TimelineEdit};

fn range(start_ms: i64, end_ms: i64) -> TimeRange {
    TimeRange::new(
        MediaTime::from_nanoseconds(start_ms * 1_000_000),
        MediaTime::from_nanoseconds(end_ms * 1_000_000),
    )
    .expect("range")
}

fn decoded(path: &Path) -> (Vec<Vec<u8>>, Vec<u8>) {
    let (mut frames, mut audio) = (Vec::new(), Vec::new());
    crate::decode::decode_file(path, |output| {
        match output {
            DecodeOutput::Video(frame) => frames.push(frame.rgba[..3].to_vec()),
            DecodeOutput::Audio(chunk) => audio.extend(chunk.bytes),
        }
        true
    })
    .expect("decode result");
    (frames, audio)
}

#[test]
fn timeline_exports_join_selected_source_frames_and_samples_without_touching_inputs() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("towavue-timeline-export-{unique}"));
    fs::create_dir(&directory).expect("owned fixture directory");
    let source = directory.join("source.mkv");
    let executable =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe");
    let generated = Command::new(&executable).creation_flags(0x0800_0000).args([
        "-v", "error", "-f", "lavfi", "-i",
        "nullsrc=size=160x96:rate=20:duration=2,geq=r='mod(N*37,256)':g='mod(N*67,256)':b='mod(N*97,256)'",
        "-f", "lavfi", "-i", "sine=sample_rate=48000:duration=2", "-c:v", "ffv1", "-c:a", "pcm_s16le",
    ]).arg(&source).output().expect("generate source");
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let source_bytes = fs::read(&source).expect("source bytes");
    let (source_frames, source_audio) = decoded(&source);
    assert_eq!(source_frames.len(), 40);
    assert_eq!(source_audio.len(), 96000 * 8);
    let shifted = directory.join("shifted.mkv");
    let video_only = directory.join("video-only.mkv");
    for (path, video_only) in [(&shifted, false), (&video_only, true)] {
        let mut command = Command::new(&executable);
        command
            .creation_flags(0x0800_0000)
            .args(["-v", "error", "-i"])
            .arg(&source)
            .args(["-c", "copy"]);
        if video_only {
            command.arg("-an");
        } else {
            command.args(["-output_ts_offset", "5"]);
        }
        let output = command.arg(path).output().expect("owned variant");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for input in [&source, &shifted, &video_only] {
        for kind in [MediaKind::Audio, MediaKind::Video] {
            if input == &video_only && kind == MediaKind::Audio {
                continue;
            }
            for mute in [false, true] {
                let target = directory.join(if kind == MediaKind::Audio {
                    "result.wav"
                } else {
                    "result.avi"
                });
                let mut operations = vec![
                    EditOperation::Timeline(TimelineEdit::Delete(range(500, 1000))),
                    EditOperation::Timeline(TimelineEdit::Keep(range(250, 1250))),
                ];
                if mute {
                    operations.push(EditOperation::Timeline(TimelineEdit::SetVolume(
                        range(250, 750),
                        0.0,
                    )));
                }
                export_media(&ExportRequest {
                    source: input.clone(),
                    target: target.clone(),
                    kind,
                    operations,
                    hardware_encode: false,
                })
                .expect("timeline export");
                let (frames, audio) = decoded(&target);
                let mut expected_audio = [
                    source_audio[12000 * 8..24000 * 8].to_vec(),
                    source_audio[48000 * 8..84000 * 8].to_vec(),
                ]
                .concat();
                if mute {
                    expected_audio[12000 * 8..36000 * 8].fill(0);
                }
                if input == &video_only {
                    expected_audio.clear();
                }
                assert_eq!(
                    audio.len(),
                    expected_audio.len(),
                    "sample count {kind:?}, mute={mute}"
                );
                assert!(
                    audio == expected_audio,
                    "source sample identity {kind:?}, mute={mute}"
                );
                if kind == MediaKind::Video {
                    let expected = [&source_frames[5..10], &source_frames[20..35]].concat();
                    assert_eq!(frames.len(), expected.len());
                    for (actual, expected) in frames.iter().zip(expected) {
                        assert!(
                            actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 4),
                            "wrong source frame {actual:?}"
                        );
                    }
                } else {
                    assert!(frames.is_empty());
                }
            }
        }
    }
    let target = directory.join("stretched.wav");
    export_media(&ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: MediaKind::Audio,
        operations: vec![EditOperation::Timeline(TimelineEdit::Stretch(
            range(500, 1500),
            MediaTime::from_nanoseconds(500_000_000),
        ))],
        hardware_encode: false,
    })
    .expect("stretch export");
    let (_, stretched) = decoded(&target);
    assert_eq!(
        stretched.len(),
        72000 * 8,
        "stretch keeps the planned sample axis"
    );
    assert_eq!(&stretched[..24000 * 8], &source_audio[..24000 * 8]);
    assert_eq!(&stretched[48000 * 8..], &source_audio[72000 * 8..]);
    let previous = fs::read(&target).expect("previous export");
    let gain_target = directory.join("gain.wav");
    export_media(&ExportRequest {
        source: source.clone(),
        target: gain_target.clone(),
        kind: MediaKind::Audio,
        operations: vec![
            EditOperation::SetVolume(0.5),
            EditOperation::Timeline(TimelineEdit::SetVolume(range(500, 1500), 0.5)),
        ],
        hardware_encode: false,
    })
    .expect("local and master gain");
    let (_, gained) = decoded(&gain_target);
    assert_eq!(gained.len(), source_audio.len());
    for (index, (actual, original)) in gained
        .as_chunks::<4>()
        .0
        .iter()
        .zip(source_audio.as_chunks::<4>().0)
        .enumerate()
    {
        let actual = f32::from_le_bytes(*actual);
        let original = f32::from_le_bytes(*original);
        let gain = if (24000 * 2..72000 * 2).contains(&index) {
            0.25
        } else {
            0.5
        };
        assert!(
            (actual - original * gain).abs() <= 1.0 / 32768.0,
            "PCM gain at sample {index}"
        );
    }
    export_media(&ExportRequest {
        source: source.clone(),
        target: gain_target.clone(),
        kind: MediaKind::Audio,
        operations: vec![
            EditOperation::SetRate(2.0),
            EditOperation::Timeline(TimelineEdit::Keep(range(500, 1500))),
        ],
        hardware_encode: false,
    })
    .expect("master rate after timeline selection");
    assert_eq!(decoded(&gain_target).1.len(), 24000 * 8);
    let result = export_media(&ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: MediaKind::Audio,
        operations: vec![EditOperation::Timeline(TimelineEdit::Delete(range(
            0, 2000,
        )))],
        hardware_encode: false,
    });
    assert!(matches!(result, Err(ExportError::InvalidTimeline)));
    assert_eq!(fs::read(&target).expect("target remains"), previous);
    let cancelled = AtomicBool::new(false);
    let result = export_cancellable(
        &ExportRequest {
            source: source.clone(),
            target: target.clone(),
            kind: MediaKind::Audio,
            operations: vec![EditOperation::Timeline(TimelineEdit::Delete(range(
                500, 1000,
            )))],
            hardware_encode: false,
        },
        &cancelled,
        &|_| cancelled.store(true, Ordering::Relaxed),
    );
    assert!(
        matches!(result, Err(ExportError::Cancelled)),
        "progress-triggered cancellation: {result:?}"
    );
    assert_eq!(fs::read(&target).expect("cancel retains target"), previous);
    assert_eq!(fs::read(&source).expect("source remains"), source_bytes);
    assert!(
        !fs::read_dir(&directory)
            .expect("fixture directory")
            .any(|entry| entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".towavue-export-"))
    );
    fs::remove_dir_all(directory).expect("remove owned fixture directory");
}

#[test]
fn long_timeline_graphs_are_staged_outside_the_windows_command_line() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("towavue-timeline-graph-{unique}"));
    fs::create_dir(&directory).expect("owned fixture directory");
    let mut operations = Vec::new();
    for index in 0..400 {
        operations.push(EditOperation::Timeline(TimelineEdit::SetVolume(
            range(index * 2, index * 2 + 2),
            if index % 2 == 0 { 0.5 } else { 0.25 },
        )));
    }
    let request = ExportRequest {
        source: directory.join("source.wav"),
        target: directory.join("result.wav"),
        kind: MediaKind::Audio,
        operations,
        hardware_encode: false,
    };
    let streams = ExportStreams {
        audio: Some((0, ffmpeg::Rational(1, 48000))),
        timeline: towavue_core::EditTimeline::from_operations(
            MediaTime::from_nanoseconds(1_000_000_000),
            &request.operations,
        ),
        ..Default::default()
    };
    let staging = StagedExport::new(&request.target).expect("staging");
    let arguments = staging
        .arguments(&request, false, &streams)
        .expect("staged graph");
    let graph = fs::read_to_string(staging.directory.join("timeline-filter.txt")).expect("graph");
    assert!(graph.len() > 32768);
    assert!(arguments.join(" ").len() < 2048);
    assert!(
        arguments
            .iter()
            .any(|argument| argument == "-/filter_complex")
    );
    let staged_directory = staging.directory.clone();
    drop(staging);
    assert!(!staged_directory.exists());
    fs::remove_dir_all(directory).expect("remove owned fixture directory");
}
