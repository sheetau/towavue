use super::*;

#[test]
fn high_depth_raster_edits_transform_full_resolution_chroma_before_subsampling() {
    let root = depth_directory();
    let source = root.join("source.mkv");
    let reference = root.join("reference.yuv");
    for (width, height) in [(32, 24), (96, 64)] {
        for pixel in ["yuv420p10le", "yuv420p12le"] {
            for (location, vertical) in [("left", 128), ("topleft", 0)] {
                run(
                    "ffmpeg.exe",
                    &[
                        "-v",
                        "error",
                        "-y",
                        "-f",
                        "lavfi",
                        "-i",
                        &format!("testsrc2=size={width}x{height}:rate=1:duration=1,format={pixel}"),
                        "-c:v",
                        "ffv1",
                        "-chroma_sample_location",
                        location,
                        source.to_str().expect("path"),
                    ],
                );
                // Independently reconstruct each full-resolution component, then
                // transform its pixels in Rust rather than reusing export filters.
                let reconstruct = format!(
                    "scale=flags=bilinear+accurate_rnd:in_h_chr_pos=0:in_v_chr_pos={vertical},format=yuv444p16le"
                );
                let full = run(
                    "ffmpeg.exe",
                    &[
                        "-v",
                        "error",
                        "-i",
                        source.to_str().expect("path"),
                        "-vf",
                        &reconstruct,
                        "-pix_fmt",
                        "yuv444p16le",
                        "-f",
                        "rawvideo",
                        "-",
                    ],
                )
                .stdout;
                assert_eq!(full.len(), width as usize * height as usize * 2 * 3);
                let original = fs::read(&source).expect("source bytes");
                for (name, operations) in [
                    ("horizontal", vec![EditOperation::FlipHorizontal]),
                    ("vertical", vec![EditOperation::FlipVertical]),
                    ("quarter", vec![EditOperation::RotateClockwise]),
                    (
                        "odd_crop",
                        vec![EditOperation::Crop(towavue_core::PixelCrop {
                            x: 1,
                            y: 3,
                            width: 24,
                            height: 18,
                        })],
                    ),
                    (
                        "combined",
                        vec![
                            EditOperation::Crop(towavue_core::PixelCrop {
                                x: 3,
                                y: 1,
                                width: 24,
                                height: 20,
                            }),
                            EditOperation::FlipHorizontal,
                            EditOperation::RotateCounterclockwise,
                        ],
                    ),
                ] {
                    let mut planes = Vec::new();
                    let mut dimensions = (0, 0);
                    for bytes in full.chunks_exact(width as usize * height as usize * 2) {
                        let samples: Vec<_> = bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|bytes| u16::from_le_bytes(*bytes))
                            .collect();
                        let mut plane = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(
                            width, height, samples,
                        )
                        .expect("component");
                        for operation in &operations {
                            plane = match *operation {
                                EditOperation::FlipHorizontal => {
                                    image::imageops::flip_horizontal(&plane)
                                }
                                EditOperation::FlipVertical => {
                                    image::imageops::flip_vertical(&plane)
                                }
                                EditOperation::RotateClockwise => image::imageops::rotate90(&plane),
                                EditOperation::RotateCounterclockwise => {
                                    image::imageops::rotate270(&plane)
                                }
                                EditOperation::Crop(crop) => image::imageops::crop_imm(
                                    &plane,
                                    crop.x,
                                    crop.y,
                                    crop.width,
                                    crop.height,
                                )
                                .to_image(),
                                _ => unreachable!("geometric reference cases"),
                            };
                        }
                        dimensions = plane.dimensions();
                        planes.extend(plane.into_raw().into_iter().flat_map(u16::to_le_bytes));
                    }
                    fs::write(&reference, &planes).expect("independently transformed planes");
                    let subsample = format!(
                        "scale=flags=bilinear+accurate_rnd:out_h_chr_pos=0:out_v_chr_pos={vertical},format={pixel}"
                    );
                    let expected = run(
                        "ffmpeg.exe",
                        &[
                            "-v",
                            "error",
                            "-f",
                            "rawvideo",
                            "-pixel_format",
                            "yuv444p16le",
                            "-video_size",
                            &format!("{}x{}", dimensions.0, dimensions.1),
                            "-i",
                            reference.to_str().expect("path"),
                            "-vf",
                            &subsample,
                            "-pix_fmt",
                            pixel,
                            "-f",
                            "rawvideo",
                            "-",
                        ],
                    )
                    .stdout;
                    let request = ExportRequest {
                        source: source.clone(),
                        target: root.join("output.mkv"),
                        kind: MediaKind::Video,
                        operations,
                        hardware_encode: false,
                    };
                    let streams = ExportStreams {
                        video_encoding: video_encoding::HighDepth::probe(&request)
                            .expect("encoding plan"),
                        ..Default::default()
                    };
                    let filters = streams.visual_filters(&request.operations).join(",");
                    let actual = run(
                        "ffmpeg.exe",
                        &[
                            "-v",
                            "error",
                            "-i",
                            source.to_str().expect("path"),
                            "-vf",
                            &filters,
                            "-pix_fmt",
                            pixel,
                            "-f",
                            "rawvideo",
                            "-",
                        ],
                    )
                    .stdout;
                    assert!(
                        actual == expected,
                        "chroma geometry: {pixel}, {location}, {name}"
                    );
                    fs::write(&request.target, b"existing target").expect("replacement target");
                    export_media(&request).expect("public AV1 export");
                    let encoded = run(
                        "ffmpeg.exe",
                        &[
                            "-v",
                            "error",
                            "-i",
                            request.target.to_str().expect("target path"),
                            "-pix_fmt",
                            pixel,
                            "-f",
                            "rawvideo",
                            "-",
                        ],
                    )
                    .stdout;
                    assert_eq!(
                        encoded.len(),
                        expected.len(),
                        "encoded geometry and frame count"
                    );
                    let squared_error: f64 = encoded
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .zip(expected.as_chunks::<2>().0)
                        .map(|(a, b)| {
                            (f64::from(u16::from_le_bytes(*a)) - f64::from(u16::from_le_bytes(*b)))
                                .powi(2)
                        })
                        .sum();
                    let samples = encoded.len() / 2;
                    let peak: f64 = if pixel == "yuv420p10le" {
                        1023.0
                    } else {
                        4095.0
                    };
                    let psnr = 10.0 * (peak.powi(2) / (squared_error / samples as f64)).log10();
                    eprintln!(
                        "CHROMA_GEOMETRY {width}x{height} {pixel} {location} {name}: {psnr:.3} dB"
                    );
                    assert!(psnr >= 45.0, "encoded chroma geometry: {psnr:.3} dB");
                    assert_eq!(fs::read(&source).expect("unchanged source"), original);
                }
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
