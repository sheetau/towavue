use super::tests::{run, video};
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
use towavue_core::{ImageResize, MediaTime, PixelCrop, ResampleFilter, VideoResize, VideoRotation};

fn directory(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "towavue-resize-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir(&path).expect("owned fixture directory");
    path
}

fn same_frames(actual: &Path, reference: &Path, size: (u32, u32), count: usize) {
    let actual = video(actual);
    let expected = video(reference);
    assert_eq!(actual.len(), count);
    assert_eq!(expected.len(), count);
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!((actual.width, actual.height), size);
        assert_eq!(actual.pixel_aspect, 1.0);
        assert_eq!(actual.orientation, crate::VideoOrientation::default());
        assert_eq!(actual.presentation_time, expected.presentation_time);
        assert!(
            actual.rgba == expected.rgba,
            "export/reference pixels differ"
        );
    }
}

#[test]
fn video_resize_exports_all_filters_exact_dimensions_sar_and_identity() {
    let root = directory("filters");
    let executable = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
    for (index, (source_size, aspect, output)) in [
        ((64, 48), 1.0, (96, 72)),
        ((64, 48), 1.0, (30, 18)),
        ((65, 49), 1.5, (98, 50)),
        ((65, 49), 0.75, (64, 66)),
        ((64, 48), 2.0, (64, 48)),
        ((64, 48), 1.0, (16, 16)),
        ((64, 48), 1.0, (64, 48)),
    ]
    .into_iter()
    .enumerate()
    {
        let source = root.join(format!("source-{index}.mkv"));
        run(
            &executable,
            &[
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &format!(
                    "testsrc=size={}x{}:rate=4:duration=1,format=gbrp,setsar={aspect}",
                    source_size.0, source_size.1
                ),
                "-c:v",
                "ffv1",
            ],
            None,
            &[],
            &source,
        );
        let bytes = fs::read(&source).expect("source bytes");
        let decoded = video(&source);
        assert_eq!((decoded[0].width, decoded[0].height), source_size);
        assert_eq!(decoded[0].pixel_aspect, aspect);
        let mut filtered = Vec::new();
        for (filter, flag) in [
            (ResampleFilter::Nearest, "neighbor"),
            (ResampleFilter::Bilinear, "bilinear"),
            (ResampleFilter::Bicubic, "bicubic"),
            (ResampleFilter::Lanczos, "lanczos"),
        ] {
            let value = VideoResize::new(output, filter, source_size, aspect).expect("resize");
            let request = ExportRequest {
                source: source.clone(),
                target: root.join(format!("actual-{index}-{flag}.mp4")),
                kind: MediaKind::Video,
                hardware_encode: false,
                operations: vec![EditOperation::ResizeVideo(value)],
            };
            export_media(&request).expect("resize export");
            let reference = root.join(format!("reference-{index}-{flag}.mp4"));
            let filter = if value.is_identity() {
                assert!(visual_filters(&request.operations).is_empty());
                "null".to_owned()
            } else {
                format!(
                    "format=gbrp,scale=w={}:h={}:flags={flag},format=gbrp,setsar=1,copy",
                    output.0, output.1
                )
            };
            run(
                &executable,
                &["-v", "error", "-i"],
                Some(&source),
                &["-vf", &filter, "-c:v", "libopenh264"],
                &reference,
            );
            same_frames(&request.target, &reference, output, 4);
            filtered.push(video(&request.target)[0].rgba.clone());
            assert_eq!(fs::read(&source).expect("source remains"), bytes);
        }
        if index == 1 {
            for pair in filtered.windows(2) {
                assert_ne!(
                    pair[0], pair[1],
                    "chosen filters must produce distinct samples"
                );
            }
        }
    }
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}

