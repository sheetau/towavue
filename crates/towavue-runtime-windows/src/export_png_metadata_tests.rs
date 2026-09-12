use super::*;
use crate::export::audio_tests::root;
use std::io::Cursor;
use std::os::windows::process::CommandExt;
use towavue_core::{ImageResize, PixelCrop, ResampleFilter};

fn fixture() -> Vec<u8> {
    let pixels = image::RgbaImage::from_fn(32, 24, |x, y| {
        image::Rgba([
            (x * 7) as u8,
            (y * 9) as u8,
            (x * y) as u8,
            (x * 5 + y * 3) as u8,
        ])
    });
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("PNG fixture");
    encoded.into_inner()
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::new();
    write_chunk(&mut encoded, kind, data).expect("encode chunk");
    encoded
}

fn compressed(text: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(text).expect("compress text");
    encoder.finish().expect("zlib end")
}

fn international(key: &str, text: &str, compress: bool) -> Vec<u8> {
    let mut data = key.as_bytes().to_vec();
    data.extend_from_slice(&[0, u8::from(compress), 0]);
    data.extend_from_slice("ja\0日本語\0".as_bytes());
    data.extend_from_slice(&if compress {
        compressed(text.as_bytes())
    } else {
        text.as_bytes().to_vec()
    });
    chunk(b"iTXt", &data)
}

fn with_texts(texts: &[Vec<u8>]) -> Vec<u8> {
    let mut png = fixture();
    let end = png.len() - 12;
    png.splice(end..end, texts.iter().flatten().copied());
    png
}

fn scan_bytes(data: &[u8]) -> Result<Vec<TextChunk>, ExportError> {
    scan(Cursor::new(data), None, &AtomicBool::new(false))
}

fn without_text(data: &[u8]) -> Vec<u8> {
    let mut result = data[..8].to_vec();
    let mut position = 8;
    while position < data.len() {
        let size =
            u32::from_be_bytes(data[position..position + 4].try_into().expect("length")) as usize;
        let end = position + size + 12;
        if !matches!(
            &data[position + 4..position + 8],
            b"tEXt" | b"zTXt" | b"iTXt"
        ) {
            result.extend_from_slice(&data[position..end]);
        }
        position = end;
    }
    result
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

fn title(value: &str) -> MetadataExportOptions {
    let mut options = MetadataExportOptions::default();
    options
        .set(MetadataField::Title, Some(value.into()))
        .expect("title option");
    options
}

fn animation_fixture(plays: u32, delays: &[[u8; 4]]) -> Vec<u8> {
    animation_fixture_with_regions(plays, delays, false)
}

fn animation_fixture_with_regions(plays: u32, delays: &[[u8; 4]], regions: bool) -> Vec<u8> {
    let mut result = SIGNATURE.to_vec();
    let mut sequence = 0u32;
    for (index, delay) in delays.iter().enumerate() {
        let (width, height, x_offset, y_offset, dispose, blend) = if regions && index > 0 {
            (16, 12, index as u32 * 2, index as u32, index as u8 % 2, 1)
        } else {
            (32, 24, 0, 0, 0, 0)
        };
        let pixels = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x * 7) as u8, (y * 9) as u8, (index * 73) as u8, 128])
        });
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("frame PNG");
        let encoded = encoded.into_inner();
        if index == 0 {
            result.extend_from_slice(&encoded[8..33]);
            result.extend(chunk(
                b"acTL",
                &[
                    &(delays.len() as u32).to_be_bytes()[..],
                    &plays.to_be_bytes(),
                ]
                .concat(),
            ));
        }
        let mut control = Vec::new();
        for number in [sequence, width, height, x_offset, y_offset] {
            control.extend_from_slice(&number.to_be_bytes());
        }
        sequence += 1;
        control.extend_from_slice(delay);
        control.extend_from_slice(&[dispose, blend]);
        result.extend(chunk(b"fcTL", &control));
        let mut offset = 33;
        while offset < encoded.len() {
            let length = u32::from_be_bytes(encoded[offset..offset + 4].try_into().expect("length"))
                as usize;
            if &encoded[offset + 4..offset + 8] == b"IDAT" {
                let data = &encoded[offset + 8..offset + 8 + length];
                if index == 0 {
                    result.extend(chunk(b"IDAT", data));
                } else {
                    result.extend(chunk(
                        b"fdAT",
                        &[&sequence.to_be_bytes()[..], data].concat(),
                    ));
                    sequence += 1;
                }
            }
            offset += length + 12;
        }
    }
    result.extend(chunk(b"tEXt", b"Title\0Animated title"));
    result.extend(chunk(b"IEND", &[]));
    result
}

