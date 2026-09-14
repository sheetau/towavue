use super::*;
use image::metadata::Orientation;
use std::io::Cursor;
use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

/// Use image's pinned JPEG backend, but decode directly into the retained RGBA
/// canvas instead of allocating RGB/Luma and then converting the entire image.
pub(super) fn decode(
    mut input: impl Read,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
) -> Result<DecodedImageFrame, ImageDecodeError> {
    check_current(current)?;
    let mut bytes = Vec::new();
    input
        .read_to_end(&mut bytes)
        .map_err(ImageDecodeError::Open)?;
    check_current(current)?;
    let options = DecoderOptions::default()
        .set_strict_mode(false)
        .set_max_width(usize::MAX)
        .set_max_height(usize::MAX)
        .jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder =
        zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes.as_slice()), options);
    decoder.decode_headers().map_err(decode_error)?;
    let (width, height) = decoder.dimensions().expect("decoded JPEG headers");
    let len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|len| *len <= byte_limit)
        .ok_or(ImageDecodeError::TooLarge)?;
    // Direct RGB lacks RGBA conversion; pinned zune's CMYK/YCCK RGBA paths use
    // three-channel strides. Retain image's proven path for these layouts.
    if !matches!(
        decoder.input_colorspace(),
        Some(ColorSpace::YCbCr | ColorSpace::Luma)
    ) {
        drop(decoder);
        return static_frame(
            image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))?,
            byte_limit,
            current,
        );
    }
    let orientation = decoder
        .exif()
        .and_then(|exif| Orientation::from_exif_chunk(exif))
        .unwrap_or(Orientation::NoTransforms);
    check_current(current)?;
    let mut rgba = vec![0; len];
    // Damaged-input recovery can differ by output layout. Try strict decoding
    // and retain the original lenient RGB/Luma path on failure.
    decoder.set_options(decoder.options().set_strict_mode(true));
    if decoder.decode_into(&mut rgba).is_err() {
        drop(rgba);
        drop(decoder);
        check_current(current)?;
        return static_frame(
            image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))?,
            byte_limit,
            current,
        );
    }
    // Release compressed input and decoder scratch before orientation allocates
    // a rotated canvas. The cancellable reader still bounds reads to 64 KiB;
    // entropy decoding remains a single non-interruptible backend call.
    drop(decoder);
    drop(bytes);
    check_current(current)?;
    let mut image = DynamicImage::ImageRgba8(
        image::RgbaImage::from_raw(width as u32, height as u32, rgba)
            .expect("validated JPEG canvas"),
    );
    image.apply_orientation(orientation);
    check_current(current)?;
    let image = image.into_rgba8();
    Ok(DecodedImageFrame {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        delay: Duration::ZERO,
    })
}

