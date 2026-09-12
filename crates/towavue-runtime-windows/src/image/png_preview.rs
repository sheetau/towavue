use image::metadata::Orientation;

use super::*;
use crate::{CachedImagePreview, PreviewImage};

/// Thumbnail-only row sampling; never add a full PNG decode before foreground loading.
pub(crate) fn png_thumbnail(
    path: &Path,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
) -> Result<Option<CachedImagePreview>, ImageDecodeError> {
    let result = (|| {
        check_current(current)?;
        let reader = image::ImageReader::new(open(path, current)?)
            .with_guessed_format()
            .map_err(ImageDecodeError::Open)?;
        if reader.format() != Some(ImageFormat::Png) {
            return Ok(None);
        }
        let mut decoder =
            png::Decoder::new_with_limits(reader.into_inner(), png::Limits { bytes: byte_limit });
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info()?;
        let info = reader.info();
        if info.interlaced || info.animation_control.is_some() {
            return Ok(None);
        }
        let (width, height) = (info.width, info.height);
        if (u64::from(width) * u64::from(height))
            .checked_mul(4)
            .is_none_or(|bytes| bytes > byte_limit as u64)
        {
            return Err(ImageDecodeError::TooLarge);
        }
        let orientation = info
            .exif_metadata
            .as_deref()
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms);
        let swapped = matches!(
            orientation,
            Orientation::Rotate90
                | Orientation::Rotate270
                | Orientation::Rotate90FlipH
                | Orientation::Rotate270FlipH
        );
        let source_size = if swapped {
            (height, width)
        } else {
            (width, height)
        };
        let ratio = (240.0 / f64::from(source_size.0)).min(160.0 / f64::from(source_size.1));
        let target_width = (f64::from(source_size.0) * ratio).round().max(1.0) as u32;
        let target_height = (f64::from(source_size.1) * ratio).round().max(1.0) as u32;
        let (color, depth) = reader.output_color_type();
        let channels = color.samples();
        let sample_bytes = match depth {
            png::BitDepth::Eight => 1,
            png::BitDepth::Sixteen => 2,
            _ => return Ok(None),
        };
        // Match image::resize(Nearest)'s pixel-center sampling *after* orientation.
        // Sorting the bounded target map lets each source row be discarded immediately.
        let mut samples = Vec::with_capacity((target_width * target_height) as usize);
        for y in 0..target_height {
            let sy = (((y as f32 + 0.5) * (source_size.1 as f32 / target_height as f32)) as u32)
                .min(source_size.1 - 1);
            for x in 0..target_width {
                let sx = (((x as f32 + 0.5) * (source_size.0 as f32 / target_width as f32)) as u32)
                    .min(source_size.0 - 1);
                let (sx, sy) = match orientation {
                    Orientation::NoTransforms => (sx, sy),
                    Orientation::FlipHorizontal => (width - 1 - sx, sy),
                    Orientation::FlipVertical => (sx, height - 1 - sy),
                    Orientation::Rotate180 => (width - 1 - sx, height - 1 - sy),
                    Orientation::Rotate90 => (sy, height - 1 - sx),
                    Orientation::Rotate270 => (width - 1 - sy, sx),
                    Orientation::Rotate90FlipH => (sy, sx),
                    Orientation::Rotate270FlipH => (width - 1 - sy, height - 1 - sx),
                };
                samples.push((sy, sx, (y * target_width + x) as usize * 4));
            }
        }
        samples.sort_unstable_by_key(|&(y, x, _)| (y, x));
        let mut pixels = vec![0; (target_width * target_height * 4) as usize];
        let mut next = 0;
        for y in 0..height {
            check_current(current)?;
            let row = reader.next_row()?.ok_or(ImageDecodeError::Empty)?;
            while let Some(&(sy, sx, destination)) = samples.get(next) {
                if sy != y {
                    break;
                }
                let offset = sx as usize * channels * sample_bytes;
                let channel = |index| {
                    let offset = offset + index * sample_bytes;
                    if sample_bytes == 1 {
                        row.data()[offset]
                    } else {
                        let value =
                            u16::from_be_bytes([row.data()[offset], row.data()[offset + 1]]);
                        ((u32::from(value) + 128) / 257) as u8
                    }
                };
                let rgba = match color {
                    png::ColorType::Grayscale => [channel(0), channel(0), channel(0), 255],
                    png::ColorType::GrayscaleAlpha => {
                        [channel(0), channel(0), channel(0), channel(1)]
                    }
                    png::ColorType::Rgb => [channel(0), channel(1), channel(2), 255],
                    png::ColorType::Rgba => [channel(0), channel(1), channel(2), channel(3)],
                    png::ColorType::Indexed => return Ok(None),
                };
                pixels[destination..destination + 4].copy_from_slice(&rgba);
                next += 1;
            }
        }
        // Consume the trailing image data/CRC as well; sampled pixels alone aren't validity proof.
        if reader.next_row()?.is_some() {
            return Err(ImageDecodeError::TooLarge);
        }
        reader.finish()?;
        Ok(Some(CachedImagePreview {
            source_size,
            image: PreviewImage {
                width: target_width,
                height: target_height,
                rgba: pixels,
            },
        }))
    })();
    check_current(current)?;
    result
}

#[cfg(test)]
mod tests;
