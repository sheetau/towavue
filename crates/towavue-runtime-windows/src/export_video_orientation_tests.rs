use super::*;

fn matrix(path: &Path) -> [i32; 4] {
    let input = ffmpeg::format::input(path).expect("fixture input");
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .expect("video");
    let side = stream
        .side_data()
        .find(|data| data.kind() == ffmpeg::codec::packet::side_data::Type::DisplayMatrix);
    let Some(side) = side else {
        return [1, 0, 0, 1];
    };
    let bytes = side.data();
    assert_eq!(bytes.len(), 36);
    [0, 1, 3, 4].map(|index| {
        let value = i32::from_ne_bytes(
            bytes[index * 4..index * 4 + 4]
                .try_into()
                .expect("matrix entry"),
        );
        assert_eq!(value % 65536, 0);
        value / 65536
    })
}

// Independent integer-coordinate mapping, without VideoOrientation's corner
// table or the production FFmpeg transpose/flip strings.
fn orient_planes(full: &[u8], linear: [i32; 4]) -> (Vec<u8>, (u32, u32)) {
    let [a, b, c, d] = linear;
    let points = [(0, 0), (95, 0), (0, 63), (95, 63)].map(|(x, y)| (a * x + c * y, b * x + d * y));
    let min_x = points.iter().map(|p| p.0).min().expect("four corners");
    let min_y = points.iter().map(|p| p.1).min().expect("four corners");
    let width = (points.iter().map(|p| p.0).max().expect("four corners") - min_x + 1) as u32;
    let height = (points.iter().map(|p| p.1).max().expect("four corners") - min_y + 1) as u32;
    let mut result = vec![0; full.len()];
    for plane in 0..3 {
        for y in 0..64_i32 {
            for x in 0..96_i32 {
                let source = (plane * 96 * 64 + y * 96 + x) as usize * 2;
                let dx = a * x + c * y - min_x;
                let dy = b * x + d * y - min_y;
                let target = (plane * 96 * 64 + dy * width as i32 + dx) as usize * 2;
                result[target..target + 2].copy_from_slice(&full[source..source + 2]);
            }
        }
    }
    (result, (width, height))
}

