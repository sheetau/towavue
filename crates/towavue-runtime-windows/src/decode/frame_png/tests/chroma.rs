use super::*;
use ffmpeg::ffi::AVChromaLocation::*;

#[test]
fn frame_png_uses_declared_chroma_positions_before_raster_edits() {
    ffmpeg::init().expect("FFmpeg");
    let root = std::env::temp_dir().join(format!(
        "towavue-frame-chroma-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("owned fixture directory");
    let ffmpeg = std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg"))
        .join("bin/ffmpeg.exe");
    for (pixel, name, depth, chroma_width, chroma_height) in [
        (Pixel::YUV420P, "yuv420p", 8, 8, 4),
        (Pixel::YUV420P10LE, "yuv420p10le", 10, 8, 4),
        (Pixel::YUV422P12LE, "yuv422p12le", 12, 8, 8),
        (Pixel::YUV444P16LE, "yuv444p16le", 16, 16, 8),
    ] {
        let mut source = frame::Video::new(pixel, 16, 8);
        source.set_color_space(ffmpeg::color::Space::BT709);
        source.set_color_range(ffmpeg::color::Range::MPEG);
        let mut raw = Vec::new();
        for plane in 0..3 {
            source.data_mut(plane).fill(0);
            let width = if plane == 0 { 16 } else { chroma_width };
            let height = if plane == 0 { 8 } else { chroma_height };
            for y in 0..height {
                for x in 0..width {
                    let value = if plane == 0 {
                        128
                    } else {
                        [80, 128, 176][(x + y * 2 + plane) % 3]
                    } << (depth - 8);
                    let stride = source.stride(plane);
                    if depth == 8 {
                        source.data_mut(plane)[y * stride + x] = value as u8;
                        raw.push(value as u8);
                    } else {
                        let bytes = (value as u16).to_le_bytes();
                        source.data_mut(plane)[y * stride + x * 2..y * stride + x * 2 + 2]
                            .copy_from_slice(&bytes);
                        raw.extend(bytes);
                    }
                }
            }
        }
        let input = root.join(format!("{name}.raw"));
        fs::write(&input, &raw).expect("raw fixture");
        let mut results = Vec::new();
        for (location, horizontal, vertical) in [
            (AVCHROMA_LOC_UNSPECIFIED, 128, 128),
            (AVCHROMA_LOC_LEFT, 0, 128),
            (AVCHROMA_LOC_CENTER, 128, 128),
            (AVCHROMA_LOC_TOPLEFT, 0, 0),
            (AVCHROMA_LOC_TOP, 128, 0),
            (AVCHROMA_LOC_BOTTOMLEFT, 0, 256),
            (AVCHROMA_LOC_BOTTOM, 128, 256),
        ] {
            // SAFETY: the fixture exclusively owns the frame; only a scalar
            // chroma-location property changes, never its buffers or ownership.
            unsafe {
                (*source.as_mut_ptr()).chroma_location = location;
            }
            let output_pixel = if depth == 8 { "rgb24" } else { "gbrp16be" };
            // Explicit independent coordinates ensure a conversion that silently
            // ignores metadata cannot also define the expected pixels.
            let reference = Command::new(&ffmpeg)
                .args(["-v", "error", "-f", "rawvideo", "-pixel_format", name,
                    "-video_size", "16x8", "-i"])
                .arg(&input)
                .args(["-vf", &format!(
                    "scale=flags=bilinear+accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=limited:out_range=full:in_h_chr_pos={horizontal}:in_v_chr_pos={vertical}"),
                    "-frames:v", "1", "-pix_fmt", output_pixel, "-f", "rawvideo", "-"])
                .output().expect("explicit-location reference");
            assert!(
                reference.status.success(),
                "{}",
                String::from_utf8_lossy(&reference.stderr)
            );
            // Packed 16-bit full-chroma output is the defective path under test.
            // Use planar CLI samples here; the numeric regression independently
            // verifies colors instead of trusting either scaler layout.
            let reference_pixels = if depth == 8 {
                reference.stdout
            } else {
                planar_reference_rgb(&reference.stdout, false)
            };
            let bytes = if depth == 8 { 3 } else { 6 };
            assert_eq!(reference_pixels.len(), 16 * 8 * bytes);
            for flip in [false, true] {
                let operations = if flip {
                    vec![EditOperation::FlipHorizontal]
                } else {
                    vec![]
                };
                let png = encode(&source, VideoOrientation::default(), &operations, &|| false)
                    .expect("chroma-position PNG");
                let (info, actual) = unpack(&png);
                assert_eq!(
                    info.bit_depth,
                    if depth == 8 {
                        png::BitDepth::Eight
                    } else {
                        png::BitDepth::Sixteen
                    }
                );
                let mut expected = Vec::new();
                for y in 0..8 {
                    for x in 0..16 {
                        let sx = if flip { 15 - x } else { x };
                        let at = (y * 16 + sx) * bytes;
                        expected.extend_from_slice(&reference_pixels[at..at + bytes]);
                    }
                }
                assert_eq!(actual, expected, "{name}, {location:?}, flip={flip}");
            }
            if depth == 10 && matches!(location, AVCHROMA_LOC_LEFT | AVCHROMA_LOC_TOPLEFT) {
                let video = root.join(format!("{name}-{}.mkv", location as i32));
                let encoded = Command::new(&ffmpeg)
                    .args([
                        "-v",
                        "error",
                        "-f",
                        "rawvideo",
                        "-pixel_format",
                        name,
                        "-video_size",
                        "16x8",
                        "-color_range",
                        "tv",
                        "-colorspace",
                        "bt709",
                        "-chroma_sample_location",
                        &(location as i32).to_string(),
                        "-i",
                    ])
                    .arg(&input)
                    .args([
                        "-c:v",
                        "ffv1",
                        "-level",
                        "3",
                        "-color_range",
                        "tv",
                        "-colorspace",
                        "bt709",
                        "-chroma_sample_location",
                        &(location as i32).to_string(),
                    ])
                    .arg(&video)
                    .output()
                    .expect("tagged FFV1 container");
                assert!(
                    encoded.status.success(),
                    "{}",
                    String::from_utf8_lossy(&encoded.stderr)
                );
                let input_video = format::input(&video).expect("tagged container input");
                let parameters = input_video
                    .streams()
                    .best(Type::Video)
                    .expect("video")
                    .parameters();
                // SAFETY: borrowed live stream parameters; scalar metadata only.
                assert_eq!(
                    unsafe { (*parameters.as_ptr()).chroma_location },
                    location,
                    "the container fixture must actually retain its location tag"
                );
                drop(input_video);
                // Tag the raw input as well as the encoded output: otherwise
                // FFmpeg may resample chroma while changing the output tags.
                let decoded = Command::new(&ffmpeg)
                    .args(["-v", "error", "-i"])
                    .arg(&video)
                    .args([
                        "-pix_fmt",
                        name,
                        "-chroma_sample_location",
                        &(location as i32).to_string(),
                        "-f",
                        "rawvideo",
                        "-",
                    ])
                    .output()
                    .expect("lossless container fixture decode");
                assert!(
                    decoded.status.success(),
                    "{}",
                    String::from_utf8_lossy(&decoded.stderr)
                );
                assert_eq!(
                    decoded.stdout, raw,
                    "container retains unresampled YUV samples"
                );
                let original = fs::read(&video).expect("source video bytes");
                let modified = fs::metadata(&video)
                    .expect("source video metadata")
                    .modified()
                    .expect("source video time");
                let png = source_video_frame_png(&video, MediaTime::ZERO, &|| false)
                    .expect("native tagged-frame extraction");
                assert_eq!(
                    unpack(&png).1,
                    reference_pixels,
                    "container chroma {location:?}"
                );
                assert_eq!(fs::read(&video).expect("source video bytes"), original);
                assert_eq!(
                    fs::metadata(&video)
                        .expect("source video metadata")
                        .modified()
                        .expect("source video time"),
                    modified
                );
            }
            results.push(reference_pixels);
        }
        if chroma_width == 8 {
            assert_ne!(
                results[1], results[2],
                "horizontal chroma fixture sensitivity"
            );
        } else {
            assert!(
                results.iter().all(|result| result == &results[0]),
                "full-resolution chroma has no siting offset"
            );
        }
        if chroma_height == 4 {
            assert_ne!(
                results[3], results[5],
                "vertical chroma fixture sensitivity"
            );
        }
        assert_eq!(fs::read(&input).expect("unchanged raw fixture"), raw);
    }
    fs::remove_dir_all(root).expect("remove owned fixtures");
}
