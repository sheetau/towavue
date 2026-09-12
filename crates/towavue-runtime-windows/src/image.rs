use std::fs::File;
use std::io::{BufReader, Error as IoError, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, DynamicImage, Frame, ImageDecoder, ImageError, ImageFormat};
use thiserror::Error;

use crate::decode::DecodeError;

pub(crate) mod apng;
mod avif;
mod bmp_preview;
mod jpeg_preview;
mod png_preview;
mod png_static;
pub(crate) use png_preview::png_thumbnail;

pub(crate) fn first_image_preview(
    path: &Path,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
) -> Result<Option<crate::CachedImagePreview>, ImageDecodeError> {
    let reader = image::ImageReader::new(open(path, current)?)
        .with_guessed_format()
        .map_err(ImageDecodeError::Open)?;
    match reader.format() {
        Some(ImageFormat::Jpeg) => jpeg_preview::jpeg_preview(path, byte_limit, current),
        Some(ImageFormat::Bmp) => bmp_preview::bmp_preview(reader.into_inner(), byte_limit),
        _ => Ok(None),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedImageFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub delay: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedImage {
    pub format: &'static str,
    pub frames: Vec<DecodedImageFrame>,
}

impl DecodedImage {
    pub fn retained_bytes(&self) -> usize {
        self.frames.iter().map(|frame| frame.rgba.len()).sum()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.frames
            .first()
            .map_or((0, 0), |frame| (frame.width, frame.height))
    }

    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }
}

#[derive(Debug, Error)]
pub enum ImageDecodeError {
    #[error("could not open image: {0}")]
    Open(#[source] IoError),
    #[error("could not decode image: {0}")]
    Decode(#[from] ImageError),
    #[error("FFmpeg could not decode image: {0}")]
    Ffmpeg(#[from] DecodeError),
    #[error("could not decode AVIF: {0}")]
    Avif(String),
    #[error("could not decode PNG: {0}")]
    Png(#[from] png::DecodingError),
    #[error("could not encode APNG export frames: {0}")]
    PngEncode(#[from] png::EncodingError),
    #[error("image format could not be determined")]
    UnknownFormat,
    #[error("decoded image contained no frames")]
    Empty,
    #[error("image exceeds the available decoded-image memory budget")]
    TooLarge,
    #[error("image request was superseded")]
    Cancelled,
}

pub fn decode_image(path: &Path) -> Result<DecodedImage, ImageDecodeError> {
    decode_image_cancellable(path, IMAGE_BYTE_LIMIT, &|| true)
}

pub(crate) const IMAGE_BYTE_LIMIT: usize = 512 * 1024 * 1024;

pub(crate) fn first_animation_frame(
    path: &Path,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
) -> Result<Option<DecodedImageFrame>, ImageDecodeError> {
    let result = (|| {
        check_current(current)?;
        let mut reader = image::ImageReader::new(open(path, current)?);
        if let Ok(format) = ImageFormat::from_path(path) {
            reader.set_format(format);
        }
        let reader = reader
            .with_guessed_format()
            .map_err(ImageDecodeError::Open)?;
        let mut preview = |_, _, _: &[u8]| {};
        let frames = match reader.format() {
            Some(ImageFormat::Gif) => {
                let mut decoder = GifDecoder::new(reader.into_inner())?;
                decoder.set_limits(image::Limits::default())?;
                animated_frames(decoder, byte_limit, current, &mut preview, true)?
            }
            Some(ImageFormat::Png) => {
                let decoder =
                    PngDecoder::with_limits(reader.into_inner(), image::Limits::default())?;
                if !decoder.is_apng()? {
                    return Ok(None);
                }
                drop(decoder);
                apng::decode(path, byte_limit, current, &mut preview, true)?
            }
            Some(ImageFormat::WebP) => {
                let decoder = WebPDecoder::new(reader.into_inner())?;
                if !decoder.has_animation() {
                    return Ok(None);
                }
                animated_frames(decoder, byte_limit, current, &mut preview, true)?
            }
            Some(ImageFormat::Avif) => avif::decode(path, byte_limit, current, &mut preview, true)?,
            _ => return Ok(None),
        };
        Ok(frames.into_iter().next())
    })();
    check_current(current)?;
    result
}

pub(crate) fn decode_image_for_prefetch(
    path: &Path,
    byte_limit: usize,
    is_current: &dyn Fn() -> bool,
) -> Result<Option<DecodedImage>, ImageDecodeError> {
    let result = (|| {
        check_current(is_current)?;
        let mut reader = image::ImageReader::new(open(path, is_current)?);
        if let Ok(format) = ImageFormat::from_path(path) {
            reader.set_format(format);
        }
        let reader = reader
            .with_guessed_format()
            .map_err(ImageDecodeError::Open)?;
        let format = reader.format().ok_or(ImageDecodeError::UnknownFormat)?;
        let frame = match format {
            ImageFormat::Gif | ImageFormat::Avif => return Ok(None),
            ImageFormat::Png => {
                let Some(frame) = png_static::decode(reader.into_inner(), byte_limit, is_current)?
                else {
                    return Ok(None);
                };
                frame
            }
            ImageFormat::WebP => {
                let decoder = WebPDecoder::new(reader.into_inner())?;
                if decoder.has_animation() {
                    return Ok(None);
                }
                static_frame(decoder, byte_limit, is_current)?
            }
            _ => static_frame(reader.into_decoder()?, byte_limit, is_current)?,
        };
        check_current(is_current)?;
        Ok(Some(DecodedImage {
            format: format_name(format),
            frames: vec![frame],
        }))
    })();
    check_current(is_current)?;
    result
}

pub(crate) fn decode_image_cancellable(
    path: &Path,
    byte_limit: usize,
    is_current: &dyn Fn() -> bool,
) -> Result<DecodedImage, ImageDecodeError> {
    decode_image_with_preview(path, byte_limit, is_current, &mut |_, _, _| {})
}

pub(crate) type ImagePreviewCallback<'a> = dyn FnMut(u32, u32, &[u8]) + 'a;

pub(crate) fn decode_image_with_preview(
    path: &Path,
    byte_limit: usize,
    is_current: &dyn Fn() -> bool,
    preview: &mut ImagePreviewCallback<'_>,
) -> Result<DecodedImage, ImageDecodeError> {
    let result = (|| {
        check_current(is_current)?;
        let mut reader = image::ImageReader::new(open(path, is_current)?);
        if let Ok(format) = ImageFormat::from_path(path) {
            reader.set_format(format);
        }
        let reader = reader
            .with_guessed_format()
            .map_err(ImageDecodeError::Open)?;
        let format = reader.format().ok_or(ImageDecodeError::UnknownFormat)?;
        let frames = match format {
            ImageFormat::Avif => avif::decode(path, byte_limit, is_current, preview, false)?,
            ImageFormat::Gif => {
                let mut decoder = GifDecoder::new(open(path, is_current)?)?;
                decoder.set_limits(image::Limits::default())?;
                animated_frames(decoder, byte_limit, is_current, preview, false)?
            }
            ImageFormat::WebP => {
                let decoder = WebPDecoder::new(reader.into_inner())?;
                if decoder.has_animation() {
                    animated_frames(decoder, byte_limit, is_current, preview, false)?
                } else {
                    vec![static_frame(decoder, byte_limit, is_current)?]
                }
            }
            ImageFormat::Png => {
                if let Some(frame) =
                    png_static::decode(reader.into_inner(), byte_limit, is_current)?
                {
                    vec![frame]
                } else {
                    apng::decode(path, byte_limit, is_current, preview, false)?
                }
            }
            _ => vec![static_frame(
                reader.into_decoder()?,
                byte_limit,
                is_current,
            )?],
        };
        check_current(is_current)?;
        if frames.is_empty() {
            return Err(ImageDecodeError::Empty);
        }
        Ok(DecodedImage {
            format: format_name(format),
            frames,
        })
    })();
    // Codec adapters may wrap the reader's cancellation as an ordinary decode error.
    check_current(is_current)?;
    result
}

struct CancellableReader<'a, R> {
    inner: R,
    is_current: &'a dyn Fn() -> bool,
}

impl<R: Read> Read for CancellableReader<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, IoError> {
        if !(self.is_current)() {
            return Err(IoError::other(ImageDecodeError::Cancelled));
        }
        // Bound read_to_end's growing requests; the outer BufReader amortizes
        // checks for codecs that read individual pixels or small header fields.
        let count = bytes.len().min(64 * 1024);
        self.inner.read(&mut bytes[..count])
    }
}

impl<R: Seek> Seek for CancellableReader<'_, R> {
    fn seek(&mut self, position: SeekFrom) -> Result<u64, IoError> {
        if !(self.is_current)() {
            return Err(IoError::other(ImageDecodeError::Cancelled));
        }
        self.inner.seek(position)
    }
}

fn open<'a>(
    path: &Path,
    is_current: &'a dyn Fn() -> bool,
) -> Result<BufReader<CancellableReader<'a, File>>, ImageDecodeError> {
    File::open(path)
        .map(|inner| BufReader::new(CancellableReader { inner, is_current }))
        .map_err(ImageDecodeError::Open)
}

fn static_frame(
    mut decoder: impl ImageDecoder,
    byte_limit: usize,
    is_current: &dyn Fn() -> bool,
) -> Result<DecodedImageFrame, ImageDecodeError> {
    decoder.set_limits(image::Limits::default())?;
    let (width, height) = decoder.dimensions();
    let bytes = (u64::from(width) * u64::from(height))
        .checked_mul(4)
        .ok_or(ImageDecodeError::TooLarge)?;
    if bytes > byte_limit as u64 {
        return Err(ImageDecodeError::TooLarge);
    }
    let orientation = decoder.orientation()?;
    check_current(is_current)?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    check_current(is_current)?;
    image.apply_orientation(orientation);
    check_current(is_current)?;
    let image = image.into_rgba8();
    Ok(DecodedImageFrame {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        delay: Duration::ZERO,
    })
}

fn animated_frames<'a>(
    decoder: impl AnimationDecoder<'a>,
    byte_limit: usize,
    is_current: &(impl Fn() -> bool + ?Sized),
    preview: &mut ImagePreviewCallback<'_>,
    first_only: bool,
) -> Result<Vec<DecodedImageFrame>, ImageDecodeError> {
    let mut frames = Vec::new();
    let mut remaining = byte_limit;
    let mut source = decoder.into_frames();
    loop {
        check_current(is_current)?;
        let Some(decoded) = source.next() else { break };
        let frame = frame(decoded?)?;
        remaining = remaining
            .checked_sub(frame.rgba.len())
            .ok_or(ImageDecodeError::TooLarge)?;
        check_current(is_current)?;
        if frames.is_empty() {
            preview(frame.width, frame.height, &frame.rgba);
        }
        frames.push(frame);
        if first_only {
            break;
        }
    }
    Ok(frames)
}

