use super::*;
use crate::export::audio_tests::root;
use std::io::Cursor;

const PACKET: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:d="http://purl.org/dc/elements/1.1/" xmlns:m="http://ns.adobe.com/xmp/1.0/DynamicMedia/" xmlns:t="http://ns.adobe.com/tiff/1.0/" r:about="" t:Orientation="6"><d:title><r:Alt><r:li xml:lang="x-default">Original &amp; title</r:li><r:li xml:lang="ja">日本語の題名</r:li></r:Alt></d:title><d:creator><r:Seq><r:li>First author</r:li><r:li><![CDATA[Second <author>]]></r:li></r:Seq></d:creator><d:description><r:Alt><r:li xml:lang="x-default">Original comment</r:li></r:Alt></d:description><d:rights><r:Alt><r:li xml:lang="x-default">Original rights</r:li></r:Alt></d:rights><m:album>Original album</m:album><m:composer>Original composer</m:composer><m:genre>Original genre</m:genre></r:Description></r:RDF></x:xmpmeta>"#;

fn fixture() -> Vec<u8> {
    let pixels = image::RgbImage::from_fn(32, 24, |x, y| {
        image::Rgb([(x * 7) as u8, (y * 9) as u8, (x * y) as u8])
    });
    let mut output = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(pixels)
        .write_to(&mut output, image::ImageFormat::Jpeg)
        .expect("JPEG fixture");
    output.into_inner()
}

fn segment(code: u8, bytes: &[u8]) -> Vec<u8> {
    [
        &[0xff, code][..],
        &((bytes.len() + 2) as u16).to_be_bytes(),
        bytes,
    ]
    .concat()
}

