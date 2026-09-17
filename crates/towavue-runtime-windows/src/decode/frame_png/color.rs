use super::*;

// One worker owns this context from allocation through conversion. It retains
// neither borrowed frame pointers nor Rust references, and is freed on every
// success/error path. Initialization must follow chroma-position configuration;
// changing options on an already initialized scaler would leave its old filters.
struct Scaler(*mut ffmpeg::ffi::SwsContext);

impl Drop for Scaler {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner of the allocated native context.
        unsafe { ffmpeg::ffi::sws_free_context(&mut self.0) };
    }
}

pub(super) fn convert(
    source: &frame::Video,
    pixel: Pixel,
    matrix: i32,
    full: bool,
    subsampled: (bool, bool),
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<frame::Video, DecodeError> {
    let planar = match pixel {
        Pixel::RGB48BE => Pixel::GBRP16BE,
        Pixel::RGBA64BE => Pixel::GBRAP16BE,
        _ => return convert_planes(source, pixel, matrix, full, subsampled),
    };
    // The pinned scaler's packed 16-bit RGB/full-chroma path can corrupt
    // colors. Keep its full-depth planar conversion and explicitly interleave
    // samples, without another color conversion, quantization or resampling.
    let planes = convert_planes(source, planar, matrix, full, subsampled)?;
    let channels = if pixel == Pixel::RGBA64BE { 4 } else { 3 };
    let mut output = frame::Video::new(pixel, source.width(), source.height());
    let stride = output.stride(0);
    for y in 0..source.height() as usize {
        check_cancelled(cancelled)?;
        for x in 0..source.width() as usize {
            for (channel, plane) in [2, 0, 1, 3].into_iter().take(channels).enumerate() {
                let from = y * planes.stride(plane) + x * 2;
                let to = y * stride + (x * channels + channel) * 2;
                output.data_mut(0)[to..to + 2].copy_from_slice(&planes.data(plane)[from..from + 2]);
            }
        }
    }
    Ok(output)
}

fn convert_planes(
    source: &frame::Video,
    pixel: Pixel,
    matrix: i32,
    full: bool,
    subsampled: (bool, bool),
) -> Result<frame::Video, DecodeError> {
    // The legacy slice API has no frame flags and would interpolate vertical
    // chroma across temporal fields. Use the native field-aware frame API only
    // when this input actually has vertically subsampled interlaced chroma.
    // SAFETY: read one scalar from the borrowed, live decoded frame.
    if subsampled.1
        && unsafe { (*source.as_ptr()).flags & ffmpeg::ffi::AV_FRAME_FLAG_INTERLACED != 0 }
    {
        return convert_interlaced(source, pixel, full);
    }
    use ffmpeg::ffi::*;
    // SAFETY: native allocation has no borrowed inputs. The guard immediately
    // owns its result and also handles null/all initialization failure paths.
    let scaler = Scaler(unsafe { sws_alloc_context() });
    if scaler.0.is_null() {
        return Err(invalid("cannot allocate frame color converter"));
    }
    let input_format: AVPixelFormat = source.format().into();
    let output_format: AVPixelFormat = pixel.into();
    // SAFETY: exclusively owned, uninitialized context; dimensions were checked
    // against the frame-export pixel limit before this helper. The source stays
    // borrowed and alive throughout this synchronous conversion.
    unsafe {
        (*scaler.0).src_w = source.width() as i32;
        (*scaler.0).src_h = source.height() as i32;
        (*scaler.0).dst_w = source.width() as i32;
        (*scaler.0).dst_h = source.height() as i32;
        (*scaler.0).src_format = input_format as i32;
        (*scaler.0).dst_format = output_format as i32;
        (*scaler.0).flags =
            (Flags::BILINEAR | Flags::ACCURATE_RND | Flags::FULL_CHR_H_INT).bits() as u32;
        if (subsampled.0 || subsampled.1)
            && source.chroma_location() != ffmpeg::util::chroma::Location::Unspecified
        {
            let (mut horizontal, mut vertical) = (0, 0);
            let result = av_chroma_location_enum_to_pos(
                &mut horizontal,
                &mut vertical,
                source.chroma_location().into(),
            );
            if result < 0 {
                return Err(ffmpeg::Error::from(result).into());
            }
            // An axis without subsampling has no chroma offset. Passing one
            // would shift full-resolution chroma, notably vertical YUV422.
            if subsampled.0 {
                (*scaler.0).src_h_chr_pos = horizontal;
            }
            if subsampled.1 {
                (*scaler.0).src_v_chr_pos = vertical;
            }
        }
        // Unspecified siting retains the previous sws_getContext defaults.
        let result = sws_init_context(scaler.0, std::ptr::null_mut(), std::ptr::null_mut());
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
        let coefficients = sws_getCoefficients(matrix);
        let result = sws_setColorspaceDetails(
            scaler.0,
            coefficients,
            i32::from(full),
            coefficients,
            1,
            0,
            1 << 16,
            1 << 16,
        );
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
    }
    let mut output = frame::Video::new(pixel, source.width(), source.height());
    // SAFETY: valid owned FFmpeg frames supply all plane pointers and strides.
    // The scaler reads one complete source slice and writes only to the separate
    // output allocation; no pointers survive the call or escape this function.
    let rows = unsafe {
        sws_scale(
            scaler.0,
            (*source.as_ptr()).data.as_ptr().cast(),
            (*source.as_ptr()).linesize.as_ptr(),
            0,
            source.height() as i32,
            (*output.as_mut_ptr()).data.as_ptr(),
            (*output.as_mut_ptr()).linesize.as_ptr(),
        )
    };
    if rows < 0 {
        return Err(ffmpeg::Error::from(rows).into());
    }
    if rows != source.height() as i32 {
        return Err(invalid("incomplete frame color conversion"));
    }
    Ok(output)
}

fn convert_interlaced(
    source: &frame::Video,
    pixel: Pixel,
    full: bool,
) -> Result<frame::Video, DecodeError> {
    use ffmpeg::ffi::*;
    // SAFETY: the guard owns the allocation even on initialization failure.
    let scaler = Scaler(unsafe { sws_alloc_context() });
    if scaler.0.is_null() {
        return Err(invalid("cannot allocate interlaced frame color converter"));
    }
    let mut input = frame::Video::empty();
    // SAFETY: retain the borrowed pixels in a separate frame header. Changes to
    // scalar metadata below must not change the decoder's source frame.
    let result = unsafe { av_frame_ref(input.as_mut_ptr(), source.as_ptr()) };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    if matches!(
        input.color_space(),
        ffmpeg::color::Space::RGB | ffmpeg::color::Space::Unspecified
    ) {
        input.set_color_space(ffmpeg::color::Space::SMPTE170M);
    }
    input.set_color_range(if full {
        ffmpeg::color::Range::JPEG
    } else {
        ffmpeg::color::Range::MPEG
    });
    let mut output = frame::Video::new(pixel, source.width(), source.height());
    // SAFETY: copy only properties; output owns distinct writable pixels. Equal
    // primaries/transfer/alpha mode avoid adding tone mapping or alpha conversion.
    // Both headers retain interlaced flags so native conversion processes each
    // field separately and writes back to its original alternating rows.
    let result = unsafe { av_frame_copy_props(output.as_mut_ptr(), input.as_ptr()) };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    output.set_color_space(ffmpeg::color::Space::RGB);
    output.set_color_range(ffmpeg::color::Range::JPEG);
    // SAFETY: the dynamic API consumes frame properties, including field-specific
    // chroma siting. Do not call sws_init_context (legacy mode), which loses this
    // behavior. Frames and the sole-owned context remain alive until it returns;
    // this synchronous native call does not retain Rust references.
    let result = unsafe {
        (*scaler.0).flags =
            (Flags::BILINEAR | Flags::ACCURATE_RND | Flags::FULL_CHR_H_INT).bits() as u32;
        sws_scale_frame(scaler.0, output.as_mut_ptr(), input.as_ptr())
    };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    // SAFETY: these are private property copies, not the borrowed source's array.
    // The caller attaches the original metadata after orientation; do not leave
    // a second copy of each side-data item on the unrotated return path.
    unsafe {
        let header = &mut *output.as_mut_ptr();
        av_frame_side_data_free(&mut header.side_data, &mut header.nb_side_data);
    }
    Ok(output)
}
