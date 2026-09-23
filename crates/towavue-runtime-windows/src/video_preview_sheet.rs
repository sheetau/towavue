use super::*;

const COLUMNS: u32 = 4;
const CELLS: u32 = 16;
const CELL_WIDTH: u32 = 240;
const CELL_HEIGHT: u32 = 160;
const WIDTH: u32 = COLUMNS * CELL_WIDTH;
const HEIGHT: u32 = CELLS / COLUMNS * CELL_HEIGHT;
const FILTER: &str = "scale=240:160:force_original_aspect_ratio=decrease:reset_sar=1,format=rgba";

#[cfg(test)]
#[path = "video_preview_sheet_tests.rs"]
mod generation_tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoSheetLayout {
    duration: Duration,
    samples: u32,
    index: u32,
}

impl VideoSheetLayout {
    fn variant(self) -> String {
        format!("video-sheet-v3-{}-{}", self.duration.as_nanos(), self.index)
    }
    pub fn for_position(duration: Duration, position: Duration) -> Option<Self> {
        if duration.is_zero() {
            return None;
        }
        let samples = (duration.as_secs_f64() / 5.0).ceil().max(20.0);
        if samples > f64::from(u32::MAX) {
            return None;
        }
        let samples = samples as u32;
        let cell = ((position.as_secs_f64() / duration.as_secs_f64() * f64::from(samples)).floor()
            as u32)
            .min(samples - 1);
        Some(Self {
            duration,
            samples,
            index: cell / CELLS,
        })
    }

    pub fn index(self) -> u32 {
        self.index
    }

    pub fn duration(self) -> Duration {
        self.duration
    }

    pub fn position(self, slot: u32) -> Option<Duration> {
        if slot >= CELLS {
            return None;
        }
        let cell = self.index * CELLS + slot;
        (cell < self.samples).then(|| {
            self.duration
                .mul_f64((f64::from(cell) + 0.5) / f64::from(self.samples))
        })
    }

    fn slot(self, position: Duration) -> Option<u32> {
        let target = Self::for_position(self.duration, position)?;
        if target.index != self.index {
            return None;
        }
        Some(
            ((position.as_secs_f64() / self.duration.as_secs_f64() * f64::from(self.samples))
                .floor() as u32)
                .min(self.samples - 1)
                % CELLS,
        )
    }

    pub fn sample_position(self, position: Duration) -> Option<Duration> {
        self.position(self.slot(position)?)
    }

    pub fn cell_size(sheet_size: [usize; 2]) -> Option<[usize; 2]> {
        let [width, height] = sheet_size;
        let columns = COLUMNS as usize;
        let rows = (CELLS / COLUMNS) as usize;
        (width > 0
            && height > 0
            && width <= WIDTH as usize
            && height <= HEIGHT as usize
            && width.is_multiple_of(columns)
            && height.is_multiple_of(rows))
        .then_some([width / columns, height / rows])
    }

    pub fn uv(self, position: Duration, sheet_size: [usize; 2]) -> Option<[f32; 4]> {
        let [width, height] = sheet_size.map(|size| size as f32);
        let [cell_width, cell_height] = Self::cell_size(sheet_size)?.map(|size| size as f32);
        let cell = self.slot(position)?;
        let x = (cell % COLUMNS) as f32 * cell_width;
        let y = (cell / COLUMNS) as f32 * cell_height;
        // Sample texel centers so linear filtering cannot bleed an adjacent time cell.
        Some([
            (x + 0.5) / width,
            (y + 0.5) / height,
            (x + cell_width - 0.5) / width,
            (y + cell_height - 0.5) / height,
        ])
    }
}

pub struct VideoPreviewSheet {
    pub layout: VideoSheetLayout,
    pub image: PreviewImage,
}

#[derive(Default)]
struct SheetBuilder {
    image: Option<image::RgbaImage>,
}

