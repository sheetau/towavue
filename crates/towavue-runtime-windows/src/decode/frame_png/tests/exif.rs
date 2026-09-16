use super::*;
use image::{DynamicImage, Rgb, RgbImage, metadata::Orientation};
use std::path::PathBuf;
use towavue_core::PixelCrop;

fn exif(orientation: u16, little: bool, long_dimensions: bool) -> Vec<u8> {
    let short = |value: u16| {
        if little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
    };
    let long = |value: u32| {
        if little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
    };
    let mut data = if little {
        b"II".to_vec()
    } else {
        b"MM".to_vec()
    };
    data.extend(short(42));
    data.extend(long(8));
    // Top-level dimensions intentionally use the other valid TIFF integer type
    // from the Exif pixel dimensions, covering both SHORT and LONG normalization.
    let width_type = if long_dimensions { 3 } else { 4 };
    let pixel_type = if long_dimensions { 4 } else { 3 };
    let mut directory = |entries: &[(u16, u16, u32, u32)]| {
        data.extend(short(entries.len() as u16));
        for &(tag, kind, count, value) in entries {
            data.extend(short(tag));
            data.extend(short(kind));
            data.extend(long(count));
            if kind == 3 {
                data.extend(short(value as u16));
                data.extend([0, 0]);
            } else {
                data.extend(long(value));
            }
        }
        data.extend(long(0));
    };
    // Five IFD0 entries end at 74; three Exif entries end at 116.
    directory(&[
        (0x0100, width_type, 1, 32),
        (0x0101, width_type, 1, 18),
        (0x0112, 3, 1, u32::from(orientation)),
        (0x013b, 2, 8, 116),
        (0x8769, 4, 1, 74),
    ]);
    directory(&[
        (0xa001, 3, 1, 1),
        (0xa002, pixel_type, 1, 32),
        (0xa003, pixel_type, 1, 18),
    ]);
    data.extend(b"towavue\0");
    data
}

