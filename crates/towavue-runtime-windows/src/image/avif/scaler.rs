use super::*;
use ffmpeg::ffi::*;

#[derive(Clone, Copy, PartialEq)]
struct Key {
    input: Pixel,
    output: Pixel,
    width: u32,
    height: u32,
    chroma: (Option<i32>, Option<i32>),
}

impl Key {
    fn new(frame: &ffmpeg::frame::Video, output: Pixel) -> Result<Self, ImageDecodeError> {
        let mut chroma = (None, None);
        // SAFETY: immutable native descriptor for the live decoded frame; no
        // pointer is retained. Full-resolution axes must not acquire an offset.
        let descriptor = unsafe { av_pix_fmt_desc_get(frame.format().into()).as_ref() }
            .ok_or_else(|| invalid("unknown AVIF pixel format"))?;
        if descriptor.nb_components >= 3
            && descriptor.flags & AV_PIX_FMT_FLAG_RGB as u64 == 0
            && frame.chroma_location() != ffmpeg::util::chroma::Location::Unspecified
        {
            let (mut x, mut y) = (0, 0);
            // SAFETY: scalar outputs live for this call; the enum comes from the
            // decoded frame and invalid values return a checked native error.
            let result = unsafe {
                av_chroma_location_enum_to_pos(&mut x, &mut y, frame.chroma_location().into())
            };
            if result < 0 {
                return Err(ffmpeg_error(ffmpeg::Error::from(result)));
            }
            chroma = (
                (descriptor.log2_chroma_w != 0).then_some(x),
                (descriptor.log2_chroma_h != 0).then_some(y),
            );
        }
        Ok(Self {
            input: frame.format(),
            output,
            width: frame.width(),
            height: frame.height(),
            chroma,
        })
    }
}

// The plane decoder owns this native context on its worker. Initialization must
// follow chroma configuration; changing positions on an initialized legacy scaler
// does not rebuild its filters. The key includes only effective subsampled axes.
pub(super) struct Scaler {
    context: *mut SwsContext,
    key: Key,
}

impl Drop for Scaler {
    fn drop(&mut self) {
        // SAFETY: sole ownership of the context, including initialization errors.
        unsafe {
            sws_free_context(&mut self.context);
        }
    }
}

impl Scaler {
    fn new(key: Key) -> Result<Self, ImageDecodeError> {
        // SAFETY: the guard owns the allocation immediately, including null.
        let scaler = Self {
            context: unsafe { sws_alloc_context() },
            key,
        };
        if scaler.context.is_null() {
            return Err(invalid("cannot allocate AVIF color converter"));
        }
        // SAFETY: exclusively owned, not yet initialized. The plane decoder
        // checked dimensions against its canvas/byte limit before conversion.
        let result = unsafe {
            let context = &mut *scaler.context;
            context.src_w = key.width as i32;
            context.src_h = key.height as i32;
            context.dst_w = key.width as i32;
            context.dst_h = key.height as i32;
            context.src_format = AVPixelFormat::from(key.input) as i32;
            context.dst_format = AVPixelFormat::from(key.output) as i32;
            context.flags = ffmpeg::software::scaling::flag::Flags::BILINEAR.bits() as u32;
            if let Some(x) = key.chroma.0 {
                context.src_h_chr_pos = x;
            }
            if let Some(y) = key.chroma.1 {
                context.src_v_chr_pos = y;
            }
            sws_init_context(scaler.context, std::ptr::null_mut(), std::ptr::null_mut())
        };
        if result < 0 {
            return Err(ffmpeg_error(ffmpeg::Error::from(result)));
        }
        Ok(scaler)
    }
}

