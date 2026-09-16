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
) -> Result<frame::Video, DecodeError> {
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
