use super::*;

#[test]
fn webm_frame_png_retains_decoded_alpha_through_selection_and_edits() {
    let root = std::env::temp_dir().join(format!(
        "towavue-frame-webm-alpha-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("fixture directory");
    let ffmpeg = std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg"))
        .join("bin/ffmpeg.exe");
    let raw = root.join("source.yuva");
    let mut samples = Vec::new();
    for index in 0..6 {
        samples.extend((0..512).map(|i| (32 + (i + index * 13) % 180) as u8));
        samples.extend(std::iter::repeat_n(112, 128));
        samples.extend(std::iter::repeat_n(144, 128));
        samples.extend((0..512).map(|i| [0, 51, 102, 153, 204, 255][(i / 8 + index) % 6]));
    }
    fs::write(&raw, &samples).expect("raw fixture");
    for codec in ["libvpx", "libvpx-vp9"] {
        let path = root.join(format!("{codec}.webm"));
        let mut encode = Command::new(&ffmpeg);
        encode
            .args([
                "-v",
                "error",
                "-f",
                "rawvideo",
                "-pixel_format",
                "yuva420p",
                "-video_size",
                "32x16",
                "-framerate",
                "2",
                "-i",
            ])
            .arg(&raw)
            .args([
                "-c:v",
                codec,
                "-auto-alt-ref",
                "0",
                "-g",
                "3",
                "-threads",
                "1",
            ]);
        if codec == "libvpx-vp9" {
            encode.args(["-lossless", "1"]);
        } else {
            encode.args(["-crf", "4", "-b:v", "1M"]);
        }
        let result = encode.arg(&path).output().expect("encode fixture");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let original = fs::read(&path).expect("source bytes");
        let modified = fs::metadata(&path)
            .expect("source metadata")
            .modified()
            .expect("source time");
        for index in [0, 2, 5] {
            // Sequential alpha-capable native decode is independent of the
            // application's decoder choice and source-PTS seek path.
            let reference = Command::new(&ffmpeg)
                .args(["-v", "error", "-c:v", codec, "-i"]).arg(&path)
                .args([
                    "-vf", &format!("select=eq(n\\,{index}),scale=flags=bilinear+accurate_rnd+full_chroma_int:in_color_matrix=bt601:in_range=limited:out_range=full"),
                    "-frames:v", "1", "-pix_fmt", "rgba", "-f", "rawvideo", "-",
                ]).output().expect("reference decode");
            assert!(
                reference.status.success(),
                "{}",
                String::from_utf8_lossy(&reference.stderr)
            );
            assert_eq!(reference.stdout.len(), 32 * 16 * 4);
            assert!(
                reference
                    .stdout
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] == 0)
            );
            assert!(
                reference
                    .stdout
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] > 0 && pixel[3] < 255)
            );
            if codec == "libvpx-vp9" {
                let start = index * 1280 + 768;
                assert_eq!(
                    reference
                        .stdout
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| p[3])
                        .collect::<Vec<_>>(),
                    samples[start..start + 512],
                    "lossless fixture preserves original alpha"
                );
            }
            for edited in [false, true] {
                let operations = if edited {
                    vec![
                        EditOperation::FlipHorizontal,
                        EditOperation::Crop(towavue_core::PixelCrop {
                            x: 4,
                            y: 2,
                            width: 24,
                            height: 12,
                        }),
                    ]
                } else {
                    Vec::new()
                };
                let png = edited_video_frame_png(
                    &path,
                    MediaTime::from_nanoseconds(index as i64 * 500_000_000),
                    &operations,
                    &|| false,
                )
                .expect("WebM PNG export");
                let (info, actual) = unpack(&png);
                assert_eq!(
                    info.color_type,
                    png::ColorType::Rgba,
                    "{codec} frame {index}"
                );
                assert_eq!(info.bit_depth, png::BitDepth::Eight);
                let (width, height) = if edited { (24, 12) } else { (32, 16) };
                assert_eq!((info.width, info.height), (width, height));
                let mut expected = Vec::new();
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        let (sx, sy) = if edited {
                            (31 - (x + 4), y + 2)
                        } else {
                            (x, y)
                        };
                        let at = (sy * 32 + sx) * 4;
                        expected.extend_from_slice(&reference.stdout[at..at + 4]);
                    }
                }
                assert_eq!(actual, expected, "{codec} frame {index}, edited={edited}");
            }
        }
        let opaque = root.join(format!("{codec}-opaque.webm"));
        let result = Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(&path)
            .args([
                "-vf",
                "format=yuv420p",
                "-c:v",
                codec,
                "-auto-alt-ref",
                "0",
                "-threads",
                "1",
                "-metadata:s:v:0",
                "alpha_mode=0",
            ])
            .arg(&opaque)
            .output()
            .expect("opaque control");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let png = source_video_frame_png(&opaque, MediaTime::ZERO, &|| false)
            .expect("ordinary opaque WebM remains supported");
        assert_eq!(unpack(&png).0.color_type, png::ColorType::Rgb);

        // Stream metadata must follow the chosen video, not the first stream.
        for alpha_selected in [false, true] {
            let multi = root.join(format!("{codec}-multi-{alpha_selected}.mkv"));
            let inputs = if alpha_selected {
                [&opaque, &path]
            } else {
                [&path, &opaque]
            };
            let result = Command::new(&ffmpeg)
                .args(["-v", "error", "-i"])
                .arg(inputs[0])
                .arg("-i")
                .arg(inputs[1])
                .args([
                    "-map",
                    "0:v",
                    "-map",
                    "1:v",
                    "-c",
                    "copy",
                    "-disposition:v:0",
                    "0",
                    "-disposition:v:1",
                    "default",
                ])
                .arg(&multi)
                .output()
                .expect("two-video control");
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                best_stream_config(
                    &format::input(&multi).expect("two-video input"),
                    Type::Video
                )
                .expect("selected video")
                .index,
                1
            );
            let actual = source_video_frame_png(&multi, MediaTime::ZERO, &|| false)
                .expect("selected stream export");
            let expected = source_video_frame_png(inputs[1], MediaTime::ZERO, &|| false)
                .expect("single-stream reference");
            assert_eq!(unpack(&actual).1, unpack(&expected).1);
            assert_eq!(
                unpack(&actual).0.color_type,
                if alpha_selected {
                    png::ColorType::Rgba
                } else {
                    png::ColorType::Rgb
                }
            );
        }

        let missing_alpha = root.join(format!("{codec}-missing-alpha.webm"));
        let result = Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(&opaque)
            .args(["-c", "copy", "-metadata:s:v:0", "alpha_mode=1"])
            .arg(&missing_alpha)
            .output()
            .expect("missing-alpha control");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            format::input(&missing_alpha)
                .expect("declared-alpha container")
                .stream(0)
                .expect("video stream")
                .metadata()
                .get("alpha_mode"),
            Some("1")
        );
        assert!(
            matches!(source_video_frame_png(&missing_alpha, MediaTime::ZERO, &|| false),
            Err(DecodeError::FrameImage(message)) if message.contains("alpha plane"))
        );

        assert_eq!(fs::read(&path).expect("source bytes"), original);
        assert_eq!(
            fs::metadata(&path)
                .expect("source metadata")
                .modified()
                .expect("source time"),
            modified
        );
    }
    fs::remove_dir_all(root).expect("remove owned fixtures");
}
