use super::*;

fn fixture(path: &Path, loops: &str, alpha: bool) {
    let mut args = vec![
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=32x24:rate=2:duration=1.5",
    ];
    if alpha {
        args.extend([
            "-f",
            "lavfi",
            "-i",
            "nullsrc=size=32x24:rate=2:duration=1.5,geq=lum='mod(X*8+N*31,256)',format=gray",
            "-map",
            "0:v",
            "-map",
            "1:v",
        ]);
    }
    args.extend([
        "-c:v",
        "libaom-av1",
        "-cpu-used",
        "8",
        "-crf",
        "0",
        "-threads",
        "1",
        "-loop",
        loops,
    ]);
    audio_tests::ffmpeg(&args, path);
}

#[test]
fn avif_animation_scan_reads_finite_infinite_and_alpha_track_controls() {
    let root = audio_tests::root("avif-controls");
    let source = root.join("source.avif");
    for loops in [0, 1, 3] {
        for alpha in [false, true] {
            fixture(&source, &loops.to_string(), alpha);
            let animation = Animation::read(&source, &AtomicBool::new(false))
                .expect("scan")
                .expect("animation");
            assert_eq!(animation.tracks.len(), if alpha { 2 } else { 1 });
            assert_eq!(animation.samples[0].times.len(), 3);
            assert!(
                animation
                    .tracks
                    .iter()
                    .all(|track| track.loops == Some(loops))
            );
            if alpha {
                assert_eq!(animation.tracks[1].alpha_for, Some(animation.tracks[0].id));
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_animation_export_preserves_all_frames() {
    let root = audio_tests::root("avif-export");
    let source = root.join("source.avif");
    let target = root.join("saved.avif");
    for alpha in [false, true] {
        fixture(&source, "3", alpha);
        let original = rgba(&source);
        for operations in [
            vec![],
            vec![EditOperation::RotateClockwise],
            vec![
                EditOperation::Crop(towavue_core::PixelCrop {
                    x: 2,
                    y: 3,
                    width: 17,
                    height: 13,
                }),
                EditOperation::FlipHorizontal,
            ],
            vec![EditOperation::Resize(
                towavue_core::ImageResize::new(17, 13, towavue_core::ResampleFilter::Nearest)
                    .expect("fixture operation"),
            )],
            vec![EditOperation::Resize(
                towavue_core::ImageResize::new(17, 13, towavue_core::ResampleFilter::Bilinear)
                    .expect("fixture operation"),
            )],
            vec![EditOperation::Resize(
                towavue_core::ImageResize::new(17, 13, towavue_core::ResampleFilter::Bicubic)
                    .expect("fixture operation"),
            )],
            vec![EditOperation::Resize(
                towavue_core::ImageResize::new(17, 13, towavue_core::ResampleFilter::Lanczos)
                    .expect("fixture operation"),
            )],
            vec![EditOperation::RotateImage(
                towavue_core::ImageRotation::new(130, (32, 24)).expect("rotation"),
            )],
        ] {
            let request = ExportRequest {
                source: source.clone(),
                target: target.clone(),
                kind: MediaKind::Image,
                operations,
                hardware_encode: false,
            };
            export_media(&request).expect("save");
            let actual = rgba(&target);
            assert_eq!(actual.frames.len(), 3);
            let expected = crate::render_image_edits(
                &original,
                &request.operations,
                &crate::Cancellation::default(),
            )
            .expect("display edits");
            for (index, (actual, expected)) in
                actual.frames.iter().zip(&expected.frames).enumerate()
            {
                assert_eq!(
                    (actual.width, actual.height, actual.delay),
                    (expected.width, expected.height, expected.delay)
                );
                let differences: Vec<_> = actual
                    .rgba
                    .iter()
                    .zip(&expected.rgba)
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .take(8)
                    .collect();
                assert!(
                    differences.is_empty(),
                    "frame={index}, alpha={alpha}, operations={:?}, first differences={differences:?}",
                    request.operations
                );
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

fn rgba(path: &Path) -> crate::DecodedImage {
    let mut decoded = crate::decode_image(path).expect("color frames");
    let animation = Animation::read(path, &AtomicBool::new(false))
        .expect("scan")
        .expect("animation");
    if let Some(alpha) = animation.samples.get(1) {
        let target = path.with_extension("alpha.raw");
        audio_tests::ffmpeg(
            &[
                "-i",
                &path.display().to_string(),
                "-map",
                &format!("0:{}", alpha.index),
                "-fps_mode",
                "passthrough",
                "-pix_fmt",
                "gray",
                "-f",
                "rawvideo",
            ],
            &target,
        );
        let alpha = fs::read(&target).expect("alpha samples");
        let expected: usize = decoded
            .frames
            .iter()
            .map(|frame| frame.rgba.len() / 4)
            .sum();
        assert_eq!(alpha.len(), expected);
        for (pixel, alpha) in decoded
            .frames
            .iter_mut()
            .flat_map(|frame| frame.rgba.as_chunks_mut::<4>().0)
            .zip(alpha)
        {
            pixel[3] = alpha;
        }
    }
    decoded
}

fn request(source: &Path, target: &Path) -> ExportRequest {
    ExportRequest {
        source: source.into(),
        target: target.into(),
        kind: MediaKind::Image,
        operations: vec![],
        hardware_encode: false,
    }
}

#[test]
fn avif_display_and_preview_merge_auxiliary_alpha() {
    let root = audio_tests::root("avif-display-alpha");
    let path = root.join("source.avif");
    fixture(&path, "3", true);
    let expected = rgba(&path);
    let actual = crate::decode_image(&path).expect("display");
    assert_eq!(actual.frames.len(), 3);
    assert!(actual.frames[0].rgba != actual.frames[1].rgba);
    for (index, (expected, actual)) in expected.frames.iter().zip(&actual.frames).enumerate() {
        assert!(
            actual.rgba == expected.rgba,
            "frame {index} must merge alpha"
        );
    }
    let preview = crate::image::first_animation_frame(&path, 32 * 24 * 4, &|| true)
        .expect("preview")
        .expect("first frame");
    assert!(
        preview.rgba == expected.frames[0].rgba,
        "preview must merge alpha"
    );
    let mut previews = Vec::new();
    crate::image::decode_image_with_preview(
        &path,
        crate::image::IMAGE_BYTE_LIMIT,
        &|| true,
        &mut |_, _, rgba| previews.push(rgba.to_vec()),
    )
    .expect("progressive preview");
    assert!(previews.len() == 1 && previews[0] == expected.frames[0].rgba);
    assert!(matches!(
        crate::image::decode_image_cancellable(&path, 32 * 24 * 4 * 2, &|| true),
        Err(crate::ImageDecodeError::TooLarge)
    ));
    let current = std::cell::Cell::new(true);
    assert!(matches!(
        crate::image::decode_image_with_preview(
            &path,
            crate::image::IMAGE_BYTE_LIMIT,
            &|| current.get(),
            &mut |_, _, _| current.set(false)
        ),
        Err(crate::ImageDecodeError::Cancelled)
    ));
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_static_primary_and_alpha_items_match_lossless_rgba() {
    let root = audio_tests::root("avif-still-alpha");
    let png = root.join("source.png");
    let path = root.join("source.avif");
    let pixels = vec![
        240, 20, 10, 0, 0, 220, 40, 64, 40, 20, 255, 128, 80, 60, 40, 255,
    ];
    ::image::save_buffer(&png, &pixels, 2, 2, ::image::ColorType::Rgba8).expect("RGBA fixture");
    audio_tests::ffmpeg(
        &[
            "-i",
            &png.display().to_string(),
            "-filter_complex",
            "[0:v]split[color][alpha];[color]format=gbrp[v];[alpha]alphaextract,format=gray,setparams=colorspace=bt709[a]",
            "-map",
            "[v]",
            "-map",
            "[a]",
            "-c:v",
            "libaom-av1",
            "-crf",
            "0",
            "-cpu-used",
            "8",
            "-threads",
            "1",
            "-colorspace:v:1",
            "bt709",
        ],
        &path,
    );
    assert!(
        Animation::read(&path, &AtomicBool::new(false))
            .expect("static scan")
            .is_none()
    );
    let decoded = crate::decode_image(&path).expect("static display");
    assert_eq!(decoded.frames.len(), 1);
    assert_eq!(decoded.frames[0].rgba, pixels);
    let saved = root.join("saved.avif");
    export_media(&request(&path, &saved)).expect("static AVIF save");
    let output = crate::decode_image(&saved).expect("saved display");
    assert_eq!(
        output.frames[0].rgba, pixels,
        "static save must retain RGBA"
    );
    let preview = crate::image::first_animation_frame(&path, pixels.len(), &|| true)
        .expect("preview")
        .expect("frame");
    assert_eq!(preview.rgba, pixels);
    assert!(matches!(
        crate::image::first_animation_frame(&path, pixels.len() - 1, &|| true),
        Err(crate::ImageDecodeError::TooLarge)
    ));
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_export_retains_variable_timing_and_repeat_on_resave() {
    let root = audio_tests::root("avif-timing");
    let source = root.join("source.avif");
    let target = root.join("saved.avif");
    let resaved = root.join("resaved.avif");
    let cancel = AtomicBool::new(false);
    for loops in [Some(0), Some(1), Some(3), None] {
        audio_tests::ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=32x24:rate=2:duration=1.5",
                "-vf",
                "settb=1/1000,setpts='if(eq(N,0),0,if(eq(N,1),125,875))'",
                "-fps_mode",
                "passthrough",
                "-enc_time_base",
                "1/1000",
                "-c:v",
                "libaom-av1",
                "-cpu-used",
                "8",
                "-threads",
                "1",
                "-loop",
                &loops.unwrap_or(1).to_string(),
            ],
            &source,
        );
        if loops.is_none() {
            // Keep box sizes and offsets intact; independently remove the declaration.
            let mut bytes = fs::read(&source).expect("fixture operation");
            let positions: Vec<_> = bytes
                .windows(4)
                .enumerate()
                .filter(|(_, kind)| *kind == b"edts")
                .map(|(i, _)| i)
                .collect();
            assert_eq!(positions.len(), 1);
            for position in positions {
                bytes[position..position + 4].copy_from_slice(b"free");
            }
            fs::write(&source, bytes).expect("fixture operation");
        }
        let original = Animation::read(&source, &cancel)
            .expect("fixture operation")
            .expect("fixture operation");
        for (input, output) in [(&source, &target), (&target, &resaved)] {
            export_media(&request(input, output)).expect("VFR save");
            let saved = Animation::read(output, &cancel)
                .expect("fixture operation")
                .expect("fixture operation");
            assert_eq!(saved.color().loops, loops);
            assert!(same_timing(&original.samples[0], &saved.samples[0]));
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_static_edits_and_format_conversion_preserve_displayed_pixels() {
    use towavue_core::{ImageResize, ImageRotation, PixelCrop, ResampleFilter};
    let root = audio_tests::root("avif-static-edits");
    let png = root.join("original.png");
    let source = root.join("source.avif");
    let target = root.join("saved.avif");
    for alpha in [false, true] {
        let original = ::image::RgbaImage::from_fn(16, 12, |x, y| {
            ::image::Rgba([
                (x * 17) as u8,
                (y * 23) as u8,
                (x * 7 + y * 13) as u8,
                if alpha {
                    ((x * 31 + y * 37) % 256) as u8
                } else {
                    255
                },
            ])
        });
        original.save(&png).expect("original PNG");
        export_media(&request(&png, &source)).expect("PNG to AVIF");
        let decoded = crate::decode_image(&source).expect("AVIF source");
        assert!(decoded.frames[0].rgba == original.as_raw().as_slice());
        let mut operations = vec![
            vec![],
            vec![EditOperation::RotateClockwise],
            vec![
                EditOperation::Crop(PixelCrop {
                    x: 1,
                    y: 2,
                    width: 11,
                    height: 7,
                }),
                EditOperation::FlipHorizontal,
            ],
            vec![EditOperation::RotateImage(
                ImageRotation::new(137, (16, 12)).expect("rotation"),
            )],
        ];
        for filter in [
            ResampleFilter::Nearest,
            ResampleFilter::Bilinear,
            ResampleFilter::Bicubic,
            ResampleFilter::Lanczos,
        ] {
            operations.push(vec![EditOperation::Resize(
                ImageResize::new(21, 9, filter).expect("resize"),
            )]);
        }
        for operations in operations {
            let mut edited_request = request(&source, &target);
            edited_request.operations = operations;
            let expected = crate::render_image_edits(
                &decoded,
                &edited_request.operations,
                &crate::Cancellation::default(),
            )
            .expect("display edits");
            export_media(&edited_request).expect("AVIF save");
            for extension in ["avif", "png", "webp"] {
                let resaved = root.join(format!("resaved.{extension}"));
                export_media(&request(&target, &resaved)).expect("static format resave");
                let actual = crate::decode_image(&resaved).expect("saved pixels");
                assert_eq!(actual.frames.len(), 1);
                assert_eq!(actual.dimensions(), expected.dimensions());
                if extension == "webp" {
                    assert!(
                        actual.frames[0]
                            .rgba
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|pixel| pixel[3])
                            .eq(expected.frames[0]
                                .rgba
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .map(|pixel| pixel[3])),
                        "WebP alpha"
                    );
                } else {
                    assert!(
                        actual.frames[0].rgba == expected.frames[0].rgba,
                        "alpha={alpha}, {extension}, operations={:?}",
                        edited_request.operations
                    );
                }
            }
        }
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_static_save_protects_targets_and_owned_staging() {
    let root = audio_tests::root("avif-static-protection");
    let png = root.join("source.png");
    let source = root.join("source.avif");
    let target = root.join("target.avif");
    ::image::RgbaImage::from_pixel(16, 12, ::image::Rgba([100, 20, 240, 60]))
        .save(&png)
        .expect("fixture");
    export_media(&request(&png, &source)).expect("source AVIF");
    fs::write(&target, b"existing target").expect("target");
    for input in [&png, &source] {
        for before in [false, true] {
            let cancel = AtomicBool::new(before);
            let result = export_cancellable(&request(input, &target), &cancel, &|time| {
                if time > Duration::ZERO {
                    cancel.store(true, Ordering::Relaxed);
                }
            });
            assert!(matches!(result, Err(ExportError::Cancelled)), "{result:?}");
            assert_eq!(fs::read(&target).expect("target"), b"existing target");
        }
        let intact = fs::read(input).expect("source");
        let changed = AtomicBool::new(false);
        let result =
            export_cancellable(&request(input, &target), &AtomicBool::new(false), &|time| {
                if time > Duration::ZERO && !changed.swap(true, Ordering::Relaxed) {
                    use std::io::Write;
                    fs::OpenOptions::new()
                        .append(true)
                        .open(input)
                        .expect("source")
                        .write_all(b"changed")
                        .expect("change source");
                }
            });
        assert!(changed.load(Ordering::Relaxed) && result.is_err());
        assert_eq!(fs::read(&target).expect("target"), b"existing target");
        fs::write(input, intact).expect("restore owned source");
    }
    for occupied in ["output.avif", "animation-source.png"] {
        let stage = StagedExport::new(&target).expect("stage");
        let occupied = stage.directory.join(occupied);
        fs::write(&occupied, b"occupied").expect("occupied path");
        assert!(
            export_still(
                &request(&source, &target),
                &stage,
                &AtomicBool::new(false),
                &|_| {}
            )
            .is_err()
        );
        assert_eq!(fs::read(&occupied).expect("preserved stage"), b"occupied");
    }
    fs::write(&source, b"damaged source").expect("corrupt input");
    assert!(export_media(&request(&source, &target)).is_err());
    assert_eq!(fs::read(&target).expect("target"), b"existing target");
    assert!(fs::read_dir(&root).expect("root").all(|entry| {
        !entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".towavue-export-")
    }));
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_export_failure_and_cancel_preserve_existing_target() {
    let root = audio_tests::root("avif-protection");
    let source = root.join("source.avif");
    let target = root.join("saved.avif");
    fixture(&source, "3", true);
    fs::write(&target, b"existing target").expect("fixture operation");
    for before in [false, true] {
        let cancel = AtomicBool::new(before);
        let result = export_cancellable(&request(&source, &target), &cancel, &|time| {
            if time > Duration::ZERO {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert!(matches!(result, Err(ExportError::Cancelled)), "{result:?}");
        assert_eq!(
            fs::read(&target).expect("fixture operation"),
            b"existing target"
        );
    }
    let changed = AtomicBool::new(false);
    let result = export_cancellable(
        &request(&source, &target),
        &AtomicBool::new(false),
        &|time| {
            if time > Duration::ZERO && !changed.swap(true, Ordering::Relaxed) {
                use std::io::Write;
                fs::OpenOptions::new()
                    .append(true)
                    .open(&source)
                    .expect("fixture operation")
                    .write_all(b"changed")
                    .expect("fixture operation");
            }
        },
    );
    assert!(changed.load(Ordering::Relaxed));
    assert!(result.is_err());
    assert_eq!(
        fs::read(&target).expect("fixture operation"),
        b"existing target"
    );
    fixture(&source, "3", true);
    for operations in [
        vec![EditOperation::Crop(towavue_core::PixelCrop {
            x: 31,
            y: 0,
            width: 2,
            height: 1,
        })],
        vec![EditOperation::RotateImage(
            towavue_core::ImageRotation::new(130, (2, 2)).expect("fixture operation"),
        )],
    ] {
        let mut request = request(&source, &target);
        request.operations = operations;
        assert!(export_media(&request).is_err());
        assert_eq!(
            fs::read(&target).expect("fixture operation"),
            b"existing target"
        );
    }
    let animation = Animation::read(&source, &AtomicBool::new(false))
        .expect("fixture operation")
        .expect("fixture operation");
    {
        let stage = StagedExport::new(&target).expect("fixture operation");
        fs::write(&stage.output, b"occupied").expect("fixture operation");
        assert!(
            animation
                .export(
                    &request(&source, &target),
                    &stage,
                    &AtomicBool::new(false),
                    &|_| {}
                )
                .is_err()
        );
        assert_eq!(
            fs::read(&stage.output).expect("fixture operation"),
            b"occupied"
        );
    }
    let other = target.with_extension("png");
    fs::write(&other, b"other target").expect("fixture operation");
    assert!(export_media(&request(&source, &other)).is_err());
    assert_eq!(
        fs::read(&other).expect("fixture operation"),
        b"other target"
    );
    let intact = fs::read(&source).expect("fixture operation");
    for length in [0, 7, intact.len() / 2, intact.len() - 1] {
        fs::write(&source, &intact[..length]).expect("fixture operation");
        assert!(export_media(&request(&source, &target)).is_err());
        assert_eq!(
            fs::read(&target).expect("fixture operation"),
            b"existing target"
        );
    }
    assert!(
        fs::read_dir(&root)
            .expect("fixture operation")
            .all(|entry| {
                !entry
                    .expect("fixture operation")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".towavue-export-")
            })
    );
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_box_scan_checks_bounds_sizes_duplicates_and_cancellation() {
    let root = audio_tests::root("avif-boxes");
    let path = root.join("boxes.bin");
    let cancel = AtomicBool::new(false);
    for bytes in [
        [8u32.to_be_bytes().as_slice(), b"free"].concat(),
        [0u32.to_be_bytes().as_slice(), b"free", &[0; 7]].concat(),
        [1u32.to_be_bytes().as_slice(), b"free", &16u64.to_be_bytes()].concat(),
    ] {
        fs::write(&path, &bytes).expect("fixture operation");
        let mut file = fs::File::open(&path).expect("fixture operation");
        let parsed = boxes(&mut file, 0, bytes.len() as u64, &cancel).expect("fixture operation");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].end, bytes.len() as u64);
        assert!(one(&[parsed[0], parsed[0]], b"free").is_err());
        assert!(boxes(&mut file, 0, bytes.len() as u64, &AtomicBool::new(true)).is_err());
    }
    for bytes in [
        vec![0; 7],
        [7u32.to_be_bytes().as_slice(), b"free"].concat(),
        [9u32.to_be_bytes().as_slice(), b"free"].concat(),
        [1u32.to_be_bytes().as_slice(), b"free"].concat(),
        [1u32.to_be_bytes().as_slice(), b"free", &15u64.to_be_bytes()].concat(),
        [
            1u32.to_be_bytes().as_slice(),
            b"free",
            &u64::MAX.to_be_bytes(),
        ]
        .concat(),
    ] {
        fs::write(&path, &bytes).expect("fixture operation");
        assert!(
            boxes(
                &mut fs::File::open(&path).expect("fixture operation"),
                0,
                bytes.len() as u64,
                &cancel
            )
            .is_err()
        );
    }
    let item = [8u32.to_be_bytes().as_slice(), b"free"].concat();
    for count in [65536, 65537] {
        fs::write(&path, item.repeat(count)).expect("fixture operation");
        assert_eq!(
            boxes(
                &mut fs::File::open(&path).expect("fixture operation"),
                0,
                (count * 8) as u64,
                &cancel
            )
            .is_ok(),
            count == 65536
        );
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn avif_control_validation_rejects_wrong_alpha_and_edit_lists() {
    let root = audio_tests::root("avif-invalid-controls");
    let source = root.join("source.avif");
    fixture(&source, "3", true);
    let intact = fs::read(&source).expect("fixture operation");
    let locate = |kind: &[u8]| {
        intact
            .windows(kind.len())
            .position(|bytes| bytes == kind)
            .expect("fixture operation")
    };
    let edit = locate(b"elst");
    let auxiliary = locate(b"auxi\0\0\0\0");
    // elst: version/flags, entry count, duration, media time, rate.
    for (offset, replacement) in [
        (edit + 4, vec![2]),
        (edit + 7, vec![2]),
        (edit + 8, 2u32.to_be_bytes().to_vec()),
        (
            edit + 12,
            vec![0; if intact[edit + 4] == 1 { 8 } else { 4 }],
        ),
        (auxiliary + 8, b"not-alpha".to_vec()),
    ] {
        let mut bytes = intact.clone();
        bytes[offset..offset + replacement.len()].copy_from_slice(&replacement);
        fs::write(&source, bytes).expect("fixture operation");
        assert!(
            Animation::read(&source, &AtomicBool::new(false)).is_err(),
            "offset {offset}, edit {edit}, auxiliary {auxiliary}, replacement {replacement:?}, version {}",
            intact[edit + 4]
        );
    }
    let left = Samples {
        index: 0,
        id: 1,
        size: (1, 1),
        time_base: ffmpeg::Rational(1, 1000),
        times: vec![(i64::MIN + 1, 500), (i64::MAX, 500)],
    };
    assert!(same_timing(&left, &left));
    let right = Samples {
        times: vec![(0, 1), (1, 1)],
        ..left.clone()
    };
    assert!(!same_timing(&left, &right));
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
