use super::*;
use towavue_core::EditOperation;

mod color;
mod edits;
mod metadata;
#[cfg(test)]
pub(crate) mod verification;

const MAX_PIXELS: u64 = 64 * 1024 * 1024;

fn invalid(message: &str) -> DecodeError {
    DecodeError::FrameImage(message.into())
}

/// Decode an original-source PTS into an original-resolution PNG on a worker.
/// No UI scaling or user edits are applied. Orthogonal source orientation and
/// pixel aspect are retained. PNG is lossless after YUV-to-RGB conversion, not
/// a reversible representation of arbitrary YUV samples. Float/>16bit input is
/// rejected. The caller must validate source identity before publishing bytes.
pub fn source_video_frame_png(
    path: &Path,
    target: MediaTime,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<u8>, DecodeError> {
    edited_video_frame_png(path, target, &[], cancelled)
}

/// Decode an original-source PTS and apply ordered video raster edits before PNG
/// encoding. Time/audio edits do not modify the selected picture. Display zoom
/// is excluded. Source identity must be validated by the publishing caller.
pub fn edited_video_frame_png(
    path: &Path,
    target: MediaTime,
    operations: &[EditOperation],
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<u8>, DecodeError> {
    check_cancelled(cancelled)?;
    ffmpeg::init()?;
    let mut input = format::input(path)?;
    let config = best_stream_config(&input, Type::Video).ok_or(DecodeError::NoMediaStream)?;
    let orientation = VideoOrientation::from_bytes(config.display_matrix.as_deref())?;
    let mut options = ffmpeg::Dictionary::new();
    options.set("max_pixels", &MAX_PIXELS.to_string());
    options.set("err_detect", "explode");
    // WebM carries VP8/VP9 alpha in Matroska BlockAdditional packets. The
    // default native decoders ignore that plane; the bundled libvpx decoders
    // consume it. Scope this choice to the selected stream's alpha declaration.
    let alpha_decoder = match config.parameters.id() {
        codec::Id::VP8 => Some("libvpx"),
        codec::Id::VP9 => Some("libvpx-vp9"),
        _ => None,
    }
    .filter(|_| {
        input
            .stream(config.index)
            .is_some_and(|stream| stream.metadata().get("alpha_mode") == Some("1"))
    });
    let codec = if let Some(name) = alpha_decoder {
        codec::decoder::find_by_name(name)
            .ok_or_else(|| invalid("alpha-capable WebM decoder is unavailable"))?
    } else {
        codec::decoder::find(config.parameters.id()).ok_or(ffmpeg::Error::DecoderNotFound)?
    };
    let context = codec::context::Context::from_parameters(config.parameters)?;
    let mut decoder = context.decoder().open_as_with(codec, options)?.video()?;
    let origin = input_origin(&input);
    // Begin before the target tick, including when it is a keyframe, so equal
    // adjacent PTS cannot silently select one of several different pictures.
    seek_video_stream(
        &mut input,
        config.index,
        config.time_base,
        origin,
        MediaTime::from_nanoseconds(target.as_nanoseconds().saturating_sub(1)),
        cancelled,
    )?;
    let mut selected = None;
    let mut previous = None;
    let mut receive = |decoder: &mut codec::decoder::Video| -> Result<bool, DecodeError> {
        loop {
            check_cancelled(cancelled)?;
            let mut decoded = frame::Video::empty();
            match decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let timestamp = decoded
                        .timestamp()
                        .ok_or(DecodeError::MissingVideoTimestamp)?;
                    let time = timestamp_to_media_time(Some(timestamp), config.time_base);
                    if previous.is_some_and(|previous| time < previous) {
                        return Err(invalid("nonmonotonic source frame timestamps"));
                    }
                    previous = Some(time);
                    if time > target {
                        return Ok(true);
                    }
                    if time == target {
                        if selected.is_some() {
                            return Err(invalid("ambiguous duplicate frame timestamp"));
                        }
                        selected = Some(decoded);
                    }
                }
                Err(error) if decoder_is_drained(error) => return Ok(false),
                Err(error) => return Err(error.into()),
            }
        }
    };
    let mut finished = false;
    let mut started = false;
    for (stream, mut packet) in input.packets() {
        check_cancelled(cancelled)?;
        if stream.index() == config.index {
            normalize_packet_time(&mut packet, stream.time_base(), origin);
            if !started
                && !packet.is_key()
                && packet.pts().is_some_and(|pts| {
                    timestamp_to_media_time(Some(pts), config.time_base) < target
                })
            {
                // Matroska may resume at a non-key packet sharing the indexed
                // keyframe's timestamp. A fresh strict decoder cannot use that
                // leading preroll. Skip only known pre-target packets; never
                // discard a target-time packet and hide an ambiguous picture.
                continue;
            }
            started = true;
            decoder.send_packet(&packet)?;
            if receive(&mut decoder)? {
                finished = true;
                break;
            }
        }
    }
    if !finished {
        decoder.send_eof()?;
        receive(&mut decoder)?;
    }
    let selected = selected.ok_or_else(|| invalid("no frame at the requested source timestamp"))?;
    if alpha_decoder.is_some() {
        // SAFETY: immutable FFmpeg descriptor for the selected owned frame;
        // no pointer is retained. Do not silently publish an opaque substitute
        // when the container promises an alpha plane that decoding did not yield.
        let has_alpha = unsafe {
            ffmpeg::ffi::av_pix_fmt_desc_get(selected.format().into())
                .as_ref()
                .is_some_and(|desc| desc.flags & ffmpeg::ffi::AV_PIX_FMT_FLAG_ALPHA as u64 != 0)
        };
        if !has_alpha {
            return Err(invalid("declared WebM alpha plane was not decoded"));
        }
    }
    encode(&selected, orientation, operations, cancelled)
}

