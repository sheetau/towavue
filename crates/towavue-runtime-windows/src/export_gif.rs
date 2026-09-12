use super::*;
use std::io::Write;

pub(super) fn gif_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gif"))
}

fn invalid(error: impl std::fmt::Display) -> ExportError {
    ExportError::Failed(format!("GIF export: {error}"))
}

struct Reader<'a> {
    file: fs::File,
    cancelled: &'a AtomicBool,
}

impl Read for Reader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("GIF export cancelled"));
        }
        let limit = bytes.len().min(65536);
        self.file.read(&mut bytes[..limit])
    }
}

fn decoder<'a>(
    path: &Path,
    cancelled: &'a AtomicBool,
) -> Result<gif::Decoder<Reader<'a>>, ExportError> {
    let mut options = gif::DecodeOptions::new();
    options.check_frame_consistency(true);
    options.check_lzw_end_code(true);
    options.set_memory_limit(gif::MemoryLimit::Bytes(
        (512 * 1024 * 1024).try_into().expect("nonzero limit"),
    ));
    options
        .read_info(Reader {
            file: fs::File::open(path).map_err(ExportError::Output)?,
            cancelled,
        })
        .map_err(invalid)
}

#[derive(Debug, PartialEq)]
pub(super) struct Animation {
    delays: Vec<u16>,
    repeat: gif::Repeat,
}

impl Animation {
    pub(super) fn read(path: &Path, cancelled: &AtomicBool) -> Result<Self, ExportError> {
        let result = (|| {
            let mut decoder = decoder(path, cancelled)?;
            if u64::from(decoder.width()) * u64::from(decoder.height()) * 4 > 512 * 1024 * 1024 {
                return Err(invalid("canvas exceeds 512 MiB"));
            }
            let mut delays = Vec::new();
            // Validate indexed pixels too: FFmpeg can silently tolerate damaged GIF LZW,
            // even with -xerror. Retain only this frame, never the entire animation.
            while let Some(frame) = decoder.read_next_frame().map_err(invalid)? {
                check_cancelled(cancelled)?;
                if delays.len() == 65536 {
                    return Err(invalid("animation exceeds 65536 frames"));
                }
                delays.push(frame.delay);
            }
            if delays.is_empty() {
                return Err(invalid("no image frames"));
            }
            Ok(Self {
                delays,
                repeat: decoder.repeat(),
            })
        })();
        check_cancelled(cancelled)?;
        result
    }

    pub(super) fn is_animated(&self) -> bool {
        self.delays.len() > 1
    }

