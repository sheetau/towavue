use super::*;
use crate::export::audio_tests::root;
use std::os::windows::process::CommandExt;

fn run(name: &str, args: &[&str]) -> std::process::Output {
    let output = Command::new(crate::media_tools::tool_path(name).expect("helper"))
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .output()
        .expect("owned process");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn request(path: &Path, kind: MediaKind) -> ExportDialogRequest {
    ExportDialogRequest {
        input: MediaInput::new(path.to_owned()),
        kind,
        operations: vec![],
        options: ExportOptions::default(),
    }
}

fn choices(request: &ExportDialogRequest) -> Choices {
    request
        .choices(&AtomicBool::new(false))
        .expect("export format choices")
}

fn output(request: &ExportDialogRequest, target: &Path) {
    export_media_with_options(
        &ExportRequest {
            source: request.input.path().to_owned(),
            target: target.to_owned(),
            kind: request.kind,
            operations: request.operations.clone(),
            hardware_encode: false,
        },
        request.options.clone(),
    )
    .expect("offered format must export");
}

#[test]
fn export_formats_retain_supported_aliases_replace_unsupported_defaults_and_reject_mismatches() {
    for (source, format, extension) in [
        ("image.JPEG", Format::Jpeg, "JPEG"),
        ("animation.apng", Format::Png, "apng"),
        ("video.m2ts", Format::TransportStream, "m2ts"),
    ] {
        let choices = Choices::new(vec![Format::Mp4, format], Path::new(source)).expect("choices");
        assert_eq!(choices.initial, 1);
        assert_eq!(choices.default_extension, extension);
        assert_eq!(choices.filename(source), source);
        assert!(choices.validate(2, Path::new(source)).is_ok());
        assert!(choices.validate(1, Path::new(source)).is_err());
        assert!(choices.validate(0, Path::new(source)).is_err());
        assert!(choices.validate(3, Path::new(source)).is_err());
        assert!(choices.validate(2, Path::new("missing-extension")).is_err());
    }
    let choices = Choices::new(vec![Format::Mp4], Path::new("source.ogv")).expect("fallback");
    assert_eq!(
        choices.filename("source.final-export.ogv"),
        "source.final-export.mp4"
    );
    assert!(Choices::new(vec![], Path::new("source.png")).is_err());
    assert!(!Format::FramePng.accepts(Path::new("frame.apng")));
}

#[test]
fn export_formats_inspect_actual_alpha_and_keep_metadata_constraints_and_source_defaults() {
    let root = root("export-format-images");
    let path = root.join("source.png");
    for alpha in [255, 0, 128] {
        image::RgbaImage::from_fn(32, 24, |x, y| {
            image::Rgba([
                x as u8 * 7,
                y as u8 * 9,
                100,
                if x < 16 { alpha } else { 255 },
            ])
        })
        .save(&path)
        .expect("alpha fixture");
        let mut export = request(&path, MediaKind::Image);
        let offered = choices(&export);
        assert_eq!(offered.formats[offered.initial], Format::Png);
        assert_eq!(offered.formats.contains(&Format::Jpeg), alpha == 255);
        assert_eq!(offered.formats.contains(&Format::Bmp), alpha == 255);
        assert_eq!(offered.formats.contains(&Format::Gif), alpha == 255);
        for format in offered.formats {
            let target = root.join(format!("alpha-{alpha}.{}", format.extensions()[0]));
            output(&export, &target);
            let decoded = crate::decode_image(&target).expect("offered image decodes");
            assert_eq!(decoded.dimensions(), (32, 24));
            if alpha != 255 {
                assert_eq!(
                    decoded.frames[0].rgba[3],
                    alpha,
                    "{} preserves the offered transparency",
                    format.label()
                );
            }
        }
        export
            .options
            .metadata
            .set(MetadataField::Title, Some("Retained title".into()))
            .expect("metadata");
        assert_eq!(choices(&export).formats, [Format::Png]);
    }
    fs::remove_dir_all(root).expect("owned fixtures");
}

#[test]
fn export_formats_video_depth_audio_only_and_retained_original_use_compatible_containers() {
    let root = root("export-format-video");
    for pixel in ["yuv420p", "yuv420p10le", "yuv420p12le"] {
        let source = root.join(format!("source-{pixel}.mkv"));
        let filter = format!("testsrc2=size=96x64:rate=8:duration=0.5,format={pixel}");
        run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &filter,
                "-f",
                "lavfi",
                "-i",
                "sine=sample_rate=48000:duration=0.5",
                "-c:v",
                "ffv1",
                "-c:a",
                "pcm_s16le",
                source.to_str().expect("path"),
            ],
        );
        let mut export = request(&source, MediaKind::Video);
        let offered = choices(&export);
        assert_eq!(offered.formats[offered.initial], Format::Mkv);
        if pixel != "yuv420p" {
            assert_eq!(offered.formats, [Format::Mp4, Format::Mkv, Format::Webm]);
        }
        for format in offered.formats {
            for extension in format.extensions() {
                let target = root.join(format!("output-{pixel}.{extension}"));
                output(&export, &target);
                let probe = run(
                    "ffprobe.exe",
                    &[
                        "-v",
                        "error",
                        "-select_streams",
                        "v:0",
                        "-show_entries",
                        "stream=pix_fmt,width,height",
                        "-of",
                        "csv=p=0",
                        target.to_str().expect("target"),
                    ],
                );
                let fields = String::from_utf8(probe.stdout).expect("probe");
                assert!(fields.contains(&format!("96,64,{pixel}")), "{fields}");
            }
        }
        if pixel == "yuv420p" {
            for field in MetadataField::ALL {
                let value = match field {
                    MetadataField::Date => "2026",
                    MetadataField::Track => "7",
                    _ => "Requested value",
                };
                export
                    .options
                    .metadata
                    .set(field, Some(value.into()))
                    .expect("metadata");
            }
            let offered = choices(&export);
            assert_eq!(
                offered.formats,
                [Format::Mp4, Format::Mkv, Format::Webm, Format::Wmv]
            );
            for format in offered.formats {
                output(
                    &export,
                    &root.join(format!("metadata.{}", format.extensions()[0])),
                );
            }
            export.options.metadata = MetadataExportOptions::default();
        }
        export.options.output = ExportOutput::AudioOnly;
        let audio = choices(&export);
        assert_eq!(audio.formats.len(), 7);
        assert_eq!(audio.formats[audio.initial], Format::Wav);
        let retained = crate::RetainedSource::capture(
            &crate::FileOperationSource::capture(&source).expect("identity"),
        )
        .expect("retained original");
        export.input = MediaInput::retained(source.clone(), retained);
        fs::remove_file(&source).expect("owned removed source");
        export.options.output = ExportOutput::Media;
        let offered = choices(&export);
        assert_eq!(offered.formats[offered.initial], Format::Mkv);
    }
    fs::remove_dir_all(root).expect("owned fixtures");
}

