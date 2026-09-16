use super::*;

mod alpha;
mod grayscale;
use std::{fs, io::Cursor, process::Command};

mod edits;

#[test]
fn frame_level_orientation_overrides_stream_orientation_before_edits() {
    use ffmpeg::util::frame::side_data::Type;
    ffmpeg::init().expect("FFmpeg");
    let mut source = frame::Video::new(Pixel::RGB24, 7, 5);
    let stride = source.stride(0);
    for y in 0..5 {
        for x in 0..21 {
            source.data_mut(0)[y * stride + x] = (y * 29 + x * 3) as u8;
        }
    }
    let matrix: Vec<u8> = [0_i32, 65536, 0, -65536, 0, 0, 0, 0, 1 << 30]
        .into_iter()
        .flat_map(i32::to_ne_bytes)
        .collect();
    let rotated = VideoOrientation::from_bytes(Some(&matrix)).expect("frame rotation");
    let operations = [EditOperation::Crop(towavue_core::PixelCrop {
        x: 1,
        y: 2,
        width: 2,
        height: 3,
    })];
    let expected = encode(&source, rotated, &operations, &|| false).expect("rotated reference");
    let unrotated = encode(&source, VideoOrientation::default(), &operations, &|| false)
        .expect("unrotated control");
    assert_ne!(
        unpack(&expected).1,
        unpack(&unrotated).1,
        "fixture distinguishes the wrong fallback"
    );
    {
        let mut side = source
            .new_side_data(Type::DisplayMatrix, matrix.len())
            .expect("frame matrix");
        // SAFETY: the generated frame exclusively owns this new side-data region.
        unsafe {
            std::ptr::copy_nonoverlapping(matrix.as_ptr(), (*side.as_mut_ptr()).data, matrix.len());
        }
    }
    let actual = encode(&source, VideoOrientation::default(), &operations, &|| false)
        .expect("frame-local orientation");
    assert_eq!(actual, expected);
}

fn unpack(bytes: &[u8]) -> (png::OutputInfo, Vec<u8>) {
    let mut decoder = png::Decoder::new(Cursor::new(bytes))
        .read_info()
        .expect("PNG header");
    let mut pixels = vec![0; decoder.output_buffer_size().expect("size")];
    let info = decoder.next_frame(&mut pixels).expect("PNG pixels");
    pixels.truncate(info.buffer_size());
    (info, pixels)
}

fn chunks(bytes: &[u8], kind: &[u8; 4]) -> Vec<Vec<u8>> {
    let mut result = Vec::new();
    let mut position = 8;
    while position < bytes.len() {
        let length = u32::from_be_bytes(
            bytes[position..position + 4]
                .try_into()
                .expect("frame PNG fixture"),
        ) as usize;
        if &bytes[position + 4..position + 8] == kind {
            result.push(bytes[position + 8..position + 8 + length].to_vec());
        }
        position += length + 12;
    }
    result
}

