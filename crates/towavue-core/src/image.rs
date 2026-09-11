#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitPoint {
    pub x: f32,
    pub y: f32,
}

impl UnitPoint {
    pub fn clamped(self) -> Self {
        Self {
            x: self.x.clamp(0.0, 1.0),
            y: self.y.clamp(0.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitRect {
    pub min: UnitPoint,
    pub max: UnitPoint,
}

impl UnitRect {
    pub const FULL: Self = Self {
        min: UnitPoint { x: 0.0, y: 0.0 },
        max: UnitPoint { x: 1.0, y: 1.0 },
    };

    pub fn from_drag(
        start: UnitPoint,
        current: UnitPoint,
        image_size: (u32, u32),
        square: bool,
    ) -> Self {
        let start = start.clamped();
        let mut current = current.clamped();
        if square && image_size.0 > 0 && image_size.1 > 0 {
            let pixel_width = (current.x - start.x).abs() * image_size.0 as f32;
            let pixel_height = (current.y - start.y).abs() * image_size.1 as f32;
            let available_width = if current.x < start.x {
                start.x
            } else {
                1.0 - start.x
            } * image_size.0 as f32;
            let available_height = if current.y < start.y {
                start.y
            } else {
                1.0 - start.y
            } * image_size.1 as f32;
            let side = pixel_width
                .max(pixel_height)
                .min(available_width)
                .min(available_height);
            current.x = start.x + (side / image_size.0 as f32).copysign(current.x - start.x);
            current.y = start.y + (side / image_size.1 as f32).copysign(current.y - start.y);
            current = current.clamped();
        }
        Self {
            min: UnitPoint {
                x: start.x.min(current.x),
                y: start.y.min(current.y),
            },
            max: UnitPoint {
                x: start.x.max(current.x),
                y: start.y.max(current.y),
            },
        }
    }

    pub fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn contains(self, point: UnitPoint) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoomMode {
    Fit,
    Cover,
    Actual,
    Custom(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageViewState {
    pub zoom: ZoomMode,
    pub pan: (f32, f32),
    pub selection: Option<UnitRect>,
}

impl Default for ImageViewState {
    fn default() -> Self {
        Self {
            zoom: ZoomMode::Fit,
            pan: (0.0, 0.0),
            selection: None,
        }
    }
}

impl ImageViewState {
    /// Returns physical display pixels per image pixel; viewport dimensions are physical pixels.
    pub fn scale(self, image_size: (u32, u32), viewport_size: (f32, f32)) -> f32 {
        match self.zoom {
            ZoomMode::Fit => fit_scale(image_size, viewport_size),
            ZoomMode::Cover => {
                if image_size.0 == 0 || image_size.1 == 0 {
                    return 1.0;
                }
                (viewport_size.0 / image_size.0 as f32)
                    .max(viewport_size.1 / image_size.1 as f32)
                    .max(0.0)
            }
            ZoomMode::Actual => 1.0,
            ZoomMode::Custom(scale) => scale.clamp(minimum_zoom(image_size), 64.0),
        }
    }

    pub fn zoom_by(&mut self, factor: f32, image_size: (u32, u32), viewport_size: (f32, f32)) {
        self.zoom = ZoomMode::Custom(
            (self.scale(image_size, viewport_size) * factor).clamp(minimum_zoom(image_size), 64.0),
        );
    }

    pub fn fit(&mut self) {
        self.zoom = ZoomMode::Fit;
        self.pan = (0.0, 0.0);
    }

    pub fn actual_size(&mut self) {
        self.zoom = ZoomMode::Actual;
        self.pan = (0.0, 0.0);
    }

    pub fn cover(&mut self) {
        self.zoom = ZoomMode::Cover;
        self.pan = (0.0, 0.0);
    }
}

fn minimum_zoom(image_size: (u32, u32)) -> f32 {
    (1.0 / image_size.0.max(image_size.1).max(1) as f32).min(0.02)
}

pub fn fit_scale(image_size: (u32, u32), viewport_size: (f32, f32)) -> f32 {
    if image_size.0 == 0 || image_size.1 == 0 {
        return 1.0;
    }
    (viewport_size.0 / image_size.0 as f32)
        .min(viewport_size.1 / image_size.1 as f32)
        .max(0.0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadingAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadingSettings {
    pub page_count: usize,
    pub first_page_count: usize,
    pub axis: ReadingAxis,
    pub reversed: bool,
}

impl Default for ReadingSettings {
    fn default() -> Self {
        Self {
            page_count: 2,
            first_page_count: 2,
            axis: ReadingAxis::Horizontal,
            reversed: false,
        }
    }
}

impl ReadingSettings {
    pub fn increase_pages(&mut self) {
        let full_first_page = self.first_page_count == self.page_count;
        self.page_count = (self.page_count + 1).min(10);
        if full_first_page {
            self.first_page_count = self.page_count;
        }
    }

    pub fn decrease_pages(&mut self) {
        self.page_count = self.page_count.saturating_sub(1).max(2);
        self.first_page_count = self.first_page_count.clamp(1, self.page_count);
    }

    pub fn increase_first_page(&mut self) {
        self.first_page_count = self.first_page_count.saturating_add(1).min(self.page_count);
    }

    pub fn decrease_first_page(&mut self) {
        self.first_page_count = self.first_page_count.saturating_sub(1).max(1);
    }

    /// The fixed, non-overlapping spread containing this image in Shell image order.
    pub fn spread(&self, image_index: usize, image_count: usize) -> std::ops::Range<usize> {
        if image_count == 0 {
            return 0..0;
        }
        let count = self.page_count.clamp(2, 10);
        let first = self.first_page_count.clamp(1, count).min(image_count);
        let index = image_index.min(image_count - 1);
        if index < first {
            return 0..first;
        }
        let start = first + (index - first) / count * count;
        start..start.saturating_add(count).min(image_count)
    }

    pub fn adjacent_spread(
        &self,
        image_index: usize,
        image_count: usize,
        forward: bool,
    ) -> Option<usize> {
        if image_count == 0 {
            return None;
        }
        let current = self.spread(image_index, image_count);
        Some(if forward {
            if current.end == image_count {
                0
            } else {
                current.end
            }
        } else {
            self.spread(
                if current.start == 0 {
                    image_count - 1
                } else {
                    current.start - 1
                },
                image_count,
            )
            .start
        })
    }

    pub fn toggle_axis(&mut self) {
        self.axis = match self.axis {
            ReadingAxis::Horizontal => ReadingAxis::Vertical,
            ReadingAxis::Vertical => ReadingAxis::Horizontal,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_fills_both_axes_and_tracks_resize_without_editing_selection() {
        let mut view = ImageViewState {
            pan: (10.0, -20.0),
            selection: Some(UnitRect::FULL),
            ..Default::default()
        };
        view.cover();
        assert_eq!(view.pan, (0.0, 0.0));
        assert_eq!(view.selection, Some(UnitRect::FULL));
        for size in [(400, 200), (200, 400), (16_384, 512), (1, 1)] {
            for viewport in [(800.0, 600.0), (222.0, 464.0), (0.0, 0.0)] {
                let scale = view.scale(size, viewport);
                let width = size.0 as f32 * scale;
                let height = size.1 as f32 * scale;
                assert!(width >= viewport.0 - 0.001 && height >= viewport.1 - 0.001);
                assert!((width - viewport.0).abs() < 0.001 || (height - viewport.1).abs() < 0.001);
            }
        }
        assert_eq!(view.scale((0, 10), (800.0, 600.0)), 1.0);
        let covered = view.scale((400, 200), (800.0, 600.0));
        view.zoom_by(1.25, (400, 200), (800.0, 600.0));
        assert_eq!(view.zoom, ZoomMode::Custom(covered * 1.25));
        view.fit();
        assert_eq!(view.scale((400, 200), (800.0, 600.0)), 2.0);
    }

    #[test]
    fn fit_keeps_large_images_inside_small_and_reading_viewports() {
        for size in [(512, 16_384), (16_384, 512), (16_384, 16_384)] {
            for viewport in [(464.0, 222.0), (40.0, 16.0), (0.0, 0.0)] {
                let scale = fit_scale(size, viewport);
                assert!(scale.is_finite() && scale >= 0.0);
                assert!(size.0 as f32 * scale <= viewport.0 + 0.001);
                assert!(size.1 as f32 * scale <= viewport.1 + 0.001);
            }
        }
        assert_eq!(fit_scale((0, 10), (100.0, 100.0)), 1.0);
    }

    #[test]
    fn zoom_steps_from_sub_two_percent_fit_without_jumping() {
        let size = (512, 16_384);
        let viewport = (464.0, 222.0);
        let mut view = ImageViewState::default();
        let fitted = view.scale(size, viewport);
        view.zoom_by(1.25, size, viewport);
        assert!((view.scale(size, viewport) - fitted * 1.25).abs() < 0.000001);
        view.zoom_by(0.8, size, viewport);
        assert!((view.scale(size, viewport) - fitted).abs() < 0.000001);
        for _ in 0..200 {
            view.zoom_by(0.8, size, viewport);
        }
        assert_eq!(view.scale(size, viewport) * size.1 as f32, 1.0);
        assert_eq!(view.scale(size, (960.0, 576.0)), view.scale(size, viewport));
        view.zoom_by(f32::MAX, size, viewport);
        assert_eq!(view.scale(size, viewport), 64.0);
        view.zoom_by(0.0, (8, 8), viewport);
        assert_eq!(view.scale((8, 8), viewport), 0.02);
    }

    #[test]
    fn square_drag_is_square_in_image_pixels() {
        let selection = UnitRect::from_drag(
            UnitPoint { x: 0.1, y: 0.1 },
            UnitPoint { x: 0.3, y: 0.3 },
            (1_000, 500),
            true,
        );

        assert!((selection.width() - 0.2).abs() < f32::EPSILON * 2.0);
        assert!((selection.height() - 0.4).abs() < f32::EPSILON * 2.0);
    }

    #[test]
    fn square_drag_stops_at_image_bounds_without_changing_ratio() {
        for size in [(1_000, 500), (500, 1_000)] {
            for start in [UnitPoint { x: 0.9, y: 0.2 }, UnitPoint { x: 0.1, y: 0.8 }] {
                for current in [
                    UnitPoint { x: 1.2, y: 1.2 },
                    UnitPoint { x: -0.2, y: 1.2 },
                    UnitPoint { x: 1.2, y: -0.2 },
                    UnitPoint { x: -0.2, y: -0.2 },
                ] {
                    let selection = UnitRect::from_drag(start, current, size, true);
                    assert!(
                        (selection.width() * size.0 as f32 - selection.height() * size.1 as f32)
                            .abs()
                            < 0.001
                    );
                    assert!(selection.min.x >= 0.0 && selection.min.y >= 0.0);
                    assert!(selection.max.x <= 1.0 && selection.max.y <= 1.0);
                    assert!(selection.contains(start));
                    assert!(selection.min.x == start.x || selection.max.x == start.x);
                    assert!(selection.min.y == start.y || selection.max.y == start.y);
                }
            }
        }
    }

    #[test]
    fn reading_spreads_partition_every_image_and_navigation_is_reversible() {
        for total in 0..35 {
            for count in 2..=10 {
                for first in 1..=count {
                    let settings = ReadingSettings {
                        page_count: count,
                        first_page_count: first,
                        ..Default::default()
                    };
                    let mut covered = Vec::new();
                    let mut start = 0;
                    while start < total {
                        let range = settings.spread(start, total);
                        assert_eq!(range.start, start);
                        for index in range.clone() {
                            assert_eq!(settings.spread(index, total), range);
                        }
                        covered.extend(range.clone());
                        let next = settings
                            .adjacent_spread(start, total, true)
                            .expect("next spread");
                        assert_eq!(settings.adjacent_spread(next, total, false), Some(start));
                        start = range.end;
                    }
                    assert_eq!(covered, (0..total).collect::<Vec<_>>());
                    if total == 0 {
                        assert_eq!(settings.spread(usize::MAX, total), 0..0);
                        assert_eq!(settings.adjacent_spread(0, total, true), None);
                    }
                }
            }
        }
        let mut settings = ReadingSettings::default();
        settings.decrease_first_page();
        assert_eq!(settings.spread(0, 6), 0..1);
        assert_eq!(settings.spread(2, 6), 1..3);
        assert_eq!(settings.spread(5, 6), 5..6);
        settings.increase_pages();
        assert_eq!(settings.first_page_count, 1);
        settings.increase_first_page();
        assert_eq!(settings.first_page_count, 2);
        settings.decrease_pages();
        settings.increase_pages();
        assert_eq!(settings.first_page_count, 3);
        assert_eq!(settings.spread(usize::MAX, usize::MAX).end, usize::MAX);
    }

    #[test]
    fn reading_page_count_stays_within_m5_bounds() {
        let mut settings = ReadingSettings::default();
        for _ in 0..20 {
            settings.increase_pages();
        }
        assert_eq!(settings.page_count, 10);
        for _ in 0..20 {
            settings.decrease_pages();
        }
        assert_eq!(settings.page_count, 2);
    }
}
