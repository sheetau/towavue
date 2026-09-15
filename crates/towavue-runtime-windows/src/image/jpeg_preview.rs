use ffmpeg_next as ffmpeg;
use image::metadata::Orientation;

use super::*;
use crate::{CachedImagePreview, PreviewImage};

#[cfg(test)]
thread_local! {
    static RESERVE_PREVIEW_INPUT: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Optional first display only; the original decoder remains the editing authority.
pub(crate) fn jpeg_preview(
    path: &Path,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
) -> Result<Option<CachedImagePreview>, ImageDecodeError> {
    decode_preview(path, byte_limit, current, 4 * 1024 * 1024)
}

/// A requested thumbnail can benefit from reduced decoding below the size at
/// which speculative previews are worth delaying an original-image load.
pub(crate) fn jpeg_thumbnail(
    path: &Path,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
) -> Result<Option<CachedImagePreview>, ImageDecodeError> {
    decode_preview(path, byte_limit, current, 0)
}

fn decode_preview(
    path: &Path,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
    minimum_pixels: u64,
) -> Result<Option<CachedImagePreview>, ImageDecodeError> {
    check_current(current)?;
    let reader = image::ImageReader::new(open(path, current)?)
        .with_guessed_format()
        .map_err(ImageDecodeError::Open)?;
    if reader.format() != Some(ImageFormat::Jpeg) {
        return Ok(None);
    }
    // Bound speculative compressed copies independently of the original decoder.
    const INPUT_LIMIT: u64 = 32 * 1024 * 1024;
    let reader = reader.into_inner();
    #[cfg(test)]
    let reserve = RESERVE_PREVIEW_INPUT.get();
    #[cfg(not(test))]
    let reserve = true;
    // Use the opened file only as an allocation hint. Keep the bounded read and
    // length check authoritative if the source grows/shrinks or metadata fails.
    let capacity = if reserve {
        reader
            .get_ref()
            .inner
            .metadata()
            .map_or(0, |metadata| metadata.len().min(INPUT_LIMIT + 1) as usize)
    } else {
        0
    };
    let mut bytes = Vec::with_capacity(capacity);
    reader
        .take(INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(ImageDecodeError::Open)?;
    if bytes.len() as u64 > INPUT_LIMIT {
        return Ok(None);
    }
    let (width, height, orientation) = preview_header(&bytes)?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels < minimum_pixels || pixels * 4 > byte_limit as u64 {
        return Ok(None);
    }
    check_current(current)?;
    let source_size = match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
        _ => (width, height),
    };
    let native = (minimum_pixels == 0)
        .then(|| {
            super::jpeg_wic_preview::thumbnail(
                &bytes,
                width,
                height,
                source_size != (width, height),
                current,
            )
        })
        .flatten();
    check_current(current)?;
    let Some(reduced) = (if native.is_some() {
        native
    } else {
        reduced_jpeg(&bytes, width, height, source_size != (width, height))
            .map_err(DecodeError::from)?
    }) else {
        return Ok(None);
    };
    check_current(current)?;
    let mut image = DynamicImage::ImageRgba8(
        image::RgbaImage::from_raw(
            reduced.width,
            reduced.height,
            std::sync::Arc::unwrap_or_clone(reduced.rgba),
        )
        .expect("packed reduced JPEG"),
    );
    image.apply_orientation(orientation);
    let image = image.into_rgba8();
    check_current(current)?;
    Ok(Some(CachedImagePreview {
        source_size,
        image: PreviewImage {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw().into(),
        },
    }))
}

fn preview_header(bytes: &[u8]) -> Result<(u32, u32, Orientation), ImageDecodeError> {
    // Borrow the already bounded input, using image's pinned backend/options.
    // Its adapter copies all compressed bytes and reparses headers for orientation.
    let options = zune_core::options::DecoderOptions::default()
        .set_strict_mode(false)
        .set_max_width(usize::MAX)
        .set_max_height(usize::MAX);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(
        zune_core::bytestream::ZCursor::new(bytes),
        options,
    );
    decoder
        .decode_headers()
        .map_err(super::jpeg_static::decode_error)?;
    let (width, height) = decoder.dimensions().expect("decoded JPEG headers");
    let orientation = decoder
        .exif()
        .and_then(|exif| Orientation::from_exif_chunk(exif))
        .unwrap_or(Orientation::NoTransforms);
    Ok((width as u32, height as u32, orientation))
}

fn reduced_jpeg(
    bytes: &[u8],
    width: u32,
    height: u32,
    swap: bool,
) -> Result<Option<PreviewImage>, ffmpeg::Error> {
    let (bound_width, bound_height) = if swap { (160.0, 240.0) } else { (240.0, 160.0) };
    let scale = (bound_width / f64::from(width)).min(bound_height / f64::from(height));
    let target_width = (f64::from(width) * scale).round().max(1.0) as u32;
    let target_height = (f64::from(height) * scale).round().max(1.0) as u32;
    // Choose the largest decoder reduction that still supplies every thumbnail
    // pixel. Small sources retain the existing full-decode fallback.
    let Some(lowres) = (1..=3).rev().find(|shift| {
        width.div_ceil(1 << shift) >= target_width && height.div_ceil(1 << shift) >= target_height
    }) else {
        return Ok(None);
    };
    ffmpeg::init()?;
    let codec =
        ffmpeg::decoder::find(ffmpeg::codec::Id::MJPEG).ok_or(ffmpeg::Error::DecoderNotFound)?;
    let mut context = ffmpeg::codec::context::Context::new_with_codec(codec);
    context.set_threading(ffmpeg::codec::threading::Config::count(1));
    // This worker exclusively owns the unopened context. The codec descriptor is
    // immutable FFmpeg storage; neither pointer escapes this synchronous call.
    unsafe {
        if (*codec.as_ptr()).max_lowres < 3 {
            return Err(ffmpeg::Error::InvalidData);
        }
        (*context.as_mut_ptr()).lowres = lowres;
        (*context.as_mut_ptr()).max_pixels = i64::from(width) * i64::from(height);
    }
    let mut decoder = context.decoder().video()?;
    decoder.send_packet(&ffmpeg::Packet::copy(bytes))?;
    let mut decoded = ffmpeg::frame::Video::empty();
    decoder.receive_frame(&mut decoded)?;
    if decoded.width() != width.div_ceil(1 << lowres)
        || decoded.height() != height.div_ceil(1 << lowres)
    {
        return Err(ffmpeg::Error::InvalidData);
    }
    let mut scaler = ffmpeg::software::scaling::Context::get(
        decoded.format(),
        decoded.width(),
        decoded.height(),
        ffmpeg::format::Pixel::RGBA,
        target_width,
        target_height,
        ffmpeg::software::scaling::Flags::BILINEAR,
    )?;
    let mut rgba = ffmpeg::frame::Video::empty();
    scaler.run(&decoded, &mut rgba)?;
    let row_bytes = target_width as usize * 4;
    let mut pixels = Vec::with_capacity(row_bytes * target_height as usize);
    for row in 0..target_height as usize {
        let start = row * rgba.stride(0);
        pixels.extend_from_slice(&rgba.data(0)[start..start + row_bytes]);
    }
    Ok(Some(PreviewImage {
        width: target_width,
        height: target_height,
        rgba: pixels.into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "JPEG input-allocation comparison; run in Release without concurrent builds"]
    #[allow(clippy::assertions_on_constants)]
    fn reserved_preview_input_reports_whole_thumbnail_cost() {
        assert!(!cfg!(debug_assertions), "use Release for timing");
        if let Some(path) = std::env::var_os("TOWAVUE_JPEG_PREVIEW_INPUT_SOURCE") {
            report_input_cost(Path::new(&path), "reference");
            return;
        }
        let root =
            std::env::temp_dir().join(format!("towavue-preview-input-{}", std::process::id()));
        std::fs::create_dir(&root).expect("unique owned fixture directory");
        for (width, height, noisy) in [
            (503, 317, false),
            (4096, 2304, false),
            (503, 317, true),
            (1920, 1080, true),
            (4096, 2304, true),
        ] {
            let mut seed = 1_u32;
            let pixels = image::RgbImage::from_fn(width, height, |_, _| {
                image::Rgb(std::array::from_fn(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    if noisy { (seed >> 24) as u8 } else { 128 }
                }))
            });
            let mut encoded = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 95)
                .encode_image(&pixels)
                .expect("generated JPEG");
            drop(pixels);
            let path = root.join(format!("{width}x{height}.jpg"));
            std::fs::write(&path, &encoded).expect("owned fixture");
            report_input_cost(&path, &format!("{width}x{height}-noisy-{noisy}"));
            std::fs::remove_file(path).expect("owned fixture cleanup");
        }
        std::fs::remove_dir(root).expect("empty owned fixture directory");
    }

    fn report_input_cost(path: &Path, label: &str) {
        let encoded = std::fs::read(path).expect("bounded source");
        assert!(encoded.len() <= 32 * 1024 * 1024);
        let stamp = std::fs::metadata(path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        RESERVE_PREVIEW_INPUT.set(false);
        let expected = jpeg_thumbnail(path, IMAGE_BYTE_LIMIT, &|| true)
            .expect("baseline")
            .expect("reduced JPEG");
        for reserved in [false, true, true, false] {
            RESERVE_PREVIEW_INPUT.set(reserved);
            let mut times = Vec::new();
            for _ in 0..9 {
                let start = std::time::Instant::now();
                let actual = jpeg_thumbnail(path, IMAGE_BYTE_LIMIT, &|| true)
                    .expect("thumbnail")
                    .expect("reduced JPEG");
                times.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(actual.source_size, expected.source_size);
                assert_eq!(actual.image.width, expected.image.width);
                assert_eq!(actual.image.height, expected.image.height);
                assert_eq!(actual.image.rgba, expected.image.rgba);
            }
            let raw = times.clone();
            times.sort_by(f64::total_cmp);
            println!(
                "JPEG_INPUT {label} bytes={} source={:?} reserved={reserved} median_ms={:.4} raw_ms={raw:?}",
                encoded.len(),
                expected.source_size,
                times[4]
            );
        }
        RESERVE_PREVIEW_INPUT.set(true);
        assert_eq!(std::fs::read(path).expect("source bytes"), encoded);
        assert_eq!(
            std::fs::metadata(path)
                .expect("metadata")
                .modified()
                .expect("mtime"),
            stamp
        );
    }

    #[test]
    fn reserved_preview_input_retains_limits_and_read_cancellation() {
        let path = std::env::temp_dir().join(format!(
            "towavue-preview-input-limit-{}.jpg",
            std::process::id()
        ));
        image::RgbImage::new(503, 317)
            .save(&path)
            .expect("owned JPEG");
        let expected = jpeg_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
            .expect("decode")
            .expect("preview");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("owned source")
            .set_len(32 * 1024 * 1024)
            .expect("bounded padded fixture");
        let actual = jpeg_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
            .expect("limit decode")
            .expect("preview");
        assert_eq!(actual.source_size, expected.source_size);
        assert_eq!(actual.image.rgba, expected.image.rgba);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("owned source")
            .set_len(32 * 1024 * 1024 + 1)
            .expect("oversized fixture");
        assert!(
            jpeg_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("limit")
                .is_none()
        );
        let reads = std::cell::Cell::new(0);
        let current = || {
            reads.set(reads.get() + 1);
            reads.get() < 12
        };
        assert!(jpeg_thumbnail(&path, IMAGE_BYTE_LIMIT, &current).is_err());
        assert_eq!(reads.get(), 12, "cancel inside bounded input reads");
        std::fs::write(&path, [0xff, 0xd8, 0xff]).expect("truncated JPEG");
        assert!(jpeg_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true).is_err());
        image::RgbImage::new(503, 317)
            .save_with_format(&path, ImageFormat::Png)
            .expect("mislabeled PNG");
        assert!(
            jpeg_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("format sniff")
                .is_none()
        );
        std::fs::remove_file(path).expect("owned fixture cleanup");
    }

    fn adapter_header(bytes: &[u8]) -> Result<(u32, u32, Orientation), ImageDecodeError> {
        let mut decoder = image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(bytes))?;
        let (width, height) = decoder.dimensions();
        Ok((width, height, decoder.orientation()?))
    }

    fn assert_adapter_preview(bytes: &[u8], preview: &CachedImagePreview) {
        let (width, height, orientation) = adapter_header(bytes).expect("adapter header");
        let swap = matches!(
            orientation,
            Orientation::Rotate90
                | Orientation::Rotate270
                | Orientation::Rotate90FlipH
                | Orientation::Rotate270FlipH
        );
        let reduced = reduced_jpeg(bytes, width, height, swap)
            .expect("decode")
            .expect("reduced");
        let mut expected = DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(
                reduced.width,
                reduced.height,
                std::sync::Arc::unwrap_or_clone(reduced.rgba),
            )
            .expect("RGBA"),
        );
        expected.apply_orientation(orientation);
        assert_eq!(
            preview.source_size,
            if swap {
                (height, width)
            } else {
                (width, height)
            }
        );
        assert_eq!(
            (preview.image.width, preview.image.height),
            (expected.width(), expected.height())
        );
        assert_eq!(preview.image.rgba.as_slice(), expected.as_bytes());
    }

    #[test]
    fn borrowed_preview_headers_match_adapter_metadata_and_errors() {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode_image(&image::RgbImage::new(47, 31))
            .expect("JPEG");
        for tag in 0..=9 {
            let mut exif =
                *b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0";
            exif[24] = tag;
            let mut encoded = vec![0xff, 0xd8, 0xff, 0xe1];
            encoded.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
            encoded.extend_from_slice(&exif);
            encoded.extend_from_slice(&bytes[2..]);
            assert_eq!(
                preview_header(&encoded).expect("borrowed"),
                adapter_header(&encoded).expect("adapter")
            );
        }
        // Both parsers must reject incomplete headers, without requiring entropy decoding.
        for end in 0..bytes.len() {
            let borrowed = preview_header(&bytes[..end]);
            let adapter = adapter_header(&bytes[..end]);
            assert_eq!(borrowed.is_ok(), adapter.is_ok(), "prefix {end}");
            if let (Ok(borrowed), Ok(adapter)) = (borrowed, adapter) {
                assert_eq!(borrowed, adapter);
            }
        }
    }

    #[test]
    #[ignore = "generated large JPEG preview-header comparison; run in Release without concurrent builds"]
    #[allow(clippy::assertions_on_constants)]
    fn borrowed_preview_headers_report_adapter_cost() {
        assert!(!cfg!(debug_assertions), "use Release for timing");
        let mut seed = 1_u32;
        let pixels = image::RgbImage::from_fn(4096, 2304, |_, _| {
            image::Rgb(std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            }))
        });
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
            .encode_image(&pixels)
            .expect("generated JPEG");
        drop(pixels);
        let expected = adapter_header(&bytes).expect("baseline");
        assert_eq!(preview_header(&bytes).expect("borrowed"), expected);
        for borrowed in [false, true, true, false] {
            let mut milliseconds = Vec::new();
            for _ in 0..15 {
                let start = std::time::Instant::now();
                let result = if borrowed {
                    preview_header(&bytes)
                } else {
                    adapter_header(&bytes)
                };
                milliseconds.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(result.expect("headers"), expected);
            }
            milliseconds.sort_by(f64::total_cmp);
            eprintln!(
                "JPEG_PREVIEW_HEADER borrowed={borrowed} compressed_bytes={} median_ms={:.4}",
                bytes.len(),
                milliseconds[7]
            );
        }
    }

    #[test]
    fn jpeg_thumbnails_reduce_medium_images_without_enabling_speculative_previews() {
        let path =
            std::env::temp_dir().join(format!("towavue-medium-jpeg-{}.jpg", std::process::id()));
        for (width, height) in [(503, 317), (1001, 701), (1920, 1080)] {
            let pixels = image::RgbImage::from_fn(width, height, |x, y| {
                image::Rgb(match (x < width / 2, y < height / 2) {
                    (true, true) => [240, 10, 20],
                    (false, true) => [10, 230, 30],
                    (true, false) => [20, 30, 220],
                    (false, false) => [230, 220, 20],
                })
            });
            pixels.save(&path).expect("fixture");
            let original = std::fs::read(&path).expect("JPEG");
            for tag in 1..=8 {
                let mut exif =
                    *b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0";
                exif[24] = tag;
                let mut encoded = vec![0xff, 0xd8, 0xff, 0xe1];
                encoded.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
                encoded.extend_from_slice(&exif);
                encoded.extend_from_slice(&original[2..]);
                std::fs::write(&path, &encoded).expect("oriented fixture");
                assert!(
                    jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| true)
                        .expect("speculation")
                        .is_none()
                );
                let preview = jpeg_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
                    .expect("thumbnail")
                    .expect("reduced JPEG");
                assert_adapter_preview(&encoded, &preview);
                let reference = decode_image(&path).expect("independent full decode");
                assert_eq!(preview.source_size, reference.dimensions());
                let image = preview.image;
                assert!(image.width <= 240 && image.height <= 160);
                assert!(
                    image
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|pixel| pixel[3] == 255)
                );
                let (width, height) = reference.dimensions();
                for (x, y) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                    let small =
                        ((image.height * y / 4 * image.width + image.width * x / 4) * 4) as usize;
                    let full = ((height * y / 4 * width + width * x / 4) * 4) as usize;
                    for channel in 0..3 {
                        assert!(
                            image.rgba[small + channel]
                                .abs_diff(reference.frames[0].rgba[full + channel])
                                <= 5
                        );
                    }
                }
                assert_eq!(std::fs::read(&path).expect("unchanged source"), encoded);
            }
        }
        assert!(
            jpeg_thumbnail(&path, 1, &|| true)
                .expect("budget")
                .is_none()
        );
        assert!(matches!(
            jpeg_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| false),
            Err(ImageDecodeError::Cancelled)
        ));
        image::RgbImage::new(239, 159)
            .save(&path)
            .expect("small fixture");
        assert!(
            jpeg_thumbnail(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("full-decode fallback")
                .is_none()
        );
        std::fs::remove_file(path).expect("owned fixture");
    }

    #[test]
    #[ignore = "requires Pillow-generated fixtures; run scripts/generate-jpeg-preview-fixtures.py"]
    fn jpeg_preview_matches_independent_encoding_and_color_samples() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/jpeg-preview");
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
            let path = root.join(format!("{name}.jpg"));
            let expected = std::fs::read(path.with_extension("samples"))
                .expect("run scripts/generate-jpeg-preview-fixtures.py first");
            assert_eq!(expected.len(), 16, "{name}");
            let reference = decode_image(&path).expect("original JPEG");
            let preview = jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("preview decode")
                .expect("supported large JPEG");
            assert_adapter_preview(&std::fs::read(&path).expect("owned JPEG"), &preview);
            assert_eq!(reference.dimensions(), (2571, 1933), "{name}");
            assert_eq!(preview.source_size, reference.dimensions(), "{name}");
            let image = preview.image;
            assert_eq!((image.width, image.height), (213, 160), "{name}");
            assert_eq!(image.rgba.len(), 213 * 160 * 4);
            assert!(
                image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| pixel[3] == 255)
            );
            for (index, (x, y)) in [(1, 1), (3, 1), (1, 3), (3, 3)].into_iter().enumerate() {
                let small =
                    ((image.height * y / 4 * image.width + image.width * x / 4) * 4) as usize;
                let full = ((1933 * y / 4 * 2571 + 2571 * x / 4) * 4) as usize;
                for channel in 0..4 {
                    let expected = expected[index * 4 + channel];
                    let original = reference.frames[0].rgba[full + channel];
                    let reduced = image.rgba[small + channel];
                    assert!(
                        original.abs_diff(expected) <= 5,
                        "{name}: original {original}, expected {expected}"
                    );
                    assert!(
                        reduced.abs_diff(expected) <= 5,
                        "{name}: preview {reduced}, expected {expected}"
                    );
                    assert!(
                        reduced.abs_diff(original) <= 5,
                        "{name}: preview {reduced}, original {original}"
                    );
                }
            }
        }
    }

    #[test]
    fn jpeg_preview_preserves_grayscale_and_opaque_alpha() {
        let path =
            std::env::temp_dir().join(format!("towavue-gray-jpeg-{}.jpg", std::process::id()));
        image::GrayImage::from_fn(2571, 1933, |x, y| {
            image::Luma([match (x < 1285, y < 966) {
                (true, true) => 0,
                (false, true) => 80,
                (true, false) => 170,
                (false, false) => 255,
            }])
        })
        .save(&path)
        .expect("owned grayscale JPEG");
        let preview = jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| true)
            .expect("grayscale preview")
            .expect("large grayscale JPEG");
        assert_eq!(preview.source_size, (2571, 1933));
        let image = preview.image;
        assert_eq!((image.width, image.height), (213, 160));
        assert_eq!(image.rgba.len(), 213 * 160 * 4);
        for pixel in image.rgba.as_chunks::<4>().0 {
            assert_eq!(pixel[0], pixel[1]);
            assert_eq!(pixel[0], pixel[2]);
            assert_eq!(pixel[3], 255);
        }
        for ((x, y), expected) in [(1, 1), (3, 1), (1, 3), (3, 3)]
            .into_iter()
            .zip([0, 80, 170, 255])
        {
            let small = ((image.height * y / 4 * image.width + image.width * x / 4) * 4) as usize;
            assert!(image.rgba[small].abs_diff(expected) <= 1);
        }
        std::fs::remove_file(path).expect("remove owned grayscale JPEG");
    }

    #[test]
    fn jpeg_preview_preserves_oriented_geometry_colors_and_limits() {
        let path =
            std::env::temp_dir().join(format!("towavue-reduced-jpeg-{}.jpg", std::process::id()));
        image::RgbImage::from_fn(2571, 1933, |x, y| {
            image::Rgb(match (x < 1285, y < 966) {
                (true, true) => [240, 10, 20],
                (false, true) => [10, 230, 30],
                (true, false) => [20, 30, 220],
                (false, false) => [230, 220, 20],
            })
        })
        .save(&path)
        .expect("owned JPEG");
        let original = std::fs::read(&path).expect("encoded JPEG");
        for tag in 1..=8 {
            let mut exif =
                *b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0";
            exif[24] = tag;
            let mut encoded = vec![0xff, 0xd8, 0xff, 0xe1];
            encoded.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
            encoded.extend_from_slice(&exif);
            encoded.extend_from_slice(&original[2..]);
            std::fs::write(&path, &encoded).expect("EXIF fixture");
            let preview = jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("preview decode")
                .expect("large JPEG");
            assert_adapter_preview(&encoded, &preview);
            let reference = decode_image(&path).expect("original decode");
            assert_eq!(
                preview.source_size,
                reference.dimensions(),
                "orientation {tag}"
            );
            let image = &preview.image;
            assert!(image.width <= 240 && image.height <= 160);
            assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
            assert!(
                image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| pixel[3] == 255)
            );
            let (width, height) = reference.dimensions();
            for (x, y) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                let small =
                    ((image.height * y / 4 * image.width + image.width * x / 4) * 4) as usize;
                let full = ((height * y / 4 * width + width * x / 4) * 4) as usize;
                for channel in 0..3 {
                    assert!(
                        image.rgba[small + channel]
                            .abs_diff(reference.frames[0].rgba[full + channel])
                            <= 5,
                        "color/orientation {tag}"
                    );
                }
            }
        }
        assert!(
            jpeg_preview(&path, 1, &|| true)
                .expect("budget skip")
                .is_none()
        );
        assert!(matches!(
            jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| false),
            Err(ImageDecodeError::Cancelled)
        ));
        let polls = std::cell::Cell::new(0);
        assert!(
            jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| {
                polls.set(polls.get() + 1);
                polls.get() < 5
            })
            .is_err()
        );
        File::options()
            .write(true)
            .open(&path)
            .expect("owned JPEG")
            .set_len(32 * 1024 * 1024 + 1)
            .expect("oversized compressed input");
        assert!(
            jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("input cap")
                .is_none()
        );
        image::RgbImage::new(16, 16)
            .save(&path)
            .expect("small JPEG");
        assert!(
            jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("small skip")
                .is_none()
        );
        std::fs::write(&path, b"not an image").expect("unsupported fixture");
        assert!(
            jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("format skip")
                .is_none()
        );
        std::fs::write(&path, [0xff, 0xd8, 0xff, 0xe0, 0, 16]).expect("truncated JPEG");
        assert!(jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| true).is_err());
        std::fs::remove_file(path).expect("remove owned JPEG");
    }
}
