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

impl std::io::Seek for Reader<'_> {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("GIF export cancelled"));
        }
        std::io::Seek::seek(&mut self.file, position)
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
            let (width, height) = (decoder.width(), decoder.height());
            if u64::from(width) * u64::from(height) * 4 > 512 * 1024 * 1024 {
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

    pub(super) fn apply_png(
        &self,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportError> {
        // GIF stores repeats after the initial play; APNG stores total plays.
        // Finite(0) is the GIF crate's absent-loop-extension representation.
        let plays = self.plays();
        let delays = self
            .delays
            .iter()
            .map(|delay| {
                let [high, low] = delay.to_be_bytes();
                [high, low, 0, 100]
            })
            .collect();
        png_metadata::PngMetadata::from_animation(plays, delays).apply(staging, cancelled)
    }

    fn plays(&self) -> u32 {
        match self.repeat {
            gif::Repeat::Infinite => 0,
            gif::Repeat::Finite(repeats) => u32::from(repeats) + 1,
        }
    }

    pub(super) fn webp_plays(&self) -> Result<u16, ExportError> {
        u16::try_from(self.plays()).map_err(|_| invalid(
            "65536 total plays exceed WebP's 65535-play limit; use GIF or APNG to retain repetition"))
    }

    pub(super) fn apply_webp(
        &self,
        staging: &StagedExport,
        cancelled: &AtomicBool,
        progress: &(impl Fn(Duration) + Sync),
    ) -> Result<(), ExportError> {
        webp_metadata::apply_png_frames(
            staging,
            self.delays
                .iter()
                .map(|delay| u32::from(*delay) * 10)
                .collect(),
            self.webp_plays()?,
            cancelled,
            progress,
        )
    }

    pub(super) fn apply(
        &self,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportError> {
        let temporary = staging.directory.join("animation.gif");
        let result = (|| {
            let mut input = BufReader::new(Reader {
                file: fs::File::open(&staging.output).map_err(ExportError::Output)?,
                cancelled,
            });
            let (width, height, mut pixels) = read_png(&mut input)?;
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(ExportError::Output)?;
            let mut encoded = gif::Encoder::new(std::io::BufWriter::new(file), width, height, &[])
                .map_err(invalid)?;
            encoded.set_repeat(self.repeat).map_err(invalid)?;
            // FFmpeg emits full-canvas RGBA PNG snapshots. Clear each before the next
            // snapshot so newly transparent pixels cannot expose a previous frame's colors.
            for (index, delay) in self.delays.iter().enumerate() {
                check_cancelled(cancelled)?;
                if index != 0 {
                    // Release the previous RGBA canvas before allocating another.
                    drop(pixels);
                    let (w, h, next) = read_png(&mut input)?;
                    if (w, h) != (width, height) {
                        return Err(invalid("encoded frame dimensions differ"));
                    }
                    pixels = next;
                }
                let mut frame = palette_frame(width, height, &mut pixels);
                frame.delay = *delay;
                frame.dispose = gif::DisposalMethod::Background;
                check_cancelled(cancelled)?;
                encoded.write_frame(&frame).map_err(invalid)?;
            }
            if input.read(&mut [0]).map_err(ExportError::Output)? != 0 {
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

pub(super) fn read_png(
    input: &mut (impl std::io::BufRead + std::io::Seek),
) -> Result<(u16, u16, Vec<u8>), ExportError> {
    let mut decoder = png::Decoder::new(input);
    decoder.set_limits(png::Limits {
        bytes: 512 * 1024 * 1024,
    });
    let mut reader = decoder.read_info().map_err(invalid)?;
    let info = reader.info();
    let width = u16::try_from(info.width).map_err(invalid)?;
    let height = u16::try_from(info.height).map_err(invalid)?;
    if info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
        || info.animation_control.is_some()
    {
        return Err(invalid("expected static RGBA8 PNG snapshot"));
    }
    let bytes = usize::from(width) * usize::from(height) * 4;
    if bytes > 512 * 1024 * 1024 {
        return Err(invalid("canvas exceeds 512 MiB"));
    }
    let mut pixels = vec![0; bytes];
    reader.next_frame(&mut pixels).map_err(invalid)?;
    reader.finish().map_err(invalid)?;
    Ok((width, height, pixels))
}

fn palette_frame(width: u16, height: u16, pixels: &mut [u8]) -> gif::Frame<'static> {
    // Match the previous GIF alpha threshold. Normalize invisible RGB before palette
    // construction so hidden colors cannot consume entries intended for visible pixels.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        if pixel[3] < 128 {
            pixel.fill(0);
        } else {
            pixel[3] = 255;
        }
    }
    let mut frame = gif::Frame::from_rgba_speed(width, height, pixels, 10);
    // NeuQuant is used only beyond 256 RGBA colors. It can map an opaque color to
    // the transparent entry; keep that pixel visible using the closest opaque entry.
    if let Some(transparent) = frame.transparent {
        let palette = frame
            .palette
            .as_ref()
            .expect("RGBA frame has a local palette");
        for (index, pixel) in frame
            .buffer
            .to_mut()
            .iter_mut()
            .zip(pixels.as_chunks::<4>().0)
        {
            if pixel[3] == 0 {
                *index = transparent;
            } else if *index == transparent {
                *index = palette
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != usize::from(transparent))
                    .min_by_key(|(_, color)| {
                        color
                            .iter()
                            .zip(pixel)
                            .map(|(a, b)| (i32::from(*a) - i32::from(*b)).pow(2))
                            .sum::<i32>()
                    })
                    .expect("opaque entry")
                    .0 as u8;
            }
        }
    }
    frame
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
                for extension in ["png", "webp"] {
                    let converted_target = root.join("converted").with_extension(extension);
                    request.target = converted_target.clone();
                    export_media(&request).expect("transparent GIF conversion");
                    let converted =
                        crate::decode_image(&converted_target).expect("converted display");
                    assert_eq!(converted.frames.len(), expected.frames.len());
                    for (converted, expected) in converted.frames.iter().zip(&expected.frames) {
                        assert_eq!(
                            (converted.width, converted.height, converted.delay),
                            (expected.width, expected.height, expected.delay)
                        );
                        assert!(
                            converted
                                .rgba
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .zip(expected.rgba.as_chunks::<4>().0)
                                .all(|(a, b)| a == b || a[3] == 0 && b[3] == 0),
                            "{extension} visible pixels: {dispose:?}"
                        );
                    }
                }
                request.target = target.clone();
            }
        }
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn gif_to_webp_preserves_edited_frames_delays_and_repetition_limits() {
        let root = audio_tests::root("gif-to-webp");
        let source = root.join("source.gif");
        let target = root.join("converted.webp");
        for repeat in [
            gif::Repeat::Finite(0),
            gif::Repeat::Finite(1),
            gif::Repeat::Finite(u16::MAX - 1),
            gif::Repeat::Infinite,
        ] {
            fixture(&source, repeat);
            let before = fs::read(&source).expect("source");
            let original = crate::decode_image(&source).expect("display");
            let mut request = request(&source, &target);
            request.operations = vec![
                EditOperation::RotateClockwise,
                EditOperation::FlipHorizontal,
            ];
            export_media(&request).expect("GIF to animated WebP");
            let expected = crate::render_image_edits(
                &original,
                &request.operations,
                &crate::Cancellation::default(),
            )
            .expect("edited display");
            let actual = crate::decode_image(&target).expect("WebP display");
            assert_eq!(actual.frames, expected.frames);
            let resaved = root.join("resaved.webp");
            export_media(&self::request(&target, &resaved)).expect("WebP resave");
            assert_eq!(
                crate::decode_image(&resaved).expect("resaved").frames,
                actual.frames
            );
            for output in [&target, &resaved] {
                let mut decoder = image_webp::WebPDecoder::new(BufReader::new(
                    fs::File::open(output).expect("WebP"),
                ))
                .expect("independent controls");
                assert_eq!(decoder.num_frames(), 3);
                let expected_loop = match repeat {
                    gif::Repeat::Infinite => image_webp::LoopCount::Forever,
                    gif::Repeat::Finite(n) => image_webp::LoopCount::Times(
                        std::num::NonZeroU16::new(n + 1).expect("total plays"),
                    ),
                };
                assert_eq!(decoder.loop_count(), expected_loop);
                let mut pixels = vec![0; decoder.output_buffer_size().expect("canvas")];
                for delay in [0, 10, 655350] {
                    assert_eq!(decoder.read_frame(&mut pixels).expect("frame"), delay);
                }
            }
            assert_eq!(fs::read(&source).expect("source intact"), before);
        }
        fixture(&source, gif::Repeat::Finite(u16::MAX));
        let before = fs::read(&target).expect("existing WebP");
        let error =
            export_cancellable(&request(&source, &target), &AtomicBool::new(false), &|_| {
                panic!("unrepresentable repeat must fail before encoding")
            })
            .expect_err("repeat limit");
        assert!(error.to_string().contains("65536 total plays"));
        assert_eq!(fs::read(&target).expect("existing output retained"), before);

        regions(&source, gif::DisposalMethod::Previous);
        let mut graded_alpha = false;
        for operations in [
            vec![EditOperation::RotateImage(
                towavue_core::ImageRotation::new(137, (4, 3)).expect("free rotation"),
            )],
            vec![EditOperation::Resize(
                towavue_core::ImageResize::new(7, 5, towavue_core::ResampleFilter::Bilinear)
                    .expect("bilinear"),
            )],
            vec![EditOperation::Resize(
                towavue_core::ImageResize::new(7, 5, towavue_core::ResampleFilter::Bicubic)
                    .expect("bicubic"),
            )],
            vec![EditOperation::Resize(
                towavue_core::ImageResize::new(7, 5, towavue_core::ResampleFilter::Lanczos)
                    .expect("Lanczos"),
            )],
        ] {
            let mut request = request(&source, &target);
            request.operations = operations;
            export_media(&request).expect("WebP with interpolated alpha");
            let actual = crate::decode_image(&target).expect("WebP pixels");
            request.target = root.join("reference.apng");
            export_media(&request).expect("same edited PNG snapshots in APNG");
            assert_eq!(
                actual.frames,
                crate::decode_image(&request.target)
                    .expect("APNG pixels")
                    .frames
            );
            graded_alpha |= actual.frames.iter().any(|frame| {
                frame
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[3] > 0 && p[3] < 255)
            });
        }
        assert!(
            graded_alpha,
            "conversion must retain non-binary edited alpha"
        );
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn gif_to_apng_preserves_edited_frames_exact_delays_and_total_plays() {
        let root = audio_tests::root("gif-to-apng");
        let source = root.join("source.gif");
        for (repeat, plays) in [
            (gif::Repeat::Finite(0), 1),
            (gif::Repeat::Finite(1), 2),
            (gif::Repeat::Finite(u16::MAX), 65536),
            (gif::Repeat::Infinite, 0),
        ] {
            fixture(&source, repeat);
            let before = fs::read(&source).expect("source");
            let original = crate::decode_image(&source).expect("display");
            for extension in ["png", "apng"] {
                let target = root.join("converted").with_extension(extension);
                let mut request = request(&source, &target);
                request.operations = vec![
                    EditOperation::RotateClockwise,
                    EditOperation::FlipHorizontal,
                ];
                export_media(&request).expect("animated GIF to APNG");
                let expected = crate::render_image_edits(
                    &original,
                    &request.operations,
                    &crate::Cancellation::default(),
                )
                .expect("edited display");
                let actual = crate::decode_image(&target).expect("saved animation");
                assert_eq!(actual.frames, expected.frames);
                let mut reader =
                    png::Decoder::new(BufReader::new(fs::File::open(&target).expect("APNG")))
                        .read_info()
                        .expect("independent PNG controls");
                let animation = reader.info().animation_control.expect("animated output");
                assert_eq!(animation.num_frames, 3);
                assert_eq!(animation.num_plays, plays);
                let mut pixels = vec![0; reader.output_buffer_size().expect("canvas")];
                for delay in [0, 1, u16::MAX] {
                    reader.next_frame(&mut pixels).expect("frame");
                    let frame = reader.info().frame_control.expect("frame control");
                    assert_eq!((frame.delay_num, frame.delay_den), (delay, 100));
                    assert_eq!(frame.blend_op, png::BlendOp::Source);
                    assert_eq!(frame.dispose_op, png::DisposeOp::None);
                }
                drop(reader);
                let resaved = root.join("resaved.png");
                export_media(&self::request(&target, &resaved)).expect("APNG resave");
                assert_eq!(
                    crate::decode_image(&resaved)
                        .expect("resaved animation")
                        .frames,
                    actual.frames
                );
                assert_eq!(fs::read(&source).expect("source intact"), before);
            }
        }
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn gif_export_rejects_flattening_and_preserves_existing_targets_on_failure() {
        for extension in ["gif", "png", "webp"] {
            check_gif_export_failures(extension);
        }
    }

    fn check_gif_export_failures(extension: &str) {
        let root = audio_tests::root("gif-failures");
        let source = root.join("source.gif");
        let target = root.join("target").with_extension(extension);
        fixture(&source, gif::Repeat::Infinite);
        let before = fs::read(&source).expect("source bytes");
        let cancel = AtomicBool::new(false);
        for extension in ["bmp", "jpg", "tiff", "avif"] {
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
        let mut bytes = Vec::new();
        for _ in 0..3 {
            let mut encoder = png::Encoder::new(&mut bytes, 4, 3);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("PNG header");
            writer
                .write_image_data(&[255, 0, 0, 255].repeat(12))
                .expect("PNG frame");
            writer.finish().expect("PNG end");
        }
        let cancel = AtomicBool::new(false);
        for count in [2, 4] {
            let staging = StagedExport::new(&target).expect("staging");
            fs::write(&staging.output, &bytes).expect("encoded output");
            let controls = Animation {
                delays: vec![1; count],
                repeat: gif::Repeat::Infinite,
            };
            assert!(controls.apply(&staging, &cancel).is_err());
            assert!(controls.apply_png(&staging, &cancel).is_err());
            assert!(controls.apply_webp(&staging, &cancel, &|_| {}).is_err());
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
        let temporary_png = staging.directory.join("animation.png");
        fs::write(&temporary_png, b"occupied PNG").expect("owned occupied path");
        assert!(controls.apply_png(&staging, &cancel).is_err());
        assert_eq!(
            fs::read(&temporary_png).expect("not overwritten"),
            b"occupied PNG"
        );
        let temporary_webp = staging.directory.join("animation.webp");
        fs::write(&temporary_webp, b"occupied WebP").expect("owned occupied path");
        assert!(controls.apply_webp(&staging, &cancel, &|_| {}).is_err());
        assert_eq!(
            fs::read(&temporary_webp).expect("not overwritten"),
            b"occupied WebP"
        );
        drop(staging);
        let staging = StagedExport::new(&target).expect("partial-frame staging");
        regions(&staging.output, gif::DisposalMethod::Keep);
        let partial = fs::read(&staging.output).expect("partial frames");
        let controls = Animation::read(&staging.output, &cancel).expect("partial controls");
        assert!(
            controls
                .apply(&staging, &cancel)
                .expect_err("only RGBA PNG snapshots may use normalized disposal")
                .to_string()
                .contains("GIF export")
        );
        assert_eq!(
            fs::read(&staging.output).expect("output untouched"),
            partial
        );
        assert_eq!(
            fs::read(&target).expect("target untouched"),
            b"existing target"
        );
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
    fn gif_export_clears_transparent_overlays_background_holes_and_previous_restore() {
        let root = audio_tests::root("gif-opacity");
        let source = root.join("source.gif");
        let target = root.join("target.gif");
        let disposals = [
            gif::DisposalMethod::Any,
            gif::DisposalMethod::Keep,
            gif::DisposalMethod::Background,
            gif::DisposalMethod::Previous,
        ];
        for first in disposals {
            for second in disposals {
                for fill_hole in [false, true] {
                    let mut encoder = gif::Encoder::new(
                        fs::File::create(&source).expect("source"),
                        3,
                        2,
                        &[255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0],
                    )
                    .expect("encoder");
                    for (width, height, left, top, dispose, pixels) in [
                        (3, 2, 0, 0, first, vec![0; 6]),
                        (1, 1, 1, 0, second, vec![1]),
                        (
                            3,
                            2,
                            0,
                            0,
                            gif::DisposalMethod::Keep,
                            vec![3, if fill_hole { 1 } else { 3 }, 3, 3, 3, 3],
                        ),
                        (1, 1, 0, 1, gif::DisposalMethod::Previous, vec![3]),
                    ] {
                        encoder
                            .write_frame(&gif::Frame {
                                width,
                                height,
                                left,
                                top,
                                dispose,
                                delay: 5,
                                transparent: Some(3),
                                buffer: pixels.into(),
                                ..Default::default()
                            })
                            .expect("frame");
                    }
                    encoder.into_inner().expect("trailer");
                    let original = crate::decode_image(&source).expect("display");
                    export_media(&request(&source, &target)).expect("save");
                    let saved = crate::decode_image(&target).expect("saved");
                    assert_eq!(saved.frames.len(), original.frames.len());
                    for (saved, original) in saved.frames.iter().zip(&original.frames) {
                        assert_eq!(saved.delay, original.delay);
                        assert!(
                            saved
                                .rgba
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .zip(original.rgba.as_chunks::<4>().0)
                                .all(|(a, b)| a == b || a[3] == 0 && b[3] == 0),
                            "first={first:?}, second={second:?}, fill={fill_hole}, saved={:?}, expected={:?}",
                            saved.rgba,
                            original.rgba
                        );
                    }
                }
            }
        }
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn gif_opaque_palette_keeps_all_256_colors_through_edits_and_resave() {
        use towavue_core::{ImageResize, ResampleFilter};
        let root = audio_tests::root("gif-full-palette");
        let source = root.join("source.gif");
        let target = root.join("target.gif");
        let palette: Vec<u8> = (0..256)
            .flat_map(|i| {
                [
                    (i & 7) as u8 * 36,
                    ((i >> 3) & 7) as u8 * 36,
                    ((i >> 6) & 3) as u8 * 85,
                ]
            })
            .collect();
        let mut encoder =
            gif::Encoder::new(fs::File::create(&source).expect("source"), 16, 16, &palette)
                .expect("encoder");
        for index in 0..3 {
            let frame = gif::Frame {
                width: 16,
                height: 16,
                delay: 2 + index,
                palette: (index == 1).then(|| {
                    palette
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .rev()
                        .flatten()
                        .copied()
                        .collect()
                }),
                buffer: (0..256)
                    .map(|i| (i + usize::from(index)) as u8)
                    .collect::<Vec<_>>()
                    .into(),
                ..Default::default()
            };
            encoder.write_frame(&frame).expect("frame");
        }
        encoder.into_inner().expect("trailer");
        let decoded = crate::decode_image(&source).expect("source pixels");
        for operations in [
            vec![],
            vec![
                EditOperation::RotateClockwise,
                EditOperation::FlipVertical,
                EditOperation::Resize(
                    ImageResize::new(32, 32, ResampleFilter::Nearest).expect("resize"),
                ),
            ],
        ] {
            let mut request = request(&source, &target);
            request.operations = operations;
            export_media(&request).expect("save");
            let actual = crate::decode_image(&target).expect("saved pixels");
            let expected = crate::render_image_edits(
                &decoded,
                &request.operations,
                &crate::Cancellation::default(),
            )
            .expect("displayed edits");
            assert!(
                actual.frames == expected.frames,
                "opaque GIF must not lose a color to an unused transparent palette entry"
            );
            let resave = root.join("resaved.gif");
            export_media(&self::request(&target, &resave)).expect("resave");
            assert_eq!(
                crate::decode_image(&resave).expect("resaved pixels").frames,
                actual.frames
            );
        }
        let rotation = towavue_core::ImageRotation::new(130, (16, 16)).expect("free rotation");
        let mut request = request(&source, &target);
        request.operations = vec![EditOperation::RotateImage(rotation)];
        export_media(&request).expect("rotation introduces transparency");
        let rotated = crate::decode_image(&target).expect("rotated");
        assert_eq!(rotated.frames.len(), decoded.frames.len());
        for frame in rotated.frames {
            assert_eq!((frame.width, frame.height), rotation.size());
            assert_eq!(
                frame.rgba[3], 0,
                "rotated canvas corner must remain transparent"
            );
            assert!(
                frame
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] == 255)
            );
        }
        let mut encoder =
            gif::Encoder::new(fs::File::create(&source).expect("source"), 16, 16, &palette)
                .expect("encoder");
        for index in 0..3 {
            encoder
                .write_frame(&gif::Frame {
                    width: 16,
                    height: 16,
                    delay: 2 + index,
                    transparent: Some(255),
                    dispose: gif::DisposalMethod::Background,
                    buffer: (0..256)
                        .map(|i| (i + usize::from(index)) as u8)
                        .collect::<Vec<_>>()
                        .into(),
                    ..Default::default()
                })
                .expect("255 colors plus transparency");
        }
        encoder.into_inner().expect("trailer");
        let original = crate::decode_image(&source).expect("transparent source");
        export_media(&self::request(&source, &target)).expect("transparent palette save");
        let saved = crate::decode_image(&target).expect("transparent saved");
        assert_eq!(saved.frames.len(), original.frames.len());
        for (saved, original) in saved.frames.iter().zip(&original.frames) {
            assert_eq!(saved.delay, original.delay);
            let differences: Vec<_> = saved
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .zip(original.rgba.as_chunks::<4>().0)
                .enumerate()
                .filter(|(_, (a, b))| a != b && !(a[3] == 0 && b[3] == 0))
                .collect();
            assert!(
                differences.is_empty(),
                "255 visible colors and one transparent entry must all survive: {differences:?}"
            );
        }
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn gif_quantization_preserves_alpha_threshold_and_fully_transparent_frames() {
        let mut pixels: Vec<u8> = (0..4096)
            .flat_map(|i| {
                [
                    (i % 64 * 4) as u8,
                    (i / 64 * 4) as u8,
                    (i % 251) as u8,
                    if i % 7 == 0 { 127 } else { 128 },
                ]
            })
            .collect();
        let alpha: Vec<_> = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[3] >= 128)
            .collect();
        let frame = palette_frame(64, 64, &mut pixels);
        assert_eq!(frame.palette.as_ref().expect("palette").len(), 768);
        let transparent = frame.transparent.expect("transparent entry");
        for (index, opaque) in frame.buffer.iter().zip(alpha) {
            assert_eq!(
                *index != transparent,
                opaque,
                "quantization must not punch opaque holes"
            );
        }
        let mut invisible: Vec<u8> = (0..256).flat_map(|i| [i as u8, 25, 73, 0]).collect();
        let frame = palette_frame(16, 16, &mut invisible);
        assert_eq!(frame.palette.as_ref().expect("palette").len(), 3);
        assert!(
            frame
                .buffer
                .iter()
                .all(|index| Some(*index) == frame.transparent)
        );
    }

    #[test]
    fn gif_png_staging_rejects_corruption_dimensions_extra_data_and_cancellation() {
        fn png(width: u32, height: u32, color: png::ColorType) -> Vec<u8> {
            let mut bytes = Vec::new();
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(color);
            let mut writer = encoder.write_header().expect("header");
            // Invalid oversized headers must be rejected before any image allocation.
            if width <= 4 && height <= 4 {
                writer
                    .write_image_data(&vec![
                        42;
                        width as usize * height as usize * color.samples()
                    ])
                    .expect("pixels");
                writer.finish().expect("end");
            } else {
                writer
                    .write_chunk(png::chunk::IDAT, &[])
                    .expect("empty oversized image");
                drop(writer);
            }
            bytes
        }
        let first = png(2, 2, png::ColorType::Rgba);
        let mut corrupt = first.clone();
        corrupt[29] ^= 1; // IHDR checksum.
        let root = audio_tests::root("gif-png-validation");
        let target = root.join("target.gif");
        fs::write(&target, b"existing target").expect("target");
        for (bytes, frames, cancelled, error) in [
            (corrupt, 1, false, "CRC"),
            (first[..first.len() - 1].to_vec(), 1, false, "GIF export"),
            (
                [first.as_slice(), &[0]].concat(),
                1,
                false,
                "added animation frames",
            ),
            (
                [first.clone(), png(3, 2, png::ColorType::Rgba)].concat(),
                2,
                false,
                "dimensions differ",
            ),
            (png(2, 2, png::ColorType::Rgb), 1, false, "RGBA8"),
            (png(65536, 1, png::ColorType::Rgba), 1, false, "GIF export"),
            (png(16384, 8193, png::ColorType::Rgba), 1, false, "512 MiB"),
            (first, 1, true, "cancelled"),
        ] {
            let staging = StagedExport::new(&target).expect("staging");
            fs::write(&staging.output, &bytes).expect("staged input");
            let controls = Animation {
                delays: vec![1; frames],
                repeat: gif::Repeat::Infinite,
            };
            let result = controls
                .apply(&staging, &AtomicBool::new(cancelled))
                .expect_err("reject invalid snapshot");
            assert!(
                result.to_string().contains(error),
                "expected {error}: {result}"
            );
            assert_eq!(
                fs::read(&staging.output).expect("staged input preserved"),
                bytes
            );
            assert_eq!(
                fs::read(&target).expect("target preserved"),
                b"existing target"
            );
        }
        assert_eq!(fs::read_dir(&root).expect("stages cleaned").count(), 1);
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
