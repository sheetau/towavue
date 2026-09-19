use super::*;

fn aligned_psnr(source: &str, encoded: &str) -> f64 {
    // Geometry/timestamps are checked separately. Match decoded frame ordinal
    // here so container time-base rounding cannot compare adjacent frames.
    let output = run(
        "ffmpeg.exe",
        &[
            "-hide_banner",
            "-i",
            source,
            "-i",
            encoded,
            "-filter_complex",
            "[0:v]hflip,settb=1/8,setpts=N[r];[1:v]settb=1/8,setpts=N[e];[r][e]psnr",
            "-an",
            "-f",
            "null",
            "-",
        ],
    );
    let log = String::from_utf8(output.stderr).expect("PSNR log");
    log.lines()
        .find(|line| line.contains("PSNR y:"))
        .expect("PSNR summary")
        .split("average:")
        .nth(1)
        .expect("average")
        .split_whitespace()
        .next()
        .expect("score")
        .parse()
        .expect("PSNR")
}

fn frame_times(path: &Path) -> Vec<f64> {
    let output = run(
        "ffprobe.exe",
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "frame=best_effort_timestamp_time",
            "-of",
            "csv=p=0",
            path.to_str().expect("path"),
        ],
    );
    String::from_utf8(output.stdout)
        .expect("timestamps")
        .lines()
        .filter_map(|line| line.split(',').next()?.parse().ok())
        .collect()
}

#[test]
fn video_presets_compare_actual_encodings_without_reducing_geometry_timing_or_depth() {
    let directory = depth_directory();
    for (pixel, scale, texture, extensions) in [
        ("yuv420p", 1, true, &["mp4", "webm", "avi", "wmv"][..]),
        ("yuv420p10le", 4, true, &["mp4"][..]),
        ("yuv420p10le", 4, false, &["mp4"][..]),
        ("yuv420p12le", 16, true, &["mp4"][..]),
    ] {
        let source = directory.join(format!("source-{pixel}-{texture}.mkv"));
        let filter = format!(
            "nullsrc=size=160x96:rate=8:duration=1.5,format={pixel},geq=lum='(16+mod(X*7+Y*13+N*17,220))*{scale}':cb='(96+mod(X+N,64))*{scale}':cr='(96+mod(Y+N,64))*{scale}'"
        );
        let filter = if texture {
            filter
        } else {
            format!("testsrc2=size=160x96:rate=8:duration=1.5,format={pixel}")
        };
        run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &filter,
                "-c:v",
                "ffv1",
                source.to_str().expect("source"),
            ],
        );
        let original = frame_times(&source);
        assert_eq!(original.len(), 12);
        for extension in extensions {
            let mut results = Vec::new();
            for quality in [
                VideoExportQuality::High,
                VideoExportQuality::Balanced,
                VideoExportQuality::Smaller,
            ] {
                let target = directory.join(format!("{pixel}-{texture}-{quality:?}.{extension}"));
                let outcome = export_media_with_options(
                    &ExportRequest {
                        source: source.clone(),
                        target: target.clone(),
                        kind: MediaKind::Video,
                        operations: vec![EditOperation::FlipHorizontal],
                        hardware_encode: scale > 1,
                    },
                    ExportOptions {
                        video_quality: quality,
                        ..Default::default()
                    },
                )
                .expect("preset export");
                assert!(!outcome.used_hardware_encoder);
                assert_video_fields(
                    &target,
                    &[
                        &format!("pix_fmt={pixel}"),
                        "width=160",
                        "height=96",
                        "nb_read_frames=12",
                    ],
                );
                let actual = frame_times(&target);
                assert_eq!(actual.len(), original.len());
                for (a, b) in actual.iter().zip(&original) {
                    assert!(
                        (a - b).abs() <= 0.001,
                        "{pixel}/{extension}/{quality:?}: {actual:?}"
                    );
                }
                let score = aligned_psnr(
                    source.to_str().expect("source"),
                    target.to_str().expect("target"),
                );
                let bytes = std::fs::metadata(&target).expect("encoded length").len();
                eprintln!(
                    "VIDEO_PRESET pixel={pixel} texture={texture} format={extension} quality={quality:?} bytes={bytes} psnr={score:.4}"
                );
                assert!(score.is_finite());
                results.push((bytes, score));
            }
            // A bounded deterministic texture comparison, not a promise that
            // arbitrary clips always have this size or perceptual ordering.
            for adjacent in results.windows(2) {
                assert!(
                    adjacent[0].0 > adjacent[1].0,
                    "{pixel}/{extension}: {results:?}"
                );
                // SVT perceptual tuning can lower PSNR at a stronger setting
                // on a moving synthetic ramp. Keep that measurement without
                // claiming a universal numerical ordering; the moving bars
                // fixture verifies the three distinct quality levels instead.
                assert!(
                    (pixel == "yuv420p10le" && texture) || adjacent[0].1 > adjacent[1].1,
                    "{pixel}/{extension}: {results:?}"
                );
            }
            if scale == 1 && *extension == "mp4" {
                for (index, quality) in [
                    VideoExportQuality::High,
                    VideoExportQuality::Balanced,
                    VideoExportQuality::Smaller,
                ]
                .into_iter()
                .enumerate()
                {
                    let target = directory.join(format!("hardware-{quality:?}.mp4"));
                    let outcome = export_media_with_options(
                        &ExportRequest {
                            source: source.clone(),
                            target: target.clone(),
                            kind: MediaKind::Video,
                            operations: vec![EditOperation::FlipHorizontal],
                            hardware_encode: true,
                        },
                        ExportOptions {
                            video_quality: quality,
                            ..Default::default()
                        },
                    )
                    .expect("hardware preference with fallback");
                    assert_video_fields(
                        &target,
                        &[
                            "pix_fmt=yuv420p",
                            "width=160",
                            "height=96",
                            "nb_read_frames=12",
                        ],
                    );
                    assert_eq!(frame_times(&target), original);
                    let score = aligned_psnr(
                        source.to_str().expect("source"),
                        target.to_str().expect("target"),
                    );
                    eprintln!(
                        "VIDEO_PRESET hardware_requested=true hardware_used={} quality={quality:?} psnr={score:.4}",
                        outcome.used_hardware_encoder
                    );
                    if !outcome.used_hardware_encoder {
                        assert!(
                            (score - results[index].1).abs() < 0.00001,
                            "fallback retains the selected preset"
                        );
                    }
                }
            }
        }
    }
    std::fs::remove_dir_all(directory).expect("owned fixtures");
}