#[test]
fn high_depth_display_matrices_transform_chroma_before_edits_and_clear_output_orientation() {
    let root = depth_directory();
    let encoded = root.join("encoded.mp4");
    let source = root.join("oriented.mp4");
    let reference = root.join("reference.yuv");
    for pixel in ["yuv420p10le", "yuv420p12le"] {
        for (location, vertical, declaration) in
            [("left", 128, "vertical"), ("topleft", 0, "colocated")]
        {
            run(
                "ffmpeg.exe",
                &[
                    "-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("testsrc2=size=96x64:rate=1:duration=1,format={pixel},setsar=2"),
                    "-c:v",
                    "libaom-av1",
                    "-crf",
                    "0",
                    "-cpu-used",
                    "6",
                    "-threads:v",
                    "4",
                    "-chroma_sample_location",
                    location,
                    "-bsf:v",
                    &format!("av1_metadata=chroma_sample_position={declaration}"),
                    encoded.to_str().expect("fixture path"),
                ],
            );
            let reconstruct = format!(
                "scale=flags=bilinear+accurate_rnd:in_h_chr_pos=0:in_v_chr_pos={vertical},format=yuv444p16le"
            );
            let full = run(
                "ffmpeg.exe",
                &[
                    "-v",
                    "error",
                    "-noautorotate",
                    "-i",
                    encoded.to_str().expect("fixture path"),
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
            assert_eq!(full.len(), 96 * 64 * 3 * 2);
            let mut matrices = Vec::new();
            for (index, (angle, flip)) in [
                (0, false),
                (90, false),
                (180, false),
                (270, false),
                (0, true),
                (90, true),
                (180, true),
                (270, true),
            ]
            .into_iter()
            .enumerate()
            {
                run(
                    "ffmpeg.exe",
                    &[
                        "-v",
                        "error",
                        "-y",
                        "-display_rotation",
                        &angle.to_string(),
                        if flip {
                            "-display_hflip"
                        } else {
                            "-nodisplay_hflip"
                        },
                        "-i",
                        encoded.to_str().expect("fixture path"),
                        "-c",
                        "copy",
                        source.to_str().expect("fixture path"),
                    ],
                );
                let linear = matrix(&source);
                assert!(!matrices.contains(&linear), "eight distinct orientations");
                matrices.push(linear);
                let original = fs::read(&source).expect("source bytes");
                let modified = fs::metadata(&source)
                    .expect("source metadata")
                    .modified()
                    .expect("source mtime");
                let (oriented, size) = orient_planes(&full, linear);
                for edited in [false, true] {
                    let operations = if edited {
                        vec![
                            EditOperation::Crop(towavue_core::PixelCrop {
                                x: 3,
                                y: 1,
                                width: 48,
                                height: 40,
                            }),
                            EditOperation::FlipVertical,
                        ]
                    } else {
                        Vec::new()
                    };
                    let output_size = if edited { (48, 40) } else { size };
                    let mut planes = Vec::new();
                    for bytes in oriented.chunks_exact((size.0 * size.1 * 2) as usize) {
                        let values = bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|v| u16::from_le_bytes(*v))
                            .collect();
                        let plane = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(
                            size.0, size.1, values,
                        )
                        .expect("valid fixture");
                        let plane = if edited {
                            image::imageops::flip_vertical(
                                &image::imageops::crop_imm(&plane, 3, 1, 48, 40).to_image(),
                            )
                        } else {
                            plane
                        };
                        planes.extend(plane.into_raw().into_iter().flat_map(u16::to_le_bytes));
                    }
                    fs::write(&reference, planes).expect("reference planes");
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
                            &format!("{}x{}", output_size.0, output_size.1),
                            "-i",
                            reference.to_str().expect("fixture path"),
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
                        target: root.join(format!("output.{}", ["mp4", "mkv", "webm"][index % 3])),
                        kind: MediaKind::Video,
                        operations,
                        hardware_encode: true,
                    };
                    let mut streams = ExportStreams::probe(&request).expect("source streams");
                    streams.video_encoding =
                        video_encoding::HighDepth::probe(&request).expect("encoding plan");
                    // Preserve production input options, but collect lossless raw
                    // samples before encoding so a compression tolerance cannot
                    // hide a geometric error.
                    let mut arguments = ffmpeg_arguments(&request, false, &streams);
                    arguments.truncate(
                        arguments
                            .iter()
                            .position(|a| a == "-map_metadata")
                            .expect("valid fixture"),
                    );
                    let progress = arguments
                        .iter()
                        .position(|a| a == "-progress")
                        .expect("valid fixture");
                    arguments.drain(progress..progress + 2);
                    let filters = streams.visual_filters(&request.operations);
                    if !filters.is_empty() {
                        arguments.extend(["-vf".into(), filters.join(",")]);
                    }
                    arguments.extend([
                        "-pix_fmt".into(),
                        pixel.into(),
                        "-f".into(),
                        "rawvideo".into(),
                        "-".into(),
                    ]);
                    let actual = run(
                        "ffmpeg.exe",
                        &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
                    )
                    .stdout;
                    // The identity/no-edit route intentionally avoids resampling.
                    if linear != [1, 0, 0, 1] || edited {
                        assert!(
                            actual == expected,
                            "orientation sample geometry: {pixel}, {location}, {linear:?}, edited={edited}"
                        );
                    }
                    fs::write(&request.target, b"existing target").expect("existing target");
                    export_media(&request).expect("oriented public export");
                    let decoded = run(
                        "ffmpeg.exe",
                        &[
                            "-v",
                            "error",
                            "-noautorotate",
                            "-i",
                            request.target.to_str().expect("fixture path"),
                            "-pix_fmt",
                            pixel,
                            "-f",
                            "rawvideo",
                            "-",
                        ],
                    )
                    .stdout;
                    assert_eq!(
                        decoded.len(),
                        actual.len(),
                        "encoded dimensions and frame count"
                    );
                    let error: f64 = decoded
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .zip(actual.as_chunks::<2>().0)
                        .map(|(a, b)| {
                            (f64::from(u16::from_le_bytes(*a)) - f64::from(u16::from_le_bytes(*b)))
                                .powi(2)
                        })
                        .sum();
                    let peak: f64 = if pixel == "yuv420p10le" {
                        1023.0
                    } else {
                        4095.0
                    };
                    let psnr = 10.0 * (peak.powi(2) / (error / (decoded.len() / 2) as f64)).log10();
                    assert!(
                        psnr >= 45.0,
                        "oriented AV1 quality: {pixel}, {location}, {linear:?}, edited={edited}: {psnr:.3} dB"
                    );
                    if index == 1 && !edited {
                        let wrong = root.join("wrong-orientation.mp4");
                        run(
                            "ffmpeg.exe",
                            &[
                                "-v",
                                "error",
                                "-y",
                                "-display_rotation",
                                "90",
                                "-i",
                                request.target.to_str().expect("fixture path"),
                                "-c",
                                "copy",
                                wrong.to_str().expect("fixture path"),
                            ],
                        );
                        let unchanged = run(
                            "ffmpeg.exe",
                            &[
                                "-v",
                                "error",
                                "-noautorotate",
                                "-i",
                                wrong.to_str().expect("fixture path"),
                                "-pix_fmt",
                                pixel,
                                "-f",
                                "rawvideo",
                                "-",
                            ],
                        )
                        .stdout;
                        assert_eq!(
                            unchanged, decoded,
                            "negative control changes only orientation"
                        );
                        assert!(
                            streams
                                .video_encoding
                                .as_ref()
                                .expect("valid fixture")
                                .verify(&wrong)
                                .is_err(),
                            "publication must reject double orientation"
                        );
                    }
                    assert_eq!(
                        matrix(&request.target),
                        [1, 0, 0, 1],
                        "orientation must be baked only once"
                    );
                    let sar = if linear[0] == 0 { "1:2" } else { "2:1" };
                    let fields = run(
                        "ffprobe.exe",
                        &[
                            "-v",
                            "error",
                            "-select_streams",
                            "v:0",
                            "-show_entries",
                            "stream=width,height,sample_aspect_ratio,pix_fmt,chroma_location",
                            "-of",
                            "default=noprint_wrappers=1",
                            request.target.to_str().expect("fixture path"),
                        ],
                    );
                    let fields = String::from_utf8(fields.stdout).expect("probe fields");
                    for field in [
                        format!("width={}", output_size.0),
                        format!("height={}", output_size.1),
                        format!("sample_aspect_ratio={sar}"),
                        format!("pix_fmt={pixel}"),
                        format!("chroma_location={location}"),
                    ] {
                        assert!(
                            fields.lines().any(|line| line == field),
                            "missing {field}: {fields}"
                        );
                    }
                    assert_eq!(fs::read(&source).expect("source bytes"), original);
                    assert_eq!(
                        fs::metadata(&source)
                            .expect("source metadata")
                            .modified()
                            .expect("source mtime"),
                        modified
                    );
                }
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn oriented_high_depth_trim_and_timeline_keep_video_audio_aligned_with_input_seek() {
    use towavue_core::{MediaTime, TimeRange, TimelineEdit};
    let root = depth_directory();
    let encoded = root.join("encoded.mkv");
    let source = root.join("source.mp4");
    depth_fixture(
        &encoded,
        "yuv420p10le",
        "96x64",
        &[
            "-color_range",
            "tv",
            "-colorspace",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_primaries",
            "bt709",
            "-chroma_sample_location",
            "left",
            "-bsf:v",
            "av1_metadata=chroma_sample_position=vertical",
        ],
    );
    run(
        "ffmpeg.exe",
        &[
            "-v",
            "error",
            "-display_rotation",
            "90",
            "-i",
            encoded.to_str().expect("fixture path"),
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            source.to_str().expect("fixture path"),
        ],
    );
    let original = fs::read(&source).expect("source bytes");
    let modified = fs::metadata(&source)
        .expect("source metadata")
        .modified()
        .expect("source mtime");
    let start = MediaTime::from_nanoseconds(1_250_000_000);
    let end = MediaTime::from_nanoseconds(1_750_000_000);
    let mut previous = None;
    for timeline in [false, true] {
        let mut operations = if timeline {
            vec![EditOperation::Timeline(TimelineEdit::Keep(
                TimeRange::new(start, end).expect("trim interval"),
            ))]
        } else {
            vec![
                EditOperation::SetTrimStart(start),
                EditOperation::SetTrimEnd(end),
            ]
        };
        operations.push(EditOperation::Crop(towavue_core::PixelCrop {
            x: 3,
            y: 1,
            width: 48,
            height: 40,
        }));
        let request = ExportRequest {
            source: source.clone(),
            target: root.join(format!("output-{timeline}.mkv")),
            kind: MediaKind::Video,
            operations,
            hardware_encode: true,
        };
        let mut streams = ExportStreams::probe(&request).expect("source streams");
        streams.video_encoding = video_encoding::HighDepth::probe(&request).expect("encoding plan");
        if timeline {
            streams.timeline = towavue_core::EditTimeline::from_operations(
                streams.duration.expect("source duration"),
                &request.operations,
            );
            assert!(streams.timeline.is_some());
        }
        // Both forms start far enough into the source to use a separate video
        // input seek, while audio retains sequential decoder priming.
        assert!(video_input_seek(&request, &streams).is_some());
        let args = ffmpeg_arguments(&request, false, &streams);
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 2);
        assert_eq!(args.iter().filter(|a| *a == "-noautorotate").count(), 1);
        export_media(&request).expect("oriented trimmed export");
        assert_video_fields(
            &request.target,
            &[
                "width=48",
                "height=40",
                "nb_read_frames=4",
                "pix_fmt=yuv420p10le",
                "chroma_location=left",
            ],
        );
        assert_eq!(matrix(&request.target), [1, 0, 0, 1]);
        let video = run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-i",
                request.target.to_str().expect("fixture path"),
                "-map",
                "0:v:0",
                "-pix_fmt",
                "yuv420p10le",
                "-f",
                "rawvideo",
                "-",
            ],
        )
        .stdout;
        let audio = run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-i",
                request.target.to_str().expect("fixture path"),
                "-map",
                "0:a:0",
                "-f",
                "f32le",
                "-",
            ],
        )
        .stdout;
        assert!(!audio.is_empty());
        if let Some((previous_video, previous_audio)) = &previous {
            assert_eq!(&video, previous_video, "trim/timeline decoded pictures");
            assert_eq!(&audio, previous_audio, "trim/timeline decoded audio");
        }
        previous = Some((video, audio));
        assert_eq!(fs::read(&source).expect("source bytes"), original);
        assert_eq!(
            fs::metadata(&source)
                .expect("source metadata")
                .modified()
                .expect("source mtime"),
            modified
        );
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
