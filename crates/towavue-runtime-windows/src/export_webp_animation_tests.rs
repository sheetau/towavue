use super::*;
use std::io::Cursor;
use towavue_core::{ImageResize, PixelCrop, ResampleFilter};

fn fixture(loops: u16, delays: &[u32], dispose: bool) -> Vec<u8> {
    let mut output = b"RIFF\0\0\0\0WEBP".to_vec();
    chunk(&mut output, b"VP8X", &[18, 0, 0, 0, 3, 0, 0, 2, 0, 0]).expect("canvas");
    let mut control = vec![31, 63, 95, 127];
    control.extend_from_slice(&loops.to_le_bytes());
    chunk(&mut output, b"ANIM", &control).expect("animation");
    for (index, delay) in delays.iter().enumerate() {
        let (width, height, left, color) = match index {
            0 => (4, 3, 0, [255, 0, 0, 255]),
            1 => (2, 2, 2, [0, 255, 0, 127]),
            _ => (4, 3, 0, [0, 0, 255, 0]),
        };
        let mut encoded = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
            .encode(
                &color.repeat(width as usize * height as usize),
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )
            .expect("frame");
        let mut header = vec![0; 16];
        header[..3].copy_from_slice(&(left / 2u32).to_le_bytes()[..3]);
        header[6..9].copy_from_slice(&(width - 1).to_le_bytes()[..3]);
        header[9..12].copy_from_slice(&(height - 1).to_le_bytes()[..3]);
        header[12..15].copy_from_slice(&delay.to_le_bytes()[..3]);
        header[15] = u8::from(dispose && index == 1);
        header.extend_from_slice(&encoded[12..]);
        chunk(&mut output, b"ANMF", &header).expect("frame container");
    }
    let size = (output.len() - 8) as u32;
    output[4..8].copy_from_slice(&size.to_le_bytes());
    output
}

fn request(source: &Path, target: &Path, operations: Vec<EditOperation>) -> ExportRequest {
    ExportRequest {
        source: source.into(),
        target: target.into(),
        kind: MediaKind::Image,
        operations,
        hardware_encode: false,
    }
}