pub(super) fn decode_error(error: zune_jpeg::errors::DecodeErrors) -> ImageDecodeError {
    ImageError::Decoding(image::error::DecodingError::new(
        ImageFormat::Jpeg.into(),
        error,
    ))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline(bytes: &[u8]) -> DecodedImageFrame {
        static_frame(
            image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).expect("JPEG"),
            IMAGE_BYTE_LIMIT,
            &|| true,
        )
        .expect("baseline frame")
    }

    #[test]
    fn direct_jpeg_rgba_matches_color_gray_orientation_and_limits() {
        for (width, height) in [(1, 1), (17, 13), (32, 24), (129, 65)] {
            let rgb = image::RgbImage::from_fn(width, height, |x, y| {
                image::Rgb([(x * 71) as u8, (y * 39) as u8, (x * 17 + y * 83) as u8])
            });
            for source in [
                DynamicImage::ImageLuma8(DynamicImage::ImageRgb8(rgb.clone()).into_luma8()),
                DynamicImage::ImageRgb8(rgb),
            ] {
                let mut jpeg = Cursor::new(Vec::new());
                source
                    .write_to(&mut jpeg, ImageFormat::Jpeg)
                    .expect("encode");
                for tag in 0..=9 {
                    let mut exif = *b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0";
                    exif[24] = tag;
                    let mut bytes = vec![0xff, 0xd8, 0xff, 0xe1];
                    bytes.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
                    bytes.extend_from_slice(&exif);
                    bytes.extend_from_slice(&jpeg.get_ref()[2..]);
                    let len = width as usize * height as usize * 4;
                    let actual = decode(bytes.as_slice(), len, &|| true).expect("direct RGBA");
                    assert_eq!(actual, baseline(&bytes), "{width}x{height}, EXIF {tag}");
                    assert!(matches!(
                        decode(bytes.as_slice(), len - 1, &|| true),
                        Err(ImageDecodeError::TooLarge)
                    ));
                    assert!(matches!(
                        decode(bytes.as_slice(), len, &|| false),
                        Err(ImageDecodeError::Cancelled)
                    ));
                }
                for end in [0, 2, 20, jpeg.get_ref().len() / 2, jpeg.get_ref().len() - 2] {
                    let truncated = &jpeg.get_ref()[..end];
                    let original = image::codecs::jpeg::JpegDecoder::new(Cursor::new(truncated))
                        .map_err(ImageDecodeError::from)
                        .and_then(|decoder| static_frame(decoder, IMAGE_BYTE_LIMIT, &|| true));
                    let actual = decode(truncated, IMAGE_BYTE_LIMIT, &|| true);
                    assert_eq!(actual.is_ok(), original.is_ok(), "truncation {end}");
                    if let (Ok(actual), Ok(original)) = (actual, original) {
                        assert!(
                            actual == original,
                            "accepted truncation {end}, {:?}",
                            source.color()
                        );
                    }
                }
                assert_eq!(
                    decode(jpeg.get_ref().as_slice(), IMAGE_BYTE_LIMIT, &|| true).expect("no EXIF"),
                    baseline(jpeg.get_ref())
                );
                for checkpoint in 1..=5 {
                    let checks = std::cell::Cell::new(0);
                    let current = || {
                        checks.set(checks.get() + 1);
                        checks.get() < checkpoint
                    };
                    assert!(matches!(
                        decode(jpeg.get_ref().as_slice(), IMAGE_BYTE_LIMIT, &current),
                        Err(ImageDecodeError::Cancelled)
                    ));
                }
            }
        }
    }

    #[test]
    fn jpeg_foreground_and_prefetch_sniff_content_and_preserve_pixels() {
        let path = std::env::temp_dir().join(format!(
            "towavue-jpeg-rgba-content-{}.png",
            std::process::id()
        ));
        let mut jpeg = Cursor::new(Vec::new());
        image::RgbImage::from_fn(33, 21, |x, y| {
            image::Rgb([(x * 71) as u8, (y * 39) as u8, (x * 17 + y * 83) as u8])
        })
        .write_to(&mut jpeg, ImageFormat::Jpeg)
        .expect("encode owned JPEG");
        std::fs::write(&path, jpeg.get_ref()).expect("misnamed owned JPEG");
        let original = decode_image(&path).expect("foreground");
        assert_eq!(original.format, "JPEG");
        assert_eq!(original.frames, vec![baseline(jpeg.get_ref())]);
        assert_eq!(
            decode_image_for_prefetch(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("prefetch")
                .expect("static JPEG"),
            original
        );
        for prefetch in [false, true] {
            let result = if prefetch {
                decode_image_for_prefetch(&path, 33 * 21 * 4 - 1, &|| true)
            } else {
                decode_image_cancellable(&path, 33 * 21 * 4 - 1, &|| true).map(Some)
            };
            assert!(matches!(result, Err(ImageDecodeError::TooLarge)));
        }
        std::fs::remove_file(path).expect("remove owned JPEG");
    }

    #[test]
    #[ignore = "requires owned Pillow fixtures from scripts/generate-jpeg-preview-fixtures.py"]
    fn direct_jpeg_rgba_matches_progressive_cmyk_and_direct_rgb() {
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
            let bytes =
                std::fs::read(root.join(format!("{name}.jpg"))).expect("owned JPEG fixture");
            let actual = decode(bytes.as_slice(), IMAGE_BYTE_LIMIT, &|| true).expect("RGBA frame");
            assert!(actual == baseline(&bytes), "pixel equality: {name}");
        }
    }
}