fn check_current(is_current: &(impl Fn() -> bool + ?Sized)) -> Result<(), ImageDecodeError> {
    if is_current() {
        Ok(())
    } else {
        Err(ImageDecodeError::Cancelled)
    }
}

fn frame(frame: Frame) -> Result<DecodedImageFrame, ImageDecodeError> {
    let delay = frame.delay();
    let (numerator, denominator) = delay.numer_denom_ms();
    let delay =
        Duration::from_secs_f64(f64::from(numerator) / f64::from(denominator.max(1)) / 1_000.0)
            .max(Duration::from_millis(10));
    let image = frame.into_buffer();
    Ok(DecodedImageFrame {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        delay,
    })
}

fn format_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Avif => "AVIF",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Gif => "GIF",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Png => "PNG",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::WebP => "WebP",
        _ => "Image",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reused_png_decoder_preserves_depth_alpha_orientation_and_budget() {
        use image::ImageEncoder;
        use image::metadata::Orientation;

        let path = temporary_path("png");
        let source = DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(37, 23, |x, y| {
            image::Rgba([
                (x * 1733) as u16,
                (y * 2891) as u16,
                ((x + y) * 1031) as u16,
                ((x * y) * 71) as u16,
            ])
        }));
        let variants = [
            DynamicImage::ImageLuma8(source.to_luma8()),
            DynamicImage::ImageLumaA8(source.to_luma_alpha8()),
            DynamicImage::ImageRgb8(source.to_rgb8()),
            DynamicImage::ImageRgba8(source.to_rgba8()),
            DynamicImage::ImageLuma16(source.to_luma16()),
            DynamicImage::ImageLumaA16(source.to_luma_alpha16()),
            DynamicImage::ImageRgb16(source.to_rgb16()),
            source,
        ];
        for source in variants {
            for tag in 1..=8 {
                let mut exif =
                    *b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0";
                exif[18] = tag;
                let mut bytes = Vec::new();
                let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
                encoder.set_exif_metadata(exif.to_vec()).expect("PNG EXIF");
                encoder
                    .write_image(
                        source.as_bytes(),
                        source.width(),
                        source.height(),
                        source.color().into(),
                    )
                    .expect("typed PNG");
                std::fs::write(&path, bytes).expect("owned oriented PNG");
                let mut expected = source.clone();
                expected.apply_orientation(Orientation::from_exif(tag).expect("orientation tag"));
                let expected = expected.into_rgba8();
                for prefetch in [false, true] {
                    let decode = |limit| {
                        if prefetch {
                            decode_image_for_prefetch(&path, limit, &|| true)
                                .map(|image| image.expect("static PNG"))
                        } else {
                            decode_image_cancellable(&path, limit, &|| true)
                        }
                    };
                    let decoded = decode(IMAGE_BYTE_LIMIT).expect("oriented typed PNG");
                    assert_eq!(decoded.dimensions(), expected.dimensions());
                    assert_eq!(
                        decoded.frames[0].rgba,
                        *expected.as_raw(),
                        "{:?}, EXIF {tag}, prefetch {prefetch}",
                        source.color()
                    );
                    assert!(matches!(decode(1), Err(ImageDecodeError::TooLarge)));
                }
            }
        }
        std::fs::remove_file(path).expect("remove owned typed PNG");
    }

    #[test]
    fn static_png_metadata_is_read_once_for_foreground_and_prefetch() {
        let source = image::RgbaImage::from_fn(32, 16, |x, y| {
            image::Rgba([x as u8, y as u8, 180, (x * y) as u8])
        });
        let mut encoded = std::io::Cursor::new(Vec::new());
        source
            .write_to(&mut encoded, ImageFormat::Png)
            .expect("PNG fixture");
        let mut encoded = encoded.into_inner();
        let mut payload = b"Comment\0".to_vec();
        payload.resize(16 * 1024 * 1024, b'a');
        let mut chunk = (payload.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"tEXt");
        chunk.extend_from_slice(&payload);
        let checksum = crc32fast::hash(&chunk[4..]);
        chunk.extend_from_slice(&checksum.to_be_bytes());
        encoded.splice(33..33, chunk);
        let path = std::env::temp_dir().join(format!(
            "towavue-png-metadata-once-{}.png",
            std::process::id()
        ));
        std::fs::write(&path, encoded).expect("owned metadata PNG");
        for prefetch in [false, true] {
            let polls = std::cell::Cell::new(0);
            let current = || {
                polls.set(polls.get() + 1);
                true
            };
            let started = std::time::Instant::now();
            let decoded = if prefetch {
                decode_image_for_prefetch(&path, IMAGE_BYTE_LIMIT, &current)
                    .expect("static prefetch")
                    .expect("not animated")
            } else {
                decode_image_cancellable(&path, IMAGE_BYTE_LIMIT, &current).expect("foreground")
            };
            eprintln!(
                "metadata PNG prefetch={prefetch}: {:?}, polls={}",
                started.elapsed(),
                polls.get()
            );
            assert_eq!(decoded.dimensions(), (32, 16));
            assert_eq!(decoded.frames[0].rgba, *source.as_raw());
            // The pinned PNG decoder streams text through roughly 2,048 buffered reads.
            assert!(
                polls.get() < 2500,
                "metadata header read twice: {} polls",
                polls.get()
            );
        }
        std::fs::remove_file(path).expect("remove owned metadata PNG");
    }
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Rgb, RgbImage, Rgba, RgbaImage};

    use super::*;

    #[test]
    fn cancellable_reader_bounds_reads_and_does_not_touch_superseded_input() {
        let current = std::cell::Cell::new(true);
        let check = || current.get();
        let mut reader = CancellableReader {
            inner: std::io::Cursor::new(vec![7; 256 * 1024]),
            is_current: &check,
        };
        let mut bytes = vec![0; 256 * 1024];
        assert_eq!(reader.read(&mut bytes).expect("bounded read"), 64 * 1024);
        assert!(bytes[..64 * 1024].iter().all(|byte| *byte == 7));
        assert_eq!(reader.seek(SeekFrom::Start(17)).expect("seek"), 17);
        current.set(false);
        assert!(reader.read(&mut bytes).is_err());
        assert!(reader.seek(SeekFrom::Start(0)).is_err());
        assert_eq!(reader.inner.position(), 17, "no I/O after cancellation");
    }

    #[test]
    fn static_decodes_preserve_pixels_and_cancel_inside_foreground_and_prefetch() {
        for extension in ["png", "jpg", "bmp", "tiff", "webp"] {
            let path = temporary_path(extension);
            RgbImage::from_fn(512, 256, |x, y| {
                let n = (x + y * 512).wrapping_mul(0x9e37_79b9);
                let n = (n ^ (n >> 16)).wrapping_mul(0x85eb_ca6b);
                Rgb([n as u8, (n >> 8) as u8, (n >> 16) as u8])
            })
            .save(&path)
            .expect("owned static fixture");
            let expected = image::open(&path)
                .expect("reference decoder")
                .into_rgba8()
                .into_raw();
            for prefetch in [false, true] {
                let decode = |current: &dyn Fn() -> bool| {
                    if prefetch {
                        decode_image_for_prefetch(&path, IMAGE_BYTE_LIMIT, current)
                            .map(|image| image.expect("static prefetch"))
                    } else {
                        decode_image_cancellable(&path, IMAGE_BYTE_LIMIT, current)
                    }
                };
                let polls = std::cell::Cell::new(0);
                let decoded = decode(&|| {
                    polls.set(polls.get() + 1);
                    true
                })
                .expect("current image");
                assert_eq!(decoded.dimensions(), (512, 256));
                assert_eq!(decoded.frames[0].rgba, expected, "{extension}");
                let full_polls = polls.get();
                assert!(
                    full_polls > 8,
                    "must check within the codec, not just at entry/exit"
                );
                polls.set(0);
                let result = decode(&|| {
                    polls.set(polls.get() + 1);
                    polls.get() < full_polls / 2
                });
                assert!(
                    matches!(result, Err(ImageDecodeError::Cancelled)),
                    "{extension}, prefetch={prefetch}"
                );
                assert!(
                    polls.get() < full_polls,
                    "stop without finishing the old decode"
                );
            }
            fs::remove_file(path).expect("remove owned fixture");
        }
    }

    #[test]
    fn image_limit_rejects_rgba_expansion_and_cancelled_requests() {
        let path = temporary_path("png");
        RgbImage::from_pixel(4, 3, Rgb([10, 20, 30]))
            .save(&path)
            .expect("fixture");
        let oversized = decode_image_cancellable(&path, 47, &|| true);
        let cancelled = decode_image_cancellable(&path, 48, &|| false);
        fs::remove_file(path).expect("remove fixture");
        assert!(
            oversized.is_err(),
            "48 RGBA bytes must not fit a 47-byte budget"
        );
        assert!(cancelled.is_err(), "cancelled requests must not decode");
    }

    fn temporary_path(extension: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("towavue-image-{unique}.{extension}"))
    }

    #[test]
    fn decodes_static_rgba_pixels() {
        let path = temporary_path("png");
        let image = RgbaImage::from_pixel(3, 2, Rgba([10, 20, 30, 255]));
        image
            .save_with_format(&path, ImageFormat::Png)
            .expect("write PNG fixture");

        let decoded = decode_image(&path).expect("decode PNG fixture");
        fs::remove_file(path).expect("remove PNG fixture");

        assert_eq!(decoded.format, "PNG");
        assert_eq!(decoded.dimensions(), (3, 2));
        assert_eq!(decoded.frames.len(), 1);
        assert_eq!(&decoded.frames[0].rgba[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn decodes_each_advertised_static_format() {
        for (extension, format) in [
            ("bmp", ImageFormat::Bmp),
            ("jpg", ImageFormat::Jpeg),
            ("png", ImageFormat::Png),
            ("tiff", ImageFormat::Tiff),
            ("webp", ImageFormat::WebP),
        ] {
            let path = temporary_path(extension);
            RgbImage::from_pixel(3, 2, Rgb([10, 20, 30]))
                .save_with_format(&path, format)
                .expect("write static fixture");

            let decoded = decode_image(&path)
                .unwrap_or_else(|error| panic!("decode {format:?} fixture: {error}"));
            assert_eq!(
                decode_image_for_prefetch(&path, IMAGE_BYTE_LIMIT, &|| true)
                    .expect("prefetch fixture")
                    .expect("static prefetch"),
                decoded
            );
            fs::remove_file(path).expect("remove static fixture");

            assert_eq!(decoded.dimensions(), (3, 2), "format {format:?}");
            assert_eq!(decoded.frames.len(), 1, "format {format:?}");
        }
    }

    #[test]
    fn animated_format_previews_match_the_original_first_frame() {
        let ffmpeg =
            std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
                .join("bin/ffmpeg.exe");
        for (extension, codec) in [
            ("apng", "apng"),
            ("webp", "libwebp_anim"),
            ("avif", "libaom-av1"),
        ] {
            let path = temporary_path(extension);
            let output = std::process::Command::new(&ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=32x24:rate=2:duration=1",
                    "-frames:v",
                    "2",
                    "-c:v",
                    codec,
                    "-threads",
                    "1",
                ])
                .arg(&path)
                .output()
                .expect("generate owned animation");
            assert!(
                output.status.success(),
                "{extension}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let reference = decode_image(&path).expect("reference decode");
            assert_eq!(reference.frames.len(), 2, "{extension}");
            assert!(
                reference
                    .frames
                    .iter()
                    .all(|frame| frame.delay == Duration::from_millis(500)),
                "{extension} must retain the last frame's duration too: {:?}",
                reference
                    .frames
                    .iter()
                    .map(|frame| frame.delay)
                    .collect::<Vec<_>>()
            );
            let first = first_animation_frame(&path, 32 * 24 * 4, &|| true)
                .expect("one-frame budget")
                .expect("first frame");
            assert_eq!(first.rgba, reference.frames[0].rgba, "{extension}");
            assert_eq!(
                first.delay, reference.frames[0].delay,
                "{extension} preview timing"
            );
            assert_eq!((first.width, first.height), (32, 24));
            assert!(matches!(
                first_animation_frame(&path, 1, &|| true),
                Err(ImageDecodeError::TooLarge)
            ));
            assert!(matches!(
                first_animation_frame(&path, IMAGE_BYTE_LIMIT, &|| false),
                Err(ImageDecodeError::Cancelled)
            ));
            let mut previews = Vec::new();
            let decoded =
                decode_image_with_preview(&path, IMAGE_BYTE_LIMIT, &|| true, &mut |w, h, rgba| {
                    previews.push((w, h, rgba.to_vec()));
                })
                .expect("decode with first-frame callback");
            assert_eq!(decoded, reference);
            assert_eq!(
                previews,
                vec![(32, 24, reference.frames[0].rgba.clone())],
                "{extension}"
            );
            let current = std::cell::Cell::new(true);
            let result = decode_image_with_preview(
                &path,
                IMAGE_BYTE_LIMIT,
                &|| current.get(),
                &mut |_, _, _| {
                    current.set(false);
                },
            );
            assert!(
                matches!(result, Err(ImageDecodeError::Cancelled)),
                "{extension}: {result:?}"
            );
            let mut previews = 0;
            let result =
                decode_image_with_preview(&path, 1, &|| true, &mut |_, _, _| previews += 1);
            assert!(
                matches!(result, Err(ImageDecodeError::TooLarge)),
                "{extension}: {result:?}"
            );
            assert_eq!(previews, 0);
            fs::remove_file(path).expect("remove owned animation");
        }
    }

    #[test]
    fn avif_variable_frame_timing_keeps_the_tail_and_first_preview_duration() {
        use std::os::windows::process::CommandExt;
        let ffmpeg =
            std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
                .join("bin/ffmpeg.exe");
        for loops in ["0", "1", "3"] {
            let path = temporary_path("avif");
            let output = std::process::Command::new(&ffmpeg)
                .creation_flags(0x08000000)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=32x24:rate=2:duration=1.5",
                    "-vf",
                    "settb=1/1000,setpts='if(eq(N,0),0,if(eq(N,1),125,875))'",
                    "-fps_mode",
                    "passthrough",
                    "-enc_time_base",
                    "1/1000",
                    "-c:v",
                    "libaom-av1",
                    "-cpu-used",
                    "8",
                    "-threads",
                    "1",
                    "-loop",
                    loops,
                ])
                .arg(&path)
                .output()
                .expect("generate VFR AVIF");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let decoded = decode_image(&path).expect("variable-timing AVIF");
            assert_eq!(
                decoded
                    .frames
                    .iter()
                    .map(|frame| frame.delay)
                    .collect::<Vec<_>>(),
                [125, 750, 500].map(Duration::from_millis),
                "loop count {loops}"
            );
            let preview = first_animation_frame(&path, 32 * 24 * 4, &|| true)
                .expect("preview")
                .expect("AVIF frame");
            assert_eq!(preview.delay, decoded.frames[0].delay);
            assert_eq!(preview.rgba, decoded.frames[0].rgba);
            std::fs::remove_file(path).expect("owned fixture cleanup");
        }
    }

    #[test]
    fn preserves_animated_gif_frames_and_timing() {
        let path = temporary_path("gif");
        let file = File::create(&path).expect("create GIF fixture");
        let mut encoder = GifEncoder::new(file);
        encoder
            .set_repeat(Repeat::Infinite)
            .expect("set GIF repeat");
        for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
            encoder
                .encode_frame(Frame::from_parts(
                    RgbaImage::from_pixel(2, 1, Rgba(color)),
                    0,
                    0,
                    Delay::from_numer_denom_ms(50, 1),
                ))
                .expect("encode GIF frame");
        }
        drop(encoder);

        let mut previews = Vec::new();
        let decoded =
            decode_image_with_preview(&path, IMAGE_BYTE_LIMIT, &|| true, &mut |w, h, rgba| {
                previews.push((w, h, rgba.to_vec()));
            })
            .expect("decode GIF fixture");
        assert_eq!(previews, vec![(2, 1, decoded.frames[0].rgba.clone())]);
        assert_eq!(
            first_animation_frame(&path, 8, &|| true).expect("one-frame budget"),
            Some(decoded.frames[0].clone())
        );
        let current = std::cell::Cell::new(true);
        let cancelled = decode_image_with_preview(
            &path,
            IMAGE_BYTE_LIMIT,
            &|| current.get(),
            &mut |_, _, _| {
                current.set(false);
            },
        );
        assert!(matches!(cancelled, Err(ImageDecodeError::Cancelled)));
        let mut previews = 0;
        let oversized = decode_image_with_preview(&path, 7, &|| true, &mut |_, _, _| previews += 1);
        assert!(matches!(oversized, Err(ImageDecodeError::TooLarge)));
        assert_eq!(
            previews, 0,
            "an over-budget first frame cannot be published"
        );
        assert!(
            decode_image_for_prefetch(&path, 0, &|| true)
                .expect("skip animated prefetch")
                .is_none(),
            "speculation must not collect animation frames"
        );
        assert!(
            matches!(
                decode_image_cancellable(&path, 8, &|| true),
                Err(ImageDecodeError::TooLarge)
            ),
            "two animation frames must share one budget"
        );
        fs::remove_file(path).expect("remove GIF fixture");

        assert_eq!(decoded.frames.len(), 2);
        assert_eq!(decoded.frames[0].delay, Duration::from_millis(50));
        assert_eq!(&decoded.frames[1].rgba[..4], &[0, 255, 0, 255]);
    }
}
