use super::*;
use std::fs;

#[test]
#[ignore = "Release measurement; set TOWAVUE_PREVIEW_BENCH_IMAGE to a static PNG"]
fn png_static_decode_reports_full_decode_comparison() {
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_PREVIEW_BENCH_IMAGE").expect("PNG path"),
    );
    let expected = decode_image(&path).expect("reference decode");
    for rows in [false, true, true, false] {
        let mut samples = Vec::new();
        for _ in 0..5 {
            let started = std::time::Instant::now();
            let pixels = if rows {
                super::super::png_static::decode(
                    open(&path, &|| true).expect("open"),
                    IMAGE_BYTE_LIMIT,
                    &|| true,
                )
                .expect("PNG decode")
                .expect("static PNG")
                .rgba
            } else {
                let mut decoder =
                    PngDecoder::new(open(&path, &|| true).expect("open")).expect("PNG header");
                let orientation = decoder.orientation().expect("orientation");
                let mut image = DynamicImage::from_decoder(decoder).expect("pixels");
                image.apply_orientation(orientation);
                image.into_rgba8().into_raw()
            };
            let total = started.elapsed();
            assert_eq!(pixels, expected.frames[0].rgba);
            samples.push(total.as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        eprintln!(
            "PNG_DECODE rows={rows} size={:?} median_ms={:.3}",
            expected.dimensions(),
            samples[2]
        );
    }
}

#[test]
#[ignore = "Release measurement; set TOWAVUE_PREVIEW_BENCH_IMAGE to a large PNG"]
fn png_row_thumbnail_reports_full_decode_comparison() {
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_PREVIEW_BENCH_IMAGE").expect("PNG path"),
    );
    let reference = png_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
        .expect("valid PNG fixture")
        .expect("valid PNG fixture");
    let modes: &[bool] = match std::env::var("TOWAVUE_PNG_PREVIEW_MODE").as_deref() {
        Ok("stream") => &[true],
        Ok("full") => &[false],
        _ => &[false, true, true, false],
    };
    for &streaming in modes {
        let mut samples = Vec::new();
        for _ in 0..5 {
            let started = std::time::Instant::now();
            let preview = if streaming {
                png_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
                    .expect("valid PNG fixture")
                    .expect("valid PNG fixture")
            } else {
                let image = decode_image_for_prefetch(&path, IMAGE_BYTE_LIMIT, &|| true)
                    .expect("valid PNG fixture")
                    .expect("valid PNG fixture");
                let frame = image.frames.into_iter().next().expect("valid PNG fixture");
                let small = DynamicImage::ImageRgba8(
                    image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
                        .expect("valid PNG fixture"),
                )
                .resize(240, 160, image::imageops::FilterType::Nearest)
                .into_rgba8();
                CachedImagePreview {
                    source_size: (frame.width, frame.height),
                    image: PreviewImage {
                        width: small.width(),
                        height: small.height(),
                        rgba: small.into_raw(),
                    },
                }
            };
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(preview.source_size, reference.source_size);
            assert_eq!(preview.image, reference.image);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "png-row-thumbnail streaming={streaming} source={:?} median_ms={:.3}",
            reference.source_size, samples[2]
        );
    }
}

fn path(name: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("valid PNG fixture")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "towavue-png-thumbnail-{name}-{}-{nonce}.png",
        std::process::id()
    ))
}

fn compare(path: &Path) {
    let preview = png_thumbnail(path, IMAGE_BYTE_LIMIT, &|| true)
        .expect("streaming preview")
        .expect("static PNG");
    let reference = decode_image(path).expect("original decode");
    let mut decoder = PngDecoder::new(open(path, &|| true).expect("open")).expect("PNG");
    let orientation = decoder.orientation().expect("orientation");
    let mut legacy = DynamicImage::from_decoder(decoder).expect("independent full decode");
    legacy.apply_orientation(orientation);
    assert_eq!(reference.frames[0].rgba, legacy.into_rgba8().into_raw());
    assert_eq!(
        decode_image_for_prefetch(path, IMAGE_BYTE_LIMIT, &|| true)
            .expect("prefetch")
            .expect("static PNG"),
        reference
    );
    assert_eq!(preview.source_size, reference.dimensions());
    let frame = &reference.frames[0];
    let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
        .expect("valid PNG fixture");
    let expected = DynamicImage::ImageRgba8(image)
        .resize(240, 160, image::imageops::FilterType::Nearest)
        .into_rgba8();
    assert_eq!(
        (preview.image.width, preview.image.height),
        expected.dimensions()
    );
    assert_eq!(
        preview.image.rgba,
        expected.into_raw(),
        "preview pixels must match oriented RGBA resize"
    );
}