    pub(super) fn apply(
        &self,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportError> {
        let temporary = staging.directory.join("animation.gif");
        let result = (|| {
            let mut decoded = decoder(&staging.output, cancelled)?;
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(ExportError::Output)?;
            let mut encoded = gif::Encoder::new(
                std::io::BufWriter::new(file),
                decoded.width(),
                decoded.height(),
                decoded.global_palette().unwrap_or_default(),
            )
            .map_err(invalid)?;
            encoded.set_repeat(self.repeat).map_err(invalid)?;
            // Re-encode palette indices, not colors: preserve encoder pixels and disposal
            // while restoring source delays (including zero) independently of FFmpeg timestamps.
            for delay in &self.delays {
                check_cancelled(cancelled)?;
                let mut frame = decoded
                    .read_next_frame()
                    .map_err(invalid)?
                    .ok_or_else(|| invalid("encoder omitted animation frames"))?
                    .clone();
                frame.delay = *delay;
                encoded.write_frame(&frame).map_err(invalid)?;
            }
            if decoded.read_next_frame().map_err(invalid)?.is_some() {
                return Err(invalid("encoder added animation frames"));
            }
            encoded
                .into_inner()
                .map_err(invalid)?
                .flush()
                .map_err(ExportError::Output)?;
            if Self::read(&temporary, cancelled)? != *self {
                return Err(invalid("saved animation controls did not match source"));
            }
            check_cancelled(cancelled)?;
            fs::rename(&temporary, &staging.output).map_err(ExportError::Output)
        })();
        check_cancelled(cancelled)?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(source: &Path, target: &Path) -> ExportRequest {
        ExportRequest {
            source: source.into(),
            target: target.into(),
            kind: MediaKind::Image,
            operations: Vec::new(),
            hardware_encode: false,
        }
    }

    fn regions(path: &Path, dispose: gif::DisposalMethod) {
        let file = fs::File::create(path).expect("GIF regions");
        let mut encoder =
            gif::Encoder::new(file, 4, 3, &[255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0])
                .expect("encoder");
        encoder.set_repeat(gif::Repeat::Finite(7)).expect("repeat");
        for index in 0..5 {
            let mut frame = gif::Frame {
                delay: index + 1,
                transparent: Some(3),
                dispose,
                ..Default::default()
            };
            if index == 0 {
                frame.width = 4;
                frame.height = 3;
                frame.buffer = vec![0, 1, 2, 3, 3, 3, 3, 3, 0, 1, 2, 3].into();
            } else {
                frame.width = 2;
                frame.height = 2;
                frame.left = 1;
                frame.top = 1;
                frame.buffer = vec![2, 3, 3, 1].into();
            }
            encoder.write_frame(&frame).expect("region");
        }
        encoder.into_inner().expect("trailer");
    }

    #[test]
    fn gif_export_preserves_transparent_regions_disposal_and_duplicate_frames() {
        use towavue_core::{ImageResize, PixelCrop, ResampleFilter};
        let root = audio_tests::root("gif-regions");
        let source = root.join("source.gif");
        let target = root.join("target.gif");
        for dispose in [
            gif::DisposalMethod::Any,
            gif::DisposalMethod::Keep,
            gif::DisposalMethod::Background,
            gif::DisposalMethod::Previous,
        ] {
            regions(&source, dispose);
            let before = fs::read(&source).expect("source bytes");
            let controls = Animation::read(&source, &AtomicBool::new(false)).expect("controls");
            let original = crate::decode_image(&source).expect("display source");
            let mut request = request(&source, &target);
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
                    EditOperation::FlipHorizontal,
                    EditOperation::Resize(
                        ImageResize::new(6, 6, ResampleFilter::Nearest).expect("resize"),
                    ),
                ],
            ] {
                request.operations = operations;
                export_media(&request).expect("region save");
                let actual = crate::decode_image(&target).expect("saved display");
                let expected = crate::render_image_edits(
                    &original,
                    &request.operations,
                    &crate::Cancellation::default(),
                )
                .expect("expected edits");
                assert_eq!(actual.frames.len(), expected.frames.len());
                for (index, (actual, expected)) in
                    actual.frames.iter().zip(&expected.frames).enumerate()
                {
                    assert_eq!(
                        (actual.width, actual.height, actual.delay),
                        (expected.width, expected.height, expected.delay)
                    );
                    assert!(
                        actual
                            .rgba
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .zip(expected.rgba.as_chunks::<4>().0)
                            .all(|(a, b)| a == b || a[3] == 0 && b[3] == 0),
                        "visible pixels differ: disposal={dispose:?}, frame={index}"
                    );
                }
                assert_eq!(
                    Animation::read(&target, &AtomicBool::new(false)).expect("saved controls"),
                    controls
                );
                let resave = root.join("resaved.gif");
                export_media(&self::request(&target, &resave)).expect("resave");
                assert_eq!(
                    crate::decode_image(&resave).expect("resaved").frames,
                    actual.frames
                );
                assert_eq!(fs::read(&source).expect("source untouched"), before);
            }
        }
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn gif_export_rejects_flattening_and_preserves_existing_targets_on_failure() {
        let root = audio_tests::root("gif-failures");
        let source = root.join("source.gif");
        let target = root.join("target.gif");
        fixture(&source, gif::Repeat::Infinite);
        let before = fs::read(&source).expect("source bytes");
        let cancel = AtomicBool::new(false);
        for extension in ["png", "jpg", "webp", "avif"] {
            let target = target.with_extension(extension);
            fs::write(&target, b"existing target").expect("target");
            assert!(
                export_media(&request(&source, &target))
                    .expect_err("no silent flatten")
                    .to_string()
                    .contains("must not discard frames")
            );
            assert_eq!(fs::read(&target).expect("preserved"), b"existing target");
            fs::remove_file(target).expect("owned target cleanup");
        }
        fs::write(&target, b"existing target").expect("target");
        let mut metadata = MetadataExportOptions::default();
        metadata
            .set(MetadataField::Title, Some("unsupported title".into()))
            .expect("title");
        assert!(
            export_media_with_options(
                &request(&source, &target),
                ExportOptions {
                    metadata,
                    ..Default::default()
                }
            )
            .expect_err("unsupported metadata")
            .to_string()
            .contains("GIF metadata editing")
        );
        assert_eq!(
            fs::read(&target).expect("target preserved"),
            b"existing target"
        );
        cancel.store(true, Ordering::Relaxed);
        assert!(matches!(
            export_cancellable(&request(&source, &target), &cancel, &|_| {}),
            Err(ExportError::Cancelled)
        ));
        cancel.store(false, Ordering::Relaxed);
        assert!(matches!(
            export_cancellable(&request(&source, &target), &cancel, &|_| {
                cancel.store(true, Ordering::Relaxed);
            }),
            Err(ExportError::Cancelled)
        ));
        cancel.store(false, Ordering::Relaxed);
        assert!(
            export_cancellable(&request(&source, &target), &cancel, &|_| {
                fs::write(&source, b"changed while saving").expect("change source");
            })
            .is_err()
        );
        assert_eq!(
            fs::read(&target).expect("target preserved"),
            b"existing target"
        );
        let mut corrupt_pixels = before.clone();
        let descriptor = corrupt_pixels
            .iter()
            .position(|byte| *byte == 0x2c)
            .expect("fixture image descriptor");
        let packed = corrupt_pixels[descriptor + 9];
        let palette_bytes = if packed & 0x80 != 0 {
            3 * (1 << ((packed & 7) + 1))
        } else {
            0
        };
        let lzw = descriptor + 10 + palette_bytes;
        let block_bytes = usize::from(corrupt_pixels[lzw + 1]);
        corrupt_pixels[lzw + 2..lzw + 2 + block_bytes].fill(255);
        fs::write(&source, &corrupt_pixels).expect("invalid LZW with intact container");
        let mut options = gif::DecodeOptions::new();
        options.skip_frame_decoding(true);
        assert!(
            options
                .read_info(fs::File::open(&source).expect("source"))
                .expect("container header")
                .next_frame_info()
                .expect("structurally valid control")
                .is_some()
        );
        assert!(export_media(&request(&source, &target)).is_err());
        assert_eq!(
            fs::read(&target).expect("target preserved"),
            b"existing target"
        );
        for bytes in [&before[..10], b"not a GIF".as_slice()] {
            fs::write(&source, bytes).expect("corrupt source");
            assert!(export_media(&request(&source, &target)).is_err());
            assert_eq!(
                fs::read(&target).expect("target preserved"),
                b"existing target"
            );
        }
        assert_eq!(fs::read_dir(&root).expect("staging cleaned").count(), 2);
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn gif_staging_validates_counts_and_single_frame_conversion_still_works() {
        let root = audio_tests::root("gif-staging");
        let source = root.join("source.gif");
        let target = root.join("target.gif");
        fixture(&source, gif::Repeat::Finite(3));
        fs::write(&target, b"existing target").expect("target");
        let bytes = fs::read(&source).expect("source bytes");
        let cancel = AtomicBool::new(false);
        for count in [2, 4] {
            let staging = StagedExport::new(&target).expect("staging");
            fs::write(&staging.output, &bytes).expect("encoded output");
            let controls = Animation {
                delays: vec![1; count],
                repeat: gif::Repeat::Infinite,
            };
            assert!(controls.apply(&staging, &cancel).is_err());
            assert_eq!(fs::read(&staging.output).expect("output untouched"), bytes);
            assert_eq!(
                fs::read(&target).expect("target untouched"),
                b"existing target"
            );
        }
        let staging = StagedExport::new(&target).expect("staging");
        fs::write(&staging.output, &bytes).expect("encoded output");
        let controls = Animation::read(&source, &cancel).expect("controls");
        let temporary = staging.directory.join("animation.gif");
        fs::write(&temporary, b"occupied").expect("owned occupied path");
        assert!(controls.apply(&staging, &cancel).is_err());
        assert_eq!(fs::read(&temporary).expect("not overwritten"), b"occupied");
        drop(staging);
        assert_eq!(fs::read_dir(&root).expect("stages cleaned").count(), 2);

        let mut encoder = gif::Encoder::new(
            fs::File::create(&source).expect("static source"),
            1,
            1,
            &[255, 0, 0, 0, 0, 0],
        )
        .expect("encoder");
        encoder
            .write_frame(&gif::Frame {
                width: 1,
                height: 1,
                delay: 17,
                buffer: vec![0].into(),
                ..Default::default()
            })
            .expect("static frame");
        encoder.into_inner().expect("trailer");
        export_media(&request(&source, &target)).expect("single-frame GIF save");
        assert_eq!(
            Animation::read(&target, &cancel).expect("saved controls"),
            Animation::read(&source, &cancel).expect("source controls")
        );
        let png = target.with_extension("png");
        export_media(&request(&source, &png)).expect("static conversion");
        assert_eq!(
            crate::decode_image(&png).expect("PNG").frames[0].rgba,
            [255, 0, 0, 255]
        );
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    fn fixture(path: &Path, repeat: gif::Repeat) {
        let file = fs::File::create(path).expect("GIF source");
        let mut encoder = gif::Encoder::new(file, 4, 3, &[]).expect("encoder");
        encoder.set_repeat(repeat).expect("repeat");
        for (index, delay) in [0, 1, u16::MAX].into_iter().enumerate() {
            let color = match index {
                0 => [255, 0, 0, 255],
                1 => [0, 255, 0, 255],
                _ => [0, 0, 255, 255],
            };
            let mut rgba = color.repeat(12);
            let mut frame = gif::Frame::from_rgba_speed(4, 3, &mut rgba, 10);
            frame.delay = delay;
            frame.dispose = gif::DisposalMethod::Background;
            encoder.write_frame(&frame).expect("frame");
        }
        encoder.into_inner().expect("trailer");
    }

    #[test]
    fn gif_scan_enforces_frame_canvas_and_cancellation_limits() {
        let root = audio_tests::root("gif-scan-limits");
        let source = root.join("source.gif");
        let cancel = AtomicBool::new(false);
        let frame = gif::Frame {
            width: 1,
            height: 1,
            buffer: vec![0].into(),
            ..Default::default()
        };
        for count in [65536, 65537] {
            let mut encoder = gif::Encoder::new(
                fs::File::create(&source).expect("source"),
                1,
                1,
                &[255, 0, 0, 0, 0, 0],
            )
            .expect("encoder");
            for _ in 0..count {
                encoder.write_frame(&frame).expect("frame");
            }
            encoder.into_inner().expect("trailer");
            let scanned = Animation::read(&source, &cancel);
            if count == 65536 {
                assert_eq!(scanned.expect("maximum frame count").delays.len(), count);
            } else {
                assert!(
                    scanned
                        .expect_err("frame limit")
                        .to_string()
                        .contains("65536")
                );
            }
        }
        for (width, height, allowed) in [(16384, 8192, true), (16384, 8193, false)] {
            let mut encoder = gif::Encoder::new(
                fs::File::create(&source).expect("source"),
                width,
                height,
                &[255, 0, 0, 0, 0, 0],
            )
            .expect("encoder");
            encoder.write_frame(&frame).expect("partial frame");
            encoder.into_inner().expect("trailer");
            assert_eq!(Animation::read(&source, &cancel).is_ok(), allowed);
        }
        cancel.store(true, Ordering::Relaxed);
        assert!(matches!(
            Animation::read(&source, &cancel),
            Err(ExportError::Cancelled)
        ));
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn gif_export_preserves_frames_timing_repeat_and_edited_pixels() {
        let root = audio_tests::root("gif-save");
        for repeat in [
            gif::Repeat::Finite(0),
            gif::Repeat::Infinite,
            gif::Repeat::Finite(3),
        ] {
            let source = root.join("source.GIF");
            let target = root.join("output.GIF");
            fixture(&source, repeat);
            let original = crate::decode_image(&source).expect("source frames");
            let request = ExportRequest {
                source,
                target: target.clone(),
                kind: MediaKind::Image,
                operations: vec![EditOperation::RotateClockwise],
                hardware_encode: false,
            };
            export_media(&request).expect("GIF export");
            let actual = crate::decode_image(&target).expect("saved frames");
            assert_eq!(actual.frames.len(), 3, "GIF export must not flatten frames");
            let expected = crate::render_image_edits(
                &original,
                &request.operations,
                &crate::Cancellation::default(),
            )
            .expect("displayed edits");
            assert_eq!(actual.frames, expected.frames);
            let mut decoder = gif::DecodeOptions::new()
                .read_info(fs::File::open(&target).expect("saved file"))
                .expect("saved decoder");
            let mut delays = Vec::new();
            while let Some(frame) = decoder.read_next_frame().expect("saved frame") {
                delays.push(frame.delay);
            }
            assert_eq!(delays, [0, 1, u16::MAX]);
            assert_eq!(decoder.repeat(), repeat);
        }
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }
}
