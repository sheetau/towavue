use super::*;
use crate::{CachedImagePreview, PreviewImage};

/// Sample only needed scanlines of plain 24-bit BMPs; original decoding remains authoritative.
pub(super) fn bmp_preview(
    mut reader: impl Read + Seek,
    byte_limit: usize,
) -> Result<Option<CachedImagePreview>, ImageDecodeError> {
    let mut header = [0; 54];
    reader
        .read_exact(&mut header)
        .map_err(ImageDecodeError::Open)?;
    let u32_at = |offset| {
        u32::from_le_bytes(
            header[offset..offset + 4]
                .try_into()
                .expect("fixed header field"),
        )
    };
    // BITMAPINFOHEADER + BI_RGB only: no palette, bitfield, profile or alpha interpretation.
    if &header[..2] != b"BM"
        || header[6..10] != [0; 4]
        || u32_at(14) != 40
        || header[26..30] != [1, 0, 24, 0]
        || u32_at(30) != 0
    {
        return Ok(None);
    }
    let width = u32_at(18) as i32;
    let signed_height = u32_at(22) as i32;
    if width <= 0 || signed_height == 0 || signed_height == i32::MIN {
        return Ok(None);
    }
    let width = width as u32;
    let height = signed_height.unsigned_abs();
    let pixels = u64::from(width) * u64::from(height);
    if pixels < 4 * 1024 * 1024 || pixels * 4 > byte_limit as u64 {
        return Ok(None);
    }
    let scale = (240.0 / f64::from(width)).min(160.0 / f64::from(height));
    let target_width = (f64::from(width) * scale).round().max(1.0) as u32;
    let target_height = (f64::from(height) * scale).round().max(1.0) as u32;
    let stride = (u64::from(width) * 3).div_ceil(4) * 4;
    // Bound the speculative row allocation and total scanline reads separately.
    if stride > 1024 * 1024 || stride * u64::from(target_height) > 32 * 1024 * 1024 {
        return Ok(None);
    }
    let offset = u64::from(u32_at(10));
    let file_length = reader
        .seek(SeekFrom::End(0))
        .map_err(ImageDecodeError::Open)?;
    if offset < header.len() as u64 || offset + stride * u64::from(height) > file_length {
        return Ok(None);
    }
    let mut row = vec![0; stride as usize];
    let mut rgba = vec![0; (target_width * target_height * 4) as usize];
    for y in 0..target_height {
        let source_y =
            (u64::from(2 * y + 1) * u64::from(height) / u64::from(2 * target_height)) as u32;
        let file_y = if signed_height > 0 {
            height - 1 - source_y
        } else {
            source_y
        };
        reader
            .seek(SeekFrom::Start(offset + stride * u64::from(file_y)))
            .map_err(ImageDecodeError::Open)?;
        reader
            .read_exact(&mut row)
            .map_err(ImageDecodeError::Open)?;
        for x in 0..target_width {
            let source_x =
                (u64::from(2 * x + 1) * u64::from(width) / u64::from(2 * target_width)) as usize;
            let source = &row[source_x * 3..source_x * 3 + 3];
            let target = ((y * target_width + x) * 4) as usize;
            rgba[target..target + 4].copy_from_slice(&[source[2], source[1], source[0], 255]);
        }
    }
    Ok(Some(CachedImagePreview {
        source_size: (width, height),
        image: PreviewImage {
            width: target_width,
            height: target_height,
            rgba,
        },
    }))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::Cursor;

    use super::*;

    struct Counted<'a> {
        inner: Cursor<&'a [u8]>,
        bytes: &'a Cell<usize>,
    }

    impl Read for Counted<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let count = self.inner.read(bytes)?;
            self.bytes.set(self.bytes.get() + count);
            Ok(count)
        }
    }

    impl Seek for Counted<'_> {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(position)
        }
    }

    fn encoded_bmp(width: u32, height: u32) -> (image::RgbImage, Vec<u8>) {
        let source = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 253) as u8, ((x ^ y) % 255) as u8])
        });
        let mut encoded = Cursor::new(Vec::new());
        source
            .write_to(&mut encoded, ImageFormat::Bmp)
            .expect("owned BMP");
        (source, encoded.into_inner())
    }

    #[test]
    fn bmp_preview_samples_padded_rows_in_both_orders() {
        let path =
            std::env::temp_dir().join(format!("towavue-bmp-preview-{}.data", std::process::id()));
        for (width, height) in [(2571, 1933), (1933, 2571)] {
            let (source, mut encoded) = encoded_bmp(width, height);
            let offset =
                u32::from_le_bytes(encoded[10..14].try_into().expect("pixel offset")) as usize;
            let stride = (width as usize * 3).div_ceil(4) * 4;
            for top_down in [false, true] {
                if top_down {
                    encoded[22..26].copy_from_slice(&(-(height as i32)).to_le_bytes());
                    let bottom_up = encoded[offset..].to_vec();
                    for (y, row) in encoded[offset..].chunks_exact_mut(stride).enumerate() {
                        let from = (height as usize - y - 1) * stride;
                        row.copy_from_slice(&bottom_up[from..from + stride]);
                    }
                }
                let bytes = Cell::new(0);
                let preview = bmp_preview(
                    Counted {
                        inner: Cursor::new(&encoded),
                        bytes: &bytes,
                    },
                    IMAGE_BYTE_LIMIT,
                )
                .expect("sample BMP")
                .expect("large plain BMP");
                assert_eq!(preview.source_size, (width, height));
                let image = preview.image;
                assert!(image.width <= 240 && image.height <= 160);
                assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
                assert_eq!(bytes.get(), 54 + stride * image.height as usize);
                assert!(bytes.get() < encoded.len() / 10);
                for y in 0..image.height {
                    for x in 0..image.width {
                        let sx = ((u64::from(x) * 2 + 1) * u64::from(width)
                            / (u64::from(image.width) * 2)) as u32;
                        let sy = ((u64::from(y) * 2 + 1) * u64::from(height)
                            / (u64::from(image.height) * 2))
                            as u32;
                        let rgb = source.get_pixel(sx, sy).0;
                        let index = ((y * image.width + x) * 4) as usize;
                        assert_eq!(image.rgba[index..index + 4], [rgb[0], rgb[1], rgb[2], 255]);
                    }
                }
                std::fs::write(&path, &encoded).expect("owned extensionless BMP");
                let dispatched = first_image_preview(&path, IMAGE_BYTE_LIMIT, &|| true)
                    .expect("content dispatch")
                    .expect("BMP preview");
                assert_eq!(dispatched.image.rgba, image.rgba);
                let original = decode_image(&path).expect("original BMP");
                assert_eq!(
                    original.frames[0].rgba,
                    DynamicImage::ImageRgb8(source.clone())
                        .into_rgba8()
                        .into_raw()
                );
            }
        }
        std::fs::remove_file(path).expect("remove owned BMP");
    }

    #[test]
    fn bmp_preview_bounds_fallback_and_cancellation_are_explicit() {
        let (_, encoded) = encoded_bmp(2571, 1933);
        for (offset, replacement) in [
            (0, b"XX".as_slice()),
            (6, &[1, 0, 0, 0]),
            (14, &[108, 0, 0, 0]),
            (18, &[0, 0, 0, 0]),
            (18, &[255, 255, 255, 255]),
            (22, &[0, 0, 0, 0]),
            (22, &[0, 0, 0, 128]),
            (26, &[2, 0]),
            (28, &[32, 0]),
            (30, &[1, 0, 0, 0]),
            (10, &[53, 0, 0, 0]),
            (10, &[255, 255, 255, 255]),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset..offset + replacement.len()].copy_from_slice(replacement);
            assert!(
                bmp_preview(Cursor::new(invalid), IMAGE_BYTE_LIMIT)
                    .expect("unsupported skip")
                    .is_none()
            );
        }
        assert!(
            bmp_preview(Cursor::new(&encoded[..encoded.len() - 1]), IMAGE_BYTE_LIMIT)
                .expect("truncated pixels")
                .is_none()
        );
        assert!(bmp_preview(Cursor::new(&encoded[..53]), IMAGE_BYTE_LIMIT).is_err());
        let bytes = Cell::new(0);
        assert!(
            bmp_preview(
                Counted {
                    inner: Cursor::new(&encoded),
                    bytes: &bytes
                },
                1
            )
            .expect("budget skip")
            .is_none()
        );
        assert_eq!(bytes.get(), 54);
        let bytes = Cell::new(0);
        let current = || bytes.get() < 20_000;
        let reader = CancellableReader {
            inner: Counted {
                inner: Cursor::new(&encoded),
                bytes: &bytes,
            },
            is_current: &current,
        };
        assert!(bmp_preview(reader, IMAGE_BYTE_LIMIT).is_err());
        assert!(bytes.get() < 30_000);
        let (_, small) = encoded_bmp(16, 16);
        assert!(
            bmp_preview(Cursor::new(small), IMAGE_BYTE_LIMIT)
                .expect("small skip")
                .is_none()
        );
        let mut wide = encoded[..54].to_vec();
        wide[18..22].copy_from_slice(&400_000u32.to_le_bytes());
        wide[22..26].copy_from_slice(&20u32.to_le_bytes());
        assert!(
            bmp_preview(Cursor::new(wide), IMAGE_BYTE_LIMIT)
                .expect("row cap")
                .is_none()
        );
        let mut many_rows = encoded[..54].to_vec();
        many_rows[18..22].copy_from_slice(&100_000u32.to_le_bytes());
        many_rows[22..26].copy_from_slice(&100_000u32.to_le_bytes());
        assert!(
            bmp_preview(Cursor::new(many_rows), usize::MAX)
                .expect("total scanline cap")
                .is_none()
        );

        let path = std::env::temp_dir().join(format!(
            "towavue-bmp-alpha-fallback-{}.bmp",
            std::process::id()
        ));
        let source = image::RgbaImage::from_fn(2571, 1933, |x, y| {
            image::Rgba([20, 80, 190, ((x + y) % 256) as u8])
        });
        source.save(&path).expect("owned alpha BMP");
        assert!(
            first_image_preview(&path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("unsupported preview")
                .is_none()
        );
        let original = decode_image(&path).expect("original alpha BMP remains supported");
        assert_eq!(original.dimensions(), (2571, 1933));
        assert_eq!(original.frames[0].rgba, source.into_raw());
        std::fs::remove_file(path).expect("remove owned alpha BMP");
    }
}
