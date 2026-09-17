use super::*;

#[test]
fn interlaced_frame_png_preserves_field_colors_before_edits() {
    ffmpeg::init().expect("FFmpeg");
    let root = std::env::temp_dir().join(format!(
        "towavue-frame-fields-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("owned fixture directory");
    let helper = std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg"))
        .join("bin/ffmpeg.exe");
    for (pixel, name, depth, cw, ch, alpha) in [
        (Pixel::YUV420P, "yuv420p", 8, 16, 8, false),
        (Pixel::YUV420P10LE, "yuv420p10le", 10, 16, 8, false),
        (Pixel::YUV422P12LE, "yuv422p12le", 12, 16, 16, false),
        (Pixel::YUVA420P, "yuva420p", 8, 16, 8, true),
        (Pixel::YUVA420P10LE, "yuva420p10le", 10, 16, 8, true),
        (Pixel::YUV444P16LE, "yuv444p16le", 16, 32, 16, false),
    ] {
        let mut source = frame::Video::new(pixel, 32, 16);
        source.set_color_space(ffmpeg::color::Space::BT709);
        source.set_color_range(ffmpeg::color::Range::MPEG);
        let mut raw = Vec::new();
        let planes = if alpha { 4 } else { 3 };
        for plane in 0..planes {
            source.data_mut(plane).fill(0);
            let (width, height) = if plane == 0 || plane == 3 {
                (32, 16)
            } else {
                (cw, ch)
            };
            for y in 0..height {
                for x in 0..width {
                    let value = (if plane == 0 {
                        96 + (x + y / 2) % 48
                    } else if plane == 3 {
                        (x * 17 + y * 31) % 256
                    } else if (y + plane) % 2 == 0 {
                        80
                    } else {
                        176
                    }) << (depth - 8);
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
        let before: Vec<_> = (0..planes).map(|p| source.data(p).to_vec()).collect();
        let input = root.join(format!("{name}.raw"));
        fs::write(&input, &raw).expect("raw fixture");
        let output_pixel = match (depth == 8, alpha) {
            (true, false) => "rgb24",
            (false, false) => "gbrp16be",
            (true, true) => "rgba",
            (false, true) => "gbrap16be",
        };
        for (location, location_name) in [
            (ffmpeg::ffi::AVChromaLocation::AVCHROMA_LOC_LEFT, "left"),
            (ffmpeg::ffi::AVChromaLocation::AVCHROMA_LOC_CENTER, "center"),
            (
                ffmpeg::ffi::AVChromaLocation::AVCHROMA_LOC_TOPLEFT,
                "topleft",
            ),
        ] {
            let reference = Command::new(&helper)
            .args(["-v", "error", "-f", "rawvideo", "-pixel_format", name,
                "-video_size", "32x16", "-i"])
            .arg(&input)
            .args(["-vf", &format!("scale=interl=1:flags=bilinear+accurate_rnd+full_chroma_int:in_color_matrix=bt709:in_range=limited:out_range=full:in_chroma_loc={location_name}"),
                "-frames:v", "1", "-pix_fmt", output_pixel, "-f", "rawvideo", "-"])
            .output().expect("independent interlaced conversion");
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
                planar_reference_rgb(&reference.stdout, alpha)
            };
            let bytes = (if alpha { 4 } else { 3 }) * if depth == 8 { 1 } else { 2 };
            assert_eq!(reference_pixels.len(), 32 * 16 * bytes);
            for top_first in [false, true] {
                // SAFETY: only scalar properties of the exclusively owned fixture
                // change; all plane allocations and borrowed source pixels stay intact.
                unsafe {
                    (*source.as_mut_ptr()).flags = ffmpeg::ffi::AV_FRAME_FLAG_INTERLACED
                        | if top_first {
                            ffmpeg::ffi::AV_FRAME_FLAG_TOP_FIELD_FIRST
                        } else {
                            0
                        };
                    (*source.as_mut_ptr()).chroma_location = location;
                    (*source.as_mut_ptr()).alpha_mode =
                        ffmpeg::ffi::AVAlphaMode::AVALPHA_MODE_STRAIGHT;
                }
                for flip in [false, true] {
                    let operations = if flip {
                        vec![EditOperation::FlipVertical]
                    } else {
                        vec![]
                    };
                    if !top_first && !flip && ch == 8 {
                        // A progressive interpretation must fail this reference;
                        // otherwise the fixture cannot expose mixing between fields.
                        // SAFETY: scalar flags on an exclusively owned fixture.
                        unsafe {
                            (*source.as_mut_ptr()).flags = 0;
                        }
                        let progressive =
                            encode(&source, VideoOrientation::default(), &[], &|| false)
                                .expect("progressive negative control");
                        assert_ne!(unpack(&progressive).1, reference_pixels);
                        // SAFETY: restore the same borrowed fixture's field flag.
                        unsafe {
                            (*source.as_mut_ptr()).flags = ffmpeg::ffi::AV_FRAME_FLAG_INTERLACED;
                        }
                    }
                    let png = encode(&source, VideoOrientation::default(), &operations, &|| false)
                        .expect("interlaced PNG");
                    let (info, actual) = unpack(&png);
                    assert_eq!(
                        info.bit_depth,
                        if depth == 8 {
                            png::BitDepth::Eight
                        } else {
                            png::BitDepth::Sixteen
                        }
                    );
                    let expected: Vec<_> = (0..16)
                        .flat_map(|y| {
                            let sy = if flip { 15 - y } else { y };
                            reference_pixels[sy * 32 * bytes..(sy + 1) * 32 * bytes]
                                .iter()
                                .copied()
                        })
                        .collect();
                    assert!(
                        actual == expected,
                        "field colors differ: {name}, top_first={top_first}, flip={flip}, location={location_name}"
                    );
                    for (plane, expected) in before.iter().enumerate() {
                        assert_eq!(source.data(plane), expected);
                    }
                }
            }
            if !alpha && ch == 8 && location_name == "left" {
                // FFV1 preserves these source planes while the container/decoder
                // supplies the field flags used by the public PNG extraction path.
                let video = root.join(format!("{name}.mkv"));
                let encoded = Command::new(&helper)
                    .args([
                        "-v",
                        "error",
                        "-f",
                        "rawvideo",
                        "-pixel_format",
                        name,
                        "-video_size",
                        "32x16",
                        "-colorspace",
                        "bt709",
                        "-color_range",
                        "tv",
                        "-chroma_sample_location",
                        "left",
                        "-i",
                    ])
                    .arg(&input)
                    .args([
                        "-vf",
                        "setfield=tff",
                        "-c:v",
                        "ffv1",
                        "-level",
                        "3",
                        "-field_order",
                        "tt",
                        "-colorspace",
                        "bt709",
                        "-color_range",
                        "tv",
                        "-chroma_sample_location",
                        "left",
                    ])
                    .arg(&video)
                    .output()
                    .expect("interlaced FFV1 fixture");
                assert!(
                    encoded.status.success(),
                    "{}",
                    String::from_utf8_lossy(&encoded.stderr)
                );
                let decoded = Command::new(&helper)
                    .args(["-v", "error", "-i"])
                    .arg(&video)
                    .args([
                        "-pix_fmt",
                        name,
                        "-chroma_sample_location",
                        "left",
                        "-f",
                        "rawvideo",
                        "-",
                    ])
                    .output()
                    .expect("independent lossless decode");
                assert!(decoded.status.success());
                assert_eq!(
                    decoded.stdout, raw,
                    "fixture must not resample source planes"
                );
                let probe = Command::new(helper.with_file_name("ffprobe.exe"))
                    .args([
                        "-v",
                        "error",
                        "-show_entries",
                        "frame=interlaced_frame,top_field_first",
                        "-of",
                        "csv=p=0",
                    ])
                    .arg(&video)
                    .output()
                    .expect("frame field metadata");
                assert!(probe.status.success());
                assert_eq!(
                    String::from_utf8(probe.stdout)
                        .expect("field metadata CSV")
                        .trim(),
                    "1,1"
                );
                let modified = fs::metadata(&video)
                    .expect("source stamp")
                    .modified()
                    .expect("source time");
                let source_bytes = fs::read(&video).expect("video bytes");
                let png = source_video_frame_png(&video, MediaTime::ZERO, &|| false)
                    .expect("interlaced container PNG");
                assert!(
                    unpack(&png).1 == reference_pixels,
                    "container field colors: {name}"
                );
                assert_eq!(fs::read(&video).expect("unchanged source"), source_bytes);
                assert_eq!(
                    fs::metadata(&video)
                        .expect("source stamp")
                        .modified()
                        .expect("source time"),
                    modified
                );
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
