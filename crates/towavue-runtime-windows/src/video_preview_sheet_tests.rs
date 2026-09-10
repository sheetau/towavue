use super::*;
use std::time::UNIX_EPOCH;

fn root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "towavue-sheet-{name}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("owned fixtures");
    root
}

fn generate(arguments: &[&str], output: &Path) {
    let mut command = hidden_command(&tool_path("ffmpeg.exe").expect("FFmpeg"), ["-v", "error"]);
    let result = command
        .args(arguments)
        .arg(output)
        .output()
        .expect("fixture encoder");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn compare(source: &Path, targets: &[Duration]) {
    let mut count = 0;
    crate::decode::preview_video_frames(source, targets, FILTER, &|| false, |slot, actual| {
        let expected = reference(source, targets[slot]);
        assert_eq!(
            (actual.width, actual.height),
            (expected.width, expected.height)
        );
        let differences = actual
            .rgba
            .iter()
            .zip(&expected.rgba)
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differences,
            0,
            "{} sample {slot} differs from independent RGBA CLI reference",
            source.display()
        );
        count += 1;
    })
    .expect("shared input/decoder, without fallback");
    assert_eq!(count, targets.len());
}

fn reference(source: &Path, target: Duration) -> PreviewImage {
    let png = frame_preview(source, target, FILTER, None)
        .or_else(|error| {
            if !matches!(error, PreviewError::NoFrame) {
                return Err(error);
            }
            // The old input-seek fallback can miss the final TS frame. Independently
            // scan these small fixtures from the beginning, then use their true last frame.
            let (_, stream) = crate::decode::preview_input(
                source,
                ffmpeg_next::media::Type::Video,
                Duration::ZERO,
                &|| false,
            )?;
            let input = vec![
                "-i".into(),
                source.display().to_string(),
                "-map".into(),
                format!("0:{stream}"),
            ];
            let mut arguments = input.clone();
            arguments.extend([
                "-ss".into(),
                format!("{:.6}", target.as_secs_f64()),
                "-vf".into(),
                FILTER.into(),
                "-frames:v".into(),
                "1".into(),
            ]);
            run_ffmpeg(&arguments, None).or_else(|error| {
                if !matches!(error, PreviewError::NoFrame) {
                    return Err(error);
                }
                let mut arguments = input;
                arguments.extend([
                    "-vf".into(),
                    format!("reverse,{FILTER}"),
                    "-frames:v".into(),
                    "1".into(),
                ]);
                run_ffmpeg(&arguments, None)
            })
        })
        .unwrap_or_else(|error| {
            panic!("CLI reference {} at {target:?}: {error}", source.display())
        });
    decode_png(&png).expect("reference PNG")
}

