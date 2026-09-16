use super::*;
use ffmpeg::ffi::AVAlphaMode;

#[test]
fn premultiplied_conversion_cancels_between_rows() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut work = frame::Video::new(Pixel::RGBA, 1, 3);
    let stride = work.stride(0);
    for y in 0..3 {
        work.data_mut(0)[y * stride..y * stride + 4].copy_from_slice(&[17, 34, 51, 85]);
    }
    let checks = AtomicUsize::new(0);
    let result = restore_straight_alpha(&mut work, false, &|| {
        checks.fetch_add(1, Ordering::Relaxed) != 0
    });
    assert!(matches!(result, Err(DecodeError::ConsumerClosed)));
    assert_eq!(&work.data(0)[..4], &[51, 102, 153, 85]);
    assert_eq!(&work.data(0)[stride..stride + 4], &[17, 34, 51, 85]);
    assert_eq!(&work.data(0)[2 * stride..2 * stride + 4], &[17, 34, 51, 85]);
}

#[test]
fn premultiplied_frame_png_restores_straight_samples_before_edits() {
    ffmpeg::init().expect("FFmpeg");
    for high_depth in [false, true] {
        let (pixel, sample_bytes, input, expected): (_, _, Vec<[u16; 4]>, Vec<[u16; 4]>) =
            if high_depth {
                (
                    Pixel::RGBA64BE,
                    2,
                    vec![
                        [19, 29, 39, 0],
                        [1, 1, 0, 1],
                        [1000, 2000, 3000, 21845],
                        [16384, 8192, 4096, 32768],
                        [65535, 33, 777, 65535],
                        [7, 1, 0, 3],
                    ],
                    vec![
                        [0, 0, 0, 0],
                        [65535, 65535, 0, 1],
                        [3000, 6000, 9000, 21845],
                        [32768, 16384, 8192, 32768],
                        [65535, 33, 777, 65535],
                        [65535, 21845, 0, 3],
                    ],
                )
            } else {
                (
                    Pixel::RGBA,
                    1,
                    vec![
                        [19, 29, 39, 0],
                        [1, 1, 0, 1],
                        [17, 34, 51, 85],
                        [64, 32, 16, 128],
                        [255, 33, 77, 255],
                        [7, 1, 0, 3],
                    ],
                    vec![
                        [0, 0, 0, 0],
                        [255, 255, 0, 1],
                        [51, 102, 153, 85],
                        [128, 64, 32, 128],
                        [255, 33, 77, 255],
                        [255, 85, 0, 3],
                    ],
                )
            };
        for gray in [false, true] {
            let pixel = if gray {
                if high_depth {
                    Pixel::YA16BE
                } else {
                    Pixel::YA8
                }
            } else {
                pixel
            };
            let mut source = frame::Video::new(pixel, 3, 2);
            let mut reference = frame::Video::new(pixel, 3, 2);
            for (frame, values) in [(&mut source, &input), (&mut reference, &expected)] {
                frame.data_mut(0).fill(0);
                frame.set_color_range(ffmpeg::color::Range::JPEG);
                for (index, values) in values.iter().enumerate() {
                    let components: &[u16] = if gray {
                        &[values[0], values[3]]
                    } else {
                        values
                    };
                    let stride = frame.stride(0);
                    for (channel, value) in components.iter().enumerate() {
                        let at = index / 3 * stride
                            + (index % 3 * components.len() + channel) * sample_bytes;
                        if sample_bytes == 1 {
                            frame.data_mut(0)[at] = *value as u8;
                        } else {
                            frame.data_mut(0)[at..at + 2].copy_from_slice(&value.to_be_bytes());
                        }
                    }
                }
            }
            // SAFETY: these generated frames are exclusively owned; only their
            // alpha interpretation changes, never pixels or allocation ownership.
            unsafe {
                (*source.as_mut_ptr()).alpha_mode = AVAlphaMode::AVALPHA_MODE_PREMULTIPLIED;
                (*reference.as_mut_ptr()).alpha_mode = AVAlphaMode::AVALPHA_MODE_STRAIGHT;
            }
            let before = source.data(0).to_vec();
            let mut cases = vec![
                vec![],
                vec![EditOperation::FlipHorizontal],
                vec![
                    EditOperation::RotateClockwise,
                    EditOperation::Crop(towavue_core::PixelCrop {
                        x: 0,
                        y: 1,
                        width: 2,
                        height: 2,
                    }),
                ],
            ];
            for filter in [
                towavue_core::ResampleFilter::Nearest,
                towavue_core::ResampleFilter::Bilinear,
                towavue_core::ResampleFilter::Bicubic,
                towavue_core::ResampleFilter::Lanczos,
            ] {
                cases.push(vec![EditOperation::ResizeVideo(
                    towavue_core::VideoResize::new((16, 16), filter, (3, 2), 1.0).expect("resize"),
                )]);
            }
            cases.push(vec![EditOperation::RotateVideo(
                towavue_core::VideoRotation::new(73, (3, 2), 1.0).expect("rotation"),
            )]);
            for operations in cases {
                let actual = encode(&source, VideoOrientation::default(), &operations, &|| false)
                    .expect("premultiplied PNG");
                let expected = encode(
                    &reference,
                    VideoOrientation::default(),
                    &operations,
                    &|| false,
                )
                .expect("straight reference PNG");
                assert_eq!(
                    unpack(&actual).1,
                    unpack(&expected).1,
                    "depth={high_depth} gray={gray} edits={operations:?}"
                );
            }
            assert_eq!(source.data(0), before);
            // SAFETY: immutable access to the still-owned generated frame.
            assert_eq!(
                unsafe { (*source.as_ptr()).alpha_mode },
                AVAlphaMode::AVALPHA_MODE_PREMULTIPLIED
            );
        }
    }
}