fn tagged(packet: &[u8]) -> Vec<u8> {
    let bytes = fixture();
    let mut output = Vec::new();
    scan(
        Cursor::new(&bytes),
        Some(&mut output),
        packet,
        &AtomicBool::new(false),
    )
    .expect("fixture packet");
    output
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

fn options(field: MetadataField, text: &str) -> ExportOptions {
    let mut metadata = MetadataExportOptions::default();
    metadata.set(field, Some(text.into())).expect("metadata");
    ExportOptions {
        metadata,
        ..Default::default()
    }
}

fn without_xmp(bytes: &[u8]) -> Vec<u8> {
    let mut position = 2;
    let mut result = bytes[..2].to_vec();
    while bytes[position + 1] != 0xda {
        let length = u16::from_be_bytes([bytes[position + 2], bytes[position + 3]]) as usize;
        let end = position + 2 + length;
        if bytes[position + 1] != 0xe1 || !bytes[position + 4..end].starts_with(XMP) {
            result.extend_from_slice(&bytes[position..end]);
        }
        position = end;
    }
    result.extend_from_slice(&bytes[position..]);
    result
}

#[test]
fn jpeg_date_and_track_keep_set_remove_preserve_pixels_and_reject_invalid_updates() {
    let root = root("jpeg-typed-metadata");
    let source = root.join("source.jpg");
    let target = root.join("target.jpg");
    for (field, local, original_text, replacement, invalid_text) in [
        (
            MetadataField::Date,
            "releaseDate",
            "circa 1999",
            "2024-02-29T12:34:56.789+09:00",
            "2023-02-29",
        ),
        (MetadataField::Track, "trackNumber", "2/12", "+0002", "2/12"),
    ] {
        assert!(ImageMetadataFormat::Jpeg.fields().contains(&field));
        for attribute in [false, true] {
            let property = if attribute {
                format!("m:{local}=\"{original_text}\"")
            } else {
                String::new()
            };
            let child = if attribute {
                String::new()
            } else {
                format!("<m:{local}>{original_text}</m:{local}>")
            };
            let packet = format!(
                "<r:RDF xmlns:r=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><r:Description xmlns:m=\"http://ns.adobe.com/xmp/1.0/DynamicMedia/\" {property}>{child}</r:Description></r:RDF>"
            );
            let original = tagged(packet.as_bytes());
            fs::write(&source, &original).expect("source");
            let shown = inspect(&source).expect("read existing source spelling");
            assert_eq!(shown.len(), 1);
            assert_eq!(shown[0].field, field);
            assert_eq!(shown[0].value, original_text);
            let mut request = request(&source, &target);
            request.operations.push(EditOperation::RotateClockwise);
            export_media(&request).expect("Keep existing noncanonical spelling");
            let baseline = fs::read(&target).expect("baseline");
            assert_eq!(inspect(&target).expect("kept")[0].value, original_text);
            for text in [replacement, ""] {
                export_media_with_options(&request, options(field, text)).expect("Set or Remove");
                let actual = fs::read(&target).expect("output");
                assert_eq!(without_xmp(&actual), without_xmp(&baseline));
                assert_eq!(
                    image::open(&target).expect("pixels"),
                    image::load_from_memory(&baseline).expect("baseline pixels")
                );
                let shown = inspect(&target).expect("read output");
                if text.is_empty() {
                    assert!(shown.is_empty());
                } else {
                    assert_eq!(shown.len(), 1);
                    assert_eq!(shown[0].value, text);
                }
            }
            let protected = fs::read(&target).expect("protected target");
            assert!(
                ImageMetadataFormat::Jpeg
                    .validate_options(&options(field, invalid_text).metadata)
                    .is_err()
            );
            assert!(export_media_with_options(&request, options(field, invalid_text)).is_err());
            assert_eq!(fs::read(&target).expect("target unchanged"), protected);
            assert_eq!(fs::read(&source).expect("source unchanged"), original);
        }
    }
    assert_eq!(fs::read_dir(&root).expect("staging cleanup").count(), 2);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn jpeg_dynamic_media_text_round_trips_simple_properties_and_rejects_ambiguous_values() {
    let cancel = AtomicBool::new(false);
    let namespace = "http://ns.adobe.com/xmp/1.0/DynamicMedia/";
    for (field, local) in [
        (MetadataField::Album, "album"),
        (MetadataField::Composer, "composer"),
        (MetadataField::Genre, "genre"),
    ] {
        let packet = |attributes: &str, children: &str| {
            format!(
                "<r:RDF xmlns:r=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><r:Description xmlns:m=\"{namespace}\" {attributes}>{children}</r:Description></r:RDF>"
            )
        };
        for input in [
            packet(&format!("m:{local}=\"日本語 &amp; text&#13;&#10;\""), ""),
            packet(
                "",
                &format!("<m:{local}>日本語 &amp; text&#13;&#10;</m:{local}>"),
            ),
        ] {
            let values = xmp::parse(input.as_bytes(), &cancel).expect("simple XMP");
            assert_eq!(
                values,
                vec![xmp::Value {
                    field,
                    language: None,
                    text: "日本語 & text\r\n".into(),
                }]
            );
            let encoded = xmp::encode(&values).expect("simple encode");
            let text = std::str::from_utf8(&encoded).expect("UTF-8");
            assert!(text.contains(&format!("<xmpDM:{local}>")));
            assert!(!text.contains("rdf:Seq") && !text.contains("rdf:Alt"));
            assert_eq!(xmp::parse(&encoded, &cancel).expect("round trip"), values);
        }
        for bad in [
            packet(
                &format!("m:{local}=\"one\""),
                &format!("<m:{local}>two</m:{local}>"),
            ),
            packet(
                "",
                &format!("<m:{local}><r:Seq><r:li>wrong type</r:li></r:Seq></m:{local}>"),
            ),
            packet(
                "",
                &format!("<m:{local} xml:lang=\"ja\">qualified</m:{local}>"),
            ),
            packet(
                "",
                &format!("<m:{local} r:resource=\"https://example.invalid/\"/>"),
            ),
        ] {
            assert!(
                xmp::parse(bad.as_bytes(), &cancel).is_err(),
                "accepted {bad}"
            );
        }
        let wrong_namespace = packet("", &format!("<m:{local}>unrelated</m:{local}>"))
            .replace(namespace, "urn:unrelated");
        assert!(
            xmp::parse(wrong_namespace.as_bytes(), &cancel)
                .expect("other namespace")
                .is_empty()
        );
        let mut values = xmp::parse(PACKET.as_bytes(), &cancel).expect("existing fields");
        let original: Vec<_> = values
            .iter()
            .filter(|value| value.field != field)
            .cloned()
            .collect();
        xmp::apply(&mut values, &options(field, "New 日本語 <>&\r\n").metadata)
            .expect("set simple text");
        assert!(
            values
                .iter()
                .any(|value| value.field == field && value.language.is_none())
        );
        assert_eq!(
            xmp::parse(&xmp::encode(&values).expect("mixed encode"), &cancel).expect("mixed parse"),
            values
        );
        xmp::apply(&mut values, &MetadataExportOptions::default()).expect("Keep");
        xmp::apply(&mut values, &options(field, "").metadata).expect("Remove");
        assert_eq!(values, original);
    }
}

#[test]
fn jpeg_metadata_inspection_is_bounded_but_default_keep_preserves_full_languages_and_creators() {
    let root = root("jpeg-inspect-keep");
    let source = root.join("source.JPEG");
    let target = root.join("saved.JPG");
    let long = "日本語".repeat(200);
    let language = "a".repeat(100);
    let packet = PACKET
        .replace("日本語の題名", &long)
        .replace("xml:lang=\"ja\"", &format!("xml:lang=\"{language}\""));
    let original = tagged(packet.as_bytes());
    fs::write(&source, &original).expect("source");
    let shown = read_export_metadata(&source, MediaKind::Image).expect("public inspection");
    assert_eq!(shown.len(), 9);
    assert_eq!(shown[0].scope, "JPEG XMP (x-default)");
    assert_eq!(shown[1].scope, format!("JPEG XMP ({}…)", "a".repeat(63)));
    assert_eq!(shown[1].value, long[..long.floor_char_boundary(1024)]);
    assert!(shown[1].truncated);
    assert_eq!(shown[2].scope, "JPEG XMP (creator 1)");
    assert_eq!(shown[3].scope, "JPEG XMP (creator 2)");
    assert_eq!(shown[3].value, "Second <author>");
    for (field, text) in [
        (MetadataField::Album, "Original album"),
        (MetadataField::Composer, "Original composer"),
        (MetadataField::Genre, "Original genre"),
    ] {
        assert!(
            shown.iter().any(|value| value.field == field
                && value.scope == "JPEG XMP"
                && value.value == text)
        );
    }
    assert!(
        shown
            .iter()
            .enumerate()
            .all(|(index, value)| value.truncated == (index == 1))
    );
    let cancel = AtomicBool::new(false);
    let expected = read(&source, &cancel).expect("full source");
    let mut request = request(&source, &target);
    request.operations.push(EditOperation::RotateClockwise);
    export_media(&request).expect("default all Keep");
    assert_eq!(read(&target, &cancel).expect("full output"), expected);
    assert_eq!(
        read_export_metadata(&target, MediaKind::Image).expect("display output"),
        shown
    );
    let pixels = image::open(&target).expect("rotated image");
    assert_eq!((pixels.width(), pixels.height()), (24, 32));
    assert_eq!(fs::read(&source).expect("source unchanged"), original);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn jpeg_metadata_capability_and_xml_validation_match_export_contract() {
    for extension in ["jpg", "jpeg", "JPG", "JPEG"] {
        assert_eq!(
            ImageMetadataFormat::from_path(Path::new(&format!("source.{extension}"))),
            Some(ImageMetadataFormat::Jpeg)
        );
    }
    assert_eq!(
        ImageMetadataFormat::from_path(Path::new("source.PnG")),
        Some(ImageMetadataFormat::Png)
    );
    for path in ["source", "source.tiff", "source.jpg.pngx"] {
        assert!(ImageMetadataFormat::from_path(Path::new(path)).is_none());
        assert!(read_export_metadata(Path::new(path), MediaKind::Image).is_err());
    }
    assert_eq!(ImageMetadataFormat::Png.fields(), &MetadataField::ALL);
    assert_eq!(
        ImageMetadataFormat::Jpeg.fields(),
        &[
            MetadataField::Title,
            MetadataField::Artist,
            MetadataField::Album,
            MetadataField::Composer,
            MetadataField::Genre,
            MetadataField::Date,
            MetadataField::Track,
            MetadataField::Comment,
            MetadataField::Copyright
        ]
    );
    for field in MetadataField::ALL {
        for text in ["", "日本語 & <text>\r\n\t"] {
            let metadata = options(field, text).metadata;
            assert!(ImageMetadataFormat::Png.validate_options(&metadata).is_ok());
            assert_eq!(
                ImageMetadataFormat::Jpeg
                    .validate_options(&metadata)
                    .is_ok(),
                ImageMetadataFormat::Jpeg.fields().contains(&field)
                    && (text.is_empty()
                        || !matches!(field, MetadataField::Date | MetadataField::Track))
            );
        }
    }
    for text in ["bad\u{1}", "bad\u{b}", "bad\u{fffe}", "bad\u{ffff}"] {
        assert!(
            ImageMetadataFormat::Jpeg
                .validate_options(&options(MetadataField::Title, text).metadata)
                .is_err()
        );
    }
}

#[test]
fn jpeg_default_keep_corruption_cancellation_and_source_change_preserve_target() {
    let root = root("jpeg-default-protection");
    let source = root.join("source.jpg");
    let target = root.join("target.jpg");
    let original = tagged(PACKET.as_bytes());
    fs::write(&source, &original).expect("source");
    fs::write(&target, b"existing target").expect("target");
    let request = request(&source, &target);
    for mode in 0..3 {
        let cancel = AtomicBool::new(mode == 0);
        let error = export_options_cancellable(
            &request,
            ExportOptions::default(),
            &cancel,
            &|_| {
                if mode == 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
                if mode == 2 {
                    fs::OpenOptions::new()
                        .append(true)
                        .open(&source)
                        .expect("source")
                        .write_all(b"changed")
                        .expect("modify fixture");
                }
            },
            &|_| panic!("no audio"),
        )
        .expect_err("must not publish");
        if mode == 2 {
            assert!(error.to_string().contains("source changed"), "{error}");
        } else {
            assert!(matches!(error, ExportError::Cancelled));
        }
        assert_eq!(fs::read(&target).expect("protected"), b"existing target");
        assert_eq!(fs::read_dir(&root).expect("no stage leaks").count(), 2);
        fs::write(&source, &original).expect("restore fixture");
    }
    let invalid = tagged(
        PACKET
            .replace("Original &amp; title", "&missing;")
            .as_bytes(),
    );
    fs::write(&source, &invalid).expect("invalid source");
    assert!(read_export_metadata(&source, MediaKind::Image).is_err());
    assert!(export_media(&request).is_err());
    assert_eq!(fs::read(&target).expect("protected"), b"existing target");
    assert_eq!(fs::read(&source).expect("unchanged"), invalid);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn xmp_round_trips_namespace_aliases_languages_creators_escaping_and_overrides() {
    let cancelled = AtomicBool::new(false);
    let original = xmp::parse(PACKET.as_bytes(), &cancelled).expect("source properties");
    assert_eq!(original.len(), 9);
    assert_eq!(original[0].text, "Original & title");
    assert_eq!(original[1].language.as_deref(), Some("ja"));
    assert_eq!(original[3].text, "Second <author>");
    let encoded = xmp::encode(&original).expect("encode");
    assert_eq!(
        xmp::parse(&encoded, &cancelled).expect("round trip"),
        original
    );
    assert!(!String::from_utf8_lossy(&encoded).contains("Orientation"));
    let mut values = original.clone();
    for field in [
        MetadataField::Title,
        MetadataField::Artist,
        MetadataField::Album,
        MetadataField::Composer,
        MetadataField::Genre,
        MetadataField::Comment,
        MetadataField::Copyright,
    ] {
        let text = format!("{} 日本語\r\n<>&'\"", field.key());
        xmp::apply(&mut values, &options(field, &text).metadata).expect("set");
    }
    assert_eq!(values.len(), 7);
    assert_eq!(
        xmp::parse(&xmp::encode(&values).expect("encode"), &cancelled).expect("all values"),
        values
    );
    xmp::apply(&mut values, &options(MetadataField::Title, "").metadata).expect("remove");
    assert!(
        values
            .iter()
            .all(|value| value.field != MetadataField::Title)
    );
    let attribute = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" dc:title="Title &#13;&amp;&quot;" dc:creator="Creator"/></rdf:RDF>"#;
    let values = xmp::parse(attribute.as_bytes(), &cancelled).expect("attribute properties");
    assert_eq!(values[0].text, "Title \r&\"");
    assert_eq!(
        xmp::parse(&xmp::encode(&values).expect("encode"), &cancelled)
            .expect("attributes round trip"),
        values
    );
    assert!(matches!(
        xmp::parse(PACKET.as_bytes(), &AtomicBool::new(true)),
        Err(ExportError::Cancelled)
    ));
}

#[test]
fn xmp_rejects_entities_invalid_xml_ambiguous_properties_and_resource_limits() {
    let cancel = AtomicBool::new(false);
    for bad in [
        format!("<!DOCTYPE x [<!ENTITY e SYSTEM 'file:///C:/private'>]>{PACKET}"),
        PACKET.replace("Original &amp; title", "&unknown;"),
        PACKET.replace("Original &amp; title", "&#0;"),
        PACKET.replace("Original &amp; title", "&#xFFFF;"),
        PACKET.replace("xml:lang=\"ja\"", "xml:lang=\"x-default\""),
        PACKET.replace("<d:title>", "<d:title r:parseType=\"Resource\">"),
        PACKET.replace("r:Alt", "r:Bag"),
        PACKET.replace("</d:title>", "</d:title><d:title>duplicate</d:title>"),
        PACKET.replace("<d:title>", "<undefined:title>"),
        PACKET.replace("</d:title>", "</d:rights>"),
        PACKET.replace(
            "http://purl.org/dc/elements/1.1/",
            "&#104;ttp://purl.org/dc/elements/1.1/",
        ),
        PACKET.replace("r:about=\"\"", "r:about=\"https://example.invalid/other\""),
        format!("{PACKET}{PACKET}"),
        format!("junk{PACKET}"),
        format!("<?xml version=\"1.1\"?>{PACKET}"),
        format!("<?xml version=\"1.0\" encoding=\"UTF-16\"?>{PACKET}"),
        format!("{}x", " ".repeat(xmp::LIMIT)),
        format!("{}{}", "<a>".repeat(33), "</a>".repeat(33)),
        format!("<a>{}</a>", "<b/>".repeat(4096)),
    ] {
        assert!(
            xmp::parse(bad.as_bytes(), &cancel).is_err(),
            "accepted invalid input: {}",
            &bad[..bad.len().min(80)]
        );
    }
    for length in 0..PACKET.len() {
        assert!(
            xmp::parse(&PACKET.as_bytes()[..length], &cancel).is_err(),
            "truncated {length}"
        );
    }
    let too_many = PACKET.replace(
        "<r:li>First author</r:li>",
        &"<r:li>author</r:li>".repeat(129),
    );
    assert!(xmp::parse(too_many.as_bytes(), &cancel).is_err());
    let expensive = PACKET.replace(
        "Original &amp; title",
        &format!("<![CDATA[{}]]>", "&".repeat(20000)),
    );
    let values = xmp::parse(expensive.as_bytes(), &cancel).expect("bounded source text");
    assert!(
        xmp::encode(&values).is_err(),
        "escaped output must fit APP1 too"
    );
    for field in [
        MetadataField::AlbumArtist,
        MetadataField::Date,
        MetadataField::Track,
    ] {
        assert!(xmp::apply(&mut vec![], &options(field, "value").metadata).is_err());
    }
    assert!(
        xmp::apply(
            &mut vec![],
            &options(MetadataField::Title, "bad\u{0001}").metadata
        )
        .is_err()
    );
}

#[test]
fn unedited_jpeg_export_preserves_compressed_pixels_and_non_xmp_markers() {
    let root = root("jpeg-unedited-export");
    let source = root.join("source.JPEG");
    let target = root.join("target.jpg");
    let mut original = tagged(PACKET.as_bytes());
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    for (code, payload) in [
        (0xe1, exif.as_slice()),
        (
            0xe2,
            b"ICC_PROFILE\0\x01\x01preserved opaque profile".as_slice(),
        ),
        (0xed, b"Photoshop 3.0\0preserved opaque IPTC".as_slice()),
        (0xfe, b"preserved JPEG comment".as_slice()),
    ] {
        original.splice(2..2, segment(code, payload));
    }
    fs::write(&source, &original).expect("source");
    let cancel = AtomicBool::new(false);
    let original_values = read(&source, &cancel).expect("source metadata");
    for settings in [
        ExportOptions::default(),
        options(MetadataField::Title, "new title"),
        options(MetadataField::Title, ""),
    ] {
        let mut expected = original_values.clone();
        xmp::apply(&mut expected, &settings.metadata).expect("expected values");
        export_media_with_options(&request(&source, &target), settings).expect("save");
        let actual = fs::read(&target).expect("output");
        assert_eq!(without_xmp(&actual), without_xmp(&original));
        assert_eq!(read(&target, &cancel).expect("metadata"), expected);
        assert_eq!(
            image::load_from_memory(&actual).expect("pixels"),
            image::load_from_memory(&original).expect("source pixels")
        );
        let resaved = root.join("resaved.jpeg");
        export_media(&request(&target, &resaved)).expect("resave");
        assert_eq!(fs::read(&resaved).expect("resaved bytes"), actual);
        fs::remove_file(&resaved).expect("owned resave cleanup");
    }
    assert_eq!(fs::read(&source).expect("unchanged source"), original);
    assert_eq!(fs::read_dir(&root).expect("no stage leaks").count(), 2);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn unedited_jpeg_export_rejects_undecodable_samples_and_protects_target() {
    let root = root("jpeg-undecodable-export");
    let source = root.join("source.jpg");
    let target = root.join("target.jpg");
    let original = fixture();
    let mut no_samples = original.clone();
    let sos = no_samples
        .windows(2)
        .position(|bytes| bytes == [0xff, 0xda])
        .expect("SOS");
    let length = u16::from_be_bytes([no_samples[sos + 2], no_samples[sos + 3]]) as usize;
    no_samples.drain(sos + 2 + length..no_samples.len() - 2);
    let mut bad_table = original.clone();
    let dht = bad_table
        .windows(2)
        .position(|bytes| bytes == [0xff, 0xc4])
        .expect("DHT");
    bad_table[dht + 5] = 255;
    for damaged in [no_samples, bad_table] {
        fs::write(&source, &damaged).expect("damaged source");
        fs::write(&target, b"existing target").expect("target");
        read(&source, &AtomicBool::new(false)).expect("marker scanner alone accepts container");
        let error = export_media(&request(&source, &target)).expect_err("decode must fail");
        assert!(
            error.to_string().contains("JPEG decode validation failed"),
            "{error}"
        );
        assert_eq!(
            fs::read(&target).expect("protected target"),
            b"existing target"
        );
        assert_eq!(fs::read(&source).expect("protected source"), damaged);
        assert_eq!(fs::read_dir(&root).expect("no stage leaks").count(), 2);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
#[ignore = "requires Pillow-generated fixtures; run scripts/generate-jpeg-preview-fixtures.py"]
fn unedited_jpeg_export_preserves_progressive_and_color_variants() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/jpeg-preview");
    let root = root("jpeg-variants-export");
    let source = root.join("source.jpg");
    let target = root.join("target.jpg");
    for name in [
        "RGB-False",
        "RGB-True",
        "L-False",
        "L-True",
        "CMYK-False",
        "CMYK-True",
        "CMYK-black-False",
        "CMYK-black-True",
        "RGB-direct",
    ] {
        let original = fs::read(fixtures.join(format!("{name}.jpg"))).expect("generated fixture");
        fs::write(&source, &original).expect("owned source");
        export_media(&request(&source, &target)).expect("unedited save");
        assert_eq!(
            fs::read(&target).expect("byte-identical copy"),
            original,
            "{name}"
        );
        export_media_with_options(
            &request(&source, &target),
            options(MetadataField::Title, name),
        )
        .expect("metadata-only save");
        assert_eq!(
            without_xmp(&fs::read(&target).expect("output")),
            without_xmp(&original),
            "{name}"
        );
        assert_eq!(inspect(&target).expect("new title")[0].value, name);
        assert_eq!(fs::read(&source).expect("source preserved"), original);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn jpeg_metadata_splice_preserves_every_non_xmp_byte_and_decoded_pixel() {
    let root = root("jpeg-splice");
    let source = root.join("source.JPEG");
    let target = root.join("target.jpg");
    let original = tagged(PACKET.as_bytes());
    fs::write(&source, &original).expect("source");
    let staging = StagedExport::new(&target).expect("stage");
    let mut encoded = tagged(
        PACKET
            .replace("Original &amp; title", "Encoder title")
            .as_bytes(),
    );
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    encoded.splice(2..2, segment(0xe1, exif));
    fs::write(&staging.output, &encoded).expect("stage");
    let cancel = AtomicBool::new(false);
    let metadata = JpegMetadata::prepare(
        &request(&source, &target),
        &options(MetadataField::Title, "新しい題名").metadata,
        &cancel,
    )
    .expect("prepare");
    metadata.apply(&staging, &cancel).expect("rewrite");
    let actual = fs::read(&staging.output).expect("rewritten");
    let mut independent = image::codecs::jpeg::JpegDecoder::new(Cursor::new(&actual))
        .expect("independent JPEG decoder");
    assert_eq!(
        image::ImageDecoder::xmp_metadata(&mut independent).expect("independent XMP extraction"),
        Some(metadata.packet.clone())
    );
    assert_eq!(without_xmp(&actual), without_xmp(&encoded));
    assert_eq!(
        image::load_from_memory(&actual).expect("pixels"),
        image::load_from_memory(&encoded).expect("baseline pixels")
    );
    assert_eq!(
        read(&staging.output, &cancel).expect("values"),
        metadata.values
    );
    assert_eq!(
        metadata
            .values
            .iter()
            .filter(|value| value.field == MetadataField::Artist)
            .map(|value| value.text.as_str())
            .collect::<Vec<_>>(),
        vec!["First author", "Second <author>"]
    );
    assert_eq!(fs::read(&source).expect("unchanged"), original);
    drop(staging);
    assert_eq!(fs::read_dir(&root).expect("cleanup").count(), 1);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn jpeg_export_metadata_set_keep_remove_matches_default_rotated_jpeg_pixels() {
    let root = root("jpeg-export");
    let source = root.join("source.jpg");
    let target = root.join("target.jpeg");
    let original = tagged(PACKET.as_bytes());
    fs::write(&source, &original).expect("source");
    let mut request = request(&source, &target);
    request.operations = vec![
        EditOperation::RotateClockwise,
        EditOperation::FlipHorizontal,
    ];
    export_media(&request).expect("baseline save");
    let baseline = fs::read(&target).expect("baseline");
    for field in [
        MetadataField::Title,
        MetadataField::Artist,
        MetadataField::Album,
        MetadataField::Composer,
        MetadataField::Genre,
        MetadataField::Comment,
        MetadataField::Copyright,
    ] {
        for value in ["日本語\r\n<&> text", ""] {
            export_media_with_options(&request, options(field, value))
                .expect("JPEG export with metadata");
            let actual = fs::read(&target).expect("export");
            assert_eq!(without_xmp(&actual), without_xmp(&baseline));
            assert_eq!(
                image::open(&target).expect("pixels"),
                image::load_from_memory(&baseline).expect("baseline pixels")
            );
            let values = read(&target, &AtomicBool::new(false)).expect("metadata");
            let fields = values
                .iter()
                .filter(|item| item.field == field)
                .collect::<Vec<_>>();
            if value.is_empty() {
                assert!(fields.is_empty());
            } else {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].text, value);
            }
        }
    }
    assert_eq!(fs::read(&source).expect("source unchanged"), original);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn jpeg_marker_parser_preserves_stuffed_bytes_restarts_and_multiple_synthetic_scans() {
    let mut synthetic = vec![0xff, 0xd8];
    synthetic.extend(segment(0xe0, b"JFIF\0"));
    synthetic.extend(segment(0xda, b"scan1"));
    synthetic.extend([1, 2, 0xff, 0, 3, 0xff, 0xd0, 4, 0xff, 0xff, 0xd7, 5]);
    synthetic.extend(segment(0xda, b"scan2"));
    synthetic.extend([6, 7, 0xff, 0, 8, 0xff, 0xd9]);
    let mut output = Vec::new();
    scan(
        Cursor::new(&synthetic),
        Some(&mut output),
        &[],
        &AtomicBool::new(false),
    )
    .expect("synthetic multi-scan traversal");
    assert_eq!(output, synthetic);
    for end in 0..synthetic.len() {
        assert!(
            scan(
                Cursor::new(&synthetic[..end]),
                None,
                &[],
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    for prefix in [XMP, EXTENDED] {
        let mut bad = tagged(PACKET.as_bytes());
        bad.splice(2..2, segment(0xe1, &[prefix, PACKET.as_bytes()].concat()));
        assert!(scan(Cursor::new(bad), None, &[], &AtomicBool::new(false)).is_err());
    }
    let mut trailing = fixture();
    trailing.push(0);
    assert!(scan(Cursor::new(trailing), None, &[], &AtomicBool::new(false)).is_err());
}

#[test]
fn jpeg_metadata_failures_cancellation_source_change_and_unsupported_fields_protect_target() {
    let root = root("jpeg-protection");
    let source = root.join("source.jpg");
    let target = root.join("target.jpeg");
    let original = tagged(PACKET.as_bytes());
    fs::write(&source, &original).expect("source");
    fs::write(&target, b"existing target").expect("target");
    let request = request(&source, &target);
    let settings = options(MetadataField::Title, "new");
    for mode in 0..3 {
        let cancel = AtomicBool::new(mode == 0);
        let result = export_options_cancellable(
            &request,
            settings.clone(),
            &cancel,
            &|_| {
                if mode == 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
                if mode == 2 {
                    fs::OpenOptions::new()
                        .append(true)
                        .open(&source)
                        .expect("owned source")
                        .write_all(b"changed")
                        .expect("change");
                }
            },
            &|_| panic!("no audio"),
        );
        let error = result.expect_err("must not publish");
        if mode == 2 {
            assert!(error.to_string().contains("source changed"), "{error}");
        } else {
            assert!(matches!(error, ExportError::Cancelled));
        }
        assert_eq!(fs::read(&target).expect("protected"), b"existing target");
        assert_eq!(fs::read_dir(&root).expect("stage cleanup").count(), 2);
        fs::write(&source, &original).expect("restore fixture");
    }
    assert!(
        export_media_with_options(&request, options(MetadataField::AlbumArtist, "unsupported"))
            .is_err()
    );
    let invalid = tagged(
        PACKET
            .replace("Original &amp; title", "&missing;")
            .as_bytes(),
    );
    fs::write(&source, &invalid).expect("invalid source");
    assert!(export_media_with_options(&request, settings.clone()).is_err());
    assert_eq!(fs::read(&source).expect("source preserved"), invalid);
    assert_eq!(
        fs::read(&target).expect("target preserved"),
        b"existing target"
    );
    let mut same = request.clone();
    same.target = source;
    assert!(matches!(
        export_media_with_options(&same, settings),
        Err(ExportError::SameAsSource)
    ));
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn jpeg_metadata_copy_cancel_write_failure_and_invalid_stage_leave_owned_files_intact() {
    struct CancelWriter<'a>(&'a AtomicBool);
    impl Write for CancelWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            assert!(bytes.len() <= 65536);
            if bytes.len() == 65536 {
                self.0.store(true, Ordering::Relaxed);
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let original = tagged(PACKET.as_bytes());
    let mut large = original.clone();
    let end = large.len() - 2;
    large.splice(end..end, vec![1; 200000]);
    let cancel = AtomicBool::new(false);
    let mut writer = CancelWriter(&cancel);
    assert!(matches!(
        scan(Cursor::new(large), Some(&mut writer), &[], &cancel),
        Err(ExportError::Cancelled)
    ));
    cancel.store(false, Ordering::Relaxed);
    let mut writer = Cursor::new([0_u8; 40]);
    assert!(matches!(
        scan(Cursor::new(&original), Some(&mut writer), &[], &cancel),
        Err(ExportError::Output(_))
    ));
    let root = root("jpeg-stage-protection");
    let source = root.join("source.jpg");
    let target = root.join("target.jpg");
    fs::write(&source, &original).expect("source");
    fs::write(&target, b"existing target").expect("target");
    let options = options(MetadataField::Title, "new");
    let metadata = JpegMetadata::prepare(&request(&source, &target), &options.metadata, &cancel)
        .expect("prepare");
    for collision in [false, true] {
        let staging = StagedExport::new(&target).expect("stage");
        let encoded = if collision {
            original.clone()
        } else {
            original[..original.len() - 1].to_vec()
        };
        fs::write(&staging.output, &encoded).expect("stage bytes");
        if collision {
            fs::write(staging.directory.join("metadata.jpg"), b"sentinel").expect("collision");
        }
        assert!(metadata.apply(&staging, &cancel).is_err());
        assert_eq!(fs::read(&staging.output).expect("stage preserved"), encoded);
        assert_eq!(
            fs::read(&target).expect("target preserved"),
            b"existing target"
        );
        drop(staging);
        assert_eq!(fs::read_dir(&root).expect("cleanup").count(), 2);
    }
    for extension in ["png", "avif", "tiff", "bmp", "gif"] {
        let unsupported = root.join(format!("unsupported.{extension}"));
        fs::write(&unsupported, b"unchanged").expect("sentinel");
        assert!(
            export_media_with_options(&request(&source, &unsupported), options.clone()).is_err()
        );
        assert_eq!(
            fs::read(unsupported).expect("target protected"),
            b"unchanged"
        );
    }
    let mut remove = ExportOptions::default();
    for field in [
        MetadataField::Title,
        MetadataField::Artist,
        MetadataField::Album,
        MetadataField::Composer,
        MetadataField::Genre,
        MetadataField::Comment,
        MetadataField::Copyright,
    ] {
        remove
            .metadata
            .set(field, Some(String::new()))
            .expect("remove all");
    }
    export_media_with_options(&request(&source, &target), remove).expect("remove packet");
    assert!(read(&target, &cancel).expect("no text").is_empty());
    assert_eq!(fs::read(&source).expect("source protected"), original);
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
