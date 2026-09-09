use ffmpeg_next::{filter, format::Pixel, frame};
use towavue_core::EditOperation;

use crate::{Cancellation, DecodedImage, DecodedImageFrame};

pub fn render_image_edits(
    source: &DecodedImage,
    operations: &[EditOperation],
    cancel: &Cancellation,
) -> Result<DecodedImage, String> {
    let mut frames = Vec::with_capacity(source.frames.len());
    let mut retained = 0_u64;
    for frame in &source.frames {
        if cancel.is_cancelled() {
            return Err("Image edit cancelled".into());
        }
        let size = output_size((frame.width, frame.height), operations)?;
        retained += u64::from(size.0) * u64::from(size.1) * 4;
        if retained > 512 * 1024 * 1024 {
            return Err("Resampled image exceeds 512 MiB".into());
        }
        frames.push(render_frame(frame, operations, cancel)?);
    }
    Ok(DecodedImage {
        format: source.format,
        frames,
    })
}

fn output_size(mut size: (u32, u32), operations: &[EditOperation]) -> Result<(u32, u32), String> {
    if size.0 == 0 || size.1 == 0 || u64::from(size.0) * u64::from(size.1) > 128 * 1024 * 1024 {
        return Err("Invalid source image dimensions".into());
    }
    for operation in operations {
        match *operation {
            EditOperation::Resize(resize) => size = resize.size(),
            EditOperation::RotateClockwise | EditOperation::RotateCounterclockwise => {
                size = (size.1, size.0)
            }
            EditOperation::Crop(crop) => {
                if crop.width == 0
                    || crop.height == 0
                    || crop
                        .x
                        .checked_add(crop.width)
                        .is_none_or(|right| right > size.0)
                    || crop
                        .y
                        .checked_add(crop.height)
                        .is_none_or(|bottom| bottom > size.1)
                {
                    return Err("Invalid image crop".into());
                }
                size = (crop.width, crop.height);
            }
            _ => {}
        }
        if size.0 == 0 || size.1 == 0 || u64::from(size.0) * u64::from(size.1) > 128 * 1024 * 1024 {
            return Err("Resampled image exceeds 512 MiB".into());
        }
    }
    Ok(size)
}

