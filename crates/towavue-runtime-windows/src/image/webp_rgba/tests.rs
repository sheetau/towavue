use std::cell::Cell;
use std::io::{BufReader, Cursor};

use image::codecs::webp::WebPDecoder as Reference;
use image::{DynamicImage, ImageError};

use super::*;
use crate::image::{CancellableReader, ImageDecodeError, static_frame};

fn fixture(alpha: bool, orientation: u8) -> (Vec<u8>, Vec<u8>) {
    let channels = if alpha { 4 } else { 3 };
    let pixels: Vec<_> = (0..17 * 19 * channels)
        .map(|n| (n * 37 + n / 7) as u8)
        .collect();
    let mut exif = *b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0";
    exif[18] = orientation;
    let mut bytes = Vec::new();
    let mut encoder = image_webp::WebPEncoder::new(&mut bytes);
    encoder.set_exif_metadata(exif.to_vec());
    encoder.set_icc_profile(b"owned ICC metadata control".to_vec());
    encoder.set_xmp_metadata(b"<x:xmpmeta>owned XMP control</x:xmpmeta>".to_vec());
    encoder
        .encode(
            &pixels,
            17,
            19,
            if alpha {
                image_webp::ColorType::Rgba8
            } else {
                image_webp::ColorType::Rgb8
            },
        )
        .expect("owned WebP fixture");
    (bytes, pixels)
}

fn error_key(error: ImageError) -> String {
    match error {
        ImageError::IoError(error) => format!("IO {:?}: {error}", error.kind()),
        other => format!(
            "{}: {other}",
            match other {
                ImageError::Decoding(_) => "decode",
                ImageError::Encoding(_) => "encode",
                ImageError::Parameter(_) => "parameter",
                ImageError::Limits(_) => "limits",
                ImageError::Unsupported(_) => "unsupported",
                ImageError::IoError(_) => unreachable!(),
            }
        ),
    }
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    dimensions: (u32, u32),
    metadata: Vec<Result<Option<Vec<u8>>, String>>,
    orientation: Vec<Result<Orientation, String>>,
    pixels: Result<Vec<u8>, String>,
}

fn snapshot(mut decoder: impl ImageDecoder) -> Snapshot {
    let dimensions = decoder.dimensions();
    let orientation = vec![
        decoder.orientation().map_err(error_key),
        decoder.orientation().map_err(error_key),
    ];
    let metadata = vec![
        decoder.icc_profile().map_err(error_key),
        decoder.exif_metadata().map_err(error_key),
        decoder.xmp_metadata().map_err(error_key),
    ];
    let pixels = DynamicImage::from_decoder(decoder)
        .map(|image| image.into_rgba8().into_raw())
        .map_err(error_key);
    Snapshot {
        dimensions,
        metadata,
        orientation,
        pixels,
    }
}

#[test]
fn rgba_adapter_preserves_metadata_orientation_pixels_and_budget() {
    for alpha in [false, true] {
        for tag in 1..=8 {
            let (bytes, pixels) = fixture(alpha, tag);
            assert_eq!(
                snapshot(Reference::new(Cursor::new(&bytes)).expect("reference")),
                snapshot(WebPDecoder::new(Cursor::new(&bytes)).expect("candidate"))
            );
            let expected = static_frame(
                Reference::new(Cursor::new(&bytes)).expect("reference"),
                17 * 19 * 4,
                &|| true,
            )
            .expect("reference frame");
            let actual = static_frame(
                WebPDecoder::new(Cursor::new(&bytes)).expect("candidate"),
                17 * 19 * 4,
                &|| true,
            )
            .expect("candidate frame");
            assert_eq!(actual, expected, "alpha={alpha}, orientation={tag}");
            assert_eq!(actual.rgba.len(), 17 * 19 * 4);
            if tag == 1 {
                if alpha {
                    assert_eq!(actual.rgba, pixels);
                } else {
                    assert!(
                        actual
                            .rgba
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|pixel| pixel[3] == 255)
                    );
                }
            }
            assert!(matches!(
                static_frame(
                    WebPDecoder::new(Cursor::new(&bytes)).expect("candidate"),
                    17 * 19 * 4 - 1,
                    &|| true
                ),
                Err(ImageDecodeError::TooLarge)
            ));
        }
    }
}

#[test]
fn rgba_adapter_preserves_non_send_readers_and_cancelled_io_errors() {
    let (bytes, _) = fixture(false, 1);
    let current = std::rc::Rc::new(Cell::new(true));
    let check = || current.get();
    let reader = || {
        BufReader::with_capacity(
            1,
            CancellableReader {
                inner: Cursor::new(&bytes),
                is_current: &check,
            },
        )
    };
    let candidate = WebPDecoder::new(reader()).expect("candidate header");
    let reference = Reference::new(reader()).expect("reference header");
    current.set(false);
    let candidate_error = DynamicImage::from_decoder(candidate).expect_err("cancelled candidate");
    let reference_error = DynamicImage::from_decoder(reference).expect_err("cancelled reference");
    assert!(matches!(candidate_error, ImageError::IoError(_)));
    assert_eq!(error_key(candidate_error), error_key(reference_error));
    current.set(true);
    assert!(matches!(
        static_frame(
            WebPDecoder::new(reader()).expect("candidate"),
            17 * 19 * 4,
            &|| false
        ),
        Err(ImageDecodeError::Cancelled)
    ));
}