#[test]
fn video_presets_keep_audio_independent_and_tune_hardware_with_software_fallback() {
    let request = |kind, target: &str| ExportRequest {
        source: "source.mkv".into(),
        target: target.into(),
        kind,
        operations: Vec::new(),
        hardware_encode: true,
    };
    for (quality, hardware_quality, software_qmax) in [
        (VideoExportQuality::High, "95", "20"),
        (VideoExportQuality::Balanced, "75", "32"),
        (VideoExportQuality::Smaller, "50", "42"),
    ] {
        let video = request(MediaKind::Video, "out.mp4");
        let hardware = codec_arguments(&video, true, quality);
        assert!(
            hardware
                .windows(2)
                .any(|pair| pair == ["-quality", hardware_quality])
        );
        assert!(
            hardware
                .windows(2)
                .any(|pair| pair == ["-rate_control", "quality"])
        );
        let fallback = codec_arguments(&video, false, quality);
        assert!(
            fallback
                .windows(2)
                .any(|pair| pair == ["-qmax:v", software_qmax])
        );
        for args in [hardware, fallback] {
            assert!(args.windows(2).any(|pair| pair == ["-c:a", "aac"]));
            assert!(
                !args
                    .iter()
                    .any(|arg| ["-b:a", "-q:a", "-r", "-s"].contains(&arg.as_str()))
            );
        }
        for (kind, target) in [(MediaKind::Audio, "out.m4a"), (MediaKind::Image, "out.png")] {
            let request = request(kind, target);
            assert_eq!(
                codec_arguments(&request, false, quality),
                codec_arguments(&request, false, VideoExportQuality::High)
            );
        }
    }
}
