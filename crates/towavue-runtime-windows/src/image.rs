use std::fs::File;
use std::io::{BufReader, Error as IoError};
use std::path::Path;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, DynamicImage, Frame, ImageDecoder, ImageError, ImageFormat};
use thiserror::Error;

use crate::decode::{self, DecodeError, DecodeOutput};

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
}

pub fn decode_image(path: &Path) -> Result<DecodedImage, ImageDecodeError> {
    let reader = image::ImageReader::open(path)
        .map_err(ImageDecodeError::Open)?
        .with_guessed_format()
        .map_err(ImageDecodeError::Open)?;
    let format = reader.format().ok_or(ImageDecodeError::UnknownFormat)?;
    let frames = match format {
        ImageFormat::Avif => ffmpeg_frames(path)?,
        ImageFormat::Gif => animated_frames(GifDecoder::new(open(path)?)?)?,
        ImageFormat::WebP => {
            let decoder = WebPDecoder::new(open(path)?)?;
            if decoder.has_animation() {
                animated_frames(decoder)?
            } else {
                vec![static_frame(reader)?]
            }
        }
        ImageFormat::Png => {
            let decoder = PngDecoder::new(open(path)?)?;
            if decoder.is_apng()? {
                animated_frames(decoder.apng()?)?
            } else {
                vec![static_frame(reader)?]
            }
        }
        _ => vec![static_frame(reader)?],
    };
    if frames.is_empty() {
        return Err(ImageDecodeError::Empty);
    }
    Ok(DecodedImage {
        format: format_name(format),
        frames,
    })
}

fn ffmpeg_frames(path: &Path) -> Result<Vec<DecodedImageFrame>, ImageDecodeError> {
    let mut decoded = Vec::new();
    decode::decode_file(path, |output| {
        if let DecodeOutput::Video(frame) = output {
            decoded.push(frame);
        }
        true
    })?;
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

fn open(path: &Path) -> Result<BufReader<File>, ImageDecodeError> {
    File::open(path)
        .map(BufReader::new)
        .map_err(ImageDecodeError::Open)
}

fn static_frame(
    reader: image::ImageReader<BufReader<File>>,
) -> Result<DecodedImageFrame, ImageDecodeError> {
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
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
) -> Result<Vec<DecodedImageFrame>, ImageDecodeError> {
    decoder
        .into_frames()
        .collect_frames()?
        .into_iter()
        .map(frame)
        .collect()
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
            fs::remove_file(path).expect("remove static fixture");

            assert_eq!(decoded.dimensions(), (3, 2), "format {format:?}");
            assert_eq!(decoded.frames.len(), 1, "format {format:?}");
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

        let decoded = decode_image(&path).expect("decode GIF fixture");
        fs::remove_file(path).expect("remove GIF fixture");

        assert_eq!(decoded.frames.len(), 2);
        assert_eq!(decoded.frames[0].delay, Duration::from_millis(50));
        assert_eq!(&decoded.frames[1].rgba[..4], &[0, 255, 0, 255]);
    }
}
