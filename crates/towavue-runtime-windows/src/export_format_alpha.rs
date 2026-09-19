use super::*;
use image::AnimationDecoder;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Transparency {
    Opaque,
    Binary,
    Full,
}

fn sample(value: u16) -> Transparency {
    match value {
        u16::MAX => Transparency::Opaque,
        0 => Transparency::Binary,
        _ => Transparency::Full,
    }
}

fn rgba(bytes: &[u8]) -> Transparency {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| sample(u16::from(pixel[3]) * 257))
        .max()
        .unwrap_or(Transparency::Opaque)
}

fn frames(
    frames: image::Frames<'_>,
    cancelled: &AtomicBool,
) -> Result<Transparency, image::ImageError> {
    let mut result = Transparency::Opaque;
    for frame in frames {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        result = result.max(rgba(frame?.buffer().as_raw()));
        if result == Transparency::Full {
            break;
        }
    }
    Ok(result)
}

/// Inspect only when a palette/opaque destination remains eligible. Animation
/// iterators hold one composed frame; native AVIF uses its existing bounded
/// decoder. A decoding/limit error conservatively retains full-alpha choices;
/// actual export still performs its ordinary integrity validation.
pub(super) fn inspect(
    path: &Path,
    animated: bool,
    declared: bool,
    cancelled: &AtomicBool,
) -> Transparency {
    if !declared {
        return Transparency::Opaque;
    }
    if avif::avif_path(path) {
        return crate::image::decode_image_cancellable(
            path,
            crate::image::IMAGE_BYTE_LIMIT,
            &|| !cancelled.load(Ordering::Relaxed),
        )
        .map(|image| {
            image
                .frames
                .iter()
                .map(|frame| rgba(&frame.rgba))
                .max()
                .unwrap_or(Transparency::Opaque)
        })
        .unwrap_or(Transparency::Full);
    }
    let inspect = || -> Result<Transparency, image::ImageError> {
        let file = BufReader::new(fs::File::open(path)?);
        use image::ImageDecoder;
        if animated && gif_animation::gif_path(path) {
            let mut decoder = image::codecs::gif::GifDecoder::new(file)?;
            decoder.set_limits(image::Limits::default())?;
            return frames(decoder.into_frames(), cancelled);
        }
        if animated && png_metadata::png_path(path) {
            let mut decoder = image::codecs::png::PngDecoder::new(file)?;
            decoder.set_limits(image::Limits::default())?;
            // Display adapters expose RGBA8 frames; retain the declared higher
            // precision alpha when that conversion could hide a fractional value.
            if matches!(
                decoder.color_type(),
                image::ColorType::La16 | image::ColorType::Rgba16
            ) {
                return Ok(Transparency::Full);
            }
            return frames(decoder.apng()?.into_frames(), cancelled);
        }
        if animated && webp_metadata::webp_path(path) {
            let mut decoder = image::codecs::webp::WebPDecoder::new(file)?;
            decoder.set_limits(image::Limits::default())?;
            return frames(decoder.into_frames(), cancelled);
        }
        let image = image::ImageReader::new(file)
            .with_guessed_format()?
            .decode()?;
        Ok(match image {
            image::DynamicImage::ImageLumaA8(buffer) => {
                buffer.pixels().map(|p| sample(u16::from(p[1]) * 257)).max()
            }
            image::DynamicImage::ImageRgba8(buffer) => Some(rgba(buffer.as_raw())),
            image::DynamicImage::ImageLumaA16(buffer) => {
                buffer.pixels().map(|p| sample(p[1])).max()
            }
            image::DynamicImage::ImageRgba16(buffer) => buffer.pixels().map(|p| sample(p[3])).max(),
            image::DynamicImage::ImageRgba32F(buffer) => buffer
                .pixels()
                .map(|p| {
                    if p[3] == 1.0 {
                        Transparency::Opaque
                    } else if p[3] == 0.0 {
                        Transparency::Binary
                    } else {
                        Transparency::Full
                    }
                })
                .max(),
            _ => Some(Transparency::Opaque),
        }
        .unwrap_or(Transparency::Opaque))
    };
    inspect().unwrap_or(Transparency::Full)
}
