use super::*;
use crate::export::audio_tests::root;
use image::ImageDecoder;
use std::io::Cursor;

fn fixture(alpha: bool) -> Vec<u8> {
    let pixels = image::RgbaImage::from_fn(32, 24, |x, y| {
        image::Rgba([
            (x * 7) as u8,
            (y * 9) as u8,
            (x * y) as u8,
            if alpha { (x * 8) as u8 } else { 255 },
        ])
    });
    let mut output = Cursor::new(Vec::new());
    let pixels = image::DynamicImage::ImageRgba8(pixels);
    let pixels = if alpha {
        pixels
    } else {
        image::DynamicImage::ImageRgb8(pixels.to_rgb8())
    };
    pixels
        .write_to(&mut output, image::ImageFormat::WebP)
        .expect("WebP fixture");
    output.into_inner()
}

fn chunks(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut result = Vec::new();
    walk(
        Cursor::new(bytes),
        |kind, _, input| {
            let mut bytes = Vec::new();
            input.read_to_end(&mut bytes).expect("payload");
            result.push((kind, bytes));
            Ok(())
        },
        &AtomicBool::new(false),
    )
    .expect("chunks");
    result
}

fn riff(chunks: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
    for (kind, payload) in chunks {
        chunk(&mut bytes, kind, payload).expect("chunk");
    }
    let size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    bytes
}

fn tagged(bytes: &[u8], packet: &[u8]) -> Vec<u8> {
    let cancel = AtomicBool::new(false);
    let info = container(Cursor::new(bytes), &cancel).expect("container");
    let mut output = Cursor::new(Vec::new());
    rewrite(Cursor::new(bytes), &mut output, &info, packet, &cancel).expect("rewrite");
    output.into_inner()
}

fn settings() -> MetadataExportOptions {
    let mut options = MetadataExportOptions::default();
    for field in ImageMetadataFormat::Webp.fields() {
        let text = match field {
            MetadataField::Date => "2024-02-29T12:34:56.7+09:00".into(),
            MetadataField::Track => "+0002".into(),
            _ => format!("日本語 & <{}>\r\ntext", field.label()),
        };
        options.set(*field, Some(text)).expect("option");
    }
    options
}

fn packet() -> Vec<u8> {
    let mut values = Vec::new();
    xmp::apply(&mut values, &settings()).expect("values");
    xmp::encode(&values).expect("packet")
}

fn non_xmp(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    chunks(bytes)
        .into_iter()
        .filter(|(kind, _)| kind != b"XMP " && kind != b"VP8X")
        .collect()
}

#[test]
fn webp_rewrite_preserves_lossless_bitstreams_pixels_and_extended_payloads() {
    let packet = packet();
    for alpha in [false, true] {
        let source = fixture(alpha);
        let output = tagged(&source, &packet);
        assert_eq!(non_xmp(&source), non_xmp(&output));
        assert_eq!(
            image::load_from_memory(&source).expect("source pixels"),
            image::load_from_memory(&output).expect("output pixels")
        );
        let mut decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(&output))
            .expect("independent decoder");
        assert_eq!(
            decoder.xmp_metadata().expect("independent XMP"),
            Some(packet.clone())
        );
        assert_eq!(chunks(&output)[0].1[0], 4 | if alpha { 16 } else { 0 });
        let mut extended = chunks(&output);
        extended[0].1[0] |= 32 | 8;
        extended[0].1.extend_from_slice(&[12, 34]);
        extended.insert(1, (*b"ICCP", vec![1, 2, 3]));
        extended.push((*b"EXIF", vec![4, 5, 6]));
        extended.push((*b"TEST", vec![7, 8, 9]));
        let extended = riff(&extended);
        for replacement in [packet.as_slice(), &[]] {
            let rewritten = tagged(&extended, replacement);
            assert_eq!(non_xmp(&extended), non_xmp(&rewritten));
            let mut expected = chunks(&extended)[0].1.clone();
            if replacement.is_empty() {
                expected[0] &= !4;
            }
            assert_eq!(chunks(&rewritten)[0].1, expected);
            assert_eq!(
                container(Cursor::new(&rewritten), &AtomicBool::new(false))
                    .expect("valid result")
                    .packet
                    .as_deref(),
                (!replacement.is_empty()).then_some(replacement)
            );
        }
    }
}