#[test]
fn shared_sheet_decoder_matches_orientation_color_streams_offsets_and_eof() {
    let root = root("compatibility");
    let base = root.join("base.mp4");
    generate(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x96:rate=10:duration=2",
            "-vf",
            "setsar=2",
            "-c:v",
            "mpeg4",
            "-bf",
            "2",
            "-g",
            "20",
            "-q:v",
            "2",
            "-colorspace",
            "bt709",
            "-color_range",
            "pc",
            "-an",
        ],
        &base,
    );
    let targets = [
        Duration::ZERO,
        Duration::from_millis(799),
        Duration::from_millis(1999),
    ];
    compare(&base, &targets);
    for (name, options) in [
        ("rotation.mp4", vec!["-display_rotation:v:0", "90"]),
        (
            "reflection.mp4",
            vec!["-display_rotation:v:0", "90", "-display_hflip:v:0"],
        ),
    ] {
        let target = root.join(name);
        let mut arguments = options;
        arguments.extend(["-i", base.to_str().expect("path"), "-c", "copy"]);
        generate(&arguments, &target);
        compare(&target, &targets);
    }
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    for extension in ["mp4", "mkv", "ts"] {
        let target = root.join(format!("offset.{extension}"));
        generate(
            &[
                "-i",
                fixture.to_str().expect("fixture path"),
                "-map",
                "0",
                "-c",
                "copy",
                "-output_ts_offset",
                "5",
            ],
            &target,
        );
        compare(&target, &targets);
    }
    let multiple = root.join("multiple.mkv");
    generate(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=red:size=96x64:rate=10:duration=2",
            "-f",
            "lavfi",
            "-i",
            "color=blue:size=96x64:rate=10:duration=2",
            "-map",
            "0:v",
            "-map",
            "1:v",
            "-c:v",
            "mpeg4",
            "-disposition:v:0",
            "0",
            "-disposition:v:1",
            "default",
        ],
        &multiple,
    );
    compare(&multiple, &targets);
    crate::decode::preview_video_frames(
        &multiple,
        &[Duration::from_secs(1)],
        FILTER,
        &|| false,
        |_, image| {
            let center = ((CELL_HEIGHT / 2 * CELL_WIDTH + CELL_WIDTH / 2) * 4) as usize;
            assert!(
                image.rgba[center + 2] > 200 && image.rgba[center] < 10,
                "the default blue stream is selected"
            );
        },
    )
    .expect("best stream");
    let tail = root.join("audio-tail.mkv");
    generate(
        &[
            "-i",
            base.to_str().expect("path"),
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=mono:d=12",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-c:v",
            "copy",
            "-c:a",
            "pcm_s16le",
        ],
        &tail,
    );
    compare(&tail, &[Duration::from_secs(3), Duration::from_secs(11)]);
    let vfr = root.join("vfr.mp4");
    generate(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=128x80:rate=10:duration=2",
            "-vf",
            "select=lt(n\\,5)+not(mod(n\\,3))",
            "-fps_mode",
            "vfr",
            "-c:v",
            "mpeg4",
            "-q:v",
            "2",
            "-an",
        ],
        &vfr,
    );
    compare(&vfr, &targets);
    let alpha = root.join("alpha.mov");
    generate(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=red@0.5:size=64x48:rate=5:duration=1,format=rgba",
            "-c:v",
            "qtrle",
            "-pix_fmt",
            "argb",
            "-an",
        ],
        &alpha,
    );
    compare(
        &alpha,
        &[
            Duration::ZERO,
            Duration::from_millis(799),
            Duration::from_millis(999),
        ],
    );
    crate::decode::preview_video_frames(
        &alpha,
        &[Duration::ZERO],
        FILTER,
        &|| false,
        |_, image| {
            let center = ((CELL_HEIGHT / 2 * CELL_WIDTH + CELL_WIDTH / 2) * 4) as usize;
            assert!(
                image.rgba[center] > 250 && (125..=129).contains(&image.rgba[center + 3]),
                "preview retains source alpha"
            );
        },
    )
    .expect("alpha preview");
    fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn shared_sheet_unsupported_matrix_keeps_the_existing_frame_fallback() {
    let root = root("fallback");
    let source = root.join("arbitrary-matrix.mp4");
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    generate(
        &[
            "-display_rotation:v:0",
            "45",
            "-i",
            fixture.to_str().expect("fixture"),
            "-map",
            "0:v",
            "-c",
            "copy",
        ],
        &source,
    );
    assert!(matches!(
        crate::decode::preview_video_frames(
            &source,
            &[Duration::ZERO],
            FILTER,
            &|| false,
            |_, _| panic!("unsupported native orientation")
        ),
        Err(crate::DecodeError::UnsupportedOrientation)
    ));
    let cache = PreviewCache::new(root.join("cache")).expect("cache");
    let duration = cache.duration(&source).expect("duration");
    let layout = VideoSheetLayout::for_position(duration, duration).expect("tail sheet");
    let sheet = cache
        .video_sheet(&source, layout)
        .expect("legacy fallback sheet");
    let reference = reference(&source, layout.position(0).expect("cell"));
    for y in 0..CELL_HEIGHT {
        let row = (y * WIDTH * 4) as usize;
        let reference_row = (y * CELL_WIDTH * 4) as usize;
        assert_eq!(
            &sheet.image.rgba[row..row + (CELL_WIDTH * 4) as usize],
            &reference.rgba[reference_row..reference_row + (CELL_WIDTH * 4) as usize]
        );
    }
    fs::remove_dir_all(root).expect("remove owned fallback fixture");
}

#[test]
fn shared_sheet_decoder_stops_before_open_and_between_cells() {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    let cancel = Cancellation::default();
    let mut calls = 0;
    let result = crate::decode::preview_video_frames(
        &source,
        &[Duration::from_millis(50), Duration::from_millis(150)],
        FILTER,
        &|| cancel.is_cancelled(),
        |_, _| {
            calls += 1;
            cancel.cancel();
        },
    );
    assert!(matches!(result, Err(crate::DecodeError::ConsumerClosed)));
    assert_eq!(calls, 1);
    assert!(matches!(
        crate::decode::preview_video_frames(
            Path::new("missing-cancelled-source.mp4"),
            &[Duration::ZERO],
            FILTER,
            &|| true,
            |_, _| panic!("cancelled publish")
        ),
        Err(crate::DecodeError::ConsumerClosed)
    ));
}

#[test]
fn shared_sheet_decoder_long_gop_generation_measurement() {
    let root = root("long-gop");
    let source = root.join("long-gop.mp4");
    generate(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1920x1080:rate=30:duration=6",
            "-c:v",
            "mpeg4",
            "-g",
            "180",
            "-bf",
            "2",
            "-q:v",
            "5",
            "-an",
        ],
        &source,
    );
    let layout =
        VideoSheetLayout::for_position(Duration::from_secs(6), Duration::ZERO).expect("layout");
    let targets: Vec<_> = (0..CELLS)
        .filter_map(|slot| layout.position(slot))
        .collect();
    let start = std::time::Instant::now();
    let mut references = Vec::new();
    for target in &targets {
        references.push(
            decode_png(&frame_preview(&source, *target, FILTER, None).expect("CLI frame"))
                .expect("PNG"),
        );
    }
    let legacy = start.elapsed();
    let start = std::time::Instant::now();
    crate::decode::preview_video_frames(&source, &targets, FILTER, &|| false, |slot, image| {
        assert_eq!(
            image
                .rgba
                .iter()
                .zip(&references[slot].rgba)
                .filter(|(a, b)| a != b)
                .count(),
            0,
            "long-GOP sample {slot}"
        );
    })
    .expect("long-GOP shared decoder");
    eprintln!(
        "1080p long-GOP 16-cell generation: CLI={legacy:?}, shared={:?}",
        start.elapsed()
    );
    fs::remove_dir_all(root).expect("remove owned long-GOP fixture");
}
