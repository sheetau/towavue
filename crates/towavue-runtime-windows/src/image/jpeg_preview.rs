use ffmpeg_next as ffmpeg;
use image::codecs::jpeg::JpegDecoder;
use image::metadata::Orientation;

use super::*;
use crate::{CachedImagePreview, PreviewImage};

/// Optional first display only; the original decoder remains the editing authority.
pub(crate) fn jpeg_preview(
    path: &Path,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
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
    let mut bytes = Vec::new();
    reader
        .into_inner()
        .take(INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(ImageDecodeError::Open)?;
    if bytes.len() as u64 > INPUT_LIMIT {
        return Ok(None);
    }
    let mut header = JpegDecoder::new(std::io::Cursor::new(&bytes))?;
    let (width, height) = header.dimensions();
    let pixels = u64::from(width) * u64::from(height);
    if pixels < 4 * 1024 * 1024 || pixels * 4 > byte_limit as u64 {
        return Ok(None);
    }
    let orientation = header.orientation()?;
    drop(header);
    check_current(current)?;
    let source_size = match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
        _ => (width, height),
    };
    let reduced = reduced_jpeg(&bytes, width, height, source_size != (width, height))
        .map_err(DecodeError::from)?;
    check_current(current)?;
    let mut image = DynamicImage::ImageRgba8(
        image::RgbaImage::from_raw(reduced.width, reduced.height, reduced.rgba)
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
            rgba: image.into_raw(),
        },
    }))
}

fn reduced_jpeg(
    bytes: &[u8],
    width: u32,
    height: u32,
    swap: bool,
) -> Result<PreviewImage, ffmpeg::Error> {
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
        (*context.as_mut_ptr()).lowres = 3;
        (*context.as_mut_ptr()).max_pixels = i64::from(width) * i64::from(height);
    }
    let mut decoder = context.decoder().video()?;
    decoder.send_packet(&ffmpeg::Packet::copy(bytes))?;
    let mut decoded = ffmpeg::frame::Video::empty();
    decoder.receive_frame(&mut decoded)?;
    if decoded.width() != width.div_ceil(8) || decoded.height() != height.div_ceil(8) {
        return Err(ffmpeg::Error::InvalidData);
    }
    let (bound_width, bound_height) = if swap { (160.0, 240.0) } else { (240.0, 160.0) };
    let scale = (bound_width / f64::from(width)).min(bound_height / f64::from(height));
    let target_width = (f64::from(width) * scale).round().max(1.0) as u32;
    let target_height = (f64::from(height) * scale).round().max(1.0) as u32;
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
    Ok(PreviewImage {
        width: target_width,
        height: target_height,
        rgba: pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
            std::fs::write(&path, encoded).expect("EXIF fixture");
            let preview = jpeg_preview(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("preview decode")
                .expect("large JPEG");
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
