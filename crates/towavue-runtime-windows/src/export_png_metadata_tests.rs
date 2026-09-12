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

fn animation_fixture(plays: u32, delays: &[[u8; 4]]) -> Vec<u8> {
    animation_fixture_with_regions(plays, delays, false)
}

fn animation_fixture_with_regions(plays: u32, delays: &[[u8; 4]], regions: bool) -> Vec<u8> {
    let mut result = SIGNATURE.to_vec();
    let mut sequence = 0u32;
    for (index, delay) in delays.iter().enumerate() {
        let (width, height, x_offset, y_offset, dispose, blend) = if regions && index > 0 {
            (16, 12, index as u32 * 2, index as u32, index as u8 % 3, 1)
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
fn apng_to_webp_preserves_animation_and_rejects_unrepresentable_controls() {
    let root = root("apng-to-webp");
    let source = root.join("source.APNG");
    let target = root.join("converted.webp");
    let delays = [
        [0, 0, 0, 0],
        [0, 1, 3, 232],
        [255, 255, 255, 255],
        [255, 255, 0, 4],
        [0, 7, 0, 0],
    ];
    for (plays, count, regions) in [(0, 5, false), (1, 1, false), (3, 5, true), (65535, 5, true)] {
        let bytes = animation_fixture_with_regions(plays, &delays[..count], regions);
        fs::write(&source, &bytes).expect("source");
        let original = crate::decode_image(&source).expect("display");
        let mut request = request(&source, &target);
        request.operations = vec![
            EditOperation::Crop(PixelCrop {
                x: 2,
                y: 2,
                width: 28,
                height: 20,
            }),
            EditOperation::RotateClockwise,
            EditOperation::FlipHorizontal,
            EditOperation::Resize(
                ImageResize::new(30, 42, ResampleFilter::Nearest).expect("resize"),
            ),
        ];
        export_media(&request).expect("APNG to animated WebP");
        let expected = crate::render_image_edits(
            &original,
            &request.operations,
            &crate::Cancellation::default(),
        )
        .expect("displayed edits");
        assert_eq!(
            crate::decode_image(&target).expect("converted").frames,
            expected.frames
        );
        let resaved = root.join("resaved.webp");
        export_media(&self::request(&target, &resaved)).expect("WebP resave");
        assert_eq!(
            crate::decode_image(&resaved).expect("resaved").frames,
            expected.frames
        );
        for output in [&target, &resaved] {
            let mut decoder =
                image_webp::WebPDecoder::new(BufReader::new(fs::File::open(output).expect("WebP")))
                    .expect("independent controls");
            assert_eq!(decoder.num_frames() as usize, count);
            assert_eq!(
                decoder.loop_count(),
                match plays {
                    0 => image_webp::LoopCount::Forever,
                    plays => image_webp::LoopCount::Times(
                        std::num::NonZeroU16::new(plays as u16).expect("plays")
                    ),
                }
            );
            let mut pixels = vec![0; decoder.output_buffer_size().expect("canvas")];
            for delay in [0, 1, 1000, 16383750, 70].into_iter().take(count) {
                assert_eq!(decoder.read_frame(&mut pixels).expect("frame"), delay);
            }
        }
        assert_eq!(fs::read(&source).expect("source untouched"), bytes);
        assert!(
            read_export_metadata(&target, MediaKind::Image)
                .expect("no metadata transfer")
                .is_empty()
        );
    }
    fs::write(
        &source,
        animation_fixture_with_regions(3, &[[0, 1, 0, 10]; 3], true),
    )
    .expect("partial frames");
    for filter in [
        ResampleFilter::Nearest,
        ResampleFilter::Bilinear,
        ResampleFilter::Bicubic,
        ResampleFilter::Lanczos,
    ] {
        let mut request = request(&source, &target);
        request.operations = vec![
            EditOperation::RotateImage(
                towavue_core::ImageRotation::new(137, (32, 24)).expect("rotation"),
            ),
            EditOperation::Resize(ImageResize::new(17, 13, filter).expect("resize")),
        ];
        export_media(&request).expect("rotated/resampled WebP");
        let webp = crate::decode_image(&target).expect("WebP pixels");
        request.target = root.join("reference.apng");
        export_media(&request).expect("same edited snapshots in APNG");
        assert_eq!(
            webp.frames,
            crate::decode_image(&request.target)
                .expect("APNG pixels")
                .frames
        );
    }
    let before = fs::read(&target).expect("existing WebP");
    for (bytes, message) in [
        (poster_fixture(3), "separate poster"),
        (animation_fixture(65536, &[[0, 1, 0, 100]]), "65535"),
        (animation_fixture(1, &[[0, 1, 0, 3]]), "whole milliseconds"),
        (animation_fixture(1, &[[255, 255, 0, 1]]), "24-bit"),
    ] {
        fs::write(&source, bytes).expect("unrepresentable source");
        let error =
            export_cancellable(&request(&source, &target), &AtomicBool::new(false), &|_| {
                panic!("unrepresentable controls must fail before encoding");
            })
            .expect_err("conversion limit");
        assert!(error.to_string().contains(message), "{error}");
        assert_eq!(fs::read(&target).expect("target preserved"), before);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
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
                actual.frames == expected.frames,
                "all frames must match the displayed edits: plays={plays}, differences={:?}",
                actual
                    .frames
                    .iter()
                    .zip(&expected.frames)
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
fn apng_previous_restores_the_canvas_after_background_disposal() {
    let root = root("apng-previous-golden");
    let source = root.join("source.png");
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let green = [0, 255, 0, 255];
    let clear = [0, 0, 0, 0];
    let frames = [
        (0u32, 2u32, 0u8, red),
        (0, 1, 1, blue),
        (0, 1, 2, green),
        (1, 1, 0, clear),
    ];
    let mut bytes = SIGNATURE.to_vec();
    bytes.extend(chunk(
        b"IHDR",
        &[
            &2u32.to_be_bytes()[..],
            &1u32.to_be_bytes(),
            &[8, 6, 0, 0, 0],
        ]
        .concat(),
    ));
    bytes.extend(chunk(
        b"acTL",
        &[&4u32.to_be_bytes()[..], &2u32.to_be_bytes()].concat(),
    ));
    let mut sequence = 0u32;
    for (index, (x, width, dispose, pixel)) in frames.into_iter().enumerate() {
        let mut control = Vec::new();
        for number in [sequence, width, 1, x, 0] {
            control.extend_from_slice(&number.to_be_bytes());
        }
        sequence += 1;
        control.extend_from_slice(&[0, 1, 0, 10, dispose, 0]);
        bytes.extend(chunk(b"fcTL", &control));
        let data = compressed(&[&[0][..], &pixel.repeat(width as usize)].concat());
        if index == 0 {
            bytes.extend(chunk(b"IDAT", &data));
        } else {
            bytes.extend(chunk(
                b"fdAT",
                &[&sequence.to_be_bytes()[..], &data].concat(),
            ));
            sequence += 1;
        }
    }
    bytes.extend(chunk(b"IEND", &[]));
    fs::write(&source, &bytes).expect("golden source");
    let decoded = crate::decode_image(&source).expect("animation");
    let expected = [[red, red], [blue, red], [green, red], [clear, clear]];
    assert_eq!(decoded.frames.len(), expected.len());
    for (index, (actual, expected)) in decoded.frames.iter().zip(expected).enumerate() {
        assert_eq!(
            actual.rgba,
            expected.concat(),
            "explicit canvas for frame {index}"
        );
    }
    let target = root.join("saved.png");
    export_media(&request(&source, &target)).expect("PREVIOUS save is supported");
    assert_eq!(
        crate::decode_image(&target).expect("saved").frames,
        decoded.frames
    );
    // A 2x1 Adam7 image has one pixel in pass 1 and one in pass 6.
    let interlaced = map_chunks(&bytes, |kind, data| {
        if &kind == b"IHDR" {
            data[12] = 1;
        }
        if &kind == b"IDAT" {
            *data = compressed(&[&[0][..], &red, &[0], &red].concat());
        }
        true
    });
    fs::write(&source, &interlaced).expect("interlaced source");
    assert_eq!(
        crate::decode_image(&source).expect("Adam7 PREVIOUS").frames,
        decoded.frames
    );
    export_media(&request(&source, &target)).expect("interlaced save");
    assert_eq!(
        crate::decode_image(&target).expect("saved Adam7").frames,
        decoded.frames
    );
    for first_previous in [false, true] {
        let mut frame = 0;
        let variant = map_chunks(&bytes, |kind, data| {
            if &kind == b"fcTL" {
                if first_previous {
                    if frame == 0 {
                        data[24] = 2;
                    }
                } else if frame != 0 {
                    data[24] = 2;
                }
                frame += 1;
            }
            true
        });
        fs::write(&source, &variant).expect("PREVIOUS variant");
        let decoded = crate::decode_image(&source).expect("variant");
        let expected = if first_previous {
            [[red, red], [blue, clear], [green, clear], [clear, clear]]
        } else {
            [[red, red], [blue, red], [green, red], [red, clear]]
        };
        for (index, (actual, expected)) in decoded.frames.iter().zip(expected).enumerate() {
            assert_eq!(
                actual.rgba,
                expected.concat(),
                "first_previous={first_previous}, frame={index}"
            );
        }
        export_media(&request(&source, &target)).expect("variant save");
        assert_eq!(
            crate::decode_image(&target).expect("saved variant").frames,
            decoded.frames
        );
        assert_eq!(fs::read(&source).expect("source unchanged"), variant);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn apng_decoder_preserves_expanded_color_types_and_depth_conversion() {
    use png::{BitDepth, ColorType};
    let root = root("apng-color-types");
    let source = root.join("source.png");
    let cases = [
        (
            ColorType::Grayscale,
            BitDepth::One,
            vec![0x40],
            vec![0, 0, 0, 255, 255, 255, 255, 255],
        ),
        (
            ColorType::Grayscale,
            BitDepth::Eight,
            vec![17, 235],
            vec![17, 17, 17, 255, 235, 235, 235, 255],
        ),
        (
            ColorType::GrayscaleAlpha,
            BitDepth::Eight,
            vec![17, 128, 235, 64],
            vec![17, 17, 17, 128, 235, 235, 235, 64],
        ),
        (
            ColorType::Rgb,
            BitDepth::Eight,
            vec![10, 20, 30, 40, 50, 60],
            vec![10, 20, 30, 255, 40, 50, 60, 255],
        ),
        (
            ColorType::Rgba,
            BitDepth::Eight,
            vec![10, 20, 30, 128, 40, 50, 60, 64],
            vec![10, 20, 30, 128, 40, 50, 60, 64],
        ),
        (
            ColorType::Indexed,
            BitDepth::One,
            vec![0x40],
            vec![10, 20, 30, 255, 40, 50, 60, 128],
        ),
        (
            ColorType::Rgba,
            BitDepth::Sixteen,
            vec![10, 1, 20, 2, 30, 3, 128, 4, 40, 5, 50, 6, 60, 7, 64, 8],
            vec![10, 20, 30, 128, 40, 50, 60, 64],
        ),
    ];
    for (color, depth, pixels, expected) in cases {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(color);
            encoder.set_depth(depth);
            encoder.set_animated(2, 0).expect("animation");
            if color == ColorType::Indexed {
                encoder.set_palette(vec![10, 20, 30, 40, 50, 60]);
                encoder.set_trns(vec![255, 128]);
            }
            let mut writer = encoder.write_header().expect("header");
            writer.set_frame_delay(1, 7).expect("delay");
            writer.write_image_data(&pixels).expect("first frame");
            writer
                .set_dispose_op(png::DisposeOp::Previous)
                .expect("previous");
            writer.write_image_data(&pixels).expect("second frame");
            writer.finish().expect("finish");
        }
        fs::write(&source, &bytes).expect("source");
        let decoded = crate::decode_image(&source).expect("color frames");
        assert_eq!(decoded.frames.len(), 2);
        for frame in &decoded.frames {
            assert_eq!(frame.rgba, expected, "{color:?} {depth:?}");
        }
        let first = crate::image::first_animation_frame(&source, 8, &|| true)
            .expect("preview")
            .expect("animated");
        assert_eq!(first, decoded.frames[0]);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn apng_decoder_keeps_preview_budget_cancellation_and_poster_boundaries() {
    let root = root("apng-decode-boundaries");
    let source = root.join("source.png");
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 1).expect("animated");
        encoder.set_sep_def_img(true).expect("separate poster");
        let mut writer = encoder.write_header().expect("header");
        writer.write_image_data(&[255; 8]).expect("poster");
        writer.set_frame_delay(0, 0).expect("minimum delay");
        writer
            .write_image_data(&[0; 8])
            .expect("first animation frame");
        writer.write_image_data(&[128; 8]).expect("second frame");
        writer.finish().expect("finish");
    }
    fs::write(&source, &bytes).expect("source");
    let decoded = crate::image::decode_image_cancellable(&source, 16, &|| true)
        .expect("exact retained budget");
    assert_eq!(decoded.retained_bytes(), 16);
    assert_eq!(decoded.frames[0].rgba, [0; 8]);
    assert_eq!(decoded.frames[0].delay, Duration::from_millis(10));
    assert_eq!(
        crate::image::first_animation_frame(&source, 8, &|| true)
            .expect("preview")
            .expect("animation"),
        decoded.frames[0]
    );
    for budget in [0, 7, 15] {
        assert!(matches!(
            crate::image::decode_image_cancellable(&source, budget, &|| true),
            Err(crate::ImageDecodeError::TooLarge)
        ));
    }
    let current = AtomicBool::new(true);
    let mut previews = 0;
    let result = crate::image::decode_image_with_preview(
        &source,
        16,
        &|| current.load(Ordering::Relaxed),
        &mut |width, height, pixels| {
            previews += 1;
            assert_eq!((width, height), (2, 1));
            assert_eq!(pixels, [0; 8]);
            current.store(false, Ordering::Relaxed);
        },
    );
    assert!(matches!(result, Err(crate::ImageDecodeError::Cancelled)));
    assert_eq!(previews, 1);
    fs::write(&source, &bytes[..bytes.len() - 1]).expect("truncated IEND");
    assert!(crate::decode_image(&source).is_err());
    assert!(
        crate::image::first_animation_frame(&source, 8, &|| true)
            .expect("first-only does not read the tail")
            .is_some()
    );
    fs::remove_dir_all(root).expect("owned fixture cleanup");
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
fn apng_single_frame_save_preserves_controls_and_edited_pixels() {
    let root = root("apng-single-frame");
    let source = root.join("source.png");
    let target = root.join("saved.apng");
    let cancel = AtomicBool::new(false);
    for plays in [0, 1, 3] {
        let bytes = animation_fixture(plays, &[[0, 7, 0, 13]]);
        fs::write(&source, &bytes).expect("source");
        let decoded = crate::decode_image(&source).expect("single animation frame");
        let mut request = request(&source, &target);
        request.operations.push(EditOperation::RotateClockwise);
        let expected = crate::render_image_edits(
            &decoded,
            &request.operations,
            &crate::Cancellation::default(),
        )
        .expect("edited frame");
        export_media(&request).expect("single-frame APNG save");
        assert_eq!(
            crate::decode_image(&target).expect("reopen").frames,
            expected.frames
        );
        let control = scan_contents(Cursor::new(&bytes), None, None, &cancel)
            .expect("source controls")
            .1;
        assert_eq!(
            scan_contents(
                Cursor::new(fs::read(&target).expect("saved")),
                None,
                None,
                &cancel
            )
            .expect("saved controls")
            .1,
            control
        );
        let resave = root.join("resaved.png");
        export_media(&self::request(&target, &resave)).expect("single-frame resave");
        assert_eq!(
            scan_contents(
                Cursor::new(fs::read(&resave).expect("resaved")),
                None,
                None,
                &cancel
            )
            .expect("resaved controls")
            .1,
            control
        );
        assert_eq!(fs::read(&source).expect("source unchanged"), bytes);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

fn poster_fixture(frames: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 4, 3);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .set_animated(frames, if frames == 1 { 0 } else { 3 })
            .expect("animation");
        encoder.set_sep_def_img(true).expect("poster");
        let mut writer = encoder.write_header().expect("header");
        let poster = image::RgbaImage::from_fn(4, 3, |x, y| {
            image::Rgba([x as u8 * 40, y as u8 * 70, 201, 128])
        });
        writer
            .write_image_data(poster.as_raw())
            .expect("poster data");
        writer.set_frame_dimension(2, 2).expect("partial animation");
        writer.set_frame_position(1, 1).expect("animation offset");
        for index in 0..frames {
            writer.set_frame_delay(index as u16 + 1, 7).expect("delay");
            writer.set_blend_op(png::BlendOp::Over).expect("alpha over");
            writer
                .set_dispose_op(if index == 0 {
                    png::DisposeOp::Previous
                } else {
                    png::DisposeOp::None
                })
                .expect("disposal");
            writer
                .write_image_data(&[20, index as u8 * 70, 0, 128].repeat(4))
                .expect("animation data");
        }
        writer.finish().expect("finish");
    }
    let end = bytes.len() - 12;
    bytes.splice(end..end, chunk(b"tEXt", b"Title\0Poster title"));
    bytes
}

#[test]
fn apng_poster_save_preserves_separate_edited_poster_and_animation() {
    let root = root("apng-poster-save");
    let source = root.join("source.apng");
    let target = root.join("saved.png");
    for count in [1, 2, 3] {
        let bytes = poster_fixture(count);
        fs::write(&source, &bytes).expect("source");
        let original = crate::decode_image(&source).expect("original animation");
        let poster = image::open(&source).expect("default image").to_rgba8();
        assert_ne!(poster.as_raw(), &original.frames[0].rgba);
        let poster = crate::DecodedImage {
            format: "PNG",
            frames: vec![crate::DecodedImageFrame {
                width: 4,
                height: 3,
                rgba: poster.into_raw(),
                delay: Duration::ZERO,
            }],
        };
        let mut request = request(&source, &target);
        request.operations = vec![
            EditOperation::Crop(PixelCrop {
                x: 1,
                y: 0,
                width: 3,
                height: 2,
            }),
            EditOperation::RotateClockwise,
            EditOperation::Resize(ImageResize::new(6, 9, ResampleFilter::Nearest).expect("resize")),
        ];
        if count == 3 {
            request.operations.push(EditOperation::RotateImage(
                towavue_core::ImageRotation::new(130, (6, 9)).expect("free rotation"),
            ));
        }
        let expected = crate::render_image_edits(
            &original,
            &request.operations,
            &crate::Cancellation::default(),
        )
        .expect("edited animation");
        let expected_poster = crate::render_image_edits(
            &poster,
            &request.operations,
            &crate::Cancellation::default(),
        )
        .expect("edited poster");
        export_media(&request).expect("poster save");
        assert_eq!(
            crate::decode_image(&target)
                .expect("saved animation")
                .frames,
            expected.frames
        );
        assert_eq!(
            image::open(&target)
                .expect("saved poster")
                .to_rgba8()
                .as_raw(),
            &expected_poster.frames[0].rgba
        );
        let cancel = AtomicBool::new(false);
        let controls = scan_contents(Cursor::new(&bytes), None, None, &cancel)
            .expect("original controls")
            .1;
        assert!(!controls.as_ref().expect("animation").includes_default);
        assert_eq!(
            scan_contents(
                Cursor::new(fs::read(&target).expect("saved bytes")),
                None,
                None,
                &cancel
            )
            .expect("saved controls")
            .1,
            controls
        );
        for value in [None, Some("Edited title"), Some("")] {
            export_media_with_options(
                &request,
                ExportOptions {
                    metadata: value.map(title).unwrap_or_default(),
                    ..Default::default()
                },
            )
            .expect("poster metadata save");
            let texts = read_export_metadata(&target, MediaKind::Image).expect("metadata");
            assert_eq!(
                texts.first().map(|text| text.value.as_str()),
                match value {
                    None => Some("Poster title"),
                    Some("") => None,
                    Some(value) => Some(value),
                }
            );
            let resave = root.join("resaved.apng");
            export_media(&self::request(&target, &resave)).expect("poster resave");
            assert_eq!(
                crate::decode_image(&resave)
                    .expect("resaved animation")
                    .frames,
                expected.frames
            );
            assert_eq!(
                image::open(&resave)
                    .expect("resaved poster")
                    .to_rgba8()
                    .as_raw(),
                &expected_poster.frames[0].rgba
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
                controls
            );
            assert_eq!(
                read_export_metadata(&resave, MediaKind::Image).expect("resaved text"),
                texts
            );
        }
        assert_eq!(fs::read(&source).expect("original unchanged"), bytes);
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn apng_unsupported_exports_preserve_targets_and_static_conversions_still_work() {
    let root = root("apng-unsupported");
    let source = root.join("source.APNG");
    let bytes = animation_fixture(2, &[[0, 1, 0, 10]; 3]);
    fs::write(&source, &bytes).expect("valid source");
    for extension in ["jpg", "gif", "bmp", "avif"] {
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
    export_media(&request(&source, &root.join("static.webp"))).expect("static WebP conversion");
    assert_eq!(
        crate::decode_image(&root.join("static.webp"))
            .expect("static WebP")
            .frames
            .len(),
        1
    );
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
fn apng_to_webp_protects_targets_on_cancel_corruption_and_staging_failures() {
    let root = root("apng-webp-protection");
    let source = root.join("source.png");
    let target = root.join("target.webp");
    let bytes = animation_fixture_with_regions(3, &[[0, 1, 0, 10]; 3], true);
    fs::write(&target, b"existing target").expect("target");
    for before in [true, false] {
        fs::write(&source, &bytes).expect("source");
        let cancel = AtomicBool::new(before);
        assert!(matches!(
            export_cancellable(&request(&source, &target), &cancel, &|_| cancel
                .store(true, Ordering::Relaxed)),
            Err(ExportError::Cancelled)
        ));
        assert_eq!(fs::read(&target).expect("preserved"), b"existing target");
    }
    let changed = AtomicBool::new(false);
    assert!(
        export_cancellable(&request(&source, &target), &AtomicBool::new(false), &|_| {
            if !changed.swap(true, Ordering::Relaxed) {
                fs::OpenOptions::new()
                    .append(true)
                    .open(&source)
                    .expect("source")
                    .write_all(&[0])
                    .expect("mutate source");
            }
        })
        .expect_err("source change")
        .to_string()
        .contains("source changed")
    );
    assert!(changed.load(Ordering::Relaxed));
    assert_eq!(fs::read(&target).expect("preserved"), b"existing target");
    fs::write(&source, &bytes).expect("restore source");
    let cancel = AtomicBool::new(false);
    let metadata = PngMetadata::prepare_webp(&source, &cancel)
        .expect("prepare")
        .expect("animation");
    for name in ["animation-source.png", "animation.webp"] {
        let staging = StagedExport::new(&target).expect("stage");
        let occupied = staging.directory.join(name);
        fs::write(&occupied, b"occupied").expect("occupied file");
        if name == "animation-source.png" {
            assert!(
                metadata
                    .prepare_animation_source(&request(&source, &target), &staging, &cancel)
                    .is_err()
            );
        } else {
            fs::write(&staging.output, fixture()).expect("encoded PNG");
            assert!(metadata.apply_webp(&staging, &cancel, &|_| {}).is_err());
        }
        assert_eq!(fs::read(&occupied).expect("occupied retained"), b"occupied");
    }
    assert!(
        export_media_with_options(
            &request(&source, &target),
            ExportOptions {
                metadata: title("unsupported transfer"),
                ..Default::default()
            }
        )
        .is_err()
    );
    let damaged = map_chunks(&bytes, |kind, data| {
        if &kind == b"IDAT" {
            *data = vec![0; 4];
        }
        true
    });
    for invalid in [damaged, bytes[..bytes.len() - 1].to_vec()] {
        fs::write(&source, invalid).expect("corrupt source");
        assert!(export_media(&request(&source, &target)).is_err());
        assert_eq!(fs::read(&target).expect("preserved"), b"existing target");
    }
    assert_eq!(
        fs::read_dir(&root).expect("no owned stages remain").count(),
        2
    );
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn apng_export_cancellation_and_source_changes_never_replace_target() {
    let root = root("apng-source-protection");
    let source = root.join("source.png");
    let target = root.join("target.png");
    for bytes in [animation_fixture(2, &[[0, 1, 0, 10]; 3]), poster_fixture(3)] {
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
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn apng_preparation_streams_frames_and_preserves_targets_on_failure() {
    let root = root("apng-preparation");
    let source = root.join("source.png");
    let target = root.join("target.png");
    for (has_poster, bytes) in [
        (
            false,
            animation_fixture_with_regions(2, &[[0, 1, 0, 10]; 3], true),
        ),
        (true, poster_fixture(3)),
    ] {
        fs::write(&source, &bytes).expect("source");
        fs::write(&target, b"existing target").expect("target");
        let cancel = AtomicBool::new(false);
        let request = request(&source, &target);
        let metadata = PngMetadata::prepare(&request, &title("new"), &cancel).expect("prepare");
        let mut stream = Vec::new();
        crate::image::apng::write_frames(&source, &mut stream, &|| true).expect("stream frames");
        let mut remaining = stream.as_slice();
        let mut frames = Vec::new();
        while !remaining.is_empty() {
            assert!(remaining.starts_with(SIGNATURE));
            let mut end = 8;
            loop {
                let size =
                    u32::from_be_bytes(remaining[end..end + 4].try_into().expect("chunk size"))
                        as usize;
                let last = &remaining[end + 4..end + 8] == b"IEND";
                end += size + 12;
                if last {
                    break;
                }
            }
            frames.push(
                image::load_from_memory(&remaining[..end])
                    .expect("independent PNG")
                    .to_rgba8(),
            );
            remaining = &remaining[end..];
        }
        assert_eq!(frames.len(), 3 + usize::from(has_poster));
        if has_poster {
            assert_eq!(frames[0], image::open(&source).expect("poster").to_rgba8());
        }
        for (png, frame) in frames[usize::from(has_poster)..]
            .iter()
            .zip(crate::decode_image(&source).expect("animation").frames)
        {
            assert_eq!(png.as_raw(), &frame.rgba);
        }
        let mut assembled = Vec::new();
        metadata
            .animation
            .as_ref()
            .expect("animation controls")
            .assemble(Cursor::new(&stream), &mut assembled, &cancel)
            .expect("assemble frames with optional poster");
        assert_eq!(
            scan_contents(Cursor::new(assembled), None, None, &cancel)
                .expect("controls")
                .1,
            metadata.animation
        );

        struct CancelOnSecondPng<'a> {
            cancel: &'a AtomicBool,
            signatures: usize,
        }
        impl Write for CancelOnSecondPng<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if bytes == SIGNATURE {
                    self.signatures += 1;
                    if self.signatures == 2 {
                        self.cancel.store(true, Ordering::Relaxed);
                    }
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut writer = CancelOnSecondPng {
            cancel: &cancel,
            signatures: 0,
        };
        assert!(matches!(
            crate::image::apng::write_frames(&source, &mut writer, &|| !cancel
                .load(Ordering::Relaxed)),
            Err(crate::ImageDecodeError::Cancelled)
        ));
        assert_eq!(writer.signatures, 2);
        cancel.store(false, Ordering::Relaxed);
        assert!(
            crate::image::apng::write_frames(&source, &mut &mut [0u8; 4][..], &|| true).is_err()
        );
        for occupied in [false, true] {
            let staging = StagedExport::new(&target).expect("stage");
            let intermediate = staging.directory.join("animation-source.png");
            if occupied {
                fs::write(&intermediate, b"occupied").expect("occupied stage");
            } else {
                cancel.store(true, Ordering::Relaxed);
            }
            assert!(
                metadata
                    .prepare_animation_source(&request, &staging, &cancel)
                    .is_err()
            );
            if occupied {
                assert_eq!(
                    fs::read(&intermediate).expect("occupied intermediate"),
                    b"occupied"
                );
            }
            cancel.store(false, Ordering::Relaxed);
            drop(staging);
            assert_eq!(fs::read_dir(&root).expect("stage cleanup").count(), 2);
            assert_eq!(
                fs::read(&target).expect("target preserved"),
                b"existing target"
            );
        }
        let malformed = map_chunks(&bytes, |kind, data| {
            if &kind == b"IDAT" {
                *data = vec![0; 4];
            }
            true
        });
        fs::write(&source, &malformed).expect("malformed default image with valid CRC");
        PngMetadata::prepare(&request, &title("new"), &cancel).expect("container scan");
        assert!(
            export_media(&request)
                .expect_err("default image decoding fails")
                .to_string()
                .contains("animation frame preparation failed")
        );
        assert_eq!(fs::read(&source).expect("source preserved"), malformed);
        assert_eq!(
            fs::read(&target).expect("target preserved"),
            b"existing target"
        );
        assert_eq!(fs::read_dir(&root).expect("stage cleanup").count(), 2);
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