#[test]
fn png_row_thumbnail_matches_full_decode_for_colors_depths_and_all_orientations() {
    let path = path("colors");
    for (width, height) in [(503, 317), (7, 5), (1, 1)] {
        for color in [
            png::ColorType::Grayscale,
            png::ColorType::GrayscaleAlpha,
            png::ColorType::Rgb,
            png::ColorType::Rgba,
        ] {
            for depth in [png::BitDepth::Eight, png::BitDepth::Sixteen] {
                let mut pixels = Vec::new();
                for y in 0..height {
                    for x in 0..width {
                        for channel in 0..color.samples() {
                            let value = ((x * 719 + y * 391 + channel as u32 * 997) % 65536) as u16;
                            if depth == png::BitDepth::Sixteen {
                                pixels.extend_from_slice(&value.to_be_bytes());
                            } else {
                                pixels.push(value as u8);
                            }
                        }
                    }
                }
                for orientation in 1..=8 {
                    let mut info = png::Info::with_size(width, height);
                    info.color_type = color;
                    info.bit_depth = depth;
                    let mut exif =
                        *b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0";
                    exif[18] = orientation;
                    info.exif_metadata = Some(exif.to_vec().into());
                    let mut writer = png::Encoder::with_info(
                        File::create(&path).expect("valid PNG fixture"),
                        info,
                    )
                    .expect("valid PNG fixture")
                    .write_header()
                    .expect("valid PNG fixture");
                    writer.write_image_data(&pixels).expect("valid PNG fixture");
                    writer.finish().expect("valid PNG fixture");
                    compare(&path);
                }
            }
        }
    }
    fs::remove_file(path).expect("valid PNG fixture");
}

#[test]
fn png_row_thumbnail_expands_packed_gray_palette_and_transparency() {
    let path = path("packed");
    for color in [png::ColorType::Grayscale, png::ColorType::Indexed] {
        for (depth, bits) in [
            (png::BitDepth::One, 1),
            (png::BitDepth::Two, 2),
            (png::BitDepth::Four, 4),
            (png::BitDepth::Eight, 8),
        ] {
            let mut info = png::Info::with_size(503, 317);
            info.color_type = color;
            info.bit_depth = depth;
            if color == png::ColorType::Indexed {
                info.palette = Some(
                    (0..1_u16 << bits)
                        .flat_map(|i| [i as u8, (i * 13) as u8, (i * 37) as u8])
                        .collect::<Vec<_>>()
                        .into(),
                );
                info.trns = Some(
                    (0..1_u16 << bits)
                        .map(|i| (i * 47) as u8)
                        .collect::<Vec<_>>()
                        .into(),
                );
            } else {
                info.trns = Some(vec![0, 1].into());
            }
            let row_bytes = (503_usize * bits).div_ceil(8);
            let data: Vec<_> = (0..row_bytes * 317).map(|i| (i * 43 + 19) as u8).collect();
            let mut writer =
                png::Encoder::with_info(File::create(&path).expect("valid PNG fixture"), info)
                    .expect("valid PNG fixture")
                    .write_header()
                    .expect("valid PNG fixture");
            writer.write_image_data(&data).expect("valid PNG fixture");
            writer.finish().expect("valid PNG fixture");
            compare(&path);
        }
    }
    fs::remove_file(path).expect("valid PNG fixture");
}

#[test]
fn png_row_thumbnail_obeys_limits_cancellation_and_rejects_truncated_tail() {
    let path = path("limits");
    image::RgbImage::new(503, 317)
        .save(&path)
        .expect("valid PNG fixture");
    assert!(matches!(
        png_thumbnail(&path, 503 * 317 * 4 - 1, &|| true),
        Err(ImageDecodeError::TooLarge)
    ));
    assert!(matches!(
        png_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| false),
        Err(ImageDecodeError::Cancelled)
    ));
    let calls = std::cell::Cell::new(0);
    assert!(matches!(
        png_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| {
            calls.set(calls.get() + 1);
            calls.get() < 100
        }),
        Err(ImageDecodeError::Cancelled)
    ));
    let bytes = fs::read(&path).expect("valid PNG fixture");
    fs::write(&path, &bytes[..bytes.len() - 8]).expect("valid PNG fixture");
    assert!(png_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true).is_err());
    // A declared interlaced stream is left to the existing full decoder.
    let mut bytes = bytes;
    bytes[28] = 1;
    let crc = crc32fast::hash(&bytes[12..29]);
    bytes[29..33].copy_from_slice(&crc.to_be_bytes());
    fs::write(&path, bytes).expect("valid PNG fixture");
    assert!(
        png_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
            .expect("valid PNG fixture")
            .is_none()
    );
    let mut encoder = png::Encoder::new(File::create(&path).expect("valid PNG fixture"), 2, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_animated(1, 0).expect("valid PNG fixture");
    let mut writer = encoder.write_header().expect("valid PNG fixture");
    writer
        .write_image_data(&[255; 8])
        .expect("valid PNG fixture");
    writer.finish().expect("valid PNG fixture");
    assert!(
        png_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
            .expect("valid PNG fixture")
            .is_none(),
        "APNG must use the compositing path"
    );
    fs::write(&path, b"not PNG").expect("valid PNG fixture");
    assert!(
        png_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
            .expect("valid PNG fixture")
            .is_none()
    );
    fs::remove_file(path).expect("valid PNG fixture");
}