fn render_frame(
    source: &DecodedImageFrame,
    operations: &[EditOperation],
    cancel: &Cancellation,
) -> Result<DecodedImageFrame, String> {
    let convert = || -> Result<DecodedImageFrame, ffmpeg_next::Error> {
        ffmpeg_next::init()?;
        let mut graph = filter::Graph::new();
        // This worker exclusively owns the unconfigured graph; no filter thread exists yet.
        unsafe {
            (*graph.as_mut_ptr()).nb_threads = 1;
        }
        graph.add(
            &filter::find("buffer").ok_or(ffmpeg_next::Error::FilterNotFound)?,
            "in",
            &format!(
                "video_size={}x{}:pix_fmt=rgba:time_base=1/1:pixel_aspect=1/1",
                source.width, source.height
            ),
        )?;
        graph.add(
            &filter::find("buffersink").ok_or(ffmpeg_next::Error::FilterNotFound)?,
            "out",
            "",
        )?;
        let mut filters = crate::export::visual_filters(operations);
        // vflip may expose negative linesize; copy returns an owned positive-stride frame.
        filters.extend(["format=rgba".into(), "copy".into()]);
        graph
            .output("in", 0)?
            .input("out", 0)?
            .parse(&filters.join(","))?;
        graph.validate()?;
        let mut input = frame::Video::new(Pixel::RGBA, source.width, source.height);
        input.set_pts(Some(0));
        let stride = input.stride(0);
        let width = source.width as usize * 4;
        for (y, row) in source.rgba.chunks_exact(width).enumerate() {
            input.data_mut(0)[y * stride..y * stride + width].copy_from_slice(row);
        }
        graph
            .get("in")
            .expect("image source")
            .source()
            .add(&input)?;
        let mut output = frame::Video::empty();
        graph
            .get("out")
            .expect("image sink")
            .sink()
            .frame(&mut output)?;
        let width = output.width() as usize * 4;
        let mut rgba = Vec::with_capacity(width * output.height() as usize);
        for y in 0..output.height() as usize {
            if cancel.is_cancelled() {
                return Err(ffmpeg_next::Error::Exit);
            }
            let row = y * output.stride(0);
            rgba.extend_from_slice(&output.data(0)[row..row + width]);
        }
        Ok(DecodedImageFrame {
            width: output.width(),
            height: output.height(),
            rgba,
            delay: source.delay,
        })
    };
    if source.width == 0
        || source.height == 0
        || u64::from(source.width) * u64::from(source.height) > 128 * 1024 * 1024
        || source.rgba.len() as u64 != u64::from(source.width) * u64::from(source.height) * 4
    {
        return Err("Invalid source image pixels".into());
    }
    convert().map_err(|error| format!("Could not resample image: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_core::{ImageResize, PixelCrop, ResampleFilter};

    #[test]
    fn resampling_does_not_blend_hidden_transparent_red_into_visible_blue() {
        let source = DecodedImage {
            format: "test",
            frames: vec![DecodedImageFrame {
                width: 9,
                height: 1,
                rgba: (0..9)
                    .flat_map(|x| {
                        if x < 4 {
                            [255, 0, 0, 0]
                        } else {
                            [0, 0, 255, 255]
                        }
                    })
                    .collect(),
                delay: std::time::Duration::ZERO,
            }],
        };
        for filter in [
            ResampleFilter::Bilinear,
            ResampleFilter::Bicubic,
            ResampleFilter::Lanczos,
        ] {
            let result = render_image_edits(
                &source,
                &[EditOperation::Resize(
                    ImageResize::new(25, 3, filter).expect("size"),
                )],
                &Cancellation::default(),
            )
            .expect("transparent resample");
            let blended: Vec<_> = result.frames[0]
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[3] > 0 && pixel[3] < 255)
                .collect();
            assert!(
                !blended.is_empty(),
                "{filter:?} has interpolated edge alpha"
            );
            for pixel in blended {
                assert!(
                    pixel[0] <= 1 && pixel[1] <= 1 && pixel[2] >= 250,
                    "{filter:?}: {pixel:?}"
                );
            }
        }
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(render_image_edits(&source, &[], &cancel).is_err());
        assert!(output_size((u32::MAX, u32::MAX), &[]).is_err());
        assert!(
            output_size(
                (2, 2),
                &[EditOperation::Crop(PixelCrop {
                    x: 1,
                    y: 0,
                    width: 2,
                    height: 2
                })]
            )
            .is_err()
        );
    }

    #[test]
    fn materialized_resize_matches_png_export_for_all_filters_and_edit_order() {
        let root =
            std::env::temp_dir().join(format!("towavue-resample-export-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("owned test directory");
        let path = root.join("source.png");
        let rgba: Vec<u8> = (0..63_u8)
            .flat_map(|index| {
                [
                    index * 3,
                    255 - index * 3,
                    index * 2,
                    [0, 1, 64, 127, 255][usize::from(index) % 5],
                ]
            })
            .collect();
        ::image::save_buffer(&path, &rgba, 9, 7, ::image::ColorType::Rgba8).expect("source PNG");
        let source_bytes = std::fs::read(&path).expect("source bytes");
        let source = crate::decode_image(&path).expect("source decode");
        for (index, filter) in [
            ResampleFilter::Nearest,
            ResampleFilter::Bilinear,
            ResampleFilter::Bicubic,
            ResampleFilter::Lanczos,
        ]
        .into_iter()
        .enumerate()
        {
            let operations = vec![
                EditOperation::Crop(PixelCrop {
                    x: 1,
                    y: 1,
                    width: 7,
                    height: 5,
                }),
                EditOperation::Resize(ImageResize::new(18, 12, filter).expect("first resize")),
                EditOperation::RotateClockwise,
                EditOperation::FlipHorizontal,
                EditOperation::Crop(PixelCrop {
                    x: 1,
                    y: 2,
                    width: 10,
                    height: 14,
                }),
                EditOperation::Resize(ImageResize::new(6, 8, filter).expect("second resize")),
                EditOperation::FlipVertical,
            ];
            let result = render_image_edits(&source, &operations, &Cancellation::default())
                .expect("materialize");
            let target = root.join(format!("output-{index}.png"));
            crate::export_media(&crate::ExportRequest {
                source: path.clone(),
                target: target.clone(),
                kind: towavue_core::MediaKind::Image,
                operations,
                hardware_encode: false,
            })
            .expect("PNG export");
            let exported = crate::decode_image(&target).expect("exported PNG");
            assert_eq!(result.dimensions(), (6, 8));
            assert_eq!(result.frames[0].rgba, exported.frames[0].rgba, "{filter:?}");
            std::fs::remove_file(target).expect("owned output");
        }
        assert_eq!(std::fs::read(&path).expect("source retained"), source_bytes);
        std::fs::remove_file(path).expect("owned source");
        std::fs::remove_dir(root).expect("empty test directory");
    }

    #[test]
    fn resize_keeps_edit_order_frame_delays_and_transparent_color() {
        let source = DecodedImage {
            format: "test",
            frames: vec![DecodedImageFrame {
                width: 2,
                height: 2,
                rgba: vec![
                    255, 0, 0, 0, 0, 255, 0, 255, 0, 0, 255, 127, 255, 255, 255, 255,
                ],
                delay: std::time::Duration::from_millis(123),
            }],
        };
        for filter in [
            ResampleFilter::Nearest,
            ResampleFilter::Bilinear,
            ResampleFilter::Bicubic,
            ResampleFilter::Lanczos,
        ] {
            let operations = [
                EditOperation::Resize(ImageResize::new(4, 4, filter).expect("size")),
                EditOperation::Crop(PixelCrop {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 4,
                }),
                EditOperation::RotateClockwise,
                EditOperation::FlipVertical,
            ];
            let result = render_image_edits(&source, &operations, &Cancellation::default())
                .expect("resample");
            assert_eq!(result.dimensions(), (4, 2));
            assert_eq!(result.frames[0].delay, source.frames[0].delay);
            if filter == ResampleFilter::Nearest {
                assert_eq!(&result.frames[0].rgba[..4], &source.frames[0].rgba[8..12]);
            }
        }
    }
}