impl SheetBuilder {
    fn insert(&mut self, slot: usize, frame: image::RgbaImage) {
        // All cells use the first decoded frame's natural fitted dimensions.
        // This keeps source black pixels intact and requires no padding metadata.
        let sheet = self.image.get_or_insert_with(|| {
            image::RgbaImage::from_pixel(
                frame.width() * COLUMNS,
                frame.height() * (CELLS / COLUMNS),
                image::Rgba([0, 0, 0, 255]),
            )
        });
        let width = sheet.width() / COLUMNS;
        let height = sheet.height() / (CELLS / COLUMNS);
        let frame = if frame.dimensions() == (width, height) {
            frame
        } else {
            // A mid-stream geometry change must still fit its cell without
            // stretching or overwriting a neighboring time sample.
            image::DynamicImage::ImageRgba8(frame)
                .resize(width, height, image::imageops::FilterType::Triangle)
                .into_rgba8()
        };
        image::imageops::replace(
            sheet,
            &frame,
            i64::from(slot as u32 % COLUMNS * width + (width - frame.width()) / 2),
            i64::from(slot as u32 / COLUMNS * height + (height - frame.height()) / 2),
        );
    }

    fn finish(self) -> Result<image::RgbaImage, PreviewError> {
        self.image
            .ok_or_else(|| PreviewError::Generate("Video sheet has no frames".into()))
    }
}

