use super::*;

#[test]
fn high_depth_frame_png_matches_independent_bt709_colors_on_every_row() {
    ffmpeg::init().expect("FFmpeg");
    for (pixel, shift) in [(Pixel::YUV420P10LE, 0), (Pixel::YUV420P12LE, 2)] {
        for (u, v) in [(512_u16, 512_u16), (400, 600)] {
            let mut source = frame::Video::new(pixel, 96, 64);
            source.set_color_space(ffmpeg::color::Space::BT709);
            source.set_color_range(ffmpeg::color::Range::MPEG);
            // SAFETY: scalar metadata on the exclusively owned fixture.
            unsafe {
                (*source.as_mut_ptr()).chroma_location =
                    ffmpeg::ffi::AVChromaLocation::AVCHROMA_LOC_TOPLEFT;
            }
            for plane in 0..3 {
                let (width, height) = if plane == 0 { (96, 64) } else { (48, 32) };
                let stride = source.stride(plane);
                for y in 0..height {
                    for x in 0..width {
                        let value = match plane {
                            0 => 64 + ((x * 13 + y * 17) % 877) as u16,
                            1 => u,
                            _ => v,
                        } << shift;
                        let at = y * stride + x * 2;
                        source.data_mut(plane)[at..at + 2].copy_from_slice(&value.to_le_bytes());
                    }
                }
            }
            let png = encode(&source, VideoOrientation::default(), &[], &|| false)
                .expect("high-depth PNG");
            let (info, actual) = unpack(&png);
            assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
            assert_eq!(info.color_type, png::ColorType::Rgb);
            // Constant chroma removes interpolation from this independent matrix
            // oracle. The scaler's fixed-point 16-bit range uses 255 << 8 for
            // nominal white; allow four code values for coefficient rounding.
            for y in 0..64 {
                for x in 0..96 {
                    let luma = ((x * 13 + y * 17) % 877) as f64 / 876.0;
                    let cb = (f64::from(u) - 512.0) / 896.0;
                    let cr = (f64::from(v) - 512.0) / 896.0;
                    let expected = [
                        luma + 1.5748 * cr,
                        luma - 0.1873242729 * cb - 0.4681242729 * cr,
                        luma + 1.8556 * cb,
                    ];
                    for (channel, value) in expected.into_iter().enumerate() {
                        let expected = (value * 65280.0).round().clamp(0.0, 65535.0) as u16;
                        let at = ((y * 96 + x) * 3 + channel) * 2;
                        let actual = u16::from_be_bytes([actual[at], actual[at + 1]]);
                        assert!(
                            actual.abs_diff(expected) <= 4,
                            "{pixel:?}, U={u}, V={v}, ({x},{y}), channel={channel}: {actual} != {expected}"
                        );
                    }
                }
            }
        }
    }
}
