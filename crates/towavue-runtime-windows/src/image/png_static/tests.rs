use super::*;
use std::io::{Cursor, Write};

fn chunk(bytes: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(data);
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    bytes.extend_from_slice(&crc.finalize().to_be_bytes());
}

#[test]
fn adam7_static_pixels_match_full_decoder_and_reject_damage() {
    for (width, height) in [(1_u32, 1_u32), (19, 13)] {
        for color in [
            png::ColorType::Grayscale,
            png::ColorType::GrayscaleAlpha,
            png::ColorType::Rgb,
            png::ColorType::Rgba,
        ] {
            for depth in [png::BitDepth::Eight, png::BitDepth::Sixteen] {
                let mut raw = Vec::new();
                // Adam7 passes, assembled independently of the production decoder.
                for (x0, y0, dx, dy) in [
                    (0, 0, 8, 8),
                    (4, 0, 8, 8),
                    (0, 4, 4, 8),
                    (2, 0, 4, 4),
                    (0, 2, 2, 4),
                    (1, 0, 2, 2),
                    (0, 1, 1, 2),
                ] {
                    if x0 >= width {
                        continue;
                    }
                    for y in (y0..height).step_by(dy) {
                        raw.push(0);
                        for x in (x0..width).step_by(dx) {
                            for c in 0..color.samples() {
                                let value = (x * 719 + y * 391 + c as u32 * 997) as u16;
                                if depth == png::BitDepth::Sixteen {
                                    raw.extend_from_slice(&value.to_be_bytes());
                                } else {
                                    raw.push(value as u8);
                                }
                            }
                        }
                    }
                }
                let mut compressed =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                compressed.write_all(&raw).expect("compress passes");
                let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
                let mut header = Vec::from(width.to_be_bytes());
                header.extend_from_slice(&height.to_be_bytes());
                header.extend_from_slice(&[depth as u8, color as u8, 0, 0, 1]);
                chunk(&mut bytes, b"IHDR", &header);
                chunk(&mut bytes, b"IDAT", &compressed.finish().expect("zlib"));
                chunk(&mut bytes, b"IEND", &[]);
                let expected = DynamicImage::from_decoder(
                    PngDecoder::new(Cursor::new(&bytes)).expect("legacy header"),
                )
                .expect("legacy Adam7")
                .into_rgba8()
                .into_raw();
                let frame = decode(Cursor::new(&bytes), IMAGE_BYTE_LIMIT, &|| true)
                    .expect("Adam7")
                    .expect("static");
                assert_eq!((frame.width, frame.height), (width, height));
                assert_eq!(frame.rgba, expected);
                assert!(matches!(
                    decode(Cursor::new(&bytes), expected.len() - 1, &|| true),
                    Err(ImageDecodeError::TooLarge)
                ));
                assert!(matches!(
                    decode(Cursor::new(&bytes), IMAGE_BYTE_LIMIT, &|| false),
                    Err(ImageDecodeError::Cancelled)
                ));
                assert!(
                    decode(
                        Cursor::new(&bytes[..bytes.len() - 5]),
                        IMAGE_BYTE_LIMIT,
                        &|| true
                    )
                    .is_err()
                );
                // Corrupt the IDAT CRC, keeping geometry and compressed pixels intact.
                let crc_index = bytes.len() - 13;
                bytes[crc_index] ^= 1;
                assert!(decode(Cursor::new(&bytes), IMAGE_BYTE_LIMIT, &|| true).is_err());
            }
        }
    }
}
