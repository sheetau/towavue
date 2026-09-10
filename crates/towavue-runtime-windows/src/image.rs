use std::fs::File;
use std::io::{BufReader, Error as IoError, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, DynamicImage, Frame, ImageDecoder, ImageError, ImageFormat};
use thiserror::Error;

use crate::decode::{self, DecodeError, DecodeOutput};

mod jpeg_preview;
pub(crate) use jpeg_preview::jpeg_preview;

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
        let skip = match format {
            ImageFormat::Gif | ImageFormat::Avif => true,
            ImageFormat::Png => PngDecoder::new(open(path, is_current)?)?.is_apng()?,
            ImageFormat::WebP => WebPDecoder::new(open(path, is_current)?)?.has_animation(),
            _ => false,
        };
        if skip {
            return Ok(None);
        }
        let frame = static_frame(reader, byte_limit, is_current)?;
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
            ImageFormat::Avif => ffmpeg_frames(path, byte_limit, is_current, preview)?,
            ImageFormat::Gif => {
                let mut decoder = GifDecoder::new(open(path, is_current)?)?;
                decoder.set_limits(image::Limits::default())?;
                animated_frames(decoder, byte_limit, is_current, preview)?
            }
            ImageFormat::WebP => {
                let decoder = WebPDecoder::new(open(path, is_current)?)?;
                if decoder.has_animation() {
                    animated_frames(decoder, byte_limit, is_current, preview)?
                } else {
                    vec![static_frame(reader, byte_limit, is_current)?]
                }
            }
            ImageFormat::Png => {
                let decoder = PngDecoder::new(open(path, is_current)?)?;
                if decoder.is_apng()? {
                    animated_frames(decoder.apng()?, byte_limit, is_current, preview)?
                } else {
                    vec![static_frame(reader, byte_limit, is_current)?]
                }
            }
            _ => vec![static_frame(reader, byte_limit, is_current)?],
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

fn ffmpeg_frames(
    path: &Path,
    byte_limit: usize,
    is_current: &(impl Fn() -> bool + ?Sized),
    preview: &mut ImagePreviewCallback<'_>,
) -> Result<Vec<DecodedImageFrame>, ImageDecodeError> {
    let mut decoded = Vec::new();
    let mut remaining = byte_limit;
    let mut failure = None;
    let result = decode::decode_file(path, |output| {
        if !is_current() {
            failure = Some(ImageDecodeError::Cancelled);
            return false;
        }
        if let DecodeOutput::Video(frame) = output {
            let Some(bytes) = remaining.checked_sub(frame.rgba.len()) else {
                failure = Some(ImageDecodeError::TooLarge);
                return false;
            };
            remaining = bytes;
            if decoded.is_empty() {
                preview(frame.width, frame.height, &frame.rgba);
            }
            decoded.push(frame);
        }
        true
    });
    if let Some(error) = failure {
        return Err(error);
    }
    result?;
    let presentation_times = decoded
        .iter()
        .map(|frame| frame.presentation_time)
        .collect::<Vec<_>>();
    let mut frames = Vec::with_capacity(decoded.len());
    for (index, frame) in decoded.into_iter().enumerate() {
        let delay = presentation_times
            .get(index + 1)
            .map(|next| {
                Duration::from_nanos(
                    next.as_nanoseconds()
                        .saturating_sub(frame.presentation_time.as_nanoseconds())
                        .max(0) as u64,
                )
                .max(Duration::from_millis(10))
            })
            .unwrap_or(Duration::from_millis(100));
        frames.push(DecodedImageFrame {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba,
            delay,
        });
    }
    Ok(frames)
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
    reader: image::ImageReader<BufReader<CancellableReader<'_, File>>>,
    byte_limit: usize,
    is_current: &dyn Fn() -> bool,
) -> Result<DecodedImageFrame, ImageDecodeError> {
    let mut decoder = reader.into_decoder()?;
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
