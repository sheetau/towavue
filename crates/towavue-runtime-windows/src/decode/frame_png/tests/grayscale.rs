use super::*;

#[test]
fn decoded_gray_video_preserves_samples_through_crop_flip_and_png_publication_bytes() {
    let root = std::env::temp_dir().join(format!(
        "towavue-gray-frame-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("owned fixture");
    let source = root.join("source.mkv");
    let ffmpeg = crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg");
    let result = Command::new(ffmpeg).args([
        "-v", "error", "-f", "lavfi", "-i",
        "nullsrc=size=16x8:rate=2:duration=1,format=gray16le,geq=lum=X*4096+Y*128+N,setparams=range=full",
        "-c:v", "ffv1",
    ]).arg(&source).output().expect("fixture encoder");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original = fs::read(&source).expect("original bytes");
    let operations = [
        EditOperation::Crop(towavue_core::PixelCrop {
            x: 2,
            y: 1,
            width: 10,
            height: 6,
        }),
        EditOperation::FlipHorizontal,
    ];
    for frame in 0..2 {
        let encoded = edited_video_frame_png(
            &source,
            MediaTime::from_nanoseconds(frame * 500_000_000),
            &operations,
            &|| false,
        )
        .expect("decoded gray PNG");
        let (info, pixels) = unpack(&encoded);
        assert_eq!((info.width, info.height), (10, 6));
        assert_eq!(info.color_type, png::ColorType::Grayscale);
        assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
        let expected: Vec<_> = (0..6)
            .flat_map(|y| {
                (0..10).flat_map(move |x| {
                    (((11 - x) * 4096 + (y + 1) * 128 + frame) as u16).to_be_bytes()
                })
            })
            .collect();
        assert_eq!(pixels, expected);
    }
    assert_eq!(fs::read(&source).expect("source retained"), original);
    fs::remove_dir_all(root).expect("remove owned fixture");
}

#[test]
fn grayscale_alpha_resampling_and_free_rotation_match_equal_rgb_channels() {
    use towavue_core::{ResampleFilter, VideoResize, VideoRotation};
    ffmpeg::init().expect("FFmpeg");
    let mut gray = frame::Video::new(Pixel::YA16BE, 8, 6);
    let mut rgb = frame::Video::new(Pixel::RGBA64BE, 8, 6);
    gray.set_color_range(ffmpeg::color::Range::JPEG);
    for y in 0..6 {
        for x in 0..8 {
            let value = ((x * 977 + y * 3001) as u16).to_be_bytes();
            let alpha = ((x * 8000) as u16).to_be_bytes();
            let gray_offset = y * gray.stride(0) + x * 4;
            gray.data_mut(0)[gray_offset..gray_offset + 2].copy_from_slice(&value);
            gray.data_mut(0)[gray_offset + 2..gray_offset + 4].copy_from_slice(&alpha);
            let rgb_offset = y * rgb.stride(0) + x * 8;
            for channel in 0..3 {
                rgb.data_mut(0)[rgb_offset + channel * 2..rgb_offset + channel * 2 + 2]
                    .copy_from_slice(&value);
            }
            rgb.data_mut(0)[rgb_offset + 6..rgb_offset + 8].copy_from_slice(&alpha);
        }
    }
    for filter in [
        ResampleFilter::Nearest,
        ResampleFilter::Bilinear,
        ResampleFilter::Bicubic,
        ResampleFilter::Lanczos,
    ] {
        let resize = VideoResize::new((18, 16), filter, (8, 6), 1.0).expect("resize");
        let rotation = VideoRotation::new(123, (18, 16), 1.0).expect("rotation");
        let operations = [
            EditOperation::ResizeVideo(resize),
            EditOperation::RotateVideo(rotation),
        ];
        let (gray_info, actual) = unpack(
            &encode(&gray, VideoOrientation::default(), &operations, &|| false)
                .expect("gray edits"),
        );
        let (rgb_info, reference) = unpack(
            &encode(&rgb, VideoOrientation::default(), &operations, &|| false)
                .expect("equal-channel RGB edits"),
        );
        assert_eq!(gray_info.color_type, png::ColorType::GrayscaleAlpha);
        assert_eq!(
            (gray_info.width, gray_info.height),
            (rgb_info.width, rgb_info.height)
        );
        let mut channel_difference = 0;
        for (gray, rgb) in actual
            .as_chunks::<4>()
            .0
            .iter()
            .zip(reference.as_chunks::<8>().0)
        {
            // This comparison preserves the established RGB resampling result,
            // not original samples after interpolation. Native planar conversion
            // and alpha unpremultiplication can round channels differently.
            let red = u16::from_be_bytes([rgb[0], rgb[1]]);
            channel_difference = channel_difference
                .max(red.abs_diff(u16::from_be_bytes([rgb[2], rgb[3]])))
                .max(red.abs_diff(u16::from_be_bytes([rgb[4], rgb[5]])));
            assert_eq!(&gray[..2], &rgb[..2]);
            assert_eq!(&gray[2..], &rgb[6..]);
        }
        eprintln!(
            "GRAY_INTERPOLATION {filter:?} native_rgb_channel_difference={channel_difference}"
        );
    }
}

#[test]
fn grayscale_png_preserves_every_sample_and_gray_profile_before_and_after_edits() {
    use ffmpeg::util::frame::side_data::Type;
    ffmpeg::init().expect("FFmpeg");
    for (pixel, sample_bytes, alpha) in [
        (Pixel::GRAY8, 1, false),
        (Pixel::YA8, 1, true),
        (Pixel::GRAY16BE, 2, false),
        (Pixel::YA16BE, 2, true),
    ] {
        let channels = if alpha { 2 } else { 1 };
        let bytes = sample_bytes * channels;
        let mut source = frame::Video::new(pixel, 256, 256);
        source.set_color_range(ffmpeg::color::Range::JPEG);
        let stride = source.stride(0);
        let mut original = Vec::new();
        for y in 0..256 {
            for x in 0..256 {
                let value = (y * 256 + x) as u16;
                let mut packed = Vec::new();
                for value in [value, !value].into_iter().take(channels) {
                    if sample_bytes == 1 {
                        packed.push(value as u8);
                    } else {
                        packed.extend(value.to_be_bytes());
                    }
                }
                source.data_mut(0)[y * stride + x * bytes..y * stride + (x + 1) * bytes]
                    .copy_from_slice(&packed);
                original.extend(packed);
            }
        }
        // Profile header is sufficient to check retention and PNG color-type
        // compatibility; this fixture does not assert colorimetric behavior.
        let mut profile = vec![0_u8; 132];
        profile[..4].copy_from_slice(&132_u32.to_be_bytes());
        profile[16..20].copy_from_slice(b"GRAY");
        profile[36..40].copy_from_slice(b"acsp");
        let mut side = source
            .new_side_data(Type::IccProfile, profile.len())
            .expect("gray profile");
        // SAFETY: the generated source owns this newly allocated side data.
        unsafe {
            std::ptr::copy_nonoverlapping(
                profile.as_ptr(),
                (*side.as_mut_ptr()).data,
                profile.len(),
            );
        }
        for flipped in [false, true] {
            let operations = if flipped {
                vec![EditOperation::FlipHorizontal]
            } else {
                Vec::new()
            };
            let encoded = encode(&source, VideoOrientation::default(), &operations, &|| false)
                .expect("gray PNG export");
            let (info, actual) = unpack(&encoded);
            assert_eq!(
                info.color_type,
                if alpha {
                    png::ColorType::GrayscaleAlpha
                } else {
                    png::ColorType::Grayscale
                }
            );
            assert_eq!(
                info.bit_depth,
                if sample_bytes == 2 {
                    png::BitDepth::Sixteen
                } else {
                    png::BitDepth::Eight
                }
            );
            for y in 0..256 {
                for x in 0..256 {
                    let from = (y * 256 + if flipped { 255 - x } else { x }) * bytes;
                    let to = (y * 256 + x) * bytes;
                    assert_eq!(
                        &actual[to..to + bytes],
                        &original[from..from + bytes],
                        "{pixel:?} at {x},{y}"
                    );
                }
            }
            let profiles = chunks(&encoded, b"iCCP");
            assert_eq!(profiles.len(), 1);
            let start = profiles[0]
                .iter()
                .position(|value| *value == 0)
                .expect("profile name")
                + 2;
            let mut restored = Vec::new();
            std::io::Read::read_to_end(
                &mut flate2::read::ZlibDecoder::new(&profiles[0][start..]),
                &mut restored,
            )
            .expect("profile data");
            assert_eq!(restored, profile);
        }
    }
}