#[test]
fn animated_webp_export_preserves_pixels_timing_loops_metadata_and_resave() {
    let root = crate::export::audio_tests::root("webp-animation");
    let source = root.join("source.WeBp");
    let target = root.join("target.webp");
    let cancel = AtomicBool::new(false);
    for (loops, delays, dispose) in [
        (0, vec![0, 1, 0xffffff, 53], false),
        (3, vec![7, 31, 53, 53], true),
        (1, vec![17], false),
        (u16::MAX, vec![17, 23], false),
    ] {
        let bytes = fixture(loops, &delays, dispose);
        fs::write(&source, &bytes).expect("source");
        let original = crate::decode_image(&source).expect("display");
        let controls = container(Cursor::new(&bytes), &cancel)
            .expect("source controls")
            .animation;
        for operations in [
            vec![],
            vec![
                EditOperation::Crop(PixelCrop {
                    x: 1,
                    y: 0,
                    width: 3,
                    height: 3,
                }),
                EditOperation::RotateClockwise,
                EditOperation::FlipVertical,
                EditOperation::Resize(
                    ImageResize::new(6, 6, ResampleFilter::Nearest).expect("resize"),
                ),
            ],
            vec![EditOperation::RotateImage(
                towavue_core::ImageRotation::new(137, (4, 3)).expect("rotation"),
            )],
            vec![EditOperation::Resize(
                ImageResize::new(7, 5, ResampleFilter::Bilinear).expect("bilinear"),
            )],
            vec![EditOperation::Resize(
                ImageResize::new(7, 5, ResampleFilter::Bicubic).expect("bicubic"),
            )],
            vec![EditOperation::Resize(
                ImageResize::new(7, 5, ResampleFilter::Lanczos).expect("Lanczos"),
            )],
        ] {
            let request = request(&source, &target, operations);
            let mut metadata = MetadataExportOptions::default();
            metadata
                .set(MetadataField::Title, Some("Animation title".into()))
                .expect("title");
            export_media_with_options(
                &request,
                ExportOptions {
                    metadata,
                    ..Default::default()
                },
            )
            .expect("animation save");
            let actual = crate::decode_image(&target).expect("saved display");
            let expected = crate::render_image_edits(
                &original,
                &request.operations,
                &crate::Cancellation::default(),
            )
            .expect("displayed edits");
            assert_eq!(
                actual.frames, expected.frames,
                "loops={loops}, dispose={dispose}"
            );
            let info = container(
                Cursor::new(fs::read(&target).expect("saved bytes")),
                &cancel,
            )
            .expect("saved controls");
            assert_eq!(info.animation, controls);
            assert!(
                inspect(&target)
                    .expect("saved metadata")
                    .iter()
                    .any(|value| value.field == MetadataField::Title
                        && value.value == "Animation title")
            );
            let resave = root.join("resaved.webp");
            export_media(&self::request(&target, &resave, vec![])).expect("resave");
            assert_eq!(
                crate::decode_image(&resave)
                    .expect("resaved display")
                    .frames,
                actual.frames
            );
            assert_eq!(
                inspect(&resave).expect("metadata Keep"),
                inspect(&target).expect("metadata")
            );
            let mut metadata = MetadataExportOptions::default();
            metadata
                .set(MetadataField::Title, Some(String::new()))
                .expect("Remove");
            export_media_with_options(
                &self::request(&target, &resave, vec![]),
                ExportOptions {
                    metadata,
                    ..Default::default()
                },
            )
            .expect("Remove title");
            assert!(inspect(&resave).expect("removed metadata").is_empty());
            assert_eq!(
                crate::decode_image(&resave)
                    .expect("removed metadata pixels")
                    .frames,
                actual.frames
            );
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn animated_webp_lossy_rgb_and_alpha_sources_save_displayed_pixels_losslessly() {
    let root = crate::export::audio_tests::root("webp-lossy-animation");
    let source = root.join("source.webp");
    let target = root.join("saved.webp");
    for alpha in [false, true] {
        for index in 0..2 {
            image::RgbaImage::from_fn(32, 24, |x, y| {
                image::Rgba([
                    (x * 7 + index * 8) as u8,
                    (y * 9) as u8,
                    (x * y) as u8,
                    if alpha { (x * 8) as u8 } else { 255 },
                ])
            })
            .save(root.join(format!("frame-{index}.png")))
            .expect("PNG frame");
        }
        crate::export::audio_tests::ffmpeg(
            &[
                "-framerate",
                "2",
                "-i",
                &root.join("frame-%d.png").display().to_string(),
                "-frames:v",
                "2",
                "-c:v",
                "libwebp_anim",
                "-lossless",
                "0",
            ],
            &source,
        );
        let original = crate::decode_image(&source).expect("lossy display");
        assert_eq!(original.frames.len(), 2);
        export_media(&request(&source, &target, vec![])).expect("lossy animation save");
        assert_eq!(
            crate::decode_image(&target).expect("saved display").frames,
            original.frames
        );
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn animated_webp_scan_checks_frame_subchunks_geometry_and_resource_boundaries() {
    let cancel = AtomicBool::new(false);
    let bytes = fixture(0, &[17], false);
    let start = bytes
        .windows(4)
        .position(|bytes| bytes == b"ANMF")
        .expect("frame");
    let frame = &bytes[start..];
    let mut control = Animation {
        control: [0; 6],
        delays: Vec::new(),
    };
    assert!(
        control
            .observe(
                &mut Cursor::new(&[]),
                512 * 1024 * 1024 + 1,
                (4, 3),
                &cancel
            )
            .expect_err("encoded frame bound before reading")
            .to_string()
            .contains("512 MiB")
    );
    let finish = |mut bytes: Vec<u8>| {
        let size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        bytes
    };
    for count in [65536, 65537] {
        let mut animation = bytes[..start].to_vec();
        for _ in 0..count {
            animation.extend_from_slice(frame);
        }
        let parsed = container(Cursor::new(finish(animation)), &cancel);
        if count == 65536 {
            assert_eq!(
                parsed
                    .expect("maximum frame count")
                    .animation
                    .expect("animation")
                    .delays
                    .len(),
                count
            );
        } else {
            assert!(parsed.is_err());
        }
    }
    for (width, height, allowed) in [(16384u32, 8192u32, true), (16384, 8193, false)] {
        let mut animation = bytes.clone();
        animation[24..27].copy_from_slice(&(width - 1).to_le_bytes()[..3]);
        animation[27..30].copy_from_slice(&(height - 1).to_le_bytes()[..3]);
        assert_eq!(container(Cursor::new(animation), &cancel).is_ok(), allowed);
    }
    for (offset, value) in [
        (start + 8, 3),     // x*2 exceeds canvas.
        (start + 14, 2),    // frame width disagrees with VP8L width.
        (start + 28, 0xff), // nested bitstream length exceeds ANMF.
        (start + 32, 0),    // invalid VP8L signature.
    ] {
        let mut invalid = bytes.clone();
        invalid[offset] = value;
        assert!(
            container(Cursor::new(invalid), &cancel).is_err(),
            "accepted offset {offset}"
        );
    }
    let bitstream = &frame[24..];
    for extra in [
        bitstream.to_vec(),
        b"ALPH\x01\0\0\0\0\0".to_vec(),
        b"JUNK\x01\0\0\0\0\x01".to_vec(),
    ] {
        let mut invalid = bytes.clone();
        invalid.extend_from_slice(&extra);
        invalid[start + 4..start + 8]
            .copy_from_slice(&((frame.len() - 8 + extra.len()) as u32).to_le_bytes());
        assert!(container(Cursor::new(finish(invalid)), &cancel).is_err());
    }
    assert_eq!(
        riff_size(u64::from(u32::MAX) - 1).expect("largest RIFF"),
        u32::MAX - 9
    );
    for length in [0, 11, 13, u64::from(u32::MAX), u64::MAX] {
        assert!(riff_size(length).is_err());
    }
}

#[test]
fn animated_webp_export_protects_targets_on_cancel_source_change_and_corruption() {
    let root = crate::export::audio_tests::root("webp-animation-failure");
    let source = root.join("source.webp");
    let target = root.join("target.webp");
    let bytes = fixture(3, &[17, 31, 53], true);
    fs::write(&source, &bytes).expect("source");
    fs::write(&target, b"existing target").expect("target");
    let request = request(&source, &target, vec![]);
    for before in [true, false] {
        let cancel = AtomicBool::new(before);
        let result = export_cancellable(&request, &cancel, &|time| {
            if time > Duration::ZERO {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert!(matches!(result, Err(ExportError::Cancelled)));
        assert_eq!(fs::read(&target).expect("preserved"), b"existing target");
    }
    let changed = AtomicBool::new(false);
    let result = export_cancellable(&request, &AtomicBool::new(false), &|time| {
        if time > Duration::ZERO && !changed.swap(true, Ordering::Relaxed) {
            fs::OpenOptions::new()
                .append(true)
                .open(&source)
                .expect("source")
                .write_all(&[0])
                .expect("change source");
        }
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&target).expect("preserved"), b"existing target");
    fs::write(&source, &bytes).expect("restore fixture");
    for operations in [
        vec![EditOperation::Crop(PixelCrop {
            x: 4,
            y: 0,
            width: 1,
            height: 1,
        })],
        vec![EditOperation::RotateImage(
            towavue_core::ImageRotation::new(130, (3, 4)).expect("mismatched rotation source"),
        )],
    ] {
        assert!(export_media(&self::request(&source, &target, operations)).is_err());
        assert_eq!(
            fs::read(&target).expect("invalid edits preserve target"),
            b"existing target"
        );
    }
    let mut wide = bytes.clone();
    wide[24..27].copy_from_slice(&16384u32.to_le_bytes()[..3]);
    fs::write(&source, wide).expect("wide animation canvas");
    assert!(
        export_media(&request)
            .expect_err("output dimension limit")
            .to_string()
            .contains("16384")
    );
    assert_eq!(
        fs::read(&target).expect("wide target preserved"),
        b"existing target"
    );
    fs::write(&source, &bytes).expect("restore fixture");
    let controls = container(Cursor::new(&bytes), &AtomicBool::new(false))
        .expect("source controls")
        .animation
        .expect("animation");
    {
        let staging = StagedExport::new(&target).expect("occupied output staging");
        fs::write(&staging.output, b"occupied").expect("occupied stage");
        assert!(
            controls
                .export(&request, &staging, &AtomicBool::new(false), &|_| {})
                .is_err()
        );
        assert_eq!(
            fs::read(&staging.output).expect("occupied preserved"),
            b"occupied"
        );
    }
    for extension in ["png", "gif", "jpg", "avif"] {
        let target = target.with_extension(extension);
        fs::write(&target, b"existing target").expect("target");
        assert!(export_media(&self::request(&source, &target, vec![])).is_err());
        assert_eq!(
            fs::read(&target).expect("conversion target preserved"),
            b"existing target"
        );
    }
    let mut damaged = bytes.clone();
    let bitstream = damaged
        .windows(4)
        .position(|bytes| bytes == b"VP8L")
        .expect("VP8L")
        + 8;
    damaged[bitstream + 5..bitstream + 9].fill(0xff);
    for damaged in [damaged, bytes[..bytes.len() - 1].to_vec()] {
        fs::write(&source, damaged).expect("damaged source");
        assert!(export_media(&request).is_err());
        assert_eq!(fs::read(&target).expect("preserved"), b"existing target");
    }
    assert!(
        !fs::read_dir(&root)
            .expect("no staging left")
            .any(|entry| entry.expect("entry").file_type().expect("type").is_dir())
    );
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
