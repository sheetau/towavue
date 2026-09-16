use super::*;
use towavue_core::{MediaKind, ResampleFilter, VideoRotation};

pub(super) fn apply(
    mut source: frame::Video,
    operations: &[EditOperation],
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<frame::Video, DecodeError> {
    for operation in operations {
        check_cancelled(cancelled)?;
        let size = (source.width(), source.height());
        let mut aspect = source.aspect_ratio();
        let (output_size, filters) = match *operation {
            EditOperation::Crop(crop) => {
                if crop.width == 0
                    || crop.height == 0
                    || crop.x.checked_add(crop.width).is_none_or(|x| x > size.0)
                    || crop.y.checked_add(crop.height).is_none_or(|y| y > size.1)
                {
                    return Err(invalid("crop exceeds the current frame"));
                }
                (
                    (crop.width, crop.height),
                    format!(
                        "crop={}:{}:{}:{}:exact=1",
                        crop.width, crop.height, crop.x, crop.y
                    ),
                )
            }
            EditOperation::RotateClockwise | EditOperation::RotateCounterclockwise => {
                aspect = Rational(aspect.denominator(), aspect.numerator());
                (
                    (size.1, size.0),
                    if *operation == EditOperation::RotateClockwise {
                        "transpose=clock".into()
                    } else {
                        "transpose=cclock".into()
                    },
                )
            }
            EditOperation::FlipHorizontal => (size, "hflip".into()),
            EditOperation::FlipVertical => (size, "vflip".into()),
            EditOperation::ResizeVideo(resize) if !resize.is_identity() => {
                validate_source(&source, resize.source_size(), resize.source_pixel_aspect())?;
                if resize.filter() == ResampleFilter::Nearest {
                    source = nearest(&source, resize.size(), cancelled)?;
                    continue;
                }
                aspect = Rational(1, 1);
                (
                    resize.size(),
                    scale_filter(source.format(), resize.size(), resize.filter()),
                )
            }
            EditOperation::RotateVideo(rotation) if rotation.tenths() != 0 => {
                validate_source(
                    &source,
                    rotation.source_size(),
                    rotation.source_pixel_aspect(),
                )?;
                validate_size(rotation.square_size())?;
                validate_size(rotation.size())?;
                if size != rotation.square_size() {
                    source = filtered(
                        &source,
                        rotation.square_size(),
                        Rational(1, 1),
                        &scale_filter(
                            source.format(),
                            rotation.square_size(),
                            ResampleFilter::Bilinear,
                        ),
                        cancelled,
                    )?;
                }
                // Native rotate does not accept 16-bit RGB. Keep packed samples
                // at source precision instead of negotiating 8-bit or YUV.
                source = rotate(&source, rotation, cancelled)?;
                continue;
            }
            operation if !operation.applies_to(MediaKind::Video) => {
                return Err(invalid("image-only operation in video frame export"));
            }
            _ => continue,
        };
        source = filtered(&source, output_size, aspect, &filters, cancelled)?;
    }
    Ok(source)
}

fn validate_size(size: (u32, u32)) -> Result<(), DecodeError> {
    if size.0 == 0 || size.1 == 0 || u64::from(size.0) * u64::from(size.1) > MAX_PIXELS {
        return Err(DecodeError::FrameTooLarge);
    }
    Ok(())
}

fn validate_source(
    source: &frame::Video,
    size: (u32, u32),
    aspect: f32,
) -> Result<(), DecodeError> {
    let actual = f64::from(source.aspect_ratio()) as f32;
    if (source.width(), source.height()) != size || (actual - aspect).abs() > actual.abs() * 0.00001
    {
        return Err(invalid(
            "edit source geometry does not match the current frame",
        ));
    }
    Ok(())
}

fn packed(pixel: Pixel) -> (&'static str, usize, usize) {
    match pixel {
        Pixel::RGB24 => ("rgb24", 3, 1),
        Pixel::RGBA => ("rgba", 4, 1),
        Pixel::RGB48BE => ("rgb48be", 3, 2),
        Pixel::RGBA64BE => ("rgba64be", 4, 2),
        _ => unreachable!("frame PNG edits receive only converted packed RGB(A)"),
    }
}

fn scale_filter(pixel: Pixel, size: (u32, u32), filter: ResampleFilter) -> String {
    let (_, channels, sample_bytes) = packed(pixel);
    let flags = match filter {
        ResampleFilter::Nearest => unreachable!("nearest copies packed pixels directly"),
        ResampleFilter::Bilinear => "bilinear",
        ResampleFilter::Bicubic => "bicubic",
        ResampleFilter::Lanczos => "lanczos",
    };
    let planar = match (channels == 4, sample_bytes == 2) {
        (false, false) => "gbrp",
        (false, true) => "gbrp16le",
        (true, _) => "gbrap16le",
    };
    let scale = format!("scale={}:{}:flags={flags}+full_chroma_inp", size.0, size.1);
    if channels == 4 {
        format!(
            "format={planar},premultiply=inplace=1,{scale},format={planar},unpremultiply=inplace=1"
        )
    } else {
        format!("format={planar},{scale},format={planar}")
    }
}

fn nearest(
    source: &frame::Video,
    size: (u32, u32),
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<frame::Video, DecodeError> {
    validate_size(size)?;
    let (_, channels, sample_bytes) = packed(source.format());
    let bytes = channels * sample_bytes;
    let mut output = frame::Video::new(source.format(), size.0, size.1);
    let stride = output.stride(0);
    let step = |source: u32, target: u32| {
        ((u64::from(source) << 16) + u64::from(target / 2)) / u64::from(target)
    };
    let (dx, dy) = (step(source.width(), size.0), step(source.height(), size.1));
    // Pixel-center selection without FFmpeg's internal color conversion or
    // alpha rounding: nearest must copy the chosen sample exactly at any depth.
    // Retain the 16-bit center increment used by video_resample::FilterSpec.
    for y in 0..size.1 as usize {
        check_cancelled(cancelled)?;
        let sy = (((2 * y as u64 + 1) * dy / 131072) as usize).min(source.height() as usize - 1);
        for x in 0..size.0 as usize {
            let sx = (((2 * x as u64 + 1) * dx / 131072) as usize).min(source.width() as usize - 1);
            let from = sy * source.stride(0) + sx * bytes;
            let to = y * stride + x * bytes;
            output.data_mut(0)[to..to + bytes].copy_from_slice(&source.data(0)[from..from + bytes]);
        }
    }
    copy_properties(source, &mut output, Rational(1, 1))?;
    Ok(output)
}

fn copy_properties(
    source: &frame::Video,
    output: &mut frame::Video,
    aspect: Rational,
) -> Result<(), DecodeError> {
    // SAFETY: independently owned AVFrames; copy_props retains metadata buffers
    // without changing output geometry/storage. Only output is mutated.
    let result = unsafe {
        let result = ffmpeg::ffi::av_frame_copy_props(output.as_mut_ptr(), source.as_ptr());
        (*output.as_mut_ptr()).sample_aspect_ratio = aspect.into();
        result
    };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    Ok(())
}

fn filtered(
    source: &frame::Video,
    size: (u32, u32),
    aspect: Rational,
    filters: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<frame::Video, DecodeError> {
    validate_size(size)?;
    check_cancelled(cancelled)?;
    let mut graph = ffmpeg::filter::Graph::new();
    // SAFETY: exclusively owned, not yet configured; no filter thread exists.
    unsafe {
        (*graph.as_mut_ptr()).nb_threads = 1;
    }
    let (format, _, _) = packed(source.format());
    // SAFETY: scalar metadata from the borrowed input; configure the graph to
    // match it before submitting KEEP_REF frames, without a mid-stream change.
    let alpha_mode = unsafe { (*source.as_ptr()).alpha_mode } as i32;
    graph.add(
        &ffmpeg::filter::find("buffer").ok_or(ffmpeg::Error::FilterNotFound)?,
        "in",
        &format!(
            "video_size={}x{}:pix_fmt={format}:time_base=1/1:pixel_aspect={}/{}:colorspace=0:range=2:alpha_mode={alpha_mode}",
            source.width(),
            source.height(),
            source.aspect_ratio().numerator(),
            source.aspect_ratio().denominator()
        ),
    )?;
    graph.add(
        &ffmpeg::filter::find("buffersink").ok_or(ffmpeg::Error::FilterNotFound)?,
        "out",
        "",
    )?;
    // copy materializes crop/vflip views with an owned positive stride.
    graph
        .output("in", 0)?
        .input("out", 0)?
        .parse(&format!("{filters},format={format},copy"))?;
    graph.validate()?;
    // SAFETY: the graph is exclusively owned and KEEP_REF retains independent
    // buffer references. Unlike Source::add, this does not consume/reset the
    // borrowed AVFrame, whose format and color properties are still needed.
    let result = unsafe {
        ffmpeg::ffi::av_buffersrc_add_frame_flags(
            graph.get("in").expect("frame edit source").as_mut_ptr(),
            source.as_ptr().cast_mut(),
            ffmpeg::ffi::AV_BUFFERSRC_FLAG_KEEP_REF as i32,
        )
    };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    let mut output = frame::Video::empty();
    graph
        .get("out")
        .expect("frame edit sink")
        .sink()
        .frame(&mut output)?;
    check_cancelled(cancelled)?;
    if (output.width(), output.height()) != size || output.format() != source.format() {
        return Err(invalid(&format!(
            "unexpected edited frame: {}x{} {:?}, expected {}x{} {:?}",
            output.width(),
            output.height(),
            output.format(),
            size.0,
            size.1,
            source.format()
        )));
    }
    copy_properties(source, &mut output, aspect)?;
    Ok(output)
}

fn rotate(
    source: &frame::Video,
    rotation: VideoRotation,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<frame::Video, DecodeError> {
    let size = rotation.size();
    let raster = rotation.raster_size();
    let (_, channels, sample_bytes) = packed(source.format());
    let bytes = channels * sample_bytes;
    let max = if sample_bytes == 1 { 255.0 } else { 65535.0 };
    let mut output = frame::Video::new(source.format(), size.0, size.1);
    let (sin, cos) = (f64::from(rotation.tenths()) * std::f64::consts::PI / 1800.0).sin_cos();
    let read = |x: usize, y: usize, channel: usize| -> f64 {
        let offset = y * source.stride(0) + x * bytes + channel * sample_bytes;
        if sample_bytes == 1 {
            f64::from(source.data(0)[offset])
        } else {
            f64::from(u16::from_be_bytes([
                source.data(0)[offset],
                source.data(0)[offset + 1],
            ]))
        }
    };
    let stride = output.stride(0);
    for y in 0..size.1 as usize {
        check_cancelled(cancelled)?;
        for x in 0..size.0 as usize {
            let mut value = [0.0, 0.0, 0.0, max];
            if x < raster.0 as usize && y < raster.1 as usize {
                let exact = match rotation.tenths() {
                    900 => Some((y, source.height() as usize - 1 - x)),
                    -900 => Some((source.width() as usize - 1 - y, x)),
                    -1800 | 1800 => Some((
                        source.width() as usize - 1 - x,
                        source.height() as usize - 1 - y,
                    )),
                    _ => None,
                };
                if let Some((sx, sy)) = exact {
                    // Orthogonal edits preserve hidden transparent RGB too.
                    let offset = sy * source.stride(0) + sx * bytes;
                    output.data_mut(0)[y * stride + x * bytes..y * stride + (x + 1) * bytes]
                        .copy_from_slice(&source.data(0)[offset..offset + bytes]);
                    continue;
                }
                let dx = x as f64 - f64::from(raster.0 - 1) * 0.5;
                let dy = y as f64 - f64::from(raster.1 - 1) * 0.5;
                let sx = f64::from(source.width() - 1) * 0.5 + dx * cos + dy * sin;
                let sy = f64::from(source.height() - 1) * 0.5 - dx * sin + dy * cos;
                let (bx, by) = (sx.floor(), sy.floor());
                // Match the existing video rotation's pixel-center/border rule:
                // clamp integer neighbors, retaining the fractional position.
                if bx >= -1.0
                    && by >= -1.0
                    && bx <= f64::from(source.width())
                    && by <= f64::from(source.height())
                {
                    let x0 = bx.clamp(0.0, f64::from(source.width() - 1)) as usize;
                    let y0 = by.clamp(0.0, f64::from(source.height() - 1)) as usize;
                    let x1 = (x0 + 1).min(source.width() as usize - 1);
                    let y1 = (y0 + 1).min(source.height() as usize - 1);
                    let (fx, fy) = (sx - bx, sy - by);
                    value = [0.0; 4];
                    for (px, py, weight) in [
                        (x0, y0, (1.0 - fx) * (1.0 - fy)),
                        (x1, y0, fx * (1.0 - fy)),
                        (x0, y1, (1.0 - fx) * fy),
                        (x1, y1, fx * fy),
                    ] {
                        let alpha = if channels == 4 {
                            read(px, py, 3) / max
                        } else {
                            1.0
                        };
                        for (channel, result) in value[..3].iter_mut().enumerate() {
                            *result += weight * alpha * read(px, py, channel);
                        }
                        value[3] += weight * alpha * max;
                    }
                    if channels == 4 && value[3] > 0.0 {
                        for channel in 0..3 {
                            value[channel] *= max / value[3];
                        }
                    }
                }
            }
            for (channel, value) in value[..channels].iter().enumerate() {
                let offset = y * stride + x * bytes + channel * sample_bytes;
                let value = value.round().clamp(0.0, max) as u16;
                if sample_bytes == 1 {
                    output.data_mut(0)[offset] = value as u8;
                } else {
                    output.data_mut(0)[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
                }
            }
        }
    }
    copy_properties(source, &mut output, Rational(1, 1))?;
    Ok(output)
}
