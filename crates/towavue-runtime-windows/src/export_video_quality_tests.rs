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