impl PreviewCache {
    pub fn cached_video_sheet(
        &self,
        source: &Path,
        layout: VideoSheetLayout,
    ) -> Result<Option<VideoPreviewSheet>, PreviewError> {
        self.check_cancelled()?;
        let key = cache_key(source, &layout.variant())?;
        let image = self.memory.lock().expect("preview memory").get(&key);
        self.check_cancelled()?;
        Ok(image.map(|image| VideoPreviewSheet { layout, image }))
    }
    pub fn video_sheet(
        &self,
        source: &Path,
        layout: VideoSheetLayout,
    ) -> Result<VideoPreviewSheet, PreviewError> {
        self.check_cancelled()?;
        let key = cache_key(source, &layout.variant())?;
        let image = self.load_or_generate_ready(key.clone(), || {
            let mut sheet = SheetBuilder::default();
            let targets: Vec<_> = (0..CELLS)
                .filter_map(|slot| layout.position(slot))
                .collect();
            let result = crate::decode::preview_video_frames(
                source,
                &targets,
                FILTER,
                &|| {
                    self.cancellation
                        .as_ref()
                        .is_some_and(Cancellation::is_cancelled)
                },
                |slot, frame| {
                    let frame = image::RgbaImage::from_raw(
                        frame.width,
                        frame.height,
                        Arc::unwrap_or_clone(frame.rgba),
                    )
                    .expect("packed preview frame");
                    sheet.insert(slot, frame);
                },
            );
            self.check_cancelled()?;
            if let Err(error) = result {
                crate::diagnostic!(
                    "towavue: shared preview decoder unavailable; using frame fallback: {error}"
                );
                sheet = SheetBuilder::default();
                for (slot, position) in targets.into_iter().enumerate() {
                    self.check_cancelled()?;
                    let png = frame_preview(source, position, FILTER, self.cancellation.as_ref())?;
                    let frame = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?
                        .into_rgba8();
                    sheet.insert(slot, frame);
                }
            }
            self.check_cancelled()?;
            let current = cache_key(source, &layout.variant())?;
            if current != key {
                return Err(PreviewError::Generate(
                    "Source changed during sheet generation".into(),
                ));
            }
            ready_preview_png(sheet.finish()?)
        })?;
        if VideoSheetLayout::cell_size([image.width as usize, image.height as usize]).is_none() {
            return Err(PreviewError::Generate(
                "Invalid video sheet dimensions".into(),
            ));
        }
        Ok(VideoPreviewSheet { layout, image })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_cells_preserve_black_pixels_and_uvs_stop_at_each_texel_center() {
        let layout = VideoSheetLayout::for_position(Duration::from_secs(100), Duration::ZERO)
            .expect("layout");
        assert!(layout.variant().starts_with("video-sheet-v3-"));
        for (width, height) in [(240, 135), (240, 60), (60, 160), (160, 160), (1, 160)] {
            let mut builder = SheetBuilder::default();
            for slot in 0..16 {
                builder.insert(
                    slot,
                    image::RgbaImage::from_pixel(
                        width,
                        height,
                        image::Rgba([slot as u8, 0, 0, 255]),
                    ),
                );
            }
            let sheet = builder.finish().expect("sheet");
            assert_eq!(sheet.dimensions(), (width * 4, height * 4));
            let size = [sheet.width() as usize, sheet.height() as usize];
            assert_eq!(
                VideoSheetLayout::cell_size(size),
                Some([width as usize, height as usize])
            );
            for slot in 0..16 {
                let uv = layout
                    .uv(layout.position(slot).expect("sample"), size)
                    .expect("UV");
                let x = slot % 4 * width;
                let y = slot / 4 * height;
                for (index, expected, dimension) in [
                    (0, x, sheet.width()),
                    (1, y, sheet.height()),
                    (2, x + width - 1, sheet.width()),
                    (3, y + height - 1, sheet.height()),
                ] {
                    assert!((uv[index] * dimension as f32 - (expected as f32 + 0.5)).abs() < 0.001);
                }
                for y in y..y + height {
                    for x in x..x + width {
                        assert_eq!(*sheet.get_pixel(x, y), image::Rgba([slot as u8, 0, 0, 255]));
                    }
                }
            }
        }
        for size in [
            [0, 640],
            [960, 0],
            [959, 640],
            [960, 639],
            [964, 640],
            [960, 644],
        ] {
            assert!(layout.uv(Duration::ZERO, size).is_none());
        }
    }

    #[test]
    fn changing_frame_geometry_stays_inside_its_cell() {
        let mut builder = SheetBuilder::default();
        builder.insert(
            0,
            image::RgbaImage::from_pixel(240, 120, image::Rgba([255, 0, 0, 255])),
        );
        builder.insert(
            1,
            image::RgbaImage::from_pixel(80, 160, image::Rgba([0, 0, 255, 255])),
        );
        let sheet = builder.finish().expect("sheet");
        assert_eq!(sheet.dimensions(), (960, 480));
        assert_eq!(*sheet.get_pixel(239, 119), image::Rgba([255, 0, 0, 255]));
        assert_eq!(*sheet.get_pixel(360, 60), image::Rgba([0, 0, 255, 255]));
        assert_eq!(*sheet.get_pixel(480, 60), image::Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn sheet_layout_bounds_density_edges_and_texel_centers() {
        assert!(VideoSheetLayout::for_position(Duration::ZERO, Duration::ZERO).is_none());
        assert!(VideoSheetLayout::for_position(Duration::MAX, Duration::ZERO).is_none());
        for duration in [
            Duration::from_nanos(1),
            Duration::from_millis(100),
            Duration::from_secs(100),
            Duration::from_secs(101),
            Duration::from_secs(3600),
        ] {
            let first =
                VideoSheetLayout::for_position(duration, Duration::ZERO).expect("first sheet");
            assert_eq!(first.index(), 0);
            assert!(first.position(u32::MAX).is_none());
            assert!(first.duration() / first.samples <= Duration::from_secs(5));
            let last = VideoSheetLayout::for_position(duration, Duration::MAX)
                .expect("clamped last sheet");
            assert_eq!(
                last,
                VideoSheetLayout::for_position(duration, duration).expect("end")
            );
            assert!(last.sample_position(duration).expect("last sample") <= duration);
            for sheet in [first, last] {
                for slot in 0..CELLS {
                    let Some(position) = sheet.position(slot) else {
                        break;
                    };
                    if duration < Duration::from_nanos(20) {
                        continue;
                    }
                    let [left, top, right, bottom] = sheet
                        .uv(position, [WIDTH as usize, HEIGHT as usize])
                        .expect("own cell");
                    assert!(left >= 0.0 && top >= 0.0 && right <= 1.0 && bottom <= 1.0);
                    assert!(left < right && top < bottom);
                    assert!((right - left) < 0.25 && (bottom - top) < 0.25);
                }
            }
            if first != last {
                assert!(
                    first
                        .uv(duration, [WIDTH as usize, HEIGHT as usize])
                        .is_none()
                );
            }
        }
    }

    #[test]
    fn real_sheet_reuses_memory_and_disk_and_matches_selected_cells() {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        let root = std::env::temp_dir().join(format!("towavue-video-sheet-{}", std::process::id()));
        let cache = PreviewCache::new(root.clone()).expect("owned cache");
        let duration = cache.duration(&source).expect("fixture duration");
        for position in [Duration::ZERO, duration] {
            let layout = VideoSheetLayout::for_position(duration, position).expect("layout");
            let start = std::time::Instant::now();
            let sheet = cache
                .video_sheet(&source, layout)
                .expect("real generated sheet");
            eprintln!(
                "video sheet {} cold generation: {:?}",
                layout.index(),
                start.elapsed()
            );
            let width = sheet.image.width;
            let cell_width = width / COLUMNS;
            let cell_height = sheet.image.height / (CELLS / COLUMNS);
            assert_eq!(
                (cell_width, cell_height),
                (240, 144),
                "landscape cells contain no added bands"
            );
            let targets: Vec<_> = (0..CELLS)
                .filter_map(|slot| layout.position(slot))
                .collect();
            let start = std::time::Instant::now();
            crate::decode::preview_video_frames(
                &source,
                &targets,
                FILTER,
                &|| false,
                |slot, frame| {
                    assert_eq!((frame.width, frame.height), (cell_width, cell_height));
                    for y in 0..cell_height {
                        let offset = (((slot as u32 / COLUMNS * cell_height + y) * width
                            + slot as u32 % COLUMNS * cell_width)
                            * 4) as usize;
                        assert_eq!(
                            &sheet.image.rgba[offset..offset + (cell_width * 4) as usize],
                            &frame.rgba[(y * cell_width * 4) as usize
                                ..((y + 1) * cell_width * 4) as usize],
                            "shared decoder cell {slot} row {y}"
                        );
                    }
                },
            )
            .expect("shared preview decoder");
            eprintln!(
                "video sheet {} shared decoder: {:?}",
                layout.index(),
                start.elapsed()
            );
            for slot in [0, 3, 15] {
                let Some(position) = layout.position(slot) else {
                    continue;
                };
                let png = frame_preview(&source, position, FILTER, None)
                    .expect("independent single-frame preview");
                let reference = image::load_from_memory(&png)
                    .expect("reference PNG")
                    .into_rgba8();
                for y in 0..cell_height {
                    let start = (((slot / COLUMNS * cell_height + y) * width
                        + slot % COLUMNS * cell_width)
                        * 4) as usize;
                    assert_eq!(
                        &sheet.image.rgba[start..start + (cell_width * 4) as usize],
                        &reference.as_raw()
                            [(y * cell_width * 4) as usize..((y + 1) * cell_width * 4) as usize]
                    );
                }
            }
            let start = std::time::Instant::now();
            assert_eq!(
                cache
                    .clone()
                    .video_sheet(&source, layout)
                    .expect("shared memory")
                    .image,
                sheet.image
            );
            eprintln!(
                "video sheet {} memory hit: {:?}",
                layout.index(),
                start.elapsed()
            );
            assert_eq!(
                PreviewCache::new(root.clone())
                    .expect("disk cache")
                    .video_sheet(&source, layout)
                    .expect("disk hit")
                    .image,
                sheet.image
            );
            let cancel = Cancellation::default();
            cancel.cancel();
            assert!(matches!(
                cache
                    .clone()
                    .cancellable(cancel)
                    .video_sheet(&source, layout),
                Err(PreviewError::Cancelled)
            ));
        }
        assert_eq!(fs::read_dir(&root).expect("sheet files").count(), 2);
        fs::remove_dir_all(root).expect("remove owned cache");
    }
}
