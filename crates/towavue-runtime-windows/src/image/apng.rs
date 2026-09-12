use super::*;
use image::Pixel;

pub(super) fn decode(
    path: &Path,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
    preview: &mut ImagePreviewCallback<'_>,
    first_only: bool,
) -> Result<Vec<DecodedImageFrame>, ImageDecodeError> {
    let mut frames = Vec::new();
    read_frames(
        path,
        Some(byte_limit),
        current,
        first_only,
        false,
        &mut |width, height, rgba, delay| {
            if frames.is_empty() {
                preview(width, height, rgba);
            }
            check_current(current)?;
            frames.push(DecodedImageFrame {
                width,
                height,
                rgba: rgba.to_vec(),
                delay,
            });
            Ok(())
        },
    )?;
    Ok(frames)
}

pub(crate) fn write_frames(
    path: &Path,
    writer: &mut dyn std::io::Write,
    current: &dyn Fn() -> bool,
) -> Result<(), ImageDecodeError> {
    let result = read_frames(
        path,
        None,
        current,
        false,
        true,
        &mut |width, height, rgba, _| {
            let mut encoder = png::Encoder::new(&mut *writer, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            let mut encoder = encoder.write_header()?;
            {
                let mut stream = encoder.stream_writer()?;
                for chunk in rgba.chunks(65536) {
                    check_current(current)?;
                    std::io::Write::write_all(&mut stream, chunk)
                        .map_err(png::EncodingError::from)?;
                }
                stream.finish()?;
            }
            encoder.finish()?;
            Ok(())
        },
    );
    check_current(current)?;
    result
}

fn read_frames(
    path: &Path,
    byte_limit: Option<usize>,
    current: &dyn Fn() -> bool,
    first_only: bool,
    include_poster: bool,
    visit: &mut impl FnMut(u32, u32, &[u8], Duration) -> Result<(), ImageDecodeError>,
) -> Result<(), ImageDecodeError> {
    let mut decoder = png::Decoder::new(open(path, current)?);
    decoder.set_limits(png::Limits {
        bytes: IMAGE_BYTE_LIMIT,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let (width, height) = reader.info().size();
    let frame_count = reader
        .info()
        .animation_control
        .ok_or(ImageDecodeError::Empty)?
        .num_frames;
    let canvas_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|bytes| bytes.checked_mul(4))
        .ok_or(ImageDecodeError::TooLarge)?;
    let raw_bytes = reader
        .output_buffer_size()
        .ok_or(ImageDecodeError::TooLarge)?;
    if canvas_bytes > byte_limit.unwrap_or(IMAGE_BYTE_LIMIT)
        || canvas_bytes
            .checked_add(raw_bytes)
            .is_none_or(|bytes| bytes > IMAGE_BYTE_LIMIT)
    {
        return Err(ImageDecodeError::TooLarge);
    }
    let mut raw = vec![0; raw_bytes];
    let mut canvas = vec![0; canvas_bytes];
    let mut remaining = byte_limit;
    // The default image need not be part of the animation.
    let has_poster = reader.info().frame_control.is_none();
    let emit_poster = has_poster && include_poster;
    if has_poster && !emit_poster {
        reader.next_frame(&mut raw)?;
    }
    for index in 0..u64::from(frame_count) + u64::from(emit_poster) {
        check_current(current)?;
        if let Some(bytes) = &mut remaining {
            *bytes = bytes
                .checked_sub(canvas_bytes)
                .ok_or(ImageDecodeError::TooLarge)?;
        }
        let output = reader.next_frame(&mut raw)?;
        let control = if emit_poster && index == 0 {
            png::FrameControl {
                width,
                height,
                dispose_op: png::DisposeOp::Background,
                blend_op: png::BlendOp::Source,
                ..Default::default()
            }
        } else {
            reader.info().frame_control.ok_or(ImageDecodeError::Empty)?
        };
        let (x, y, w, h) = (
            control.x_offset as usize,
            control.y_offset as usize,
            control.width as usize,
            control.height as usize,
        );
        let stride = width as usize * 4;
        if x.checked_add(w).is_none_or(|end| end > width as usize)
            || y.checked_add(h).is_none_or(|end| end > height as usize)
            || (output.width, output.height) != (control.width, control.height)
        {
            return Err(ImageDecodeError::UnknownFormat);
        }
        let dispose =
            if index == u64::from(emit_poster) && control.dispose_op == png::DisposeOp::Previous {
                png::DisposeOp::Background
            } else {
                control.dispose_op
            };
        let mut previous = Vec::new();
        if dispose == png::DisposeOp::Previous {
            let region_bytes = w * h * 4;
            if canvas_bytes
                .checked_add(raw_bytes)
                .and_then(|bytes| bytes.checked_add(region_bytes))
                .is_none_or(|bytes| bytes > IMAGE_BYTE_LIMIT)
            {
                return Err(ImageDecodeError::TooLarge);
            }
            previous.reserve_exact(region_bytes);
            for row in y..y + h {
                check_current(current)?;
                previous
                    .extend_from_slice(&canvas[row * stride + x * 4..row * stride + (x + w) * 4]);
            }
        }
        let channels = output.color_type.samples();
        for row in 0..h {
            check_current(current)?;
            for column in 0..w {
                if column % 16384 == 0 {
                    check_current(current)?;
                }
                let start = row * output.line_size + column * channels;
                let pixel = &raw[start..start + channels];
                let rgba = match output.color_type {
                    png::ColorType::Grayscale => [pixel[0], pixel[0], pixel[0], 255],
                    png::ColorType::GrayscaleAlpha => [pixel[0], pixel[0], pixel[0], pixel[1]],
                    png::ColorType::Rgb => [pixel[0], pixel[1], pixel[2], 255],
                    png::ColorType::Rgba => [pixel[0], pixel[1], pixel[2], pixel[3]],
                    png::ColorType::Indexed => return Err(ImageDecodeError::UnknownFormat),
                };
                let offset = (y + row) * stride + (x + column) * 4;
                let target = &mut canvas[offset..offset + 4];
                if control.blend_op == png::BlendOp::Source {
                    target.copy_from_slice(&rgba);
                } else {
                    image::Rgba::from_slice_mut(target).blend(&image::Rgba(rgba));
                }
            }
        }
        check_current(current)?;
        let denominator = if control.delay_den == 0 {
            100
        } else {
            control.delay_den
        };
        let delay = Duration::from_secs_f64(f64::from(control.delay_num) / f64::from(denominator))
            .max(Duration::from_millis(10));
        visit(width, height, &canvas, delay)?;
        if first_only {
            break;
        }
        // Dispose after retaining the displayed frame, using the actual pre-blend region.
        for row in 0..h {
            check_current(current)?;
            let target = &mut canvas[(y + row) * stride + x * 4..(y + row) * stride + (x + w) * 4];
            match dispose {
                png::DisposeOp::None => break,
                png::DisposeOp::Background => target.fill(0),
                png::DisposeOp::Previous => {
                    target.copy_from_slice(&previous[row * w * 4..(row + 1) * w * 4])
                }
            }
        }
    }
    if !first_only {
        reader.finish()?;
    }
    check_current(current)?;
    Ok(())
}