#[test]
fn packed_rgb_png_preserves_full_depth_hidden_alpha_and_all_source_orientations() {
    ffmpeg::init().expect("frame PNG fixture");
    for (pixel, bytes, depth) in [
        (Pixel::RGB24, 3, png::BitDepth::Eight),
        (Pixel::RGBA, 4, png::BitDepth::Eight),
        (Pixel::RGB48BE, 6, png::BitDepth::Sixteen),
        (Pixel::RGBA64BE, 8, png::BitDepth::Sixteen),
    ] {
        let mut source = frame::Video::new(pixel, 7, 5);
        let stride = source.stride(0);
        let mut expected = Vec::new();
        for y in 0..5 {
            for x in 0..7 * bytes {
                let value = (x * 17 + y * 39) as u8;
                source.data_mut(0)[y * stride + x] = value;
                expected.push(value);
            }
        }
        // Force a fully transparent pixel with nonzero hidden RGB.
        if bytes == 4 || bytes == 8 {
            let alpha = bytes / 4;
            source.data_mut(0)[bytes - alpha..bytes].fill(0);
            expected[bytes - alpha..bytes].fill(0);
        }
        source.set_color_primaries(ffmpeg::color::Primaries::BT2020);
        source.set_color_transfer_characteristic(ffmpeg::color::TransferCharacteristic::SMPTE2084);
        // SAFETY: exclusively owned generated frame; scalar metadata only.
        unsafe {
            (*source.as_mut_ptr()).sample_aspect_ratio = Rational(3, 2).into();
        }
        for linear in [
            [1, 0, 0, 1],
            [0, 1, -1, 0],
            [-1, 0, 0, -1],
            [0, -1, 1, 0],
            [-1, 0, 0, 1],
            [1, 0, 0, -1],
            [0, 1, 1, 0],
            [0, -1, -1, 0],
        ] {
            let [a, b, c, d] = linear;
            let matrix: Vec<u8> = [
                a * 65536,
                b * 65536,
                0,
                c * 65536,
                d * 65536,
                0,
                0,
                0,
                1 << 30,
            ]
            .into_iter()
            .flat_map(i32::to_ne_bytes)
            .collect();
            let orientation =
                VideoOrientation::from_bytes(Some(&matrix)).expect("frame PNG fixture");
            let encoded =
                encode(&source, orientation, &[], &|| false).expect("encode full precision");
            let (info, actual) = unpack(&encoded);
            assert_eq!(info.bit_depth, depth);
            let (w, h) = if orientation.swaps_axes() {
                (5, 7)
            } else {
                (7, 5)
            };
            assert_eq!((info.width, info.height), (w, h));
            // Independent forward mapping of the source corners/pixels.
            let min_x = (a * 6).min(0) + (c * 4).min(0);
            let min_y = (b * 6).min(0) + (d * 4).min(0);
            for y in 0..5 {
                for x in 0..7 {
                    let dest = ((b * x + d * y - min_y) * w as i32 + a * x + c * y - min_x)
                        as usize
                        * bytes;
                    let src = (y * 7 + x) as usize * bytes;
                    assert_eq!(&actual[dest..dest + bytes], &expected[src..src + bytes]);
                }
            }
            assert_eq!(chunks(&encoded, b"cICP"), vec![vec![9, 16, 0, 1]]);
            let densities = if orientation.swaps_axes() {
                (3_u32, 2_u32)
            } else {
                (2, 3)
            };
            let mut phys = densities.0.to_be_bytes().to_vec();
            phys.extend(densities.1.to_be_bytes());
            phys.push(0);
            assert_eq!(
                chunks(&encoded, b"pHYs"),
                vec![phys],
                "PNG uses inverse pixel densities"
            );
            let edited = encode(
                &source,
                orientation,
                &[EditOperation::Crop(towavue_core::PixelCrop {
                    x: 1,
                    y: 1,
                    width: w - 2,
                    height: h - 2,
                })],
                &|| false,
            )
            .expect("crop after source orientation");
            let (edited_info, edited_pixels) = unpack(&edited);
            assert_eq!((edited_info.width, edited_info.height), (w - 2, h - 2));
            for y in 0..(h - 2) as usize {
                let from = ((y + 1) * w as usize + 1) * bytes;
                let to = y * (w - 2) as usize * bytes;
                let count = (w - 2) as usize * bytes;
                assert_eq!(&edited_pixels[to..to + count], &actual[from..from + count]);
            }
        }
    }
}

#[test]
fn frame_png_rejects_precision_matrix_missing_targets_and_cancellation() {
    ffmpeg::init().expect("frame PNG fixture");
    let source = frame::Video::new(Pixel::GBRPF32LE, 4, 4);
    assert!(matches!(
        encode(&source, VideoOrientation::default(), &[], &|| false),
        Err(DecodeError::FrameImage(_))
    ));
    let mut source = frame::Video::new(Pixel::YUV444P10LE, 4, 4);
    source.set_color_space(ffmpeg::color::Space::BT2020CL);
    assert!(matches!(
        encode(&source, VideoOrientation::default(), &[], &|| false),
        Err(DecodeError::FrameImage(_))
    ));
    assert!(matches!(
        encode(&source, VideoOrientation::default(), &[], &|| true),
        Err(DecodeError::ConsumerClosed)
    ));
    assert!(matches!(
        source_video_frame_png(Path::new("does-not-exist.mkv"), MediaTime::ZERO, &|| true),
        Err(DecodeError::ConsumerClosed)
    ));
}