#[test]
fn apng_export_preserves_all_edited_frames_delays_and_loop_count() {
    let root = root("apng-export");
    let source = root.join("source.png");
    let target = root.join("output.png");
    let bytes = animation_fixture(2, &[[0, 1, 0, 7], [0, 0, 0, 0], [0, 5, 0, 13]]);
    fs::write(&source, &bytes).expect("source APNG");
    let decoded = crate::decode_image(&source).expect("source animation");
    assert_eq!(decoded.frames.len(), 3);
    let mut request = request(&source, &target);
    request.operations = vec![EditOperation::RotateClockwise];
    export_media(&request).expect("APNG save");
    let actual = crate::decode_image(&target).expect("exported animation");
    assert_eq!(
        actual.frames.len(),
        3,
        "saving must not flatten the animation"
    );
    let expected = crate::render_image_edits(
        &decoded,
        &request.operations,
        &crate::Cancellation::default(),
    )
    .expect("edited frames");
    assert_eq!(actual.frames, expected.frames);
    let cancel = AtomicBool::new(false);
    let original = scan_contents(Cursor::new(&bytes), None, None, &cancel)
        .expect("controls")
        .1;
    assert_eq!(
        scan_contents(
            Cursor::new(fs::read(&target).expect("output")),
            None,
            None,
            &cancel
        )
        .expect("saved controls")
        .1,
        original
    );
    for (plays, regions) in [(0, false), (1, true), (i32::MAX as u32, true)] {
        let bytes = animation_fixture_with_regions(
            plays,
            &[
                [0, 1, 0, 0],
                [255, 255, 255, 255],
                [0, 1, 255, 255],
                [0, 19, 0, 100],
            ],
            regions,
        );
        let source = root.join("source.APNG");
        fs::write(&source, &bytes).expect("source alias");
        request.source = source.clone();
        request.target = root.join("output.APNG");
        request.operations = vec![
            EditOperation::Crop(PixelCrop {
                x: 2,
                y: 2,
                width: 28,
                height: 20,
            }),
            EditOperation::RotateClockwise,
            EditOperation::Resize(
                ImageResize::new(30, 42, ResampleFilter::Nearest).expect("resize"),
            ),
        ];
        let decoded = crate::decode_image(&source).expect("composited source");
        let expected = crate::render_image_edits(
            &decoded,
            &request.operations,
            &crate::Cancellation::default(),
        )
        .expect("edited animation");
        // The existing display decoder and FFmpeg round alpha-over differently.
        // Keep a strict export-decoder reference as well as the display comparison.
        let raw = Command::new(crate::media_tools::tool_path("ffmpeg.exe").expect("FFmpeg"))
            .creation_flags(CREATE_NO_WINDOW)
            .args(["-v", "error", "-ignore_loop", "1", "-i"])
            .arg(&source)
            .args([
                "-fps_mode",
                "passthrough",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "pipe:1",
            ])
            .output()
            .expect("reference decode");
        assert!(
            raw.status.success(),
            "{}",
            String::from_utf8_lossy(&raw.stderr)
        );
        assert_eq!(raw.stdout.len(), decoded.frames.len() * 32 * 24 * 4);
        let mut export_source = decoded.clone();
        for (frame, rgba) in export_source
            .frames
            .iter_mut()
            .zip(raw.stdout.as_chunks::<{ 32 * 24 * 4 }>().0)
        {
            frame.rgba.copy_from_slice(rgba);
        }
        let export_expected = crate::render_image_edits(
            &export_source,
            &request.operations,
            &crate::Cancellation::default(),
        )
        .expect("export decoder reference");
        let source_controls = scan_contents(Cursor::new(&bytes), None, None, &cancel)
            .expect("source controls")
            .1;
        for value in [Some("新しい title"), None, Some("")] {
            let metadata = value.map(title).unwrap_or_default();
            export_media_with_options(
                &request,
                ExportOptions {
                    metadata,
                    ..Default::default()
                },
            )
            .expect("APNG metadata save");
            let actual = crate::decode_image(&request.target).expect("reopen alias");
            assert_eq!(actual.frames.len(), expected.frames.len());
            assert!(
                actual.frames == export_expected.frames,
                "all frames must match the export decoder reference: plays={plays}, differences={:?}",
                actual
                    .frames
                    .iter()
                    .zip(&export_expected.frames)
                    .map(|(a, b)| (
                        a.delay,
                        b.delay,
                        a.rgba
                            .iter()
                            .zip(&b.rgba)
                            .enumerate()
                            .find(|(_, (x, y))| x != y),
                        a.rgba
                            .iter()
                            .zip(&b.rgba)
                            .map(|(x, y)| x.abs_diff(*y))
                            .max()
                    ))
                    .collect::<Vec<_>>()
            );
            for (index, (actual, expected)) in
                actual.frames.iter().zip(&expected.frames).enumerate()
            {
                assert_eq!(
                    (actual.width, actual.height, actual.delay),
                    (expected.width, expected.height, expected.delay)
                );
                let maximum = actual
                    .rgba
                    .iter()
                    .zip(&expected.rgba)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .expect("pixels");
                assert!(
                    maximum <= u8::from(regions),
                    "display decoder difference: plays={plays}, frame={index}, maximum={maximum}"
                );
            }
            let (texts, controls) = scan_contents(
                Cursor::new(fs::read(&request.target).expect("saved bytes")),
                None,
                None,
                &cancel,
            )
            .expect("saved controls");
            assert_eq!(controls, source_controls);
            assert_eq!(
                texts.first().map(|text| text.text.as_str()),
                match value {
                    Some("") => None,
                    Some(value) => Some(value),
                    None => Some("Animated title"),
                }
            );
            // A second save must retain the same timing and already-composited pixels.
            let resave = root.join("resaved.png");
            export_media(&self::request(&request.target, &resave)).expect("resave");
            assert_eq!(
                crate::decode_image(&resave).expect("resaved").frames,
                actual.frames
            );
            assert_eq!(
                scan_contents(
                    Cursor::new(fs::read(&resave).expect("resaved bytes")),
                    None,
                    None,
                    &cancel
                )
                .expect("resaved controls")
                .1,
                source_controls
            );
        }
        assert_eq!(fs::read(&source).expect("source unchanged"), bytes);
    }
    assert_eq!(fs::read(&source).expect("source unchanged"), bytes);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn png_text_inspection_reads_all_encodings_variants_and_bounds_display_only() {
    let root = root("png-inspect");
    let source = root.join("source.PNG");
    let mut ztxt = b"Author\0\0".to_vec();
    ztxt.extend_from_slice(&compressed(b"Andr\xe9"));
    let bytes = with_texts(&[
        chunk(b"tEXt", b"Title\0Old title"),
        chunk(b"zTXt", &ztxt),
        international("TITLE", "題名\ntext", false),
        international("Comment", &"音".repeat(500), true),
        international("XML:com.adobe.xmp", "not one of the editable fields", false),
    ]);
    fs::write(&source, &bytes).expect("source");
    let values = read_export_metadata(&source, MediaKind::Image).expect("PNG values");
    assert_eq!(values.len(), 4);
    assert!(values.iter().all(|value| value.scope == "PNG text"));
    assert_eq!(values[0].value, "Old title");
    assert_eq!(values[1].value, "André");
    assert_eq!(values[2].value, "題名\ntext");
    assert_eq!(values[3].value, "音".repeat(341));
    assert!(values[3].truncated);
    assert_eq!(
        scan_bytes(&bytes).expect("full Keep")[3].text,
        "音".repeat(500)
    );
    assert_eq!(fs::read(&source).expect("unchanged"), bytes);
    assert!(read_export_metadata(&source.with_extension("jpg"), MediaKind::Image).is_err());
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

fn map_chunks(bytes: &[u8], mut change: impl FnMut([u8; 4], &mut Vec<u8>) -> bool) -> Vec<u8> {
    let mut output = bytes[..8].to_vec();
    let mut position = 8;
    while position < bytes.len() {
        let length =
            u32::from_be_bytes(bytes[position..position + 4].try_into().expect("length")) as usize;
        let kind = bytes[position + 4..position + 8].try_into().expect("kind");
        let mut data = bytes[position + 8..position + 8 + length].to_vec();
        if change(kind, &mut data) {
            output.extend(chunk(&kind, &data));
        }
        position += length + 12;
    }
    output
}

#[test]
fn apng_scan_rejects_corrupt_controls_sequences_bounds_and_truncation() {
    let bytes = animation_fixture(2, &[[0, 1, 0, 10]; 3]);
    let cancel = AtomicBool::new(false);
    let parse = |bytes: &[u8]| scan_contents(Cursor::new(bytes), None, None, &cancel);
    for end in 0..bytes.len() {
        assert!(parse(&bytes[..end]).is_err(), "truncated at {end}");
    }
    for (kind, offset, value) in [
        (*b"acTL", 0, 0u32),
        (*b"acTL", 0, 2),
        (*b"acTL", 0, 65537),
        (*b"acTL", 4, u32::MAX),
        (*b"fcTL", 0, 9),
        (*b"fcTL", 4, 0),
        (*b"fcTL", 4, 33),
        (*b"fcTL", 12, u32::MAX),
        (*b"fdAT", 0, 0),
    ] {
        let bad = map_chunks(&bytes, |found, data| {
            if found == kind {
                data[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            }
            true
        });
        assert!(
            parse(&bad).is_err(),
            "{kind:?} offset={offset} value={value}"
        );
    }
    for offset in [24, 25] {
        let bad = map_chunks(&bytes, |kind, data| {
            if &kind == b"fcTL" {
                data[offset] = 3;
            }
            true
        });
        assert!(parse(&bad).is_err());
    }
    for kind in [*b"acTL", *b"fcTL", *b"fdAT"] {
        let short = map_chunks(&bytes, |found, data| {
            if found == kind {
                data.truncate(3);
            }
            true
        });
        assert!(parse(&short).is_err());
        let absent = map_chunks(&bytes, |found, _| found != kind);
        assert!(parse(&absent).is_err());
    }
    let mut duplicate = bytes.clone();
    duplicate.splice(33..33, bytes[33..53].iter().copied());
    assert!(parse(&duplicate).is_err());
    let mut crc = bytes.clone();
    crc[52] ^= 1;
    assert!(parse(&crc).is_err());
    let extra = [&bytes[..], &[0]].concat();
    assert!(parse(&extra).is_err());
    let (_, animation) = parse(&bytes).expect("valid");
    let animation = animation.expect("animation");
    assert_eq!(animation.plays, 2);
    assert_eq!(animation.delays, [[0, 1, 0, 10]; 3]);
    assert!(animation.includes_default);
}

#[test]
fn apng_unsupported_exports_preserve_targets_and_static_conversions_still_work() {
    let root = root("apng-unsupported");
    let source = root.join("source.APNG");
    let target = root.join("target.png");
    let bytes = animation_fixture(2, &[[0, 1, 0, 10]; 3]);
    let previous = map_chunks(&bytes, |kind, data| {
        if &kind == b"fcTL" {
            data[24] = 2;
        }
        true
    });
    let mut first = true;
    let poster = map_chunks(&bytes, |kind, data| {
        if &kind == b"acTL" {
            data[..4].copy_from_slice(&2u32.to_be_bytes());
        }
        if &kind == b"fcTL" && first {
            first = false;
            return false;
        }
        if matches!(&kind, b"fcTL" | b"fdAT") {
            let sequence = u32::from_be_bytes(data[..4].try_into().expect("sequence"));
            data[..4].copy_from_slice(&(sequence - 1).to_be_bytes());
        }
        true
    });
    for (bytes, message) in [
        (animation_fixture(1, &[[0, 1, 0, 10]]), "single-frame"),
        (previous, "PREVIOUS"),
        (poster, "poster"),
    ] {
        fs::write(&source, &bytes).expect("unsupported source");
        assert!(
            read_export_metadata(&source, MediaKind::Image).is_ok(),
            "inspection is not export approval"
        );
        fs::write(&target, b"existing target").expect("target");
        assert!(
            export_media(&request(&source, &target))
                .expect_err("unsupported export")
                .to_string()
                .contains(message)
        );
        assert_eq!(fs::read(&target).expect("preserved"), b"existing target");
        assert_eq!(fs::read(&source).expect("source preserved"), bytes);
    }
    fs::write(&source, &bytes).expect("valid source");
    for extension in ["jpg", "gif", "webp", "avif"] {
        let target = root.join(format!("target.{extension}"));
        fs::write(&target, b"existing target").expect("target");
        assert!(
            export_media(&request(&source, &target))
                .expect_err("no flattening")
                .to_string()
                .contains("PNG or APNG")
        );
        assert_eq!(fs::read(&target).expect("preserved"), b"existing target");
    }
    fs::write(&source, fixture()).expect("static source alias");
    export_media(&request(&source, &root.join("static.jpg"))).expect("static conversion");
    export_media(&request(&source, &root.join("static.apng"))).expect("static PNG output alias");
    assert_eq!(
        crate::decode_image(&root.join("static.apng"))
            .expect("static reopen")
            .frames
            .len(),
        1
    );
    assert!(!fs::read_dir(&root).expect("no stage").any(|entry| {
        entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".towavue-export-")
    }));
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn apng_assembly_is_bounded_and_preserves_compressed_pixels_on_failure_and_success() {
    struct CancelWriter<'a>(&'a AtomicBool);
    impl Write for CancelWriter<'_> {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            assert!(data.len() <= 65536);
            if data.len() == 65536 {
                self.0.store(true, Ordering::Relaxed);
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let bytes = animation_fixture(2, &[[0, 1, 0, 10]; 2]);
    let cancel = AtomicBool::new(false);
    let (_, animation) = scan_contents(Cursor::new(&bytes), None, None, &cancel).expect("source");
    let animation = animation.expect("animation");
    let png = fixture();
    let frames = [&png[..], &png[..]].concat();
    let mut output = Vec::new();
    animation
        .assemble(Cursor::new(&frames), &mut output, &cancel)
        .expect("assemble");
    assert_eq!(
        scan_contents(Cursor::new(&output), None, None, &cancel)
            .expect("result")
            .1,
        Some(animation::Animation {
            plays: 2,
            delays: vec![[0, 1, 0, 10]; 2],
            includes_default: true,
            has_previous_disposal: false
        })
    );
    let compressed = |bytes: &[u8]| {
        let mut data = Vec::new();
        map_chunks(bytes, |kind, bytes| {
            if &kind == b"IDAT" {
                data.extend_from_slice(bytes);
            } else if &kind == b"fdAT" {
                data.extend_from_slice(&bytes[4..]);
            }
            true
        });
        data
    };
    assert_eq!(compressed(&output), compressed(&png).repeat(2));
    for end in 0..frames.len() {
        assert!(
            animation
                .assemble(Cursor::new(&frames[..end]), &mut std::io::sink(), &cancel)
                .is_err(),
            "truncated frames {end}"
        );
    }
    assert!(
        animation
            .assemble(
                Cursor::new([&frames[..], &png].concat()),
                &mut std::io::sink(),
                &cancel
            )
            .is_err()
    );
    let changed_size = map_chunks(&png, |kind, data| {
        if &kind == b"IHDR" {
            data[3] += 1;
        }
        true
    });
    assert!(
        animation
            .assemble(
                Cursor::new([&png[..], &changed_size].concat()),
                &mut std::io::sink(),
                &cancel
            )
            .is_err()
    );
    let mut corrupt = frames.clone();
    corrupt[32] ^= 1;
    assert!(
        animation
            .assemble(Cursor::new(corrupt), &mut std::io::sink(), &cancel)
            .is_err()
    );
    assert!(
        animation
            .assemble(Cursor::new(&frames), &mut &mut [0u8; 4][..], &cancel)
            .is_err()
    );
    let mut large = png.clone();
    large.splice(33..33, chunk(b"vpAg", &vec![0; 200_000]));
    assert!(matches!(
        animation.assemble(
            Cursor::new([&large[..], &png].concat()),
            &mut CancelWriter(&cancel),
            &cancel
        ),
        Err(ExportError::Cancelled)
    ));
    cancel.store(false, Ordering::Relaxed);
    let root = root("apng-stage-protection");
    let source = root.join("source.png");
    let target = root.join("target.png");
    fs::write(&source, &bytes).expect("source");
    fs::write(&target, b"existing target").expect("target");
    let metadata =
        PngMetadata::prepare(&request(&source, &target), &title("new"), &cancel).expect("prepare");
    for encoded in [&png[..], &frames[..]] {
        let staging = StagedExport::new(&target).expect("stage");
        fs::write(&staging.output, encoded).expect("encoded");
        if encoded.len() == frames.len() {
            fs::write(staging.directory.join("animation.png"), b"occupied")
                .expect("occupied stage");
        }
        assert!(metadata.apply(&staging, &cancel).is_err());
        assert_eq!(fs::read(&staging.output).expect("stage unchanged"), encoded);
        drop(staging);
        assert_eq!(
            fs::read(&target).expect("target preserved"),
            b"existing target"
        );
        assert_eq!(fs::read_dir(&root).expect("stage cleanup").count(), 2);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn apng_export_cancellation_and_source_changes_never_replace_target() {
    let root = root("apng-source-protection");
    let source = root.join("source.png");
    let target = root.join("target.png");
    let bytes = animation_fixture(2, &[[0, 1, 0, 10]; 3]);
    for change_source in [false, true] {
        fs::write(&source, &bytes).expect("source");
        fs::write(&target, b"existing target").expect("target");
        let cancel = AtomicBool::new(false);
        let changed = AtomicBool::new(false);
        let result = export_cancellable(&request(&source, &target), &cancel, &|_| {
            if !changed.swap(true, Ordering::Relaxed) {
                if change_source {
                    fs::OpenOptions::new()
                        .append(true)
                        .open(&source)
                        .expect("owned source")
                        .write_all(&[0])
                        .expect("change length");
                } else {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        });
        assert!(changed.load(Ordering::Relaxed), "actual encoder progress");
        if change_source {
            assert!(
                result
                    .expect_err("stale source")
                    .to_string()
                    .contains("source changed")
            );
        } else {
            assert!(matches!(result, Err(ExportError::Cancelled)));
        }
        assert_eq!(
            fs::read(&target).expect("target remains"),
            b"existing target"
        );
        assert_eq!(fs::read_dir(&root).expect("no stages").count(), 2);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn png_text_rejects_corruption_truncation_invalid_text_and_bounded_expansion() {
    let original = with_texts(&[international("Title", "hello", true)]);
    for end in 0..original.len() {
        assert!(scan_bytes(&original[..end]).is_err(), "truncation at {end}");
    }
    let mut bad_crc = original.clone();
    bad_crc[20] ^= 1;
    assert!(
        scan_bytes(&bad_crc)
            .expect_err("CRC")
            .to_string()
            .contains("CRC")
    );
    let mut trailing = original.clone();
    trailing.push(0);
    assert!(scan_bytes(&trailing).is_err());
    for bad in [
        chunk(b"tEXt", b"\0no keyword"),
        chunk(b"tEXt", b"Title"),
        chunk(b"tEXt", b" Title\0bad"),
        chunk(b"tEXt", b"A  B\0bad"),
        chunk(b"tEXt", b"Title\0a\0b"),
        chunk(b"zTXt", b"Title\0\x01bad method"),
        chunk(b"iTXt", b"Title\0\x02\0\0\0bad flag"),
        chunk(b"iTXt", b"Title\0\0\0\xff\0\0bad language"),
        chunk(b"iTXt", b"Title\0\0\0\0\xff\0bad translation"),
        chunk(b"iTXt", b"Title\0\0\0\0\0\xff"),
    ] {
        assert!(scan_bytes(&with_texts(&[bad])).is_err());
    }
    let compressed = compressed(b"valid title");
    for end in 0..compressed.len() {
        let mut payload = b"Title\0\0".to_vec();
        payload.extend_from_slice(&compressed[..end]);
        assert!(
            scan_bytes(&with_texts(&[chunk(b"zTXt", &payload)])).is_err(),
            "zlib truncated {end}"
        );
    }
    let mut extra = b"Title\0\0".to_vec();
    extra.extend_from_slice(&compressed);
    extra.push(0);
    assert!(scan_bytes(&with_texts(&[chunk(b"zTXt", &extra)])).is_err());
    let oversized = international("Comment", &"x".repeat(TEXT_LIMIT + 1), true);
    assert!(
        scan_bytes(&with_texts(&[oversized]))
            .expect_err("zip bound")
            .to_string()
            .contains("1 MiB")
    );
    let half = international("Comment", &"x".repeat(TEXT_LIMIT / 2 + 1), true);
    assert_eq!(
        scan_bytes(&with_texts(std::slice::from_ref(&half))).expect("valid multi-buffer inflation")
            [0]
        .text
        .len(),
        TEXT_LIMIT / 2 + 1
    );
    assert!(scan_bytes(&with_texts(&[half.clone(), half])).is_err());
    let exact = international("Comment", &"x".repeat(TEXT_LIMIT), true);
    assert_eq!(
        scan_bytes(&with_texts(&[exact])).expect("exact expanded bound")[0]
            .text
            .len(),
        TEXT_LIMIT
    );
    let raw = chunk(
        b"tEXt",
        &[b"Title\0".as_slice(), &vec![b'x'; TEXT_LIMIT]].concat(),
    );
    assert!(scan_bytes(&with_texts(&[raw])).is_err());
    let count = vec![chunk(b"tEXt", b"Title\0x"); TEXT_COUNT_LIMIT];
    assert_eq!(
        scan_bytes(&with_texts(&count)).expect("count limit").len(),
        TEXT_COUNT_LIMIT
    );
    let count = vec![chunk(b"tEXt", b"Title\0x"); TEXT_COUNT_LIMIT + 1];
    assert!(scan_bytes(&with_texts(&count)).is_err());
    assert!(matches!(
        scan(Cursor::new(&original), None, &AtomicBool::new(true)),
        Err(ExportError::Cancelled)
    ));
}

#[test]
fn png_metadata_splice_preserves_encoded_image_and_keep_chunks_exactly() {
    let root = root("png-splice");
    let source = root.join("source.png");
    let target = root.join("target.png");
    let mut artist = b"Author\0\0".to_vec();
    artist.extend_from_slice(&compressed(b"Original artist"));
    let original = with_texts(&[
        chunk(b"tEXt", b"title\0first"),
        international("Title", "二番", true),
        chunk(b"zTXt", &artist),
        international("Comment", &"音".repeat(500), true),
        international(
            "XML:com.adobe.xmp",
            "do not copy source technical tags",
            false,
        ),
    ]);
    fs::write(&source, &original).expect("source");
    let staging = StagedExport::new(&target).expect("stage");
    let mut encoded = with_texts(&[
        international("Title", "stale encoder title", false),
        chunk(b"tEXt", b"Software\0Encoded by fixture"),
    ]);
    // Ancillary technical bytes and IDAT must be copied, not interpreted or re-encoded.
    encoded.splice(33..33, chunk(b"pHYs", &[0, 0, 0, 72, 0, 0, 0, 72, 1]));
    fs::write(&staging.output, &encoded).expect("encoded stage");
    let cancel = AtomicBool::new(false);
    let metadata = PngMetadata::prepare(
        &request(&source, &target),
        &title("新しい\n= ; \" title"),
        &cancel,
    )
    .expect("prepare");
    metadata.apply(&staging, &cancel).expect("splice");
    let actual = fs::read(&staging.output).expect("rewritten");
    assert_eq!(without_text(&actual), without_text(&encoded));
    assert_eq!(
        image::load_from_memory(&actual).expect("decode"),
        image::load_from_memory(&encoded).expect("baseline")
    );
    assert_eq!(scan_bytes(&actual).expect("read back"), metadata.chunks);
    let originals = scan_bytes(&original).expect("original tags");
    assert_eq!(metadata.chunks[0], originals[2]);
    assert_eq!(metadata.chunks[1], originals[3]);
    assert!(
        actual
            .windows(b"Software".len())
            .any(|bytes| bytes == b"Software")
    );
    assert!(
        !actual
            .windows(b"XML:com.adobe.xmp".len())
            .any(|bytes| bytes == b"XML:com.adobe.xmp")
    );
    assert_eq!(fs::read(&source).expect("source remains"), original);
    drop(staging);
    assert_eq!(fs::read_dir(&root).expect("no stage").count(), 1);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn png_export_round_trips_ten_unicode_fields_keep_remove_and_composed_pixels() {
    let root = root("png-export");
    let source = root.join("source.PNG");
    let bytes = with_texts(&[chunk(b"tEXt", b"Author\0Retained author")]);
    fs::write(&source, &bytes).expect("source");
    let target = root.join("output.PNG");
    let mut request = request(&source, &target);
    request.operations = vec![
        EditOperation::Crop(PixelCrop {
            x: 2,
            y: 4,
            width: 24,
            height: 16,
        }),
        EditOperation::RotateClockwise,
        EditOperation::FlipHorizontal,
        EditOperation::Resize(ImageResize::new(20, 30, ResampleFilter::Lanczos).expect("resize")),
    ];
    export_media(&request).expect("baseline image save");
    let baseline = fs::read(&target).expect("baseline");
    let mut settings = ExportOptions {
        metadata: title("日本語\nquotes \" = ;"),
        ..Default::default()
    };
    export_media_with_options(&request, settings.clone()).expect("PNG metadata save");
    let actual = fs::read(&target).expect("export");
    assert_eq!(without_text(&actual), without_text(&baseline));
    let values = read_export_metadata(&target, MediaKind::Image).expect("reopen");
    assert_eq!(values.len(), 2);
    assert!(
        values
            .iter()
            .any(|value| value.field == MetadataField::Artist && value.value == "Retained author")
    );
    for field in MetadataField::ALL {
        settings
            .metadata
            .set(field, Some(format!("{} 日本語\nline \" = ;", field.key())))
            .expect("all fields");
    }
    export_media_with_options(&request, settings.clone()).expect("all fields save");
    let values = read_export_metadata(&target, MediaKind::Image).expect("all fields reopened");
    assert_eq!(values.len(), 10);
    for value in values {
        assert_eq!(
            Some(value.value.as_str()),
            settings.metadata.get(value.field)
        );
    }
    let all = fs::read(&target).expect("encoded");
    assert_eq!(without_text(&all), without_text(&baseline));
    request.source = target.clone();
    request.target = root.join("removed.png");
    request.operations.clear();
    for field in MetadataField::ALL {
        settings
            .metadata
            .set(field, Some(String::new()))
            .expect("remove");
    }
    export_media_with_options(&request, settings).expect("remove tags");
    assert!(
        read_export_metadata(&request.target, MediaKind::Image)
            .expect("removed values")
            .is_empty()
    );
    assert_eq!(
        image::open(&request.target).expect("removed pixels"),
        image::open(&target).expect("set pixels")
    );
    assert_eq!(fs::read(&source).expect("source unchanged"), bytes);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn png_metadata_export_failure_cancel_and_source_change_protect_files_and_clean_staging() {
    let root = root("png-protection");
    let source = root.join("source.png");
    let bytes = fixture();
    let target = root.join("target.png");
    fs::write(&source, &bytes).expect("source");
    fs::write(&target, b"existing target").expect("target sentinel");
    let request = request(&source, &target);
    let settings = ExportOptions {
        metadata: title("title"),
        ..Default::default()
    };
    for mode in 0..3 {
        let cancel = AtomicBool::new(mode == 0);
        let result = export_options_cancellable(
            &request,
            settings.clone(),
            &cancel,
            &|_| {
                if mode == 1 {
                    cancel.store(true, Ordering::Relaxed);
                } else if mode == 2 {
                    fs::OpenOptions::new()
                        .append(true)
                        .open(&source)
                        .expect("owned source change")
                        .write_all(b"changed")
                        .expect("change length");
                }
            },
            &|_| panic!("no audio analysis"),
        );
        let error = result.expect_err("must not publish");
        if mode < 2 {
            assert!(matches!(error, ExportError::Cancelled));
        } else {
            assert!(error.to_string().contains("source changed"), "{error}");
        }
        assert_eq!(
            fs::read(&target).expect("target preserved"),
            b"existing target"
        );
        assert_eq!(fs::read_dir(&root).expect("stage cleanup").count(), 2);
        fs::write(&source, &bytes).expect("restore owned fixture");
    }
    for extension in ["jpg", "webp", "tiff", "avif", "bmp", "gif"] {
        let other = root.join(format!("target.{extension}"));
        fs::write(&other, b"other sentinel").expect("sentinel");
        let mut unsupported = request.clone();
        unsupported.target = other.clone();
        assert!(export_media_with_options(&unsupported, settings.clone()).is_err());
        assert_eq!(
            fs::read(&other).expect("other preserved"),
            b"other sentinel"
        );
        unsupported.source = other;
        unsupported.target = target.clone();
        assert!(export_media_with_options(&unsupported, settings.clone()).is_err());
    }
    let mut same = request.clone();
    same.target = source.clone();
    assert!(matches!(
        export_media_with_options(&same, settings.clone()),
        Err(ExportError::SameAsSource)
    ));
    let mut corrupt = bytes.clone();
    corrupt[20] ^= 1;
    fs::write(&source, &corrupt).expect("corrupt fixture");
    assert!(export_media_with_options(&request, settings).is_err());
    assert_eq!(
        fs::read(&source).expect("corrupt source unchanged"),
        corrupt
    );
    assert_eq!(
        fs::read(&target).expect("target preserved"),
        b"existing target"
    );
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn png_metadata_worker_publishes_once_without_audio_analysis() {
    let root = root("png-worker");
    let source = root.join("source.png");
    let target = root.join("target.png");
    fs::write(&source, fixture()).expect("source");
    let (send, receive) = std::sync::mpsc::channel();
    let job = ExportJob::start_with_options(
        request(&source, &target),
        ExportOptions {
            metadata: title("worker 日本語"),
            ..Default::default()
        },
        move |event| {
            send.send(event).expect("receiver");
        },
    )
    .expect("worker");
    loop {
        match receive
            .recv_timeout(Duration::from_secs(30))
            .expect("worker event")
        {
            ExportEvent::AnalyzingAudio(_) => panic!("image must not analyze audio"),
            ExportEvent::Progress(_) => {}
            ExportEvent::Finished(result) => {
                assert!(!result.expect("saved").used_hardware_encoder);
                break;
            }
        }
    }
    drop(job);
    assert!(receive.try_recv().is_err());
    assert_eq!(
        read_export_metadata(&target, MediaKind::Image).expect("worker metadata")[0].value,
        "worker 日本語"
    );
    assert_eq!(
        fs::read_dir(&root).expect("only source and output").count(),
        2
    );
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn png_default_save_preserves_known_text_chunks_without_explicit_options() {
    let root = root("png-default-keep");
    let source = root.join("source.png");
    let target = root.join("target.png");
    let original = with_texts(&[
        chunk(b"tEXt", b"Title\0Original title"),
        international("TITLE", "別の言語", true),
        international("Comment", &"音".repeat(500), true),
    ]);
    fs::write(&source, &original).expect("source");
    let request = request(&source, &target);
    export_media(&request).expect("default Keep");
    assert_eq!(
        read(&target, &AtomicBool::new(false)).expect("retained chunks"),
        scan_bytes(&original).expect("source chunks")
    );
    assert_eq!(
        image::open(&target).expect("saved pixels"),
        image::open(&source).expect("source pixels")
    );
    fs::write(
        &source,
        with_texts(&[international("Comment", &"x".repeat(TEXT_LIMIT + 1), true)]),
    )
    .expect("over-budget source");
    let previous = fs::read(&target).expect("previous output");
    assert!(export_media(&request).is_err());
    assert_eq!(fs::read(&target).expect("preserved target"), previous);
    assert_eq!(fs::read_dir(&root).expect("no staging").count(), 2);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn png_metadata_streaming_cancel_write_failure_and_bad_stage_do_not_publish() {
    struct CancelWriter<'a>(&'a AtomicBool);
    impl Write for CancelWriter<'_> {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            assert!(data.len() <= 65536, "no whole-image copy buffer");
            if data.len() == 65536 {
                self.0.store(true, Ordering::Relaxed);
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let root = root("png-stage-protection");
    let source = root.join("source.png");
    let target = root.join("target.png");
    let original = fixture();
    fs::write(&source, &original).expect("source");
    fs::write(&target, b"existing target").expect("target");
    let cancel = AtomicBool::new(false);
    let metadata =
        PngMetadata::prepare(&request(&source, &target), &title("new"), &cancel).expect("prepare");
    let mut large = original.clone();
    large.splice(33..33, chunk(b"vpAg", &vec![0; 200_000]));
    let mut writer = CancelWriter(&cancel);
    assert!(matches!(
        scan(
            Cursor::new(large),
            Some((&mut writer, &metadata.chunks)),
            &cancel
        ),
        Err(ExportError::Cancelled)
    ));
    cancel.store(false, Ordering::Relaxed);
    let mut writer = Cursor::new([0_u8; 40]);
    assert!(matches!(
        scan(
            Cursor::new(&original),
            Some((&mut writer, &metadata.chunks)),
            &cancel
        ),
        Err(ExportError::Output(_))
    ));
    for bad in [
        [SIGNATURE.as_slice(), &chunk(b"IEND", &[])].concat(),
        [
            original[..33].as_ref(),
            &chunk(b"IHDR", &[0; 13]),
            &original[33..],
        ]
        .concat(),
        [original[..33].as_ref(), &u32::MAX.to_be_bytes(), b"tEXt"].concat(),
        [
            original[..33].as_ref(),
            &chunk(b"texT", b"bad reserved bit"),
            &original[33..],
        ]
        .concat(),
    ] {
        assert!(scan_bytes(&bad).is_err());
    }
    for collision in [false, true] {
        let staging = StagedExport::new(&target).expect("stage");
        let mut encoded = original.clone();
        if collision {
            fs::write(staging.directory.join("metadata.png"), b"stage sentinel")
                .expect("collision fixture");
        } else {
            let end = encoded.len() - 1;
            encoded[end] ^= 1;
        }
        fs::write(&staging.output, &encoded).expect("encoded");
        assert!(metadata.apply(&staging, &cancel).is_err());
        assert_eq!(
            fs::read(&staging.output).expect("stage not replaced"),
            encoded
        );
        if collision {
            assert_eq!(
                fs::read(staging.directory.join("metadata.png")).expect("no overwrite"),
                b"stage sentinel"
            );
        }
        assert_eq!(
            fs::read(&target).expect("target preserved"),
            b"existing target"
        );
        drop(staging);
        assert_eq!(fs::read_dir(&root).expect("stage cleanup").count(), 2);
    }
    assert_eq!(fs::read(&source).expect("source preserved"), original);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