#[test]
fn video_resize_composes_with_rotation_crop_and_refuses_stale_or_wrong_media_before_replacement() {
    let root = directory("ordered");
    let executable = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
    let source = root.join("source.mkv");
    run(
        &executable,
        &[
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=4:duration=1,setsar=2",
            "-c:v",
            "ffv1",
        ],
        None,
        &[],
        &source,
    );
    let original = fs::read(&source).expect("source");
    let first = VideoResize::new((96, 48), ResampleFilter::Lanczos, (64, 48), 2.0).expect("first");
    let rotation = VideoRotation::new(317, (32, 64), 1.0).expect("rotate");
    let second =
        VideoResize::new((80, 60), ResampleFilter::Bicubic, rotation.size(), 1.0).expect("second");
    let mut request = ExportRequest {
        source: source.clone(),
        target: root.join("actual.mp4"),
        kind: MediaKind::Video,
        hardware_encode: false,
        operations: vec![
            EditOperation::ResizeVideo(first),
            EditOperation::Crop(PixelCrop {
                x: 4,
                y: 6,
                width: 64,
                height: 32,
            }),
            EditOperation::RotateClockwise,
            EditOperation::RotateVideo(rotation),
            EditOperation::FlipHorizontal,
            EditOperation::ResizeVideo(second),
            EditOperation::RotateCounterclockwise,
        ],
    };
    export_media(&request).expect("composed resize");
    let reference = root.join("reference.mp4");
    let filter = "format=gbrp,scale=96:48:flags=lanczos,format=gbrp,setsar=1,crop=64:32:4:6:exact=1,transpose=clock,rotate=317*PI/1800:ow='ceil(rotw(317*PI/1800))':oh='ceil(roth(317*PI/1800))':c=black:bilinear=1,pad=ceil(iw/2)*2:ceil(ih/2)*2:0:0:color=black,setsar=1,hflip,scale=80:60:flags=bicubic,format=gbrp,setsar=1,transpose=cclock,copy";
    run(
        &executable,
        &["-v", "error", "-i"],
        Some(&source),
        &["-vf", filter, "-c:v", "libopenh264"],
        &reference,
    );
    same_frames(&request.target, &reference, (60, 80), 4);
    let saved = fs::read(&request.target).expect("saved target");
    let operations = request.operations.clone();
    for invalid in [
        VideoResize::new((96, 48), ResampleFilter::Lanczos, (32, 48), 2.0).expect("wrong size"),
        VideoResize::new((96, 48), ResampleFilter::Lanczos, (64, 48), 1.0).expect("wrong SAR"),
    ] {
        request.operations[0] = EditOperation::ResizeVideo(invalid);
        assert!(export_media(&request).is_err());
        assert_eq!(fs::read(&request.target).expect("target preserved"), saved);
    }
    request.operations = operations;
    request.operations[5] = EditOperation::ResizeVideo(
        VideoResize::new((80, 60), ResampleFilter::Bicubic, (32, 64), 1.0)
            .expect("stale later source"),
    );
    assert!(export_media(&request).is_err());
    assert_eq!(fs::read(&request.target).expect("target preserved"), saved);
    request.operations = vec![EditOperation::ResizeVideo(first)];
    for kind in [MediaKind::Image, MediaKind::Audio] {
        request.kind = kind;
        assert!(export_media(&request).is_err());
        assert_eq!(fs::read(&request.target).expect("target preserved"), saved);
    }
    request.kind = MediaKind::Video;
    request.operations = vec![EditOperation::Resize(
        ImageResize::new(96, 48, ResampleFilter::Lanczos).expect("image size"),
    )];
    assert!(export_media(&request).is_err());
    assert_eq!(fs::read(&request.target).expect("target preserved"), saved);
    assert_eq!(fs::read(&source).expect("source preserved"), original);
    let image = crate::DecodedImage {
        format: "test",
        frames: vec![],
    };
    assert!(
        crate::render_image_edits(
            &image,
            &[EditOperation::ResizeVideo(first)],
            &crate::Cancellation::default()
        )
        .is_err()
    );
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}