#[test]
fn high_depth_source_png_matches_explicit_color_conversion_without_touching_source() {
    let root = std::env::temp_dir().join(format!(
        "towavue-native-frame-png-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("frame PNG fixture")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("frame PNG fixture");
    let path = root.join("source.mkv");
    let ffmpeg =
        std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("frame PNG fixture"))
            .join("bin/ffmpeg.exe");
    assert!(Command::new(&ffmpeg).args(["-v","error","-f","lavfi","-i",
        "nullsrc=size=32x16:rate=2:duration=2,format=yuv444p10le,geq=lum=64+X*25+N:cb=400+Y*7:cr=600-Y*5,setparams=color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc:range=limited",
        "-c:v","ffv1"]).arg(&path).status().expect("frame PNG fixture").success());
    let original = fs::read(&path).expect("frame PNG fixture");
    let modified = fs::metadata(&path)
        .expect("frame PNG fixture")
        .modified()
        .expect("frame PNG fixture");
    for n in 0..4 {
        let target = MediaTime::from_nanoseconds(n * 500_000_000);
        let encoded = source_video_frame_png(&path, target, &|| false).expect("native frame PNG");
        let (info, actual) = unpack(&encoded);
        assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
        assert_eq!((info.width, info.height), (32, 16));
        assert_eq!(chunks(&encoded, b"cICP"), vec![vec![9, 16, 0, 1]]);
        let reference = Command::new(&ffmpeg).args(["-v","error","-i"]).arg(&path)
            .args(["-vf", &format!("select=eq(n\\,{n}),scale=flags=bilinear+accurate_rnd+full_chroma_int:in_color_matrix=bt2020:in_range=limited:out_range=full"),
                "-frames:v","1","-pix_fmt","rgb48be","-f","rawvideo","-"]).output().expect("frame PNG fixture");
        assert!(
            reference.status.success(),
            "{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        assert_eq!(actual, reference.stdout);
        let operations = [
            EditOperation::Crop(towavue_core::PixelCrop {
                x: 2,
                y: 4,
                width: 18,
                height: 10,
            }),
            EditOperation::RotateClockwise,
        ];
        let edited = edited_video_frame_png(&path, target, &operations, &|| false)
            .expect("edited source frame");
        let (edited_info, edited_pixels) = unpack(&edited);
        assert_eq!((edited_info.width, edited_info.height), (10, 18));
        for y in 0..10 {
            for x in 0..18 {
                let original_offset = ((y + 4) * 32 + x + 2) * 6;
                let edited_offset = (x * 10 + 9 - y) * 6;
                assert_eq!(
                    &edited_pixels[edited_offset..edited_offset + 6],
                    &reference.stdout[original_offset..original_offset + 6]
                );
            }
        }
        assert!(
            actual
                .as_chunks::<2>()
                .0
                .iter()
                .any(|v| u16::from_be_bytes([v[0], v[1]]) % 257 != 0)
        );
    }
    assert!(matches!(
        source_video_frame_png(&path, MediaTime::from_nanoseconds(1), &|| false),
        Err(DecodeError::FrameImage(_))
    ));
    assert!(matches!(
        source_video_frame_png(&path, MediaTime::from_nanoseconds(5_000_000_000), &|| false),
        Err(DecodeError::FrameImage(_))
    ));
    assert_eq!(fs::read(&path).expect("frame PNG fixture"), original);
    assert_eq!(
        fs::metadata(&path)
            .expect("frame PNG fixture")
            .modified()
            .expect("frame PNG fixture"),
        modified
    );
    fs::remove_dir_all(root).expect("frame PNG fixture");
}

#[test]
fn native_frame_png_selects_vfr_b_frames_transport_origins_and_rejects_duplicates() {
    let root = std::env::temp_dir().join(format!(
        "towavue-frame-png-pts-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("frame PNG fixture")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("frame PNG fixture");
    let ffmpeg =
        std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("frame PNG fixture"))
            .join("bin/ffmpeg.exe");
    for (extension, codec) in [("mp4", "mpeg4"), ("ts", "mpeg2video")] {
        let path = root.join(format!("source.{extension}"));
        assert!(
            Command::new(&ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=64x48:rate=25:duration=1",
                    "-vf",
                    "select='not(eq(mod(n,3),1))'",
                    "-fps_mode",
                    "vfr",
                    "-c:v",
                    codec,
                    "-g",
                    "8",
                    "-bf",
                    "2"
                ])
                .arg(&path)
                .status()
                .expect("frame PNG fixture")
                .success()
        );
        let original = fs::read(&path).expect("frame PNG fixture");
        let modified = fs::metadata(&path)
            .expect("frame PNG fixture")
            .modified()
            .expect("frame PNG fixture");
        if extension == "ts" {
            assert!(input_origin(&format::input(&path).expect("frame PNG fixture")) > 0);
        }
        // Decode sequentially without the extraction seek path. This also checks
        // that the chosen image is not the first/key frame near the target.
        let mut frames = Vec::new();
        decode_file(&path, |output| {
            if let DecodeOutput::Video(frame) = output {
                frames.push(frame);
            }
            true
        })
        .expect("frame PNG fixture");
        assert!(frames.len() > 10);
        for index in [0, 1, 3, 7, frames.len() - 1] {
            let frame = &frames[index];
            let png = source_video_frame_png(&path, frame.presentation_time, &|| false)
                .expect("frame PNG fixture");
            let (info, actual) = unpack(&png);
            assert_eq!((info.width, info.height), (frame.width, frame.height));
            // Independent sequential selection, explicit high-quality color
            // conversion (the display path uses different swscale flags).
            let reference = Command::new(&ffmpeg).args(["-v","error","-i"]).arg(&path)
                .args(["-vf", &format!("select=eq(n\\,{index}),scale=flags=bilinear+accurate_rnd+full_chroma_int:in_color_matrix=bt601:in_range=limited:out_range=full"),
                    "-frames:v","1","-pix_fmt","rgb24","-f","rawvideo","-"]).output().expect("frame PNG fixture");
            assert!(reference.status.success());
            assert_eq!(actual, reference.stdout, "{extension} frame {index}");
        }
        let calls = std::sync::atomic::AtomicUsize::new(0);
        assert!(matches!(
            source_video_frame_png(&path, frames[7].presentation_time, &|| calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                > 8),
            Err(DecodeError::ConsumerClosed)
        ));
        assert_eq!(fs::read(&path).expect("frame PNG fixture"), original);
        assert_eq!(
            fs::metadata(&path)
                .expect("frame PNG fixture")
                .modified()
                .expect("frame PNG fixture"),
            modified
        );
    }
    let path = root.join("duplicate.mkv");
    assert!(
        Command::new(&ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=32x16:rate=4:duration=1",
                "-vf",
                "setpts=floor(N/2)",
                "-fps_mode",
                "passthrough",
                "-c:v",
                "ffv1"
            ])
            .arg(&path)
            .status()
            .expect("frame PNG fixture")
            .success()
    );
    assert!(
        matches!(source_video_frame_png(&path, MediaTime::ZERO, &|| false),
        Err(DecodeError::FrameImage(message)) if message.contains("duplicate"))
    );
    fs::remove_dir_all(root).expect("frame PNG fixture");
}

#[test]
fn frame_png_retains_icc_bytes_without_overriding_them_with_cicp() {
    use ffmpeg::util::frame::side_data::Type;
    ffmpeg::init().expect("frame PNG fixture");
    let mut source = frame::Video::new(Pixel::RGB48BE, 2, 2);
    source.data_mut(0).fill(42);
    source.set_color_primaries(ffmpeg::color::Primaries::BT2020);
    source.set_color_transfer_characteristic(ffmpeg::color::TransferCharacteristic::SMPTE2084);
    // This test checks byte retention, not the colorimetric validity of a profile.
    let mut profile = vec![0_u8; 132];
    profile[..4].copy_from_slice(&132_u32.to_be_bytes());
    profile[16..20].copy_from_slice(b"RGB ");
    profile[36..40].copy_from_slice(b"acsp");
    let mut side = source
        .new_side_data(Type::IccProfile, profile.len())
        .expect("frame PNG fixture");
    // SAFETY: the generated AVFrame exclusively owns this newly allocated region.
    unsafe {
        std::ptr::copy_nonoverlapping(profile.as_ptr(), (*side.as_mut_ptr()).data, profile.len());
    }
    let encoded =
        encode(&source, VideoOrientation::default(), &[], &|| false).expect("frame PNG fixture");
    let edited = encode(
        &source,
        VideoOrientation::default(),
        &[
            EditOperation::FlipHorizontal,
            EditOperation::RotateVideo(
                towavue_core::VideoRotation::new(123, (2, 2), 1.0).expect("ICC rotation"),
            ),
        ],
        &|| false,
    )
    .expect("edited ICC PNG");
    assert_eq!(chunks(&edited, b"iCCP"), chunks(&encoded, b"iCCP"));
    assert!(chunks(&edited, b"cICP").is_empty());
    let iccp = chunks(&encoded, b"iCCP");
    assert_eq!(iccp.len(), 1);
    let offset = iccp[0]
        .iter()
        .position(|byte| *byte == 0)
        .expect("frame PNG fixture")
        + 2;
    let mut restored = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::ZlibDecoder::new(&iccp[0][offset..]),
        &mut restored,
    )
    .expect("frame PNG fixture");
    assert_eq!(restored, profile);
    assert!(chunks(&encoded, b"cICP").is_empty());
}