#[test]
fn rgba_adapter_preserves_truncated_input_error_categories() {
    for alpha in [false, true] {
        let (bytes, _) = fixture(alpha, 1);
        for end in 0..bytes.len() {
            let reference = Reference::new(Cursor::new(&bytes[..end]))
                .map(snapshot)
                .map_err(error_key);
            let candidate = WebPDecoder::new(Cursor::new(&bytes[..end]))
                .map(snapshot)
                .map_err(error_key);
            assert_eq!(candidate, reference, "alpha={alpha}, prefix={end}");
        }
    }
}

type FrameSnapshot = Result<((u32, u32), (u32, u32), Vec<u8>), String>;

fn animation(decoder: impl for<'a> AnimationDecoder<'a>) -> (u32, Vec<FrameSnapshot>) {
    let loops = match decoder.loop_count() {
        image::metadata::LoopCount::Infinite => 0,
        image::metadata::LoopCount::Finite(count) => count.get(),
    };
    let frames = decoder
        .into_frames()
        .map(|frame| {
            frame
                .map(|frame| {
                    (
                        frame.buffer().dimensions(),
                        frame.delay().numer_denom_ms(),
                        frame.into_buffer().into_raw(),
                    )
                })
                .map_err(error_key)
        })
        .collect();
    (loops, frames)
}

#[test]
#[ignore = "requires an explicitly supplied bounded WebP corpus"]
fn external_corpus_preserves_adapter_metadata_pixels_and_animation() {
    let directories = std::env::var_os("TOWAVUE_WEBP_CORPUS").expect("explicit corpus directories");
    let mut count = 0;
    let mut animated = 0;
    let mut budget_excluded = 0;
    for directory in std::env::split_paths(&directories) {
        let mut paths: Vec<_> = std::fs::read_dir(directory)
            .expect("corpus")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "webp")
            })
            .collect();
        paths.sort();
        for path in paths {
            let bytes = std::fs::read(&path).expect("fixture");
            if let Ok(decoder) = image_webp::WebPDecoder::new(Cursor::new(&bytes)) {
                let (w, h) = decoder.dimensions();
                if u64::from(w) * u64::from(h) * 4 * u64::from(decoder.num_frames().max(1))
                    > 512 * 1024 * 1024
                    || decoder.num_frames() > 256
                {
                    budget_excluded += 1;
                    continue;
                }
            }
            let reference = Reference::new(Cursor::new(&bytes))
                .map(snapshot)
                .map_err(error_key);
            let candidate = WebPDecoder::new(Cursor::new(&bytes))
                .map(snapshot)
                .map_err(error_key);
            assert_eq!(candidate, reference, "{}", path.display());
            if let Ok(decoder) = WebPDecoder::new(Cursor::new(bytes.clone()))
                && decoder.has_animation()
            {
                animated += 1;
                assert_eq!(
                    animation(decoder),
                    animation(Reference::new(Cursor::new(bytes)).expect("reference animation")),
                    "{}",
                    path.display()
                );
            }
            count += 1;
        }
    }
    assert!(
        count > 0 && animated > 0,
        "require image and animation coverage"
    );
    eprintln!(
        "ADAPTER_CORPUS compared={count} animated={animated} budget_excluded={budget_excluded}"
    );
}

#[test]
#[ignore = "requires an explicit fixture list and an otherwise idle Release run"]
fn paired_static_frame_component() {
    let fixtures = std::env::var_os("TOWAVUE_WEBP_COMPONENT").expect("explicit fixtures");
    let paths: Vec<_> = std::env::split_paths(&fixtures).collect();
    assert!(!paths.is_empty());
    let current = || true;
    for (fixture, path) in paths.iter().enumerate() {
        let decode = |direct| {
            let reader = crate::image::open(path, &current).expect("component file");
            if direct {
                let decoder = WebPDecoder::new(reader).expect("direct header");
                assert!(!decoder.has_animation());
                static_frame(decoder, crate::image::IMAGE_BYTE_LIMIT, &current)
            } else {
                let decoder = Reference::new(reader).expect("reference header");
                assert!(!decoder.has_animation());
                static_frame(decoder, crate::image::IMAGE_BYTE_LIMIT, &current)
            }
            .expect("component frame")
        };
        let expected = decode(false);
        assert_eq!(decode(true), expected);
        for block in 0..33 {
            // Three warmup pairs and thirty balanced measured pairs, fixed
            // before observation. Pixel comparisons and destruction are untimed.
            let order = if (block + fixture) % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            };
            for direct in order {
                let start = std::time::Instant::now();
                let frame = std::hint::black_box(decode(direct));
                let elapsed = start.elapsed().as_nanos();
                assert_eq!(frame, expected);
                eprintln!(
                    "WEBP_COMPONENT fixture={fixture} block={block} direct={} ns={elapsed}",
                    u8::from(direct)
                );
            }
        }
    }
}