#[test]
fn webp_container_rejects_malformed_ambiguous_animated_and_oversized_inputs() {
    let source = tagged(&fixture(false), &packet());
    let valid = chunks(&source);
    let cancel = AtomicBool::new(false);
    for end in 0..source.len() {
        assert!(
            container(Cursor::new(&source[..end]), &cancel).is_err(),
            "truncated at {end}"
        );
    }
    let mut bad = Vec::new();
    for offset in [0, 4, 8] {
        let mut bytes = source.clone();
        bytes[offset] ^= 1;
        bad.push(bytes);
    }
    let mut bytes = source.clone();
    bytes.push(0);
    bad.push(bytes);
    let mut bytes = source.clone();
    bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    bad.push(bytes);
    for flag in [2, 4, 8, 32] {
        let mut list = valid.clone();
        list[0].1[0] ^= flag;
        bad.push(riff(&list));
    }
    let mut list = valid.clone();
    list[0].1[4] ^= 1;
    bad.push(riff(&list));
    for item in [
        valid[0].clone(),
        valid[1].clone(),
        valid[2].clone(),
        (*b"ANIM", vec![0; 6]),
        (*b"ANMF", vec![0; 16]),
        (*b"ALPH", vec![0]),
        (*b"ICCP", vec![0]),
    ] {
        let mut list = valid.clone();
        list.push(item);
        bad.push(riff(&list));
    }
    bad.push(riff(&valid[1..]));
    bad.push(riff(&valid[..1]));
    let mut list = valid.clone();
    list[1].1[0] = 0;
    bad.push(riff(&list));
    let mut list = valid.clone();
    list[1].1[4] |= 0xe0;
    bad.push(riff(&list));
    let mut list = valid.clone();
    list[2].1 = vec![0; xmp::LIMIT + 1];
    bad.push(riff(&list));
    let mut list = valid.clone();
    list.push((*b"TEST", vec![1]));
    let mut bytes = riff(&list);
    *bytes.last_mut().expect("padding") = 1;
    bad.push(bytes);
    let mut list = valid.clone();
    list.extend(std::iter::repeat_n((*b"TEST", Vec::new()), 65536));
    bad.push(riff(&list));
    for (index, bytes) in bad.iter().enumerate() {
        assert!(
            container(Cursor::new(bytes), &cancel).is_err(),
            "accepted malformed case {index}"
        );
    }
    assert!(matches!(
        container(Cursor::new(&source), &AtomicBool::new(true)),
        Err(ExportError::Cancelled)
    ));
}

