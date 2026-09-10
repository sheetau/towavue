use super::*;
use crate::export::audio_tests::root;
use std::io::Cursor;
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
