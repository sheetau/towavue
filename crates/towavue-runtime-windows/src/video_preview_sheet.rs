use super::*;

const COLUMNS: u32 = 4;
const CELLS: u32 = 16;
const CELL_WIDTH: u32 = 240;
const CELL_HEIGHT: u32 = 160;
const WIDTH: u32 = COLUMNS * CELL_WIDTH;
const HEIGHT: u32 = CELLS / COLUMNS * CELL_HEIGHT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoSheetLayout {
    duration: Duration,
    samples: u32,
    index: u32,
}

impl VideoSheetLayout {
    fn variant(self) -> String {
        format!("video-sheet-v1-{}-{}", self.duration.as_nanos(), self.index)
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

    pub fn uv(self, position: Duration) -> Option<[f32; 4]> {
        let cell = self.slot(position)?;
        let x = (cell % COLUMNS * CELL_WIDTH) as f32;
        let y = (cell / COLUMNS * CELL_HEIGHT) as f32;
        // Sample texel centers so linear filtering cannot bleed an adjacent time cell.
        Some([
            (x + 0.5) / WIDTH as f32,
            (y + 0.5) / HEIGHT as f32,
            (x + CELL_WIDTH as f32 - 0.5) / WIDTH as f32,
            (y + CELL_HEIGHT as f32 - 0.5) / HEIGHT as f32,
        ])
    }
}

pub struct VideoPreviewSheet {
    pub layout: VideoSheetLayout,
    pub image: PreviewImage,
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
        let image = self.load_or_generate(key.clone(), || {
            let mut sheet = image::RgbaImage::from_pixel(WIDTH, HEIGHT, image::Rgba([0, 0, 0, 255]));
            for slot in 0..CELLS {
                let Some(position) = layout.position(slot) else { break };
                self.check_cancelled()?;
                let png = frame_preview(source, position,
                    "scale=240:160:force_original_aspect_ratio=decrease:reset_sar=1,pad=240:160:(ow-iw)/2:(oh-ih)/2",
                    self.cancellation.as_ref())?;
                let frame = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?.into_rgba8();
                image::imageops::replace(&mut sheet, &frame,
                    i64::from(slot % COLUMNS * CELL_WIDTH), i64::from(slot / COLUMNS * CELL_HEIGHT));
            }
            self.check_cancelled()?;
            let current = cache_key(source, &layout.variant())?;
            if current != key { return Err(PreviewError::Generate("Source changed during sheet generation".into())); }
            let mut png = std::io::Cursor::new(Vec::new());
            sheet.write_to(&mut png, image::ImageFormat::Png)?;
            Ok(png.into_inner())
        })?;
        if image.width != WIDTH || image.height != HEIGHT {
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
                    let [left, top, right, bottom] = sheet.uv(position).expect("own cell");
                    assert!(left >= 0.0 && top >= 0.0 && right <= 1.0 && bottom <= 1.0);
                    assert!(left < right && top < bottom);
                    assert!((right - left) < 0.25 && (bottom - top) < 0.25);
                }
            }
            if first != last {
                assert!(first.uv(duration).is_none());
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
            assert_eq!((sheet.image.width, sheet.image.height), (WIDTH, HEIGHT));
            for slot in [0, 3, 15] {
                let Some(position) = layout.position(slot) else {
                    continue;
                };
                let png = frame_preview(&source, position,
                    "scale=240:160:force_original_aspect_ratio=decrease:reset_sar=1,pad=240:160:(ow-iw)/2:(oh-ih)/2", None)
                    .expect("independent single-frame preview");
                let reference = image::load_from_memory(&png)
                    .expect("reference PNG")
                    .into_rgba8();
                for y in 0..CELL_HEIGHT {
                    let start = (((slot / COLUMNS * CELL_HEIGHT + y) * WIDTH
                        + slot % COLUMNS * CELL_WIDTH)
                        * 4) as usize;
                    assert_eq!(
                        &sheet.image.rgba[start..start + (CELL_WIDTH * 4) as usize],
                        &reference.as_raw()
                            [(y * CELL_WIDTH * 4) as usize..((y + 1) * CELL_WIDTH * 4) as usize]
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
