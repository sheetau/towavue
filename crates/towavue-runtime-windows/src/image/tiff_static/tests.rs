use super::*;
use image::ImageEncoder;
use std::cell::Cell;
use tiff::encoder::{Compression, Predictor, TiffEncoder, TiffKind, colortype};

fn write_fixture<K: TiffKind>(
    mut encoder: TiffEncoder<Cursor<&mut Vec<u8>>, K>,
    size: (u32, u32),
    rows: u32,
    orientation: u16,
    pixels: &[u8],
) {
    let mut image = encoder
        .new_image::<colortype::RGB8>(size.0, size.1)
        .expect("fixture header");
    image.rows_per_strip(rows).expect("strip rows");
    image
        .encoder()
        .write_tag(Tag::Orientation, orientation)
        .expect("orientation");
    image
        .encoder()
        .write_tag(Tag::Artist, "generated strip comparison")
        .expect("metadata");
    image.write_data(pixels).expect("fixture pixels");
}

fn fixture(
    size: (u32, u32),
    rows: u32,
    orientation: u16,
    compression: Compression,
    big: bool,
) -> Vec<u8> {
    let pixels: Vec<u8> = (0..size.0 * size.1 * 3)
        .map(|n| ((n * 73) ^ (n / 7) ^ (n / 97)) as u8)
        .collect();
    let mut bytes = Vec::new();
    if big {
        write_fixture(
            TiffEncoder::new_big(Cursor::new(&mut bytes))
                .expect("BigTIFF")
                .with_compression(compression)
                .with_predictor(Predictor::Horizontal),
            size,
            rows,
            orientation,
            &pixels,
        );
    } else {
        write_fixture(
            TiffEncoder::new(Cursor::new(&mut bytes))
                .expect("TIFF")
                .with_compression(compression)
                .with_predictor(Predictor::Horizontal),
            size,
            rows,
            orientation,
            &pixels,
        );
    }
    bytes
}

fn reference(bytes: &[u8]) -> Result<DecodedImageFrame, ImageDecodeError> {
    static_frame(
        TiffDecoder::new(Cursor::new(bytes))?,
        IMAGE_BYTE_LIMIT,
        &|| true,
    )
}

#[test]
fn parallel_tiff_strips_match_orientation_compression_and_bigtiff() {
    for compression in [
        Compression::Uncompressed,
        Compression::Lzw,
        Compression::Deflate(Default::default()),
        Compression::Packbits,
    ] {
        for big in [false, true] {
            for orientation in 1..=8 {
                for rows in [1, 5, 11] {
                    let bytes = fixture((17, 13), rows, orientation, compression, big);
                    let expected = reference(&bytes).expect("reference");
                    for workers in [1, 2, 4, 8] {
                        let rgb = parallel_rgb(&bytes, (17, 13), workers, &|| true)
                            .expect("eligible strip decode");
                        let mut image = DynamicImage::ImageRgb8(
                            image::RgbImage::from_raw(17, 13, rgb).expect("RGB"),
                        );
                        image.apply_orientation(
                            image::metadata::Orientation::from_exif(orientation as u8)
                                .expect("orientation"),
                        );
                        let image = image.into_rgba8();
                        assert_eq!(image.dimensions(), (expected.width, expected.height));
                        assert!(image.as_raw() == &expected.rgba, "RGBA differs");
                    }
                }
            }
        }
    }
}

#[test]
fn foreground_sized_tiff_preserves_pixels_budget_and_cancellation() {
    let bytes = fixture((641, 547), 5, 6, Compression::Lzw, true);
    let expected = reference(&bytes).expect("reference");
    let limit = 641 * 547 * 4;
    let actual = decode(Cursor::new(&bytes), limit, &|| true).expect("decode");
    assert_eq!((actual.width, actual.height), (547, 641));
    assert!(actual == expected, "complete frame differs");
    assert!(matches!(
        decode(Cursor::new(&bytes), limit - 1, &|| true),
        Err(ImageDecodeError::TooLarge)
    ));
    assert!(matches!(
        decode(Cursor::new(&bytes), limit, &|| false),
        Err(ImageDecodeError::Cancelled)
    ));
    let polls = Cell::new(0);
    let current = || {
        polls.set(polls.get() + 1);
        polls.get() < 2
    };
    assert!(parallel_rgb(&bytes, (641, 547), 8, &current).is_none());
    assert!(polls.get() >= 2, "coordinator observes cancellation");
    polls.set(0);
    let current = || {
        polls.set(polls.get() + 1);
        polls.get() < 3
    };
    assert!(matches!(
        decode(Cursor::new(&bytes), limit, &current),
        Err(ImageDecodeError::Cancelled)
    ));
}

#[test]
fn tiff_fallback_preserves_other_colors_and_single_strips() {
    for color in [
        image::ExtendedColorType::L8,
        image::ExtendedColorType::Rgba8,
    ] {
        let channels = if color == image::ExtendedColorType::L8 {
            1
        } else {
            4
        };
        let pixels: Vec<_> = (0..17 * 13 * channels).map(|n| n as u8).collect();
        let mut bytes = Vec::new();
        image::codecs::tiff::TiffEncoder::new(Cursor::new(&mut bytes))
            .write_image(&pixels, 17, 13, color)
            .expect("other color fixture");
        assert!(parallel_rgb(&bytes, (17, 13), 8, &|| true).is_none());
        assert!(
            decode(Cursor::new(&bytes), IMAGE_BYTE_LIMIT, &|| true).expect("fallback")
                == reference(&bytes).expect("reference")
        );
    }
    let bytes = fixture((17, 13), 20, 3, Compression::Lzw, false);
    assert!(parallel_rgb(&bytes, (17, 13), 8, &|| true).is_none());
    assert!(
        decode(Cursor::new(&bytes), IMAGE_BYTE_LIMIT, &|| true).expect("single strip")
            == reference(&bytes).expect("reference")
    );
}

#[test]
fn damaged_tiff_falls_back_without_changing_acceptance_or_diagnostics() {
    let original = fixture((641, 547), 5, 1, Compression::Lzw, false);
    let mut random = 0x1475ab3349_u64;
    for index in 0..48 {
        let mut bytes = original.clone();
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        if index < 16 {
            bytes.truncate(index * bytes.len() / 16);
        } else {
            let position = (random as usize) % bytes.len();
            bytes[position] ^= 1 << ((random >> 32) % 8);
        }
        let expected = reference(&bytes);
        let actual = decode(Cursor::new(&bytes), IMAGE_BYTE_LIMIT, &|| true);
        match (expected, actual) {
            (Ok(a), Ok(b)) => assert!(a == b, "accepted damaged pixels differ at {index}"),
            (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
            _ => panic!("damaged input acceptance differs at {index}"),
        }
    }
}