#[test]
fn oriented_video_resize_keeps_timing_and_audio_equal_to_the_unresized_export() {
    let root = directory("oriented");
    let executable = crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg");
    let encoded = root.join("encoded.mp4");
    run(
        &executable,
        &[
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x48:rate=4:duration=1,setsar=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=431:sample_rate=48000:duration=1",
            "-c:v",
            "libopenh264",
            "-c:a",
            "aac",
        ],
        None,
        &[],
        &encoded,
    );
    let source = root.join("source.mp4");
    run(
        &executable,
        &["-v", "error", "-display_rotation:v:0", "90", "-i"],
        Some(&encoded),
        &["-c", "copy"],
        &source,
    );
    let original = fs::read(&source).expect("source");
    let mut request = ExportRequest {
        source: source.clone(),
        target: root.join("baseline.mp4"),
        kind: MediaKind::Video,
        hardware_encode: false,
        operations: vec![
            EditOperation::SetTrimStart(MediaTime::from_nanoseconds(250_000_000)),
            EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(750_000_000)),
            EditOperation::SetRate(1.25),
            EditOperation::SetVolume(0.5),
        ],
    };
    export_media(&request).expect("baseline");
    let baseline = request.target.clone();
    request.operations.push(EditOperation::ResizeVideo(
        VideoResize::new((24, 64), ResampleFilter::Bilinear, (48, 64), 0.5)
            .expect("oriented resize"),
    ));
    request.target = root.join("actual.mp4");
    export_media(&request).expect("oriented resize");
    let actual = video(&request.target);
    let expected = video(&baseline);
    assert_eq!(actual.len(), expected.len());
    assert!(!actual.is_empty());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!((actual.width, actual.height), (24, 64));
        assert_eq!(actual.pixel_aspect, 1.0);
        assert_eq!(actual.orientation, crate::VideoOrientation::default());
        assert_eq!(actual.presentation_time, expected.presentation_time);
    }
    let audio = |path: &Path| {
        let mut chunks = Vec::new();
        crate::decode::decode_file(path, |output| {
            if let crate::DecodeOutput::Audio(chunk) = output {
                chunks.push((chunk.presentation_time, chunk.frames, chunk.bytes));
            }
            true
        })
        .expect("audio decode");
        chunks
    };
    let expected = audio(&baseline);
    assert!(!expected.is_empty());
    assert!(
        expected
            .iter()
            .any(|(_, _, bytes)| bytes.iter().any(|byte| *byte != 0))
    );
    assert_eq!(audio(&request.target), expected);
    request.operations.pop();
    let range = |start, end| {
        towavue_core::TimeRange::new(
            MediaTime::from_nanoseconds(start),
            MediaTime::from_nanoseconds(end),
        )
        .expect("time range")
    };
    request.operations.extend([
        EditOperation::Timeline(towavue_core::TimelineEdit::Delete(range(
            100_000_000,
            200_000_000,
        ))),
        EditOperation::Timeline(towavue_core::TimelineEdit::Stretch(
            range(0, 100_000_000),
            MediaTime::from_nanoseconds(150_000_000),
        )),
        EditOperation::Timeline(towavue_core::TimelineEdit::SetVolume(
            range(0, 150_000_000),
            0.25,
        )),
    ]);
    request.target = root.join("timeline-baseline.mp4");
    export_media(&request).expect("timeline baseline");
    let baseline = request.target.clone();
    request.operations.push(EditOperation::ResizeVideo(
        VideoResize::new((24, 64), ResampleFilter::Lanczos, (48, 64), 0.5)
            .expect("timeline resize"),
    ));
    request.target = root.join("timeline-resized.mp4");
    export_media(&request).expect("timeline resize");
    let actual = video(&request.target);
    let expected = video(&baseline);
    assert!(!actual.is_empty());
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!((actual.width, actual.height), (24, 64));
        assert_eq!(actual.pixel_aspect, 1.0);
        assert_eq!(actual.presentation_time, expected.presentation_time);
    }
    assert_eq!(audio(&request.target), audio(&baseline));
    assert_eq!(fs::read(&source).expect("source remains"), original);
    fs::remove_dir_all(&root).expect("owned fixture cleanup");
}