#[test]
fn webp_real_export_keeps_sets_removes_nine_fields_and_protects_files_on_failure() {
    let root = root("webp-metadata");
    let source = root.join("source.webp");
    let target = root.join("target.webp");
    let original = tagged(&fixture(true), &packet());
    fs::write(&source, &original).expect("source");
    let request = ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: MediaKind::Image,
        operations: vec![EditOperation::RotateClockwise],
        hardware_encode: false,
    };
    export_media(&request).expect("all Keep export");
    let baseline = fs::read(&target).expect("baseline");
    let shown = inspect(&target).expect("inspect output");
    assert_eq!(shown.len(), 9);
    for field in ImageMetadataFormat::Webp.fields() {
        assert!(shown.iter().any(|value| value.field == *field
            && Some(value.value.as_str()) == settings().get(*field)
            && value.scope.starts_with("WebP XMP")));
    }
    for remove in [false, true] {
        let mut metadata = MetadataExportOptions::default();
        for field in ImageMetadataFormat::Webp.fields() {
            let text = if remove {
                ""
            } else {
                match field {
                    MetadataField::Date => "1999",
                    MetadataField::Track => "-12",
                    _ => "Replacement",
                }
            };
            metadata.set(*field, Some(text.into())).expect("update");
        }
        export_media_with_options(
            &request,
            ExportOptions {
                metadata: metadata.clone(),
                ..Default::default()
            },
        )
        .expect("Set/Remove export");
        let actual = fs::read(&target).expect("actual");
        assert_eq!(non_xmp(&actual), non_xmp(&baseline));
        assert_eq!(
            image::load_from_memory(&actual).expect("actual pixels"),
            image::load_from_memory(&baseline).expect("baseline pixels")
        );
        let shown = inspect(&target).expect("shown");
        if remove {
            assert!(shown.is_empty());
        } else {
            assert_eq!(shown.len(), 9);
            for value in shown {
                assert_eq!(Some(value.value.as_str()), metadata.get(value.field));
            }
        }
    }
    let protected = fs::read(&target).expect("protected");
    let mut invalid = settings();
    invalid
        .set(MetadataField::Date, Some("2023-02-29".into()))
        .expect("text");
    assert!(
        export_media_with_options(
            &request,
            ExportOptions {
                metadata: invalid,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(fs::read(&source).expect("source retained"), original);
    for malformed in [
        b"not WebP".to_vec(),
        {
            let mut list = chunks(&original);
            list[0].1[0] |= 2;
            riff(&list)
        },
        tagged(&fixture(false), b"<broken>"),
    ] {
        fs::write(&source, &malformed).expect("bad source");
        assert!(export_media(&request).is_err());
        assert_eq!(fs::read(&source).expect("bad source retained"), malformed);
        assert_eq!(fs::read(&target).expect("target retained"), protected);
    }
    assert_eq!(fs::read_dir(&root).expect("staging cleanup").count(), 2);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn webp_streaming_cancellation_io_failure_and_staging_cleanup_are_bounded() {
    struct CancellingReader<'a> {
        input: Cursor<Vec<u8>>,
        cancelled: &'a AtomicBool,
    }
    impl Read for CancellingReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            assert!(buffer.len() <= 65536, "bounded reads");
            let count = self.input.read(buffer)?;
            if self.input.position() >= 65536 {
                self.cancelled.store(true, Ordering::Relaxed);
            }
            Ok(count)
        }
    }
    let cancel = AtomicBool::new(false);
    let mut list = chunks(&fixture(false));
    list.push((*b"TEST", vec![0; 1024 * 1024]));
    let bytes = riff(&list);
    let info = container(Cursor::new(&bytes), &cancel).expect("large unknown chunk");
    let mut reader = CancellingReader {
        input: Cursor::new(bytes),
        cancelled: &cancel,
    };
    let mut output = Cursor::new(Vec::new());
    assert!(matches!(
        rewrite(&mut reader, &mut output, &info, &packet(), &cancel),
        Err(ExportError::Cancelled)
    ));
    assert!(reader.input.position() < 2 * 65536);
    let bytes = fixture(false);
    let cancel = AtomicBool::new(false);
    let info = container(Cursor::new(&bytes), &cancel).expect("fixture");
    assert!(
        rewrite(
            Cursor::new(&bytes),
            &mut Cursor::new([0; 16]),
            &info,
            &packet(),
            &cancel
        )
        .is_err()
    );

    let root = root("webp-stage-failures");
    let target = root.join("target.webp");
    fs::write(&target, b"existing target").expect("target");
    for cancelled in [false, true] {
        let staging = StagedExport::new(&target).expect("stage");
        fs::write(&staging.output, &bytes).expect("encoded stage");
        let metadata = WebpMetadata {
            animation: None,
            values: Vec::new(),
            packet: packet(),
        };
        assert!(
            metadata
                .apply(&staging, &AtomicBool::new(cancelled))
                .is_err(),
            "verification mismatch or cancellation"
        );
        assert_eq!(fs::read(&staging.output).expect("stage retained"), bytes);
        drop(staging);
        assert_eq!(
            fs::read(&target).expect("target retained"),
            b"existing target"
        );
        assert_eq!(fs::read_dir(&root).expect("cleanup").count(), 1);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn webp_keeps_language_author_order_and_noncanonical_source_spelling() {
    let root = root("webp-keep-variants");
    let source = root.join("source.webp");
    let target = root.join("target.webp");
    let values = vec![
        xmp::Value {
            field: MetadataField::Title,
            language: Some("ja".into()),
            text: "元の題名".into(),
        },
        xmp::Value {
            field: MetadataField::Title,
            language: Some("x-default".into()),
            text: "Original title".into(),
        },
        xmp::Value {
            field: MetadataField::Artist,
            language: None,
            text: "First".into(),
        },
        xmp::Value {
            field: MetadataField::Artist,
            language: None,
            text: "Second".into(),
        },
        xmp::Value {
            field: MetadataField::Date,
            language: None,
            text: "circa 1999".into(),
        },
        xmp::Value {
            field: MetadataField::Track,
            language: None,
            text: "2/12".into(),
        },
    ];
    let packet = xmp::encode(&values).expect("existing text");
    let expected = xmp::parse(&packet, &AtomicBool::new(false)).expect("source ordering");
    fs::write(&source, tagged(&fixture(false), &packet)).expect("source");
    export_media(&ExportRequest {
        source,
        target: target.clone(),
        kind: MediaKind::Image,
        operations: vec![],
        hardware_encode: false,
    })
    .expect("Keep");
    assert_eq!(
        read(&target, &AtomicBool::new(false)).expect("retained"),
        expected
    );
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