#[test]
fn export_formats_audio_metadata_excludes_unrepresentable_fields_and_normalized_track_values() {
    let root = root("export-format-audio");
    let source = root.join("source.wav");
    run(
        "ffmpeg.exe",
        &[
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=sample_rate=48000:duration=0.5",
            source.to_str().expect("source"),
        ],
    );
    let mut export = request(&source, MediaKind::Audio);
    for setting in [
        None,
        Some((MetadataField::AlbumArtist, "Album artist")),
        Some((MetadataField::Track, "003/012")),
    ] {
        export.options.metadata = MetadataExportOptions::default();
        if let Some((field, value)) = setting {
            export
                .options
                .metadata
                .set(field, Some(value.into()))
                .expect("metadata");
        }
        let offered = choices(&export);
        if setting.is_some() {
            assert!(!offered.formats.contains(&Format::Aac));
        }
        if setting.is_some_and(|(field, _)| field == MetadataField::AlbumArtist) {
            assert!(!offered.formats.contains(&Format::Wav));
        }
        if setting.is_some_and(|(field, _)| field == MetadataField::Track) {
            assert!(!offered.formats.contains(&Format::M4a));
        }
        for format in offered.formats {
            let target = root.join(format!(
                "out-{}.{}",
                setting.map_or("plain", |(field, _)| field.key()),
                format.extensions()[0]
            ));
            output(&export, &target);
        }
    }
    assert!(matches!(
        export.choices(&AtomicBool::new(true)),
        Err(ExportError::Cancelled)
    ));
    fs::remove_dir_all(root).expect("owned fixtures");
}

#[test]
fn export_formats_gif_preserves_single_frame_timing_and_finite_repetition_limits() {
    let root = root("export-format-gif");
    let source = root.join("source.gif");
    for (delays, repeat, expected) in [
        (
            vec![1, 2],
            gif::Repeat::Finite(u16::MAX),
            vec![Format::Png, Format::Avif, Format::Gif],
        ),
        (
            vec![7],
            gif::Repeat::Finite(2),
            vec![Format::Avif, Format::Gif],
        ),
        (vec![0], gif::Repeat::Infinite, vec![Format::Gif]),
    ] {
        {
            let mut writer = gif::Encoder::new(
                fs::File::create(&source).expect("GIF"),
                32,
                24,
                &[0, 0, 0, 255, 255, 255],
            )
            .expect("encoder");
            writer.set_repeat(repeat).expect("repeat");
            for delay in delays {
                writer
                    .write_frame(&gif::Frame {
                        width: 32,
                        height: 24,
                        delay,
                        buffer: std::borrow::Cow::Owned(vec![1; 32 * 24]),
                        ..Default::default()
                    })
                    .expect("frame");
            }
        }
        let export = request(&source, MediaKind::Image);
        let offered = choices(&export);
        assert_eq!(offered.formats, expected);
        assert_eq!(offered.formats[offered.initial], Format::Gif);
        for format in offered.formats {
            output(
                &export,
                &root.join(format!("output.{}", format.extensions()[0])),
            );
        }
    }
    fs::remove_dir_all(root).expect("owned fixtures");
}
