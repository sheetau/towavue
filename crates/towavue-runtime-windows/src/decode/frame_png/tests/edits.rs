use super::*;
use std::io::Write;
use std::process::Stdio;
use towavue_core::{PixelCrop, ResampleFilter, VideoResize, VideoRotation};

fn components(pixel: Pixel) -> (usize, usize) {
    match pixel {
        Pixel::RGB24 => (3, 1),
        Pixel::RGBA => (4, 1),
        Pixel::RGB48BE => (3, 2),
        Pixel::RGBA64BE => (4, 2),
        _ => unreachable!("test fixture format"),
    }
}

fn fixture(pixel: Pixel, size: (u32, u32)) -> frame::Video {
    ffmpeg::init().expect("FFmpeg");
    let (channels, sample_bytes) = components(pixel);
    let mut source = frame::Video::new(pixel, size.0, size.1);
    let stride = source.stride(0);
    for y in 0..size.1 as usize {
        for x in 0..size.0 as usize {
            for c in 0..channels {
                let value = ((x * 1103 + y * 2507 + c * 9173) % 65536) as u16;
                let offset = y * stride + (x * channels + c) * sample_bytes;
                if sample_bytes == 1 {
                    source.data_mut(0)[offset] = (value >> 8) as u8;
                } else {
                    source.data_mut(0)[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
                }
            }
        }
    }
    // Nonzero hidden colors must survive orthogonal edits and nearest resize.
    if channels == 4 {
        source.data_mut(0)[3 * sample_bytes..4 * sample_bytes].fill(0);
    }
    source.set_color_primaries(ffmpeg::color::Primaries::BT2020);
    source.set_color_transfer_characteristic(ffmpeg::color::TransferCharacteristic::SMPTE2084);
    source
}

#[test]
fn ordered_orthogonal_edits_preserve_every_sample_aspect_and_color_tags() {
    for pixel in [Pixel::RGB24, Pixel::RGBA, Pixel::RGB48BE, Pixel::RGBA64BE] {
        let mut source = fixture(pixel, (7, 5));
        // SAFETY: exclusively owned generated frame, scalar property only.
        unsafe {
            (*source.as_mut_ptr()).sample_aspect_ratio = Rational(3, 2).into();
        }
        let (channels, sample_bytes) = components(pixel);
        let bytes = channels * sample_bytes;
        let crop = EditOperation::Crop(PixelCrop {
            x: 0,
            y: 0,
            width: 6,
            height: 4,
        });
        for (op, clockwise) in [
            (EditOperation::RotateClockwise, true),
            (EditOperation::RotateCounterclockwise, false),
        ] {
            let ops = [
                crop,
                op,
                EditOperation::FlipHorizontal,
                EditOperation::FlipVertical,
                EditOperation::SetRate(2.0),
                EditOperation::SetVolume(0.0),
                EditOperation::SetTrimStart(MediaTime::ZERO),
            ];
            let png =
                encode(&source, VideoOrientation::default(), &ops, &|| false).expect("edited PNG");
            let (info, actual) = unpack(&png);
            assert_eq!((info.width, info.height), (4, 6));
            for sy in 0..4 {
                for sx in 0..6 {
                    let (dx, dy) = if clockwise {
                        (sy, 5 - sx)
                    } else {
                        (3 - sy, sx)
                    };
                    let src = sy * source.stride(0) + sx * bytes;
                    let dst = (dy * 4 + dx) * bytes;
                    assert_eq!(&actual[dst..dst + bytes], &source.data(0)[src..src + bytes]);
                }
            }
            assert_eq!(chunks(&png, b"cICP"), vec![vec![9, 16, 0, 1]]);
            assert_eq!(chunks(&png, b"pHYs"), vec![vec![0, 0, 0, 3, 0, 0, 0, 2, 0]]);
        }
        for angle in [900, -900, 1800, -1800] {
            let rotation = VideoRotation::new(angle, (7, 5), 1.0).expect("rotation");
            // SAFETY: exclusively owned test frame.
            unsafe {
                (*source.as_mut_ptr()).sample_aspect_ratio = Rational(1, 1).into();
            }
            let png = encode(
                &source,
                VideoOrientation::default(),
                &[EditOperation::RotateVideo(rotation)],
                &|| false,
            )
            .expect("quarter turn");
            let (info, actual) = unpack(&png);
            assert_eq!((info.width, info.height), rotation.size());
            for sy in 0..5 {
                for sx in 0..7 {
                    let (dx, dy) = match angle {
                        900 => (4 - sy, sx),
                        -900 => (sy, 6 - sx),
                        _ => (6 - sx, 4 - sy),
                    };
                    let src = sy * source.stride(0) + sx * bytes;
                    let dst = (dy * info.width as usize + dx) * bytes;
                    assert_eq!(&actual[dst..dst + bytes], &source.data(0)[src..src + bytes]);
                }
            }
            // Even-canvas padding is black, and opaque for alpha video.
            let last = &actual[actual.len() - bytes..];
            assert!(last[..3 * sample_bytes].iter().all(|byte| *byte == 0));
            assert!(last[3 * sample_bytes..].iter().all(|byte| *byte == 255));
        }
    }
}

#[test]
fn high_depth_resize_matches_explicit_full_precision_native_filters() {
    let exe = std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg prefix"))
        .join("bin/ffmpeg.exe");
    for pixel in [Pixel::RGB48BE, Pixel::RGBA64BE] {
        let source = fixture(pixel, (31, 23));
        let input =
            encode(&source, VideoOrientation::default(), &[], &|| false).expect("source PNG");
        for (filter, flag) in [
            (ResampleFilter::Bilinear, "bilinear"),
            (ResampleFilter::Bicubic, "bicubic"),
            (ResampleFilter::Lanczos, "lanczos"),
        ] {
            let resize = VideoResize::new((16, 18), filter, (31, 23), 1.0).expect("resize");
            let png = encode(
                &source,
                VideoOrientation::default(),
                &[EditOperation::ResizeVideo(resize)],
                &|| false,
            )
            .expect("resized PNG");
            let (info, actual) = unpack(&png);
            assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
            assert_eq!((info.width, info.height), (16, 18));
            let alpha = pixel == Pixel::RGBA64BE;
            let planar = if alpha { "gbrap16le" } else { "gbrp16le" };
            let packed = if alpha { "rgba64be" } else { "rgb48be" };
            let premul = alpha;
            let filters = format!(
                "format={planar},{}scale=16:18:flags={flag}+full_chroma_inp,format={planar},{}format={packed}",
                if premul { "premultiply=inplace=1," } else { "" },
                if premul {
                    "unpremultiply=inplace=1,"
                } else {
                    ""
                }
            );
            let mut child = Command::new(&exe)
                .args([
                    "-v",
                    "error",
                    "-threads",
                    "1",
                    "-filter_threads",
                    "1",
                    "-i",
                    "pipe:0",
                    "-vf",
                    &filters,
                    "-frames:v",
                    "1",
                    "-f",
                    "rawvideo",
                    "pipe:1",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("CLI reference");
            child
                .stdin
                .take()
                .expect("input pipe")
                .write_all(&input)
                .expect("source PNG input");
            let result = child.wait_with_output().expect("CLI completion");
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                actual, result.stdout,
                "independent {packed}/{flag} reference"
            );
            assert!(
                actual.as_chunks::<2>().0.iter().any(|v| v[0] != v[1]),
                "not expanded 8-bit samples"
            );
        }
    }
}

#[test]
fn arbitrary_rotation_retains_linear_gradient_precision_and_premultiplied_alpha() {
    for alpha in [false, true] {
        let pixel = if alpha {
            Pixel::RGBA64BE
        } else {
            Pixel::RGB48BE
        };
        let channels = if alpha { 4 } else { 3 };
        let mut source = fixture(pixel, (17, 13));
        let stride = source.stride(0);
        for y in 0..13 {
            for x in 0..17 {
                // Opaque linear gradient, with a fully transparent red strip.
                let transparent = alpha && x == 7;
                let values = if transparent {
                    [65535, 0, 0, 0]
                } else {
                    [1001 + x as u16 * 173 + y as u16 * 281, 12345, 34567, 65535]
                };
                for (c, v) in values[..channels].iter().enumerate() {
                    let offset = y * stride + (x * channels + c) * 2;
                    source.data_mut(0)[offset..offset + 2].copy_from_slice(&v.to_be_bytes());
                }
            }
        }
        for angle in [-371, 227] {
            let rotation = VideoRotation::new(angle, (17, 13), 1.0).expect("rotation");
            let png = encode(
                &source,
                VideoOrientation::default(),
                &[EditOperation::RotateVideo(rotation)],
                &|| false,
            )
            .expect("arbitrary rotation");
            let (info, actual) = unpack(&png);
            assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
            let (sin, cos) = (f64::from(angle).to_radians() / 10.0).sin_cos();
            let mut checked = 0;
            let mut translucent = 0;
            for y in 0..rotation.raster_size().1 {
                for x in 0..rotation.raster_size().0 {
                    let dx = f64::from(x) - (f64::from(rotation.raster_size().0) - 1.0) / 2.0;
                    let dy = f64::from(y) - (f64::from(rotation.raster_size().1) - 1.0) / 2.0;
                    let (sx, sy) = (8.0 + dx * cos + dy * sin, 6.0 - dx * sin + dy * cos);
                    if !(1.0..15.0).contains(&sx) || !(1.0..11.0).contains(&sy) {
                        continue;
                    }
                    let offset = (y * info.width + x) as usize * channels * 2;
                    let values: Vec<u16> = actual[offset..offset + channels * 2]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .copied()
                        .map(u16::from_be_bytes)
                        .collect();
                    if alpha && (6.0..8.0).contains(&sx) {
                        if values[3] > 0 && values[3] < 65535 {
                            assert_eq!(values[1], 12345, "hidden red does not darken green");
                            assert_eq!(values[2], 34567, "hidden red does not darken blue");
                            translucent += 1;
                        }
                    } else {
                        let expected = (1001.0 + sx * 173.0 + sy * 281.0).round() as u16;
                        assert!(values[0].abs_diff(expected) <= 1);
                        assert_eq!((values[1], values[2]), (12345, 34567));
                        checked += 1;
                    }
                }
            }
            assert!(checked > 60);
            if alpha {
                assert!(translucent > 10);
            }
        }
    }
}

#[test]
fn nearest_resize_preserves_hidden_samples_and_rotation_normalizes_pixel_aspect() {
    for pixel in [Pixel::RGB24, Pixel::RGBA, Pixel::RGB48BE, Pixel::RGBA64BE] {
        for (source_size, size) in [
            ((17, 13), (34, 26)),
            ((17, 13), (16, 18)),
            ((31, 23), (16, 18)),
            ((32, 24), (24, 16)),
        ] {
            let source = fixture(pixel, source_size);
            let (channels, sample_bytes) = components(pixel);
            let bytes = channels * sample_bytes;
            let resize = VideoResize::new(size, ResampleFilter::Nearest, source_size, 1.0)
                .expect("nearest size");
            let png = encode(
                &source,
                VideoOrientation::default(),
                &[EditOperation::ResizeVideo(resize)],
                &|| false,
            )
            .expect("nearest PNG");
            let (_, actual) = unpack(&png);
            for y in 0..size.1 as usize {
                for x in 0..size.0 as usize {
                    let sx = ((x as f64 + 0.5)
                        * (65536.0 * f64::from(source_size.0) / f64::from(size.0)).round()
                        / 65536.0)
                        .floor() as usize;
                    let sy = ((y as f64 + 0.5)
                        * (65536.0 * f64::from(source_size.1) / f64::from(size.1)).round()
                        / 65536.0)
                        .floor() as usize;
                    let from = sy * source.stride(0) + sx * bytes;
                    let to = (y * size.0 as usize + x) * bytes;
                    assert_eq!(&actual[to..to + bytes], &source.data(0)[from..from + bytes]);
                }
            }
        }
    }
    let mut source = fixture(Pixel::RGB48BE, (17, 13));
    let stride = source.stride(0);
    for y in 0..13 {
        for x in 0..17 {
            for c in 0..3 {
                let offset = y * stride + (x * 3 + c) * 2;
                source.data_mut(0)[offset..offset + 2]
                    .copy_from_slice(&(12345 + y as u16 * 109 + c as u16 * 1234).to_be_bytes());
            }
        }
    }
    // SAFETY: exclusively owned generated frame, scalar property only.
    unsafe {
        (*source.as_mut_ptr()).sample_aspect_ratio = Rational(3, 2).into();
    }
    let rotation = VideoRotation::new(900, (17, 13), 1.5).expect("aspect rotation");
    let png = encode(
        &source,
        VideoOrientation::default(),
        &[EditOperation::RotateVideo(rotation)],
        &|| false,
    )
    .expect("normalized rotation");
    let (info, actual) = unpack(&png);
    assert_eq!((info.width, info.height), (14, 26));
    assert_eq!(chunks(&png, b"pHYs"), vec![vec![0, 0, 0, 1, 0, 0, 0, 1, 0]]);
    for y in 0..26 {
        for x in 0..14 {
            for c in 0..3 {
                let offset = (y * 14 + x) * 6 + c * 2;
                let value = u16::from_be_bytes([actual[offset], actual[offset + 1]]);
                let expected = if x == 13 {
                    0
                } else {
                    12345 + (12 - x) as u16 * 109 + c as u16 * 1234
                };
                // Native interpolation has 16-bit fixed-point color rounding;
                // unlike nearest, it does not promise unchanged sample values.
                if x == 13 {
                    assert_eq!(value, 0);
                } else {
                    assert!(
                        value.abs_diff(expected) <= 4,
                        "full-depth interpolation: {value} vs {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn edited_frame_rejects_stale_geometry_unsupported_edits_budgets_and_cancellation() {
    let source = fixture(Pixel::RGB48BE, (17, 13));
    let ops = [
        EditOperation::Crop(PixelCrop {
            x: u32::MAX,
            y: 0,
            width: 4,
            height: 4,
        }),
        EditOperation::ResizeVideo(
            VideoResize::new((16, 16), ResampleFilter::Bilinear, (18, 13), 1.0)
                .expect("stale resize"),
        ),
        EditOperation::RotateVideo(VideoRotation::new(123, (17, 13), 1.5).expect("stale aspect")),
        EditOperation::Resize(
            towavue_core::ImageResize::new(16, 16, ResampleFilter::Nearest).expect("image resize"),
        ),
        EditOperation::ResizeVideo(
            VideoResize::new((16384, 8192), ResampleFilter::Nearest, (17, 13), 1.0)
                .expect("oversized resize"),
        ),
    ];
    for op in ops {
        assert!(encode(&source, VideoOrientation::default(), &[op], &|| false).is_err());
    }
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let rotation = VideoRotation::new(123, (17, 13), 1.0).expect("rotation");
    let result = encode(
        &source,
        VideoOrientation::default(),
        &[EditOperation::RotateVideo(rotation)],
        &|| calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) > 5,
    );
    assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
}
