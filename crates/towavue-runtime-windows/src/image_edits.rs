use ffmpeg_next::{filter, format::Pixel, frame};
use towavue_core::EditOperation;

use crate::{Cancellation, DecodedImage, DecodedImageFrame};

mod pixel_view;

#[cfg(test)]
thread_local! { static RENDER_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
thread_local! { pub(crate) static GRAPH_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// Compare full rendered content one frame at a time without retaining another animation.
pub fn compare_image_edits(
    source: &DecodedImage,
    current: &[EditOperation],
    saved: &[EditOperation],
    cancel: &Cancellation,
) -> Result<bool, String> {
    if current.iter().chain(saved).any(|operation| {
        matches!(
            operation,
            EditOperation::RotateVideo(_) | EditOperation::ResizeVideo(_)
        )
    }) {
        return Err("Video raster edits cannot be compared as image frames".into());
    }
    let mut current_renderer = ImageEditRenderer::new(current);
    let mut saved_renderer = ImageEditRenderer::new(saved);
    for frame in &source.frames {
        if cancel.is_cancelled() {
            return Err("Image comparison cancelled".into());
        }
        let size = (frame.width, frame.height);
        if output_size(size, current)? != output_size(size, saved)? {
            return Ok(false);
        }
        if let Some(matches) = pixel_view::compare(frame, current, frame, saved, cancel) {
            if !matches? {
                return Ok(false);
            }
            continue;
        }
        let current = if current.is_empty() {
            std::borrow::Cow::Borrowed(frame)
        } else {
            std::borrow::Cow::Owned(current_renderer.render(frame, &|| cancel.is_cancelled())?)
        };
        let saved = if saved.is_empty() {
            std::borrow::Cow::Borrowed(frame)
        } else {
            std::borrow::Cow::Owned(saved_renderer.render(frame, &|| cancel.is_cancelled())?)
        };
        if !equal_frames(&current, &saved, cancel)? {
            return Ok(false);
        }
    }
    Ok(!source.frames.is_empty())
}

/// Reuse the fully rendered current snapshot; only the saved snapshot needs rendering.
pub fn compare_rendered_image_edits(
    source: &DecodedImage,
    current: &DecodedImage,
    saved: &[EditOperation],
    cancel: &Cancellation,
) -> Result<bool, String> {
    if saved.iter().any(|operation| {
        matches!(
            operation,
            EditOperation::RotateVideo(_) | EditOperation::ResizeVideo(_)
        )
    }) {
        return Err("Video raster edits cannot be compared as image frames".into());
    }
    if source.frames.is_empty() || source.frames.len() != current.frames.len() {
        return Ok(false);
    }
    let mut saved_renderer = ImageEditRenderer::new(saved);
    for (source, current) in source.frames.iter().zip(&current.frames) {
        if cancel.is_cancelled() {
            return Err("Image comparison cancelled".into());
        }
        if output_size((source.width, source.height), saved)? != (current.width, current.height) {
            return Ok(false);
        }
        if let Some(matches) = pixel_view::compare(current, &[], source, saved, cancel) {
            if !matches? {
                return Ok(false);
            }
            continue;
        }
        let saved = if saved.is_empty() {
            std::borrow::Cow::Borrowed(source)
        } else {
            std::borrow::Cow::Owned(saved_renderer.render(source, &|| cancel.is_cancelled())?)
        };
        if !equal_frames(current, &saved, cancel)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn equal_frames(
    current: &DecodedImageFrame,
    saved: &DecodedImageFrame,
    cancel: &Cancellation,
) -> Result<bool, String> {
    if current.width != saved.width
        || current.height != saved.height
        || current.delay != saved.delay
        || current.rgba.len() != saved.rgba.len()
    {
        return Ok(false);
    }
    for (current, saved) in current.rgba.chunks(65536).zip(saved.rgba.chunks(65536)) {
        if cancel.is_cancelled() {
            return Err("Image comparison cancelled".into());
        }
        if current != saved {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn render_image_edits(
    source: &DecodedImage,
    operations: &[EditOperation],
    cancel: &Cancellation,
) -> Result<DecodedImage, String> {
    if operations.iter().any(|operation| {
        matches!(
            operation,
            EditOperation::RotateVideo(_) | EditOperation::ResizeVideo(_)
        )
    }) {
        return Err("Video raster edits cannot be applied to image frames".into());
    }
    let mut frames = Vec::with_capacity(source.frames.len());
    let mut retained = 0_u64;
    let mut renderer = ImageEditRenderer::new(operations);
    for frame in &source.frames {
        if cancel.is_cancelled() {
            return Err("Image edit cancelled".into());
        }
        let size = output_size((frame.width, frame.height), operations)?;
        retained += u64::from(size.0) * u64::from(size.1) * 4;
        if retained > 512 * 1024 * 1024 {
            return Err("Resampled image exceeds 512 MiB".into());
        }
        frames.push(renderer.render(frame, &|| cancel.is_cancelled())?);
    }
    Ok(DecodedImage {
        format: source.format,
        frames,
    })
}

pub(crate) fn output_size(
    mut size: (u32, u32),
    operations: &[EditOperation],
) -> Result<(u32, u32), String> {
    if size.0 == 0 || size.1 == 0 || u64::from(size.0) * u64::from(size.1) > 128 * 1024 * 1024 {
        return Err("Invalid source image dimensions".into());
    }
    for operation in operations {
        match *operation {
            EditOperation::Resize(resize) => size = resize.size(),
            EditOperation::RotateImage(rotation) => {
                if rotation.source_size() != size {
                    return Err("Image rotation input dimensions changed".into());
                }
                size = rotation.size();
            }
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

#[cfg(test)]
fn render_frame(
    source: &DecodedImageFrame,
    operations: &[EditOperation],
    cancel: &Cancellation,
) -> Result<DecodedImageFrame, String> {
    render_frame_cancellable(source, operations, &|| cancel.is_cancelled())
}

pub(crate) fn render_frame_cancellable(
    source: &DecodedImageFrame,
    operations: &[EditOperation],
    cancelled: &impl Fn() -> bool,
) -> Result<DecodedImageFrame, String> {
    ImageEditRenderer::new(operations).render(source, cancelled)
}

pub(crate) struct ImageEditRenderer<'a> {
    operations: &'a [EditOperation],
    graph: Option<((u32, u32), filter::Graph)>,
    next_pts: i64,
}

impl<'a> ImageEditRenderer<'a> {
    pub(crate) fn new(operations: &'a [EditOperation]) -> Self {
        Self {
            operations,
            graph: None,
            next_pts: 0,
        }
    }

    pub(crate) fn render(
        &mut self,
        source: &DecodedImageFrame,
        cancelled: &impl Fn() -> bool,
    ) -> Result<DecodedImageFrame, String> {
        let size = (source.width, source.height);
        output_size(size, self.operations)?;
        if cancelled() {
            return Err("Image edit cancelled".into());
        }
        #[cfg(test)]
        RENDER_CALLS.set(RENDER_CALLS.get() + 1);
        if source.width == 0
            || source.height == 0
            || u64::from(source.width) * u64::from(source.height) > 128 * 1024 * 1024
            || source.rgba.len() as u64 != u64::from(source.width) * u64::from(source.height) * 4
        {
            return Err("Invalid source image pixels".into());
        }
        if self.operations.is_empty() {
            let mut rgba = Vec::with_capacity(source.rgba.len());
            for bytes in source.rgba.chunks(65536) {
                if cancelled() {
                    return Err("Image edit cancelled".into());
                }
                rgba.extend_from_slice(bytes);
            }
            return Ok(DecodedImageFrame {
                width: source.width,
                height: source.height,
                rgba,
                delay: source.delay,
            });
        }
        self.render_filtered(source, cancelled)
            .map_err(|error| format!("Could not resample image: {error}"))
    }

    fn render_filtered(
        &mut self,
        source: &DecodedImageFrame,
        cancelled: &impl Fn() -> bool,
    ) -> Result<DecodedImageFrame, ffmpeg_next::Error> {
        let size = (source.width, source.height);
        if self
            .graph
            .as_ref()
            .is_none_or(|(configured, _)| *configured != size)
        {
            ffmpeg_next::init()?;
            let mut graph = filter::Graph::new();
            #[cfg(test)]
            GRAPH_BUILDS.set(GRAPH_BUILDS.get() + 1);
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
            let mut filters = crate::export::visual_filters(self.operations);
            // vflip may expose negative linesize; copy returns an owned positive-stride frame.
            filters.extend(["format=rgba".into(), "copy".into()]);
            graph
                .output("in", 0)?
                .input("out", 0)?
                .parse(&filters.join(","))?;
            graph.validate()?;
            self.graph = Some((size, graph));
        }
        let graph = &mut self.graph.as_mut().expect("configured image graph").1;
        let mut input = frame::Video::new(Pixel::RGBA, source.width, source.height);
        // The spatial filters emit one frame immediately; monotonic PTS keeps
        // reused native filter state distinct without changing the source delays.
        input.set_pts(Some(self.next_pts));
        self.next_pts += 1;
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
            if cancelled() {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_core::{ImageResize, PixelCrop, ResampleFilter};

    #[test]
    fn unedited_frames_copy_exact_pixels_without_native_filters_and_keep_validation() {
        let source = DecodedImage {
            format: "generated",
            frames: [(257, 67), (3, 2)]
                .into_iter()
                .map(|(width, height)| DecodedImageFrame {
                    width,
                    height,
                    rgba: (0..width * height)
                        .flat_map(|pixel| {
                            [
                                (pixel * 11) as u8,
                                (pixel * 7) as u8,
                                (255 - pixel % 256) as u8,
                                (pixel * 13) as u8,
                            ]
                        })
                        .collect(),
                    delay: std::time::Duration::from_nanos(u64::from(width)),
                })
                .collect(),
        };
        GRAPH_BUILDS.set(0);
        let copied = render_image_edits(&source, &[], &Cancellation::default()).expect("copy");
        assert_eq!(copied, source);
        assert_eq!(GRAPH_BUILDS.get(), 0, "unedited RGBA needs no native graph");
        assert!(!std::ptr::eq(
            copied.frames[0].rgba.as_ptr(),
            source.frames[0].rgba.as_ptr()
        ));
        let checks = std::cell::Cell::new(0);
        assert!(
            render_frame_cancellable(&source.frames[0], &[], &|| {
                checks.set(checks.get() + 1);
                checks.get() > 2
            })
            .is_err(),
            "cancel between bounded copy chunks"
        );
        assert_eq!(checks.get(), 3);
        assert!(render_frame_cancellable(&source.frames[0], &[], &|| true).is_err());
        for mutation in 0..4 {
            let mut frame = source.frames[0].clone();
            match mutation {
                0 => {
                    frame.rgba.pop();
                }
                1 => frame.width = 0,
                2 => {
                    frame.width = 128 * 1024 * 1024 + 1;
                    frame.height = 1;
                }
                _ => frame.rgba.push(0),
            }
            assert!(render_frame_cancellable(&frame, &[], &|| false).is_err());
        }
        assert_eq!(GRAPH_BUILDS.get(), 0);
    }

    #[test]
    fn animation_resampling_reuses_graphs_without_changing_frames_or_comparisons() {
        let cancel = Cancellation::default();
        for varying_size in [false, true] {
            let source = DecodedImage {
                format: "generated",
                frames: (0..6)
                    .map(|index| {
                        let (width, height) = if varying_size && (3..5).contains(&index) {
                            (23, 21)
                        } else {
                            (27, 19)
                        };
                        DecodedImageFrame {
                            width,
                            height,
                            rgba: (0..width * height)
                                .flat_map(|pixel| {
                                    [
                                        (pixel * 11 + index * 31) as u8,
                                        (pixel * 7 + index * 17) as u8,
                                        (255 - pixel % 256) as u8,
                                        (pixel * 13 + index * 61) as u8,
                                    ]
                                })
                                .collect(),
                            delay: std::time::Duration::from_millis(u64::from(index * 13)),
                        }
                    })
                    .collect(),
            };
            let builds = if varying_size { 3 } else { 1 };
            for filter in [
                ResampleFilter::Nearest,
                ResampleFilter::Bilinear,
                ResampleFilter::Bicubic,
                ResampleFilter::Lanczos,
            ] {
                let operations = [
                    EditOperation::FlipVertical,
                    EditOperation::Crop(PixelCrop {
                        x: 1,
                        y: 1,
                        width: 20,
                        height: 15,
                    }),
                    EditOperation::Resize(ImageResize::new(17, 13, filter).expect("resize")),
                    EditOperation::RotateImage(
                        towavue_core::ImageRotation::new(137, (17, 13)).expect("rotation"),
                    ),
                    EditOperation::RotateClockwise,
                ];
                let expected: Vec<_> = source
                    .frames
                    .iter()
                    .map(|frame| {
                        render_frame(frame, &operations, &cancel).expect("fresh graph reference")
                    })
                    .collect();
                GRAPH_BUILDS.set(0);
                let rendered =
                    render_image_edits(&source, &operations, &cancel).expect("shared graphs");
                assert_eq!(
                    rendered.frames, expected,
                    "{filter:?}, varying={varying_size}"
                );
                assert_eq!(GRAPH_BUILDS.get(), builds, "one graph per input-size run");
                GRAPH_BUILDS.set(0);
                assert!(
                    compare_rendered_image_edits(&source, &rendered, &operations, &cancel)
                        .expect("saved comparison")
                );
                assert_eq!(GRAPH_BUILDS.get(), builds, "saved-side graph reused");
                GRAPH_BUILDS.set(0);
                assert!(
                    compare_image_edits(&source, &operations, &operations, &cancel)
                        .expect("both comparisons")
                );
                assert_eq!(
                    GRAPH_BUILDS.get(),
                    2 * builds,
                    "independent current/saved graphs"
                );
                let mut different = rendered.clone();
                *different
                    .frames
                    .last_mut()
                    .expect("last frame")
                    .rgba
                    .last_mut()
                    .expect("last byte") ^= 1;
                assert!(
                    !compare_rendered_image_edits(&source, &different, &operations, &cancel)
                        .expect("late difference")
                );
                let mut invalid = source.clone();
                invalid.frames.last_mut().expect("last frame").rgba.pop();
                assert!(render_image_edits(&invalid, &operations, &cancel).is_err());
                let mut renderer = ImageEditRenderer::new(&operations);
                renderer
                    .render(&source.frames[0], &|| false)
                    .expect("prime graph");
                let checks = std::cell::Cell::new(0);
                assert!(
                    renderer
                        .render(&source.frames[1], &|| {
                            checks.set(checks.get() + 1);
                            checks.get() > 3
                        })
                        .is_err(),
                    "cancellation while copying a reused graph's output"
                );
                assert_eq!(
                    renderer
                        .render(&source.frames[2], &|| false)
                        .expect("no stale frame"),
                    expected[2]
                );
            }
        }
    }

    #[test]
    #[ignore = "opt-in Release animation filter-setup comparison using generated RGBA frames"]
    fn animation_filter_reuse_benchmark() {
        let cancel = Cancellation::default();
        for (name, count, width, height, output_width, output_height) in [
            ("small-animation", 96, 320, 240, 240, 180),
            ("hd-animation", 12, 1920, 1080, 1280, 720),
        ] {
            let source = DecodedImage {
                format: "generated",
                frames: (0..count)
                    .map(|index| DecodedImageFrame {
                        width,
                        height,
                        rgba: (0..width * height)
                            .flat_map(|pixel| {
                                [
                                    (pixel * 11 + index * 31) as u8,
                                    (pixel * 7 + index * 17) as u8,
                                    (255 - pixel % 256) as u8,
                                    (pixel * 13 + index * 61) as u8,
                                ]
                            })
                            .collect(),
                        delay: std::time::Duration::from_millis(10 + u64::from(index)),
                    })
                    .collect(),
            };
            let operations = [
                EditOperation::Resize(
                    ImageResize::new(output_width, output_height, ResampleFilter::Lanczos)
                        .expect("resize"),
                ),
                EditOperation::FlipVertical,
            ];
            let expected = render_image_edits(&source, &operations, &cancel).expect("reference");
            let mut times = [Vec::new(), Vec::new()];
            for run in 0..10 {
                for mode in [run % 2, 1 - run % 2] {
                    GRAPH_BUILDS.set(0);
                    let start = std::time::Instant::now();
                    let rendered = if mode == 0 {
                        DecodedImage {
                            format: source.format,
                            frames: source
                                .frames
                                .iter()
                                .map(|frame| {
                                    render_frame(frame, &operations, &cancel).expect("fresh graph")
                                })
                                .collect(),
                        }
                    } else {
                        render_image_edits(&source, &operations, &cancel).expect("shared graph")
                    };
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                    assert_eq!(rendered, expected);
                    assert_eq!(
                        GRAPH_BUILDS.get(),
                        if mode == 0 { count as usize } else { 1 }
                    );
                    if run > 0 {
                        times[mode].push(elapsed);
                    }
                }
            }
            for samples in &mut times {
                samples.sort_by(f64::total_cmp);
            }
            println!(
                "ANIMATION-FILTER case={name} frames={count} source={width}x{height} output={output_width}x{output_height} lanczos_flip runs=9 alternating_order fresh_median_ms={:.3} reused_median_ms={:.3} graphs={count}/1",
                times[0][4], times[1][4]
            );
        }
    }

    #[test]
    #[ignore = "opt-in Release unedited RGBA copy versus the native copy-filter path"]
    fn unedited_frame_copy_benchmark() {
        for (count, width, height) in [(32, 640, 360), (4, 3840, 2160)] {
            let frame = DecodedImageFrame {
                width,
                height,
                rgba: (0..width * height)
                    .flat_map(|pixel| {
                        [
                            (pixel * 11) as u8,
                            (pixel * 7) as u8,
                            (255 - pixel % 256) as u8,
                            (pixel * 13) as u8,
                        ]
                    })
                    .collect(),
                delay: std::time::Duration::from_millis(17),
            };
            let source = vec![frame; count];
            let mut times = [Vec::new(), Vec::new()];
            for run in 0..10 {
                for mode in [run % 2, 1 - run % 2] {
                    let mut renderer = ImageEditRenderer::new(&[]);
                    GRAPH_BUILDS.set(0);
                    let start = std::time::Instant::now();
                    let copied: Vec<_> = source
                        .iter()
                        .map(|frame| {
                            if mode == 0 {
                                renderer
                                    .render_filtered(frame, &|| false)
                                    .expect("native copy filter")
                            } else {
                                renderer
                                    .render(frame, &|| false)
                                    .expect("bounded direct copy")
                            }
                        })
                        .collect();
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                    assert_eq!(copied, source);
                    assert_eq!(GRAPH_BUILDS.get(), usize::from(mode == 0));
                    if run > 0 {
                        times[mode].push(elapsed);
                    }
                }
            }
            for samples in &mut times {
                samples.sort_by(f64::total_cmp);
            }
            println!(
                "UNEDITED-COPY frames={count} size={width}x{height} runs=9 alternating_order filter_median_ms={:.3} direct_median_ms={:.3} graphs=1/0",
                times[0][4], times[1][4]
            );
        }
    }

    #[test]
    fn orthogonal_comparison_reads_source_pixels_without_rendering() {
        let cancel = Cancellation::default();
        let source = DecodedImage {
            format: "test",
            frames: vec![
                DecodedImageFrame {
                    width: 5,
                    height: 3,
                    rgba: [23, 42, 67, 255].repeat(15),
                    delay: std::time::Duration::from_millis(40),
                },
                DecodedImageFrame {
                    width: 5,
                    height: 3,
                    rgba: (0..15_u8)
                        .flat_map(|n| [n * 11, n * 7, 255 - n, n * 13])
                        .collect(),
                    delay: std::time::Duration::from_millis(70),
                },
            ],
        };
        RENDER_CALLS.set(0);
        assert!(
            !compare_image_edits(&source, &[EditOperation::FlipHorizontal], &[], &cancel)
                .expect("compare")
        );
        assert_eq!(
            RENDER_CALLS.get(),
            0,
            "orthogonal comparison must not render either frame"
        );

        let mut cases = vec![vec![]];
        for _ in 0..3 {
            for operations in cases.clone() {
                let size = output_size((5, 3), &operations).expect("dimensions");
                for operation in [
                    EditOperation::RotateClockwise,
                    EditOperation::RotateCounterclockwise,
                    EditOperation::FlipHorizontal,
                    EditOperation::FlipVertical,
                    EditOperation::Crop(PixelCrop {
                        x: u32::from(size.0 > 1),
                        y: u32::from(size.1 > 1),
                        width: size.0.saturating_sub(1).max(1),
                        height: size.1.saturating_sub(1).max(1),
                    }),
                ] {
                    let mut next = operations.clone();
                    next.push(operation);
                    if !cases.contains(&next) {
                        cases.push(next);
                    }
                }
            }
        }
        let rendered: Vec<_> = cases
            .iter()
            .map(|operations| {
                render_image_edits(&source, operations, &cancel).expect("reference render")
            })
            .collect();
        RENDER_CALLS.set(0);
        for (i, current) in cases.iter().enumerate() {
            for (j, saved) in cases.iter().enumerate() {
                let expected = rendered[i] == rendered[j];
                assert_eq!(
                    compare_image_edits(&source, current, saved, &cancel)
                        .expect("source comparison"),
                    expected,
                    "current={current:?}, saved={saved:?}"
                );
                assert_eq!(
                    compare_rendered_image_edits(&source, &rendered[i], saved, &cancel)
                        .expect("rendered comparison"),
                    expected,
                    "rendered current={current:?}, saved={saved:?}"
                );
            }
        }
        assert_eq!(
            RENDER_CALLS.get(),
            0,
            "all crop/flip/quarter-turn comparisons borrow pixels"
        );
        cancel.cancel();
        assert!(compare_image_edits(&source, &[], &[], &cancel).is_err());
        assert!(compare_rendered_image_edits(&source, &rendered[0], &[], &cancel).is_err());
    }

    #[test]
    fn orthogonal_comparison_checks_long_rows_and_rejects_invalid_views() {
        let cancel = Cancellation::default();
        let mut source = DecodedImage {
            format: "test",
            frames: vec![DecodedImageFrame {
                width: 16386,
                height: 2,
                rgba: [23, 42, 67, 255].repeat(16386 * 2),
                delay: std::time::Duration::ZERO,
            }],
        };
        let left = EditOperation::Crop(PixelCrop {
            x: 0,
            y: 0,
            width: 16385,
            height: 2,
        });
        let right = EditOperation::Crop(PixelCrop {
            x: 1,
            y: 0,
            width: 16385,
            height: 2,
        });
        for tail in [false, true] {
            if tail {
                *source.frames[0].rgba.last_mut().expect("last alpha") ^= 1;
            }
            for suffix in [
                vec![],
                vec![EditOperation::FlipHorizontal],
                vec![EditOperation::RotateClockwise],
            ] {
                let current = [vec![left], suffix.clone()].concat();
                let saved = [vec![right], suffix].concat();
                assert_eq!(
                    compare_image_edits(&source, &current, &saved, &cancel).expect("chunked view"),
                    !tail
                );
            }
        }
        for crop in [
            PixelCrop {
                x: u32::MAX,
                y: 0,
                width: 2,
                height: 1,
            },
            PixelCrop {
                x: 0,
                y: 2,
                width: 1,
                height: 1,
            },
            PixelCrop {
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            },
        ] {
            assert!(
                compare_image_edits(&source, &[EditOperation::Crop(crop)], &[], &cancel).is_err()
            );
        }
        source.frames[0].rgba.pop();
        assert!(
            compare_image_edits(&source, &[EditOperation::FlipHorizontal], &[], &cancel).is_err()
        );
        source.frames[0] = DecodedImageFrame {
            width: 67,
            height: 67,
            rgba: [23, 42, 67, 255].repeat(67 * 67),
            delay: std::time::Duration::ZERO,
        };
        for tail in [false, true] {
            if tail {
                *source.frames[0].rgba.last_mut().expect("corner alpha") ^= 1;
                source.frames[0].rgba[(64 * 67 + 65) * 4 + 3] ^= 1;
            }
            for rotation in [
                EditOperation::RotateClockwise,
                EditOperation::RotateCounterclockwise,
            ] {
                for saved in [vec![], vec![EditOperation::FlipVertical]] {
                    RENDER_CALLS.set(0);
                    assert_eq!(
                        compare_image_edits(&source, &[rotation], &saved, &cancel)
                            .expect("partial edge tiles"),
                        !tail
                    );
                    assert_eq!(RENDER_CALLS.get(), 0);
                }
            }
        }
    }

    #[test]
    #[ignore = "opt-in Release comparison against the previous FFmpeg-rendered path"]
    fn orthogonal_comparison_benchmark() {
        let cancel = Cancellation::default();
        for (name, current, saved) in [
            ("flip", vec![EditOperation::FlipHorizontal], vec![]),
            (
                "quarter-turn",
                vec![EditOperation::RotateClockwise],
                vec![EditOperation::RotateCounterclockwise],
            ),
            (
                "quarter-square",
                vec![EditOperation::RotateClockwise],
                vec![],
            ),
            (
                "crop",
                vec![EditOperation::Crop(PixelCrop {
                    x: 0,
                    y: 0,
                    width: 4095,
                    height: 2303,
                })],
                vec![EditOperation::Crop(PixelCrop {
                    x: 1,
                    y: 1,
                    width: 4095,
                    height: 2303,
                })],
            ),
        ] {
            let height = if name == "quarter-square" { 4096 } else { 2304 };
            let source = DecodedImageFrame {
                width: 4096,
                height,
                rgba: [23, 42, 67, 255].repeat(4096 * height as usize),
                delay: std::time::Duration::ZERO,
            };
            let mut times = [Vec::new(), Vec::new()];
            for run in 0..10 {
                for mode in [run % 2, 1 - run % 2] {
                    let start = std::time::Instant::now();
                    let matches = if mode == 0 {
                        let render = |operations: &[EditOperation]| {
                            if operations.is_empty() {
                                std::borrow::Cow::Borrowed(&source)
                            } else {
                                std::borrow::Cow::Owned(
                                    render_frame(&source, operations, &cancel).expect("old render"),
                                )
                            }
                        };
                        equal_frames(&render(&current), &render(&saved), &cancel)
                            .expect("old comparison")
                    } else {
                        pixel_view::compare(&source, &current, &source, &saved, &cancel)
                            .expect("orthogonal view")
                            .expect("new comparison")
                    };
                    assert!(matches);
                    if run > 0 {
                        times[mode].push(start.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            }
            for samples in &mut times {
                samples.sort_by(f64::total_cmp);
            }
            println!(
                "ORTHOGONAL-COMPARE case={name} source=4096x{height} uniform_rgba runs=9 alternating_order old_median_ms={:.3} view_median_ms={:.3}",
                times[0][4], times[1][4]
            );
        }
    }

    #[test]
    fn rendered_comparison_reuses_current_pixels_and_checks_every_frame() {
        let source = DecodedImage {
            format: "test",
            frames: (0..2)
                .map(|index| DecodedImageFrame {
                    width: 4,
                    height: 4,
                    rgba: [23, 42, 67, 255].repeat(16),
                    delay: std::time::Duration::from_millis(40 + index * 10),
                })
                .collect(),
        };
        let cancel = Cancellation::default();
        let operations = [EditOperation::Resize(
            ImageResize::new(4, 4, ResampleFilter::Nearest).expect("resize"),
        )];
        let rendered = render_image_edits(&source, &operations, &cancel).expect("materialized");
        for saved in [&[][..], &operations[..]] {
            RENDER_CALLS.set(0);
            assert!(compare_image_edits(&source, &operations, saved, &cancel).expect("old path"));
            let recomputed = RENDER_CALLS.get();
            RENDER_CALLS.set(0);
            assert!(
                compare_rendered_image_edits(&source, &rendered, saved, &cancel)
                    .expect("shared path")
            );
            assert_eq!(
                recomputed - RENDER_CALLS.get(),
                source.frames.len(),
                "no current-side rendering"
            );
            assert_eq!(RENDER_CALLS.get(), if saved.is_empty() { 0 } else { 2 });
        }
        for change in 0..5 {
            let mut different = rendered.clone();
            match change {
                0 => {
                    *different.frames[1].rgba.last_mut().expect("last byte") ^= 1;
                }
                1 => different.frames[1].delay += std::time::Duration::from_nanos(1),
                2 => different.frames[1].width += 1,
                3 => {
                    different.frames.pop();
                }
                _ => {
                    different.frames[1].rgba.pop();
                }
            }
            assert!(
                !compare_rendered_image_edits(&source, &different, &[], &cancel)
                    .expect("different content")
            );
        }
        let invalid = [EditOperation::Crop(PixelCrop {
            x: 3,
            y: 3,
            width: 4,
            height: 4,
        })];
        assert!(compare_rendered_image_edits(&source, &rendered, &invalid, &cancel).is_err());
        cancel.cancel();
        assert!(compare_rendered_image_edits(&source, &rendered, &[], &cancel).is_err());
    }

    #[test]
    #[ignore = "opt-in Release large-image comparison timing and process-memory observation"]
    fn large_rendered_comparison_benchmark() {
        let mode = std::env::var("TOWAVUE_IMAGE_COMPARE_BENCH").unwrap_or_else(|_| "reuse".into());
        assert!(matches!(mode.as_str(), "reuse" | "recompute"));
        let source = DecodedImage {
            format: "generated",
            frames: vec![DecodedImageFrame {
                width: 4096,
                height: 2304,
                rgba: [23, 42, 67, 255].repeat(4096 * 2304),
                delay: std::time::Duration::ZERO,
            }],
        };
        let operations = [(2048, 1152), (4096, 2304)].map(|(width, height)| {
            EditOperation::Resize(
                ImageResize::new(width, height, ResampleFilter::Nearest).expect("resize"),
            )
        });
        let cancel = Cancellation::default();
        let start = std::time::Instant::now();
        let rendered = render_image_edits(&source, &operations, &cancel).expect("materialize");
        let materialize_ms = start.elapsed().as_secs_f64() * 1000.0;
        RENDER_CALLS.set(0);
        let mut times = Vec::new();
        for _ in 0..9 {
            let start = std::time::Instant::now();
            let matches = if mode == "reuse" {
                compare_rendered_image_edits(&source, &rendered, &[], &cancel)
            } else {
                compare_image_edits(&source, &operations, &[], &cancel)
            };
            assert!(matches.expect("equal uniform pixels"));
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        assert_eq!(RENDER_CALLS.get(), if mode == "reuse" { 0 } else { 9 });
        println!(
            "IMAGE-COMPARE mode={mode} source=4096x2304 runs=9 materialize_ms={materialize_ms:.3} compare_median_ms={:.3} min_ms={:.3} max_ms={:.3} render_calls={} retained_rgba_bytes={}",
            times[4],
            times[0],
            times[8],
            RENDER_CALLS.get(),
            source.retained_bytes() + rendered.retained_bytes()
        );
    }

    #[test]
    fn pixel_comparison_handles_crop_resize_rotation_all_frames_and_cancellation() {
        let uniform = DecodedImageFrame {
            width: 4,
            height: 4,
            rgba: [23, 42, 67, 255].repeat(16),
            delay: std::time::Duration::from_millis(40),
        };
        let varied = DecodedImageFrame {
            width: 4,
            height: 4,
            rgba: (0..16_u8)
                .flat_map(|n| [n * 11, n * 7, 255 - n * 9, n * 13])
                .collect(),
            delay: std::time::Duration::from_millis(80),
        };
        let source = DecodedImage {
            format: "test",
            frames: vec![uniform.clone(), varied],
        };
        let original = source.clone();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("towavue-pixel-equivalence-{unique}"));
        std::fs::create_dir(&root).expect("owned fixture root");
        let source_path = root.join("source.png");
        image::save_buffer(
            &source_path,
            &source.frames[1].rgba,
            4,
            4,
            image::ColorType::Rgba8,
        )
        .expect("PNG source");
        let original_file = std::fs::read(&source_path).expect("source bytes");
        let cancel = Cancellation::default();
        let resize = |size| {
            EditOperation::Resize(
                ImageResize::new(size, size, ResampleFilter::Nearest).expect("resize"),
            )
        };
        let crop = |x, y, size| {
            EditOperation::Crop(PixelCrop {
                x,
                y,
                width: size,
                height: size,
            })
        };
        for (current, saved) in [
            (vec![crop(0, 0, 4)], vec![]),
            (vec![resize(4)], vec![]),
            (vec![crop(1, 0, 3), crop(0, 1, 2)], vec![crop(1, 1, 2)]),
            (
                vec![
                    EditOperation::RotateImage(
                        towavue_core::ImageRotation::new(900, (4, 4)).expect("rotate"),
                    ),
                    EditOperation::RotateImage(
                        towavue_core::ImageRotation::new(-900, (4, 4)).expect("rotate"),
                    ),
                ],
                vec![],
            ),
        ] {
            assert!(compare_image_edits(&source, &current, &saved, &cancel).expect("compare"));
            assert_eq!(
                render_image_edits(&source, &current, &cancel).expect("current"),
                render_image_edits(&source, &saved, &cancel).expect("saved")
            );
            let mut request = crate::ExportRequest {
                source: source_path.clone(),
                target: root.join("saved.png"),
                kind: towavue_core::MediaKind::Image,
                operations: saved,
                hardware_encode: false,
            };
            crate::export_media(&request).expect("saved PNG");
            let baseline = crate::decode_image(&request.target).expect("saved pixels");
            request.target = root.join("current.png");
            request.operations = current;
            crate::export_media(&request).expect("current PNG");
            assert_eq!(
                crate::decode_image(&request.target).expect("current pixels"),
                baseline
            );
        }
        for current in [
            vec![EditOperation::FlipHorizontal],
            vec![resize(2), resize(4)],
            vec![crop(0, 0, 3)],
            vec![EditOperation::RotateImage(
                towavue_core::ImageRotation::new(1, (4, 4)).expect("rotate"),
            )],
        ] {
            assert!(
                !compare_image_edits(&source, &current, &[], &cancel)
                    .expect("different pixels or dimensions")
            );
        }
        assert!(
            compare_image_edits(
                &DecodedImage {
                    format: "test",
                    frames: vec![uniform]
                },
                &[resize(2), resize(4)],
                &[],
                &cancel
            )
            .expect("uniform content restored")
        );
        assert!(compare_image_edits(&source, &[crop(3, 3, 4)], &[], &cancel).is_err());
        cancel.cancel();
        assert!(compare_image_edits(&source, &[], &[], &cancel).is_err());
        assert_eq!(source, original);
        assert_eq!(
            std::fs::read(&source_path).expect("source retained"),
            original_file
        );
        std::fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn clean_reversible_histories_preserve_every_source_pixel() {
        use towavue_core::{EditHistory, MediaKind};
        let source = DecodedImage {
            format: "test",
            frames: vec![DecodedImageFrame {
                width: 3,
                height: 2,
                rgba: (0..6_u8)
                    .flat_map(|n| [n * 30, 255 - n * 20, n * 10, n * 40])
                    .collect(),
                delay: std::time::Duration::from_millis(40),
            }],
        };
        for operations in [
            vec![EditOperation::RotateClockwise; 4],
            vec![EditOperation::FlipHorizontal; 2],
            vec![EditOperation::FlipVertical; 2],
            vec![
                EditOperation::RotateClockwise,
                EditOperation::FlipHorizontal,
                EditOperation::RotateClockwise,
                EditOperation::FlipHorizontal,
            ],
            vec![
                EditOperation::RotateClockwise,
                EditOperation::RotateClockwise,
                EditOperation::FlipHorizontal,
                EditOperation::FlipVertical,
            ],
        ] {
            let mut history = EditHistory::default();
            for operation in operations {
                history.push(operation, MediaKind::Image);
            }
            assert!(!history.is_dirty());
            let result =
                render_image_edits(&source, history.operations(), &Cancellation::default())
                    .expect("reversible edits");
            assert_eq!(result.dimensions(), source.dimensions());
            assert_eq!(result.frames[0].rgba, source.frames[0].rgba);
            assert_eq!(result.frames[0].delay, source.frames[0].delay);
        }
    }

    #[test]
    fn free_rotation_preserves_alpha_delays_exact_quarters_and_rejects_obsolete_geometry() {
        use towavue_core::ImageRotation;
        let source = DecodedImage {
            format: "test",
            frames: vec![DecodedImageFrame {
                width: 9,
                height: 7,
                rgba: (0..63)
                    .flat_map(|index| {
                        if index % 9 < 4 {
                            [255, 0, 0, 0]
                        } else {
                            [0, 0, 255, 255]
                        }
                    })
                    .collect(),
                delay: std::time::Duration::from_millis(123),
            }],
        };
        for angle in [-1799, -450, -1, 1, 450, 899, 1350, 1799] {
            let rotation = ImageRotation::new(angle, (9, 7)).expect("rotation");
            let result = render_image_edits(
                &source,
                &[EditOperation::RotateImage(rotation)],
                &Cancellation::default(),
            )
            .expect("rotate");
            assert_eq!(result.dimensions(), rotation.size());
            assert_eq!(result.frames[0].delay, source.frames[0].delay);
            assert!(
                result.frames[0]
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|rgba| rgba[3] == 0),
                "transparent canvas"
            );
            assert!(
                result.frames[0]
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|rgba| rgba[3] > 0),
                "visible source retained"
            );
            for rgba in result.frames[0]
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|rgba| rgba[3] > 0)
            {
                assert!(
                    rgba[0] <= 1 && rgba[1] <= 1,
                    "hidden red leaked at {angle}: {rgba:?}"
                );
                assert!(rgba[2] >= 250, "visible blue darkened at {angle}: {rgba:?}");
            }
        }
        for (angle, expected) in [
            (0, vec![]),
            (900, vec![EditOperation::RotateClockwise]),
            (-900, vec![EditOperation::RotateCounterclockwise]),
            (
                1800,
                vec![EditOperation::FlipHorizontal, EditOperation::FlipVertical],
            ),
            (
                -1800,
                vec![EditOperation::FlipHorizontal, EditOperation::FlipVertical],
            ),
        ] {
            let actual = render_image_edits(
                &source,
                &[EditOperation::RotateImage(
                    ImageRotation::new(angle, (9, 7)).expect("quarter"),
                )],
                &Cancellation::default(),
            )
            .expect("quarter");
            let reference = render_image_edits(&source, &expected, &Cancellation::default())
                .expect("existing path");
            assert_eq!(actual.dimensions(), reference.dimensions());
            assert_eq!(actual.frames[0].rgba, reference.frames[0].rgba);
        }
        assert!(
            render_image_edits(
                &source,
                &[EditOperation::RotateImage(
                    ImageRotation::new(450, (7, 9)).expect("wrong source size")
                )],
                &Cancellation::default()
            )
            .is_err()
        );
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(
            render_image_edits(
                &source,
                &[EditOperation::RotateImage(
                    ImageRotation::new(450, (9, 7)).expect("angle")
                )],
                &cancel
            )
            .is_err()
        );
    }

    #[test]
    fn free_rotation_materialization_matches_png_export_in_composed_edit_order() {
        use towavue_core::ImageRotation;
        let root =
            std::env::temp_dir().join(format!("towavue-rotation-export-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("owned fixture root");
        let source_path = root.join("source.png");
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
        ::image::save_buffer(&source_path, &rgba, 9, 7, ::image::ColorType::Rgba8)
            .expect("source PNG");
        let original = std::fs::read(&source_path).expect("source bytes");
        let source = crate::decode_image(&source_path).expect("decode source");
        for angle in [
            -1800, -1799, -900, -317, -1, 1, 450, 899, 900, 1350, 1799, 1800,
        ] {
            let rotation = ImageRotation::new(angle, (18, 12)).expect("first rotation");
            let (width, height) = rotation.size();
            let operations = vec![
                EditOperation::Crop(PixelCrop {
                    x: 1,
                    y: 1,
                    width: 7,
                    height: 5,
                }),
                EditOperation::Resize(
                    ImageResize::new(18, 12, ResampleFilter::Lanczos).expect("resize"),
                ),
                EditOperation::RotateImage(rotation),
                EditOperation::FlipHorizontal,
                EditOperation::Crop(PixelCrop {
                    x: 1,
                    y: 1,
                    width: width - 2,
                    height: height - 2,
                }),
                EditOperation::RotateImage(
                    ImageRotation::new(143, (width - 2, height - 2)).expect("second rotation"),
                ),
                EditOperation::FlipVertical,
            ];
            let result = render_image_edits(&source, &operations, &Cancellation::default())
                .expect("materialize");
            let target = root.join(format!("{angle}.png"));
            crate::export_media(&crate::ExportRequest {
                source: source_path.clone(),
                target: target.clone(),
                kind: towavue_core::MediaKind::Image,
                operations,
                hardware_encode: false,
            })
            .expect("export PNG");
            let exported = crate::decode_image(&target).expect("reopen PNG");
            assert_eq!(exported.dimensions(), result.dimensions(), "angle {angle}");
            assert_eq!(
                exported.frames[0].rgba, result.frames[0].rgba,
                "angle {angle}"
            );
            std::fs::remove_file(target).expect("owned output");
        }
        assert_eq!(
            std::fs::read(&source_path).expect("source retained"),
            original
        );
        let guarded_target = root.join("unsupported.bin");
        std::fs::write(&guarded_target, b"keep existing target").expect("owned target");
        for kind in [
            towavue_core::MediaKind::Video,
            towavue_core::MediaKind::Audio,
        ] {
            let result = crate::export_media(&crate::ExportRequest {
                source: source_path.clone(),
                target: guarded_target.clone(),
                kind,
                operations: vec![EditOperation::RotateImage(
                    ImageRotation::new(450, (9, 7)).expect("rotation"),
                )],
                hardware_encode: false,
            });
            assert!(
                matches!(result, Err(crate::ExportError::Failed(message)) if message == "Image raster edits require image media")
            );
            assert_eq!(
                std::fs::read(&guarded_target).expect("target preserved"),
                b"keep existing target"
            );
        }
        std::fs::remove_file(guarded_target).expect("owned target");
        std::fs::remove_file(source_path).expect("owned source");
        std::fs::remove_dir(root).expect("empty owned root");
    }

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