fn encode(
    source: &frame::Video,
    orientation: VideoOrientation,
    operations: &[EditOperation],
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<u8>, DecodeError> {
    check_cancelled(cancelled)?;
    let orientation = frame_orientation(source, orientation)?;
    let (width, height) = (source.width(), source.height());
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(DecodeError::FrameTooLarge);
    }
    // SAFETY: the descriptor is immutable FFmpeg storage for the frame's format;
    // no pointer escapes this call and the borrowed source is never mutated.
    let descriptor = unsafe { ffmpeg::ffi::av_pix_fmt_desc_get(source.format().into()).as_ref() }
        .ok_or_else(|| invalid("unknown source pixel format"))?;
    let depth = descriptor.comp[..usize::from(descriptor.nb_components)]
        .iter()
        .map(|component| component.depth)
        .max()
        .unwrap_or(0);
    if !(1..=16).contains(&depth)
        || descriptor.flags
            & (ffmpeg::ffi::AV_PIX_FMT_FLAG_FLOAT | ffmpeg::ffi::AV_PIX_FMT_FLAG_HWACCEL) as u64
            != 0
    {
        return Err(invalid("PNG requires integer samples of at most 16 bits"));
    }
    let alpha = descriptor.flags & ffmpeg::ffi::AV_PIX_FMT_FLAG_ALPHA as u64 != 0;
    let grayscale = descriptor.nb_components <= 2
        && descriptor.flags
            & (ffmpeg::ffi::AV_PIX_FMT_FLAG_RGB | ffmpeg::ffi::AV_PIX_FMT_FLAG_PAL) as u64
            == 0;
    let (pixel, bytes) = match (depth > 8, alpha) {
        (false, false) => (Pixel::RGB24, 3),
        (false, true) => (Pixel::RGBA, 4),
        (true, false) => (Pixel::RGB48BE, 6),
        (true, true) => (Pixel::RGBA64BE, 8),
    };
    use ffmpeg::ffi::AVColorSpace::*;
    let matrix = match source.color_space().into() {
        AVCOL_SPC_RGB | AVCOL_SPC_UNSPECIFIED | AVCOL_SPC_BT470BG | AVCOL_SPC_SMPTE170M => {
            ffmpeg::ffi::SWS_CS_ITU601
        }
        AVCOL_SPC_BT709 => ffmpeg::ffi::SWS_CS_ITU709,
        AVCOL_SPC_FCC => ffmpeg::ffi::SWS_CS_FCC,
        AVCOL_SPC_SMPTE240M => ffmpeg::ffi::SWS_CS_SMPTE240M,
        AVCOL_SPC_BT2020_NCL => ffmpeg::ffi::SWS_CS_BT2020,
        _ => return Err(invalid("unsupported source color matrix")),
    };
    let full = descriptor.flags & ffmpeg::ffi::AV_PIX_FMT_FLAG_RGB as u64 != 0
        || (descriptor.nb_components <= 2 && source.color_range() != ffmpeg::color::Range::MPEG)
        || source.color_range() == ffmpeg::color::Range::JPEG;
    let yuv = descriptor.nb_components >= 3
        && descriptor.flags & ffmpeg::ffi::AV_PIX_FMT_FLAG_RGB as u64 == 0;
    let subsampled = (
        yuv && descriptor.log2_chroma_w != 0,
        yuv && descriptor.log2_chroma_h != 0,
    );
    let mut rgb = color::convert(source, pixel, matrix, full, subsampled)?;
    // SAFETY: immutable scalar metadata on the borrowed decoded frame. Legacy
    // sws_scale converts channels/ranges but does not interpret AVFrame alpha_mode.
    let premultiplied = alpha
        && unsafe { (*source.as_ptr()).alpha_mode }
            == ffmpeg::ffi::AVAlphaMode::AVALPHA_MODE_PREMULTIPLIED;
    if premultiplied {
        restore_straight_alpha(&mut rgb, depth > 8, cancelled)?;
    }
    let mut output = orient(rgb, orientation, bytes, cancelled)?;
    // Preserve color/ICC/HDR frame properties without carrying codec pixels.
    // SAFETY: both AVFrames are owned in this call; copy_props retains side-data
    // references. Geometry/format/data remain those of the newly converted frame.
    let result = unsafe { ffmpeg::ffi::av_frame_copy_props(output.as_mut_ptr(), source.as_ptr()) };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    output.set_color_space(ffmpeg::color::Space::RGB);
    output.set_color_range(ffmpeg::color::Range::JPEG);
    let aspect = source.aspect_ratio();
    let aspect = if aspect.numerator() <= 0 || aspect.denominator() <= 0 {
        Rational(1, 1)
    } else if orientation.swaps_axes() {
        Rational(aspect.denominator(), aspect.numerator())
    } else {
        aspect
    };
    // SAFETY: exclusive output ownership; orientation is already baked into its
    // pixels. Remove its old display matrix to prevent another transformation.
    unsafe {
        (*output.as_mut_ptr()).sample_aspect_ratio = aspect.into();
        if premultiplied {
            // copy_props above copied the source interpretation as well. Edits
            // and PNG must now see straight samples, including before interpolation.
            (*output.as_mut_ptr()).alpha_mode = ffmpeg::ffi::AVAlphaMode::AVALPHA_MODE_STRAIGHT;
        }
        ffmpeg::ffi::av_frame_remove_side_data(
            output.as_mut_ptr(),
            ffmpeg::ffi::AVFrameSideDataType::AV_FRAME_DATA_DISPLAYMATRIX,
        );
    }
    output.set_pts(Some(0));
    let output = edits::apply(output, operations, cancelled)?;
    let mut output = if grayscale {
        pack_grayscale(output, depth > 8, alpha, cancelled)?
    } else {
        output
    };
    check_cancelled(cancelled)?;
    metadata::remove_thumbnail(&mut output)?;
    let aspect = output.aspect_ratio();
    let codec = ffmpeg::encoder::find(codec::Id::PNG).ok_or(ffmpeg::Error::EncoderNotFound)?;
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;
    encoder.set_width(output.width());
    encoder.set_height(output.height());
    encoder.set_format(output.format());
    encoder.set_time_base(Rational(1, 1));
    // The pinned PNG encoder writes this pair directly into pHYs. PNG stores
    // pixels per unit, whose ratio is the inverse of sample width/height.
    encoder.set_aspect_ratio(Rational(aspect.denominator(), aspect.numerator()));
    encoder.set_colorspace(output.color_space());
    encoder.set_color_range(output.color_range());
    encoder.set_color_primaries(output.color_primaries());
    encoder.set_color_transfer_characteristic(output.color_transfer_characteristic());
    let mut encoder = encoder.open_as(codec)?;
    check_cancelled(cancelled)?;
    #[cfg(test)]
    verification::observe(None);
    encoder.send_frame(&output)?;
    encoder.send_eof()?;
    let mut packet = ffmpeg::Packet::empty();
    encoder.receive_packet(&mut packet)?;
    #[cfg(test)]
    verification::observe(Some(packet.size()));
    check_cancelled(cancelled)?;
    Ok(packet
        .data()
        .ok_or_else(|| invalid("empty PNG packet"))?
        .to_vec())
}