struct Tiff<'a>(&'a [u8]);
impl Tiff<'_> {
    fn short(&self, offset: usize) -> u16 {
        let bytes = self.0[offset..offset + 2].try_into().expect("TIFF short");
        if &self.0[..2] == b"II" {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    }
    fn long(&self, offset: usize) -> u32 {
        let bytes = self.0[offset..offset + 4].try_into().expect("TIFF long");
        if &self.0[..2] == b"II" {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    }
    fn optional_entry(&self, directory: usize, tag: u16) -> Option<usize> {
        let entries: Vec<_> = (0..usize::from(self.short(directory)))
            .map(|index| directory + 2 + 12 * index)
            .filter(|offset| self.short(*offset) == tag)
            .collect();
        assert!(entries.len() <= 1, "no duplicate TIFF tag {tag:04x}");
        entries.first().copied()
    }
    fn entry(&self, directory: usize, tag: u16) -> usize {
        self.optional_entry(directory, tag)
            .unwrap_or_else(|| panic!("missing TIFF tag {tag:04x}"))
    }
    fn integer(&self, directory: usize, tag: u16) -> u32 {
        let offset = self.entry(directory, tag);
        assert_eq!(self.long(offset + 4), 1);
        match self.short(offset + 2) {
            3 => u32::from(self.short(offset + 8)),
            4 | 13 => self.long(offset + 8),
            other => panic!("unexpected TIFF integer type {other}"),
        }
    }
}

fn verify_metadata(png: &[u8], size: (u32, u32)) {
    let blocks = chunks(png, b"eXIf");
    assert_eq!(blocks.len(), 1, "retain descriptive EXIF");
    let tiff = Tiff(&blocks[0]);
    let root = tiff.long(4) as usize;
    if tiff.optional_entry(root, 0x0112).is_some() {
        assert_eq!(
            tiff.integer(root, 0x0112),
            1,
            "orientation is already in the pixels"
        );
    }
    assert_eq!(tiff.integer(root, 0x0100), size.0);
    assert_eq!(tiff.integer(root, 0x0101), size.1);
    let exif = tiff.integer(root, 0x8769) as usize;
    assert_eq!(tiff.integer(exif, 0xa002), size.0);
    assert_eq!(tiff.integer(exif, 0xa003), size.1);
    let artist = tiff.entry(root, 0x013b);
    assert_eq!(tiff.short(artist + 2), 2);
    assert_eq!(tiff.long(artist + 4), 8);
    let start = tiff.long(artist + 8) as usize;
    assert_eq!(&tiff.0[start..start + 8], b"towavue\0");
}

#[test]
fn mjpeg_frame_png_bakes_all_exif_orientations_and_updates_edited_dimensions() {
    let root = std::env::temp_dir().join(format!("towavue-frame-exif-{}", std::process::id()));
    fs::create_dir_all(&root).expect("EXIF fixture directory");
    let source = RgbImage::from_fn(32, 18, |x, y| {
        Rgb([(x * 7 + y * 3) as u8, (y * 11) as u8, (x * 3) as u8])
    });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 98)
        .encode_image(&DynamicImage::ImageRgb8(source))
        .expect("JPEG fixture");
    let naked = root.join("reference.jpg");
    fs::write(&naked, &jpeg).expect("reference JPEG");
    let reference =
        source_video_frame_png(&naked, MediaTime::ZERO, &|| false).expect("reference samples");
    let (info, pixels) = unpack(&reference);
    let reference =
        DynamicImage::ImageRgb8(RgbImage::from_raw(info.width, info.height, pixels).expect("RGB"));
    for index in 0..32 {
        let orientation = (index % 8 + 1) as u16;
        let metadata = exif(orientation, (index / 8) % 2 == 0, index >= 16);
        let mut encoded = jpeg[..2].to_vec();
        encoded.extend([0xff, 0xe1]);
        encoded.extend(((metadata.len() + 8) as u16).to_be_bytes());
        encoded.extend(b"Exif\0\0");
        encoded.extend(metadata);
        encoded.extend_from_slice(&jpeg[2..]);
        fs::write(root.join(format!("frame-{index:02}.jpg")), encoded).expect("oriented JPEG");
    }
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFMPEG_DIR")).join("bin/ffmpeg.exe");
    let video = root.join("orientations.mkv");
    let result = Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-noautorotate",
            "-framerate",
            "1",
            "-i",
        ])
        .arg(root.join("frame-%02d.jpg"))
        .args(["-frames:v", "32", "-c:v", "copy"])
        .arg(&video)
        .output()
        .expect("MJPEG mux");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original = fs::read(&video).expect("source video");
    let modified = fs::metadata(&video)
        .expect("metadata")
        .modified()
        .expect("mtime");
    for index in 0..32 {
        let mut expected = reference.clone();
        expected
            .apply_orientation(Orientation::from_exif((index % 8 + 1) as u8).expect("orientation"));
        for edited in [false, true] {
            let operations = if edited {
                vec![
                    EditOperation::FlipHorizontal,
                    EditOperation::Crop(PixelCrop {
                        x: 1,
                        y: 2,
                        width: 9,
                        height: 11,
                    }),
                    EditOperation::RotateClockwise,
                ]
            } else {
                Vec::new()
            };
            let expected = if edited {
                expected.fliph().crop_imm(1, 2, 9, 11).rotate90()
            } else {
                expected.clone()
            };
            let png = edited_video_frame_png(
                &video,
                MediaTime::from_nanoseconds(index * 1_000_000_000),
                &operations,
                &|| false,
            )
            .expect("oriented frame PNG");
            let (info, actual) = unpack(&png);
            assert_eq!(
                (info.width, info.height),
                (expected.width(), expected.height()),
                "frame {index}, edited {edited}"
            );
            assert_eq!(
                actual,
                expected.to_rgb8().into_raw(),
                "frame {index}, edited {edited}"
            );
            verify_metadata(&png, (info.width, info.height));
        }
    }
    assert_eq!(fs::read(&video).expect("unchanged video"), original);
    assert_eq!(
        fs::metadata(&video)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        modified
    );
    fs::remove_dir_all(root).expect("remove owned fixtures");
}
