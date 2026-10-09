use super::*;
use std::rc::Rc;

fn encoded(width: u32, height: u32, alpha: bool, lossless: bool) -> Vec<u8> {
    let channels = if alpha { 4 } else { 3 };
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(
                &[
                    (x * 19 + y * 37) as u8,
                    (x * 71 + y * 13) as u8,
                    (x * 11 + y * 53) as u8,
                    (x * 29 + y * 61) as u8,
                ][..channels],
            );
        }
    }
    let encoder = if alpha {
        webp::Encoder::from_rgba(&pixels, width, height)
    } else {
        webp::Encoder::from_rgb(&pixels, width, height)
    };
    if lossless {
        encoder.encode_lossless().to_vec()
    } else {
        encoder.encode(73.0).to_vec()
    }
}

fn decode(bytes: &[u8], rgba: bool, simple: bool) -> Result<Vec<u8>, String> {
    let mut decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if simple {
        decoder.set_lossy_upsampling(UpsamplingMethod::Simple);
    }
    let (w, h) = decoder.dimensions();
    let alpha = decoder.has_alpha();
    let mut pixels = vec![
        0x91;
        if rgba {
            w as usize * h as usize * 4
        } else {
            decoder.output_buffer_size().unwrap()
        }
    ];
    if rgba {
        decoder.read_image_rgba(&mut pixels)
    } else {
        decoder.read_image(&mut pixels)
    }
    .map_err(|e| e.to_string())?;
    assert_eq!(decoder.has_alpha(), alpha);
    if !rgba && !alpha {
        pixels = pixels
            .chunks_exact(3)
            .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
            .collect();
    }
    Ok(pixels)
}

#[test]
fn direct_rgba_matches_rgb_expansion_and_alpha_for_odd_edges_and_upsampling() {
    for (width, height) in [
        (1, 1),
        (1, 17),
        (17, 1),
        (2, 3),
        (3, 2),
        (15, 17),
        (17, 19),
        (33, 31),
    ] {
        for alpha in [false, true] {
            for lossless in [false, true] {
                let bytes = encoded(width, height, alpha, lossless);
                for simple in [false, true] {
                    assert_eq!(
                        decode(&bytes, false, simple),
                        decode(&bytes, true, simple),
                        "{width}x{height} alpha={alpha} lossless={lossless} simple={simple}"
                    );
                }
            }
        }
    }
}

#[test]
fn direct_rgba_discards_unadvertised_lossless_alpha_like_rgb() {
    let mut bytes = encoded(17, 19, true, true);
    let offset = bytes.windows(4).position(|b| b == b"VP8L").unwrap();
    bytes[offset + 12] &= !0x10;
    if let Some(offset) = bytes.windows(4).position(|b| b == b"VP8X") {
        bytes[offset + 8] &= !0x10;
    }
    assert!(!WebPDecoder::new(Cursor::new(&bytes)).unwrap().has_alpha());
    let expected = decode(&bytes, false, false).unwrap();
    let actual = decode(&bytes, true, false).unwrap();
    assert_eq!(actual, expected);
    assert!(actual.chunks_exact(4).all(|p| p[3] == 255));
}

#[test]
fn direct_rgba_size_errors_do_not_consume_the_image() {
    for alpha in [false, true] {
        let bytes = encoded(3, 5, alpha, false);
        let mut decoder = WebPDecoder::new(Cursor::new(&bytes)).unwrap();
        for size in [0, 44, 59, 61] {
            let mut buf = vec![0x91; size];
            assert!(matches!(
                decoder.read_image_rgba(&mut buf),
                Err(DecodingError::ImageTooLarge)
            ));
            assert!(buf.iter().all(|b| *b == 0x91));
        }
        let mut actual = vec![0; 60];
        decoder.read_image_rgba(&mut actual).unwrap();
        assert_eq!(actual, decode(&bytes, false, false).unwrap());
    }
}

#[test]
fn direct_rgba_preserves_truncation_and_corrupt_stream_outcomes() {
    for alpha in [false, true] {
        for lossless in [false, true] {
            let bytes = encoded(3, 5, alpha, lossless);
            for length in 0..=bytes.len() {
                assert_eq!(
                    decode(&bytes[..length], false, false),
                    decode(&bytes[..length], true, false),
                    "alpha={alpha} lossless={lossless} length={length}"
                );
            }
            for offset in (0..bytes.len()).step_by(7) {
                let mut changed = bytes.clone();
                changed[offset] ^= 0x80;
                // Keep malformed dimensions bounded; the decoder should report
                // its error, but this test does not allocate attacker-sized output.
                let size = WebPDecoder::new(Cursor::new(&changed))
                    .ok()
                    .and_then(|d| d.output_buffer_size());
                if size.is_some_and(|n| n > 1024 * 1024) {
                    continue;
                }
                assert_eq!(
                    decode(&changed, false, false),
                    decode(&changed, true, false),
                    "offset={offset}"
                );
            }
        }
    }
}

struct LocalReader {
    cursor: Cursor<Vec<u8>>,
    _owner: Rc<()>,
}
impl Read for LocalReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.cursor.read(bytes)
    }
}
impl Seek for LocalReader {
    fn seek(&mut self, from: io::SeekFrom) -> io::Result<u64> {
        self.cursor.seek(from)
    }
}
impl BufRead for LocalReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.cursor.fill_buf()
    }
    fn consume(&mut self, n: usize) {
        self.cursor.consume(n);
    }
}

#[test]
fn direct_rgba_keeps_non_send_reader_support() {
    let bytes = encoded(17, 19, false, false);
    let mut decoder = WebPDecoder::new(LocalReader {
        cursor: Cursor::new(bytes.clone()),
        _owner: Rc::new(()),
    })
    .unwrap();
    let mut actual = vec![0; 17 * 19 * 4];
    decoder.read_image_rgba(&mut actual).unwrap();
    assert_eq!(actual, decode(&bytes, false, false).unwrap());
}