fn restore_straight_alpha(
    frame: &mut frame::Video,
    high_depth: bool,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(), DecodeError> {
    let sample_bytes = if high_depth { 2 } else { 1 };
    let maximum = if high_depth { 65535_u32 } else { 255 };
    let width = frame.width() as usize;
    let stride = frame.stride(0);
    for y in 0..frame.height() as usize {
        check_cancelled(cancelled)?;
        let row = &mut frame.data_mut(0)[y * stride..y * stride + width * 4 * sample_bytes];
        for pixel in row.chunks_exact_mut(4 * sample_bytes) {
            let alpha = if high_depth {
                u32::from(u16::from_be_bytes([pixel[6], pixel[7]]))
            } else {
                u32::from(pixel[3])
            };
            for channel in pixel[..3 * sample_bytes].chunks_exact_mut(sample_bytes) {
                let value = if high_depth {
                    u32::from(u16::from_be_bytes([channel[0], channel[1]]))
                } else {
                    u32::from(channel[0])
                };
                // Round to the nearest representable straight sample; the
                // original multiplication's quantization cannot be reversed.
                // Alpha zero has no recoverable color. Clamp malformed over-alpha
                // values; the largest 16-bit product plus half-alpha fits u32.
                let straight = (value * maximum + alpha / 2)
                    .checked_div(alpha)
                    .unwrap_or(0)
                    .min(maximum);
                if high_depth {
                    channel.copy_from_slice(&(straight as u16).to_be_bytes());
                } else {
                    channel[0] = straight as u8;
                }
            }
        }
    }
    Ok(())
}

fn pack_grayscale(
    source: frame::Video,
    high_depth: bool,
    alpha: bool,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<frame::Video, DecodeError> {
    let pixel = match (high_depth, alpha) {
        (false, false) => Pixel::GRAY8,
        (false, true) => Pixel::YA8,
        (true, false) => Pixel::GRAY16BE,
        (true, true) => Pixel::YA16BE,
    };
    let sample = if high_depth { 2 } else { 1 };
    let input_bytes = sample * if alpha { 4 } else { 3 };
    let output_bytes = sample * if alpha { 2 } else { 1 };
    let mut output = frame::Video::new(pixel, source.width(), source.height());
    let stride = output.stride(0);
    // Grayscale starts as equal RGB channels; raster edits remain achromatic
    // except for native interpolation's per-channel rounding. Repack the first
    // full-depth channel without another matrix/rounding pass; retain alpha and
    // hidden samples for orthogonal/nearest edits, as on the RGB path.
    // PNG's color type must also agree with a retained GRAY ICC profile.
    for y in 0..source.height() as usize {
        check_cancelled(cancelled)?;
        for x in 0..source.width() as usize {
            let from = y * source.stride(0) + x * input_bytes;
            let to = y * stride + x * output_bytes;
            output.data_mut(0)[to..to + sample]
                .copy_from_slice(&source.data(0)[from..from + sample]);
            if alpha {
                output.data_mut(0)[to + sample..to + output_bytes]
                    .copy_from_slice(&source.data(0)[from + 3 * sample..from + 4 * sample]);
            }
        }
    }
    // SAFETY: both frames are owned here. Copying properties retains side-data
    // references without changing the new grayscale format, geometry or pixels.
    let result = unsafe { ffmpeg::ffi::av_frame_copy_props(output.as_mut_ptr(), source.as_ptr()) };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    Ok(output)
}

fn orient(
    source: frame::Video,
    orientation: VideoOrientation,
    bytes: usize,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<frame::Video, DecodeError> {
    if orientation == VideoOrientation::default() {
        return Ok(source);
    }
    let (width, height) = if orientation.swaps_axes() {
        (source.height(), source.width())
    } else {
        (source.width(), source.height())
    };
    let mut output = frame::Video::new(source.format(), width, height);
    let uv = orientation.source_uv();
    let origin = (
        uv[0].x as i64 * i64::from(source.width() - 1),
        uv[0].y as i64 * i64::from(source.height() - 1),
    );
    let dx = ((uv[1].x - uv[0].x) as i64, (uv[1].y - uv[0].y) as i64);
    let dy = ((uv[3].x - uv[0].x) as i64, (uv[3].y - uv[0].y) as i64);
    let stride = output.stride(0);
    for y in 0..height as usize {
        check_cancelled(cancelled)?;
        for x in 0..width as usize {
            let sx = (origin.0 + dx.0 * x as i64 + dy.0 * y as i64) as usize;
            let sy = (origin.1 + dx.1 * x as i64 + dy.1 * y as i64) as usize;
            let offset = sy * source.stride(0) + sx * bytes;
            output.data_mut(0)[y * stride + x * bytes..y * stride + (x + 1) * bytes]
                .copy_from_slice(&source.data(0)[offset..offset + bytes]);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