pub(super) fn convert(
    cached: &mut Option<Scaler>,
    source: &ffmpeg::frame::Video,
    pixel: Pixel,
) -> Result<ffmpeg::frame::Video, ImageDecodeError> {
    let key = Key::new(source, pixel)?;
    if cached.as_ref().is_none_or(|scaler| scaler.key != key) {
        *cached = Some(Scaler::new(key)?);
    }
    let scaler = cached.as_mut().expect("initialized AVIF converter");
    let space: AVColorSpace = source.color_space().into();
    let space = match space {
        AVColorSpace::AVCOL_SPC_UNSPECIFIED | AVColorSpace::AVCOL_SPC_RGB => SWS_CS_DEFAULT,
        _ => space as i32,
    };
    // SAFETY: refresh range/matrix on every frame even when filters are reused.
    // Coefficients are immutable native storage; source pixels are only borrowed.
    let result = unsafe {
        let coefficients = sws_getCoefficients(space);
        sws_setColorspaceDetails(
            scaler.context,
            coefficients,
            i32::from(source.color_range() == ffmpeg::color::Range::JPEG),
            coefficients,
            1,
            0,
            1 << 16,
            1 << 16,
        )
    };
    if result < 0 {
        return Err(ffmpeg_error(ffmpeg::Error::from(result)));
    }
    let mut output = ffmpeg::frame::Video::new(pixel, key.width, key.height);
    // SAFETY: live native frames provide valid planes/strides. Output is a
    // separate writable allocation; neither frame pointer survives the call.
    let rows = unsafe {
        sws_scale(
            scaler.context,
            (*source.as_ptr()).data.as_ptr().cast(),
            (*source.as_ptr()).linesize.as_ptr(),
            0,
            key.height as i32,
            (*output.as_mut_ptr()).data.as_ptr(),
            (*output.as_mut_ptr()).linesize.as_ptr(),
        )
    };
    if rows < 0 {
        return Err(ffmpeg_error(ffmpeg::Error::from(rows)));
    }
    if rows != key.height as i32 {
        return Err(invalid("incomplete AVIF color conversion"));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avif_scaler_rebuilds_chroma_filters_and_refreshes_range_and_matrix() {
        ffmpeg::init().expect("FFmpeg");
        let mut source = ffmpeg::frame::Video::new(Pixel::YUV420P10LE, 16, 16);
        for plane in 0..3 {
            source.data_mut(plane).fill(0);
            let side = if plane == 0 { 16 } else { 8 };
            for y in 0..side {
                for x in 0..side {
                    let value: u16 = if plane == 0 {
                        512
                    } else {
                        [240, 520, 800][(x + y + plane) % 3]
                    };
                    let offset = y * source.stride(plane) + x * 2;
                    source.data_mut(plane)[offset..offset + 2]
                        .copy_from_slice(&value.to_le_bytes());
                }
            }
        }
        let original: Vec<_> = (0..3).map(|plane| source.data(plane).to_vec()).collect();
        let mut cached = None;
        let mut results = Vec::new();
        for (location, range, matrix) in [
            (
                AVChromaLocation::AVCHROMA_LOC_LEFT,
                ffmpeg::color::Range::MPEG,
                ffmpeg::color::Space::BT709,
            ),
            (
                AVChromaLocation::AVCHROMA_LOC_TOPLEFT,
                ffmpeg::color::Range::MPEG,
                ffmpeg::color::Space::BT709,
            ),
            (
                AVChromaLocation::AVCHROMA_LOC_LEFT,
                ffmpeg::color::Range::MPEG,
                ffmpeg::color::Space::BT709,
            ),
            (
                AVChromaLocation::AVCHROMA_LOC_LEFT,
                ffmpeg::color::Range::JPEG,
                ffmpeg::color::Space::BT709,
            ),
            (
                AVChromaLocation::AVCHROMA_LOC_LEFT,
                ffmpeg::color::Range::JPEG,
                ffmpeg::color::Space::SMPTE170M,
            ),
        ] {
            source.set_color_range(range);
            source.set_color_space(matrix);
            // SAFETY: exclusively owned fixture; only scalar metadata changes.
            unsafe {
                (*source.as_mut_ptr()).chroma_location = location;
            }
            let actual = convert(&mut cached, &source, Pixel::RGBA).expect("cached conversion");
            let fresh = convert(&mut None, &source, Pixel::RGBA).expect("fresh conversion");
            let pixels = |frame: &ffmpeg::frame::Video| -> Vec<u8> {
                frame
                    .data(0)
                    .chunks_exact(frame.stride(0))
                    .take(16)
                    .flat_map(|row| row[..16 * 4].iter().copied())
                    .collect()
            };
            assert_eq!(pixels(&actual), pixels(&fresh));
            results.push(pixels(&actual));
        }
        assert_ne!(
            results[0], results[1],
            "siting change must affect the fixture"
        );
        assert_eq!(
            results[0], results[2],
            "A-to-B-to-A restores the original filters"
        );
        assert_ne!(
            results[2], results[3],
            "range refresh must affect the fixture"
        );
        assert_ne!(
            results[3], results[4],
            "matrix refresh must affect the fixture"
        );
        for (plane, original) in original.iter().enumerate() {
            assert_eq!(source.data(plane), original);
        }
        for (pixel, expected) in [
            (Pixel::YUV422P10LE, (Some(0), None)),
            (Pixel::YUV444P10LE, (None, None)),
            (Pixel::GRAY8, (None, None)),
        ] {
            let mut source = ffmpeg::frame::Video::new(pixel, 16, 16);
            // SAFETY: exclusive scalar metadata on the full-resolution control.
            unsafe {
                (*source.as_mut_ptr()).chroma_location = AVChromaLocation::AVCHROMA_LOC_BOTTOMLEFT;
            }
            assert_eq!(
                Key::new(&source, Pixel::RGBA)
                    .expect("effective siting")
                    .chroma,
                expected
            );
        }
    }
}
