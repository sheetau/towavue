use super::*;
use std::os::windows::process::CommandExt;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn software_h264_quality_is_explicit_and_does_not_change_other_codecs() {
    for (extension, kind, hardware, expected) in [
        ("mp4", MediaKind::Video, false, true),
        ("mkv", MediaKind::Video, false, true),
        ("mov", MediaKind::Video, false, true),
        ("mp4", MediaKind::Video, true, false),
        ("webm", MediaKind::Video, false, false),
        ("avi", MediaKind::Video, false, false),
        ("wmv", MediaKind::Video, false, false),
        ("m4a", MediaKind::Audio, false, false),
        ("png", MediaKind::Image, false, false),
    ] {
        let arguments = codec_arguments(
            &ExportRequest {
                source: "source.mkv".into(),
                target: format!("output.{extension}").into(),
                kind,
                operations: Vec::new(),
                hardware_encode: hardware,
            },
            hardware,
        );
        for pair in [["-profile:v", "high"], ["-qmin:v", "1"], ["-qmax:v", "20"]] {
            assert_eq!(arguments.windows(2).any(|actual| actual == pair), expected);
        }
    }
}

fn run(tool: &str, arguments: &[&str]) -> std::process::Output {
    let output = Command::new(crate::media_tools::tool_path(tool).expect("media helper"))
        .creation_flags(0x0800_0000)
        .args(arguments)
        .output()
        .expect("owned media helper process");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn psnr(source: &str, encoded: &str) -> f64 {
    let output = run(
        "ffmpeg.exe",
        &[
            "-hide_banner",
            "-i",
            source,
            "-i",
            encoded,
            "-filter_complex",
            "[0:v]hflip[reference];[reference][1:v]psnr",
            "-an",
            "-f",
            "null",
            "-",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = stderr
        .lines()
        .find(|line| line.contains("PSNR y:"))
        .expect("PSNR summary");
    summary
        .split("average:")
        .nth(1)
        .expect("average field")
        .split_whitespace()
        .next()
        .expect("average value")
        .parse()
        .expect("finite PSNR")
}

#[test]
fn edited_1080p_export_preserves_detail_beyond_the_default_bitrate_budget() {
    let root = std::env::temp_dir().join(format!(
        "towavue-export-quality-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("owned fixture directory");
    for (name, noise) in [
        ("edges", ""),
        ("texture", ",noise=alls=12:allf=t+u:all_seed=73"),
    ] {
        let source = root.join(format!("{name}.mkv"));
        let baseline = root.join(format!("{name}-default.mkv"));
        let target = root.join(format!("{name}-export.mkv"));
        let source_text = source.to_str().expect("source path");
        let baseline_text = baseline.to_str().expect("baseline path");
        let target_text = target.to_str().expect("target path");
        let fixture = format!("testsrc2=size=1920x1080:rate=24:duration=1{noise}");
        run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &fixture,
                "-c:v",
                "ffv1",
                source_text,
            ],
        );
        let source_bytes = fs::read(&source).expect("source bytes");
        let source_modified = fs::metadata(&source)
            .expect("source metadata")
            .modified()
            .expect("mtime");
        // Independent historical encoder control, not the production argument builder.
        run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-i",
                source_text,
                "-vf",
                "hflip,copy",
                "-an",
                "-c:v",
                "libopenh264",
                baseline_text,
            ],
        );
        fs::write(&target, b"previous target").expect("existing output");
        export_media(&ExportRequest {
            source: source.clone(),
            target: target.clone(),
            kind: MediaKind::Video,
            operations: vec![EditOperation::FlipHorizontal],
            hardware_encode: false,
        })
        .expect("public edited export");
        let old_quality = psnr(source_text, baseline_text);
        let new_quality = psnr(source_text, target_text);
        eprintln!(
            "EXPORT_QUALITY {name} old_psnr={old_quality:.3} new_psnr={new_quality:.3} old_bytes={} new_bytes={}",
            fs::metadata(&baseline).expect("baseline metadata").len(),
            fs::metadata(&target).expect("target metadata").len()
        );
        assert!(
            new_quality > old_quality + 4.0 && new_quality >= 40.0,
            "{name}: {old_quality} -> {new_quality}"
        );
        let probe = run(
            "ffprobe.exe",
            &[
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-count_frames",
                "-show_entries",
                "stream=profile,width,height,pix_fmt,nb_read_frames",
                "-of",
                "default=noprint_wrappers=1",
                target_text,
            ],
        );
        let metadata = String::from_utf8_lossy(&probe.stdout);
        for field in [
            "profile=High",
            "width=1920",
            "height=1080",
            "pix_fmt=yuv420p",
            "nb_read_frames=24",
        ] {
            assert!(
                metadata.lines().any(|line| line == field),
                "missing {field}: {metadata}"
            );
        }
        assert!(fs::read(&source).expect("unchanged source") == source_bytes);
        assert_eq!(
            fs::metadata(&source)
                .expect("source metadata")
                .modified()
                .expect("mtime"),
            source_modified
        );
    }
    // Only this test's uniquely named generated-media directory is removed.
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

fn depth_directory() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "towavue-video-depth-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("owned fixtures");
    root
}

fn depth_fixture(path: &Path, pixel: &str, size: &str, colors: &[&str]) {
    let filter = format!(
        "nullsrc=size={size}:rate=8:duration=2,format={pixel},geq=lum='64+mod(X+3*Y+13*N,876)':cb='400+mod(X+N,200)':cr='400+mod(Y+N,200)'"
    );
    let mut args = vec![
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        &filter,
        "-f",
        "lavfi",
        "-i",
        "sine=sample_rate=48000:duration=2",
        "-c:v",
        "libaom-av1",
        "-crf",
        "0",
        "-cpu-used",
        "6",
        "-row-mt",
        "1",
        "-threads:v",
        "4",
        "-c:a",
        "pcm_s16le",
    ];
    let color_filter = format!(
        "setparams=range={}:colorspace={}:color_trc={}:color_primaries={}",
        colors[1], colors[3], colors[5], colors[7]
    );
    args.extend_from_slice(&["-vf", &color_filter]);
    args.extend_from_slice(colors);
    args.push(path.to_str().expect("path"));
    run("ffmpeg.exe", &args);
}

fn assert_video_fields(path: &Path, fields: &[&str]) {
    let probe = run(
        "ffprobe.exe",
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=codec_name,pix_fmt,width,height,nb_read_frames,color_range,color_space,color_transfer,color_primaries",
            "-of",
            "default=noprint_wrappers=1",
            path.to_str().expect("path"),
        ],
    );
    let output = String::from_utf8_lossy(&probe.stdout);
    for field in fields {
        assert!(
            output.lines().any(|line| line == *field),
            "missing {field}: {output}"
        );
    }
}

