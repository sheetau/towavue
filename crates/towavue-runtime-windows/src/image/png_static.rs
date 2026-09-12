use image::metadata::Orientation;

use super::*;

/// Decode static PNG rows directly into the retained RGBA8 canvas. APNG keeps
/// its separate compositor; Adam7 needs the decoder's full raw interlace buffer.
pub(super) fn decode(
    input: impl std::io::BufRead + Seek,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
) -> Result<Option<DecodedImageFrame>, ImageDecodeError> {
    check_current(current)?;
    let mut decoder = png::Decoder::new_with_limits(
        input,
        png::Limits {
            bytes: IMAGE_BYTE_LIMIT,
        },
    );
    decoder.set_ignore_text_chunk(false);
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    if info.animation_control.is_some() {
        return Ok(None);
    }
    let (width, height) = (info.width, info.height);
    let length = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(ImageDecodeError::TooLarge)?;
    if length > byte_limit.min(IMAGE_BYTE_LIMIT) as u64 {
        return Err(ImageDecodeError::TooLarge);
    }
    let orientation = info
        .exif_metadata
        .as_deref()
        .and_then(Orientation::from_exif_chunk)
        .unwrap_or(Orientation::NoTransforms);
    let interlaced = info.interlaced;
    let (color, depth) = reader.output_color_type();
    let mut rgba = vec![0; length as usize];
    if interlaced {
        let length = reader
            .output_buffer_size()
            .filter(|&size| size <= IMAGE_BYTE_LIMIT)
            .ok_or(ImageDecodeError::TooLarge)?;
        let mut raw = vec![0; length];
        let output = reader.next_frame(&mut raw)?;
        for (source, target) in raw[..output.buffer_size()]
            .chunks_exact(output.line_size)
            .zip(rgba.chunks_exact_mut(width as usize * 4))
        {
            check_current(current)?;
            rgba_row(source, color, depth, target);
        }
    } else {
        for target in rgba.chunks_exact_mut(width as usize * 4) {
            check_current(current)?;
            let row = reader.next_row()?.ok_or(ImageDecodeError::Empty)?;
            rgba_row(row.data(), color, depth, target);
        }
        if reader.next_row()?.is_some() {
            return Err(ImageDecodeError::TooLarge);
        }
    }
    reader.finish()?;
    check_current(current)?;
    let mut image = DynamicImage::ImageRgba8(
        image::RgbaImage::from_raw(width, height, rgba).expect("packed PNG canvas"),
    );
    image.apply_orientation(orientation);
    check_current(current)?;
    let image = image.into_rgba8();
    Ok(Some(DecodedImageFrame {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        delay: Duration::ZERO,
    }))
}

#[cfg(test)]
mod tests;

fn rgba_row(source: &[u8], color: png::ColorType, depth: png::BitDepth, target: &mut [u8]) {
    if depth == png::BitDepth::Eight && color == png::ColorType::Rgba {
        target.copy_from_slice(source);
        return;
    }
    if depth == png::BitDepth::Eight && color == png::ColorType::Rgb {
        for (rgb, rgba) in source
            .as_chunks::<3>()
            .0
            .iter()
            .zip(target.as_chunks_mut::<4>().0.iter_mut())
        {
            *rgba = [rgb[0], rgb[1], rgb[2], 255];
        }
        return;
    }
    let sample_bytes = if depth == png::BitDepth::Sixteen {
        2
    } else {
        1
    };
    for (source, rgba) in source
        .chunks_exact(color.samples() * sample_bytes)
        .zip(target.as_chunks_mut::<4>().0.iter_mut())
    {
        let channel = |index| {
            if sample_bytes == 1 {
                source[index]
            } else {
                let value = u16::from_be_bytes([source[index * 2], source[index * 2 + 1]]);
                ((u32::from(value) + 128) / 257) as u8
            }
        };
        *rgba = match color {
            png::ColorType::Grayscale => [channel(0), channel(0), channel(0), 255],
            png::ColorType::GrayscaleAlpha => [channel(0), channel(0), channel(0), channel(1)],
            png::ColorType::Rgb => [channel(0), channel(1), channel(2), 255],
            png::ColorType::Rgba => [channel(0), channel(1), channel(2), channel(3)],
            png::ColorType::Indexed => unreachable!("EXPAND resolves palette pixels"),
        };
    }
}