#[test]
fn high_depth_exports_preserve_precision_edits_color_and_container_with_hardware_requested() {
    use towavue_core::{
        MediaTime, ResampleFilter, TimeRange, TimelineEdit, VideoResize, VideoRotation,
    };
    let root = depth_directory();
    let source = root.join("source.mkv");
    depth_fixture(
        &source,
        "yuv420p10le",
        "160x96",
        &[
            "-color_range",
            "tv",
            "-colorspace",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_primaries",
            "bt709",
        ],
    );
    let source_bytes = fs::read(&source).expect("source");
    let modified = fs::metadata(&source)
        .expect("source metadata")
        .modified()
        .expect("mtime");
    let rotation = VideoRotation::new(73, (160, 96), 1.0).expect("rotation");
    let cases = [
        ("flip", vec![EditOperation::FlipHorizontal], (160, 96), 16),
        (
            "trim",
            vec![
                EditOperation::SetTrimStart(MediaTime::from_nanoseconds(500_000_000)),
                EditOperation::SetTrimEnd(MediaTime::from_nanoseconds(1_500_000_000)),
            ],
            (160, 96),
            8,
        ),
        (
            "timeline",
            vec![
                EditOperation::Timeline(TimelineEdit::Keep(
                    TimeRange::new(
                        MediaTime::from_nanoseconds(500_000_000),
                        MediaTime::from_nanoseconds(1_500_000_000),
                    )
                    .expect("range"),
                )),
                EditOperation::FlipVertical,
            ],
            (160, 96),
            8,
        ),
        (
            "resize",
            vec![EditOperation::ResizeVideo(
                VideoResize::new((128, 80), ResampleFilter::Lanczos, (160, 96), 1.0)
                    .expect("resize"),
            )],
            (128, 80),
            16,
        ),
        (
            "rotate",
            vec![EditOperation::RotateVideo(rotation)],
            rotation.size(),
            16,
        ),
    ];
    for extension in ["mp4", "mkv", "webm"] {
        for (name, operations, size, count) in &cases {
            let target = root.join(format!("{name}.{extension}"));
            fs::write(&target, b"previous output").expect("existing output");
            let outcome = export_media(&ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind: MediaKind::Video,
                operations: operations.clone(),
                hardware_encode: true,
            })
            .expect("high-depth public export");
            assert!(!outcome.used_hardware_encoder);
            assert_video_fields(
                &target,
                &[
                    "codec_name=av1",
                    "pix_fmt=yuv420p10le",
                    &format!("width={}", size.0),
                    &format!("height={}", size.1),
                    &format!("nb_read_frames={count}"),
                    "color_range=tv",
                    "color_space=bt709",
                    "color_transfer=bt709",
                    "color_primaries=bt709",
                ],
            );
            let audio = run(
                "ffprobe.exe",
                &[
                    "-v",
                    "error",
                    "-select_streams",
                    "a:0",
                    "-show_entries",
                    "stream=codec_name",
                    "-of",
                    "default=noprint_wrappers=1",
                    target.to_str().expect("path"),
                ],
            );
            assert!(
                String::from_utf8_lossy(&audio.stdout).contains(if extension == "webm" {
                    "codec_name=opus"
                } else {
                    "codec_name=aac"
                })
            );
        }
    }
    assert_eq!(fs::read(&source).expect("source"), source_bytes);
    assert_eq!(
        fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        modified
    );
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn high_depth_small_chroma_hdr_and_rejection_controls() {
    let root = depth_directory();
    for (pixel, size) in [
        ("yuv420p10le", "32x24"),
        ("yuv420p10le", "65x49"),
        ("yuv444p10le", "80x64"),
        ("yuv422p12le", "80x64"),
    ] {
        let source = root.join(format!("{pixel}-{size}.mkv"));
        depth_fixture(
            &source,
            pixel,
            size,
            &[
                "-color_range",
                "tv",
                "-colorspace",
                "bt2020nc",
                "-color_trc",
                "smpte2084",
                "-color_primaries",
                "bt2020",
            ],
        );
        let target = root.join(format!("{pixel}-{size}-export.mkv"));
        let mut request = ExportRequest {
            source: source.clone(),
            target: target.clone(),
            kind: MediaKind::Video,
            operations: vec![EditOperation::FlipVertical],
            hardware_encode: true,
        };
        export_media(&request).expect("AOM precision path");
        assert_video_fields(
            &target,
            &[
                "codec_name=av1",
                &format!("pix_fmt={pixel}"),
                "nb_read_frames=16",
                "color_space=bt2020nc",
                "color_transfer=smpte2084",
                "color_primaries=bt2020",
            ],
        );
        request.target = root.join("unsupported.avi");
        fs::write(&request.target, b"keep existing output").expect("previous output");
        let error = export_media(&request).expect_err("no implicit precision reduction");
        assert!(error.to_string().contains("requires MP4"), "{error}");
        assert_eq!(
            fs::read(&request.target).expect("existing output"),
            b"keep existing output"
        );
        request.target = target.clone();
        let previous = fs::read(&target).expect("encoded target");
        assert!(matches!(
            export_cancellable(&request, &AtomicBool::new(true), &|_| {}),
            Err(ExportError::Cancelled)
        ));
        assert_eq!(fs::read(&target).expect("unchanged output"), previous);
        let encoding = video_encoding::HighDepth::probe(&request)
            .expect("probe")
            .expect("depth plan");
        let invalid = root.join("invalid.mkv");
        fs::write(&invalid, b"invalid stage").expect("invalid output");
        assert!(
            encoding.verify(&invalid).is_err(),
            "an invalid stage cannot publish"
        );
    }
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

fn precision_psnr(reference: &Path, encoded: &Path) -> f64 {
    let output = run(
        "ffmpeg.exe",
        &[
            "-hide_banner",
            "-i",
            reference.to_str().expect("path"),
            "-i",
            encoded.to_str().expect("path"),
            "-filter_complex",
            "[0:v]format=yuv420p10le[r];[1:v]format=yuv420p10le[e];[r][e]psnr",
            "-an",
            "-f",
            "null",
            "-",
        ],
    );
    let text = String::from_utf8_lossy(&output.stderr);
    let summary = text
        .lines()
        .find(|line| line.contains("PSNR y:"))
        .expect("PSNR summary");
    summary
        .split("average:")
        .nth(1)
        .expect("average")
        .split_whitespace()
        .next()
        .expect("score")
        .parse()
        .expect("PSNR")
}

#[test]
fn high_depth_gradient_quality_survives_resize_without_an_eight_bit_intermediate() {
    use towavue_core::{ResampleFilter, VideoResize};
    let root = depth_directory();
    let source = root.join("source.mkv");
    depth_fixture(
        &source,
        "yuv420p10le",
        "512x256",
        &[
            "-color_range",
            "tv",
            "-colorspace",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_primaries",
            "bt709",
        ],
    );
    for (name, edits, filter) in [
        ("flip", vec![EditOperation::FlipHorizontal], "hflip"),
        (
            "resize",
            vec![EditOperation::ResizeVideo(
                VideoResize::new((384, 192), ResampleFilter::Lanczos, (512, 256), 1.0)
                    .expect("resize"),
            )],
            "format=gbrp16le,scale=384:192:flags=lanczos+full_chroma_inp,format=gbrp16le,setsar=1",
        ),
    ] {
        let reference = root.join(format!("{name}-reference.mkv"));
        let baseline = root.join(format!("{name}-old.mkv"));
        let actual = root.join(format!("{name}-new.mkv"));
        let reference_filter = format!("{filter},format=yuv420p10le,copy");
        run(
            "ffmpeg.exe",
            &[
                "-v",
                "error",
                "-i",
                source.to_str().expect("path"),
                "-an",
                "-vf",
                &reference_filter,
                "-c:v",
                "ffv1",
                reference.to_str().expect("path"),
            ],
        );
        let old_filter = format!("{},copy", filter.replace("gbrp16le", "gbrp"));
        let mut old_args = vec![
            "-v",
            "error",
            "-i",
            source.to_str().expect("path"),
            "-an",
            "-vf",
            &old_filter,
            "-c:v",
            "libopenh264",
        ];
        old_args.extend_from_slice(SOFTWARE_H264_QUALITY);
        old_args.push(baseline.to_str().expect("path"));
        run("ffmpeg.exe", &old_args);
        export_media(&ExportRequest {
            source: source.clone(),
            target: actual.clone(),
            kind: MediaKind::Video,
            operations: edits,
            hardware_encode: false,
        })
        .expect("precision export");
        let old = precision_psnr(&reference, &baseline);
        let new = precision_psnr(&reference, &actual);
        eprintln!("EXPORT_DEPTH_QUALITY {name} old_psnr={old:.3} new_psnr={new:.3}");
        assert!(new >= 45.0 && new > old + 1.0, "{name}: {old} -> {new}");
    }
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
#[ignore = "requires an explicitly selected short 10-bit reference clip"]
fn reference_high_depth_public_export_preserves_precision_and_reports_quality() {
    let source = PathBuf::from(
        std::env::var_os("TOWAVUE_EXPORT_DEPTH_REFERENCE").expect("explicit short reference clip"),
    );
    let stamp = fs::metadata(&source).expect("source metadata");
    let modified = stamp.modified().expect("mtime");
    let root = depth_directory();
    let target = root.join("export.mkv");
    let outcome = export_media(&ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: MediaKind::Video,
        operations: Vec::new(),
        hardware_encode: true,
    })
    .expect("public export");
    assert!(!outcome.used_hardware_encoder);
    assert_video_fields(&target, &["codec_name=av1", "pix_fmt=yuv420p10le"]);
    let score = precision_psnr(&source, &target);
    eprintln!(
        "EXPORT_DEPTH_REFERENCE psnr={score:.3} bytes={}",
        fs::metadata(&target).expect("output").len()
    );
    assert!(score >= 45.0, "reference PSNR {score}");
    assert_eq!(fs::metadata(&source).expect("source").len(), stamp.len());
    assert_eq!(
        fs::metadata(&source)
            .expect("source")
            .modified()
            .expect("mtime"),
        modified
    );
    fs::remove_dir_all(root).expect("remove owned fixtures");
}
