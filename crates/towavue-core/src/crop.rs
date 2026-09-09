use crate::{MediaKind, UnitPoint, UnitRect};

/// Integer crop bounds in the image produced by preceding visual edits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelCrop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelCrop {
    /// Centers a display-aspect preset, then snaps it to the existing media pixel grid.
    pub fn centered_aspect(
        size: (u32, u32),
        kind: MediaKind,
        aspect: (u32, u32),
        pixel_aspect: f32,
    ) -> Option<Self> {
        if size.0 == 0
            || size.1 == 0
            || aspect.0 == 0
            || aspect.1 == 0
            || !pixel_aspect.is_finite()
            || pixel_aspect <= 0.0
        {
            return None;
        }
        let source = f64::from(size.0) * f64::from(pixel_aspect) / f64::from(size.1);
        let target = f64::from(aspect.0) / f64::from(aspect.1);
        let (width, height) = if source > target {
            (target / source, 1.0)
        } else {
            (1.0, source / target)
        };
        Self::from_selection(
            UnitRect {
                min: UnitPoint {
                    x: ((1.0 - width) * 0.5) as f32,
                    y: ((1.0 - height) * 0.5) as f32,
                },
                max: UnitPoint {
                    x: ((1.0 + width) * 0.5) as f32,
                    y: ((1.0 + height) * 0.5) as f32,
                },
            },
            size,
            kind,
        )
    }

    pub fn from_selection(region: UnitRect, size: (u32, u32), kind: MediaKind) -> Option<Self> {
        let step = match kind {
            MediaKind::Image => 1,
            MediaKind::Video => 2,
            MediaKind::Audio => return None,
        };
        let axis = |min: f32, max: f32, dimension: u32| {
            if !min.is_finite() || !max.is_finite() || min < 0.0 || max > 1.0 || min >= max {
                return None;
            }
            let cells = dimension / step;
            if cells == 0 {
                return None;
            }
            let start = ((f64::from(min) * f64::from(dimension) / f64::from(step)).round() as u32)
                .min(cells - 1);
            let end = ((f64::from(max) * f64::from(dimension) / f64::from(step)).round() as u32)
                .clamp(start + 1, cells);
            Some((start * step, (end - start) * step))
        };
        let (x, width) = axis(region.min.x, region.max.x, size.0)?;
        let (y, height) = axis(region.min.y, region.max.y, size.1)?;
        Some(Self {
            x,
            y,
            width,
            height,
        })
    }

    pub fn unit_rect(self, size: (u32, u32)) -> UnitRect {
        UnitRect {
            min: UnitPoint {
                x: self.x as f32 / size.0 as f32,
                y: self.y as f32 / size.1 as f32,
            },
            max: UnitPoint {
                x: (self.x + self.width) as f32 / size.0 as f32,
                y: (self.y + self.height) as f32 / size.1 as f32,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_presets_fit_center_and_snap_on_image_and_video_pixel_grids() {
        for size in [
            (1, 1),
            (7, 9),
            (201, 111),
            (640, 360),
            (4000, 3000),
            (3000, 4000),
            (16384, 16384),
        ] {
            for kind in [MediaKind::Image, MediaKind::Video] {
                let step = if kind == MediaKind::Video { 2 } else { 1 };
                for aspect in [(1, 1), (4, 3), (3, 4), (3, 2), (2, 3), (16, 9), (9, 16)] {
                    for sar in [0.5, 1.0, 1.3333334, 2.0] {
                        let crop = PixelCrop::centered_aspect(size, kind, aspect, sar);
                        if size.0 < step || size.1 < step {
                            assert!(crop.is_none());
                            continue;
                        }
                        let crop = crop.expect("valid aspect");
                        assert!(crop.width >= step && crop.height >= step);
                        assert!(crop.x + crop.width <= size.0 && crop.y + crop.height <= size.1);
                        assert_eq!((crop.x | crop.y | crop.width | crop.height) % step, 0);
                        assert!(
                            (2 * i64::from(crop.x) + i64::from(crop.width) - i64::from(size.0))
                                .abs()
                                <= i64::from(step)
                        );
                        assert!(
                            (2 * i64::from(crop.y) + i64::from(crop.height) - i64::from(size.1))
                                .abs()
                                <= i64::from(step)
                        );
                        let ratio = f64::from(aspect.0) / f64::from(aspect.1) / f64::from(sar);
                        assert!(
                            (f64::from(crop.width) - ratio * f64::from(crop.height)).abs()
                                <= f64::from(step) * (1.0 + ratio),
                            "{size:?} {aspect:?} sar={sar} {crop:?}"
                        );
                        assert!(
                            crop.width == size.0 / step * step
                                || crop.height == size.1 / step * step
                        );
                        assert_eq!(
                            PixelCrop::from_selection(crop.unit_rect(size), size, kind),
                            Some(crop)
                        );
                    }
                }
            }
        }
        assert_eq!(
            PixelCrop::centered_aspect((640, 360), MediaKind::Image, (1, 1), 1.0),
            Some(PixelCrop {
                x: 140,
                y: 0,
                width: 360,
                height: 360
            })
        );
        assert_eq!(
            PixelCrop::centered_aspect((720, 576), MediaKind::Video, (4, 3), 16.0 / 15.0),
            Some(PixelCrop {
                x: 0,
                y: 0,
                width: 720,
                height: 576
            })
        );
        for sar in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                PixelCrop::centered_aspect((640, 360), MediaKind::Image, (1, 1), sar).is_none()
            );
        }
        for (size, aspect) in [
            ((0, 1), (1, 1)),
            ((1, 0), (1, 1)),
            ((10, 10), (0, 1)),
            ((10, 10), (1, 0)),
        ] {
            assert!(PixelCrop::centered_aspect(size, MediaKind::Image, aspect, 1.0).is_none());
        }
        assert!(PixelCrop::centered_aspect((640, 360), MediaKind::Audio, (1, 1), 1.0).is_none());
    }

    #[test]
    fn pixel_crop_snaps_edges_and_keeps_tiny_regions_inside_the_image() {
        let tiny = UnitRect {
            min: UnitPoint { x: 0.99, y: 0.99 },
            max: UnitPoint { x: 1.0, y: 1.0 },
        };
        assert_eq!(
            PixelCrop::from_selection(tiny, (7, 9), MediaKind::Image),
            Some(PixelCrop {
                x: 6,
                y: 8,
                width: 1,
                height: 1
            })
        );
        assert_eq!(
            PixelCrop::from_selection(tiny, (7, 9), MediaKind::Video),
            Some(PixelCrop {
                x: 4,
                y: 6,
                width: 2,
                height: 2
            })
        );
        assert_eq!(
            PixelCrop::from_selection(UnitRect::FULL, (u32::MAX, u32::MAX), MediaKind::Video),
            Some(PixelCrop {
                x: 0,
                y: 0,
                width: u32::MAX - 1,
                height: u32::MAX - 1
            })
        );
    }

    #[test]
    fn pixel_crop_rejects_invalid_selection_or_dimensions() {
        for value in [f32::NAN, f32::INFINITY, -0.1, 1.0] {
            let region = UnitRect {
                min: UnitPoint { x: value, y: 0.0 },
                ..UnitRect::FULL
            };
            assert!(PixelCrop::from_selection(region, (8, 8), MediaKind::Image).is_none());
        }
        for region in [
            UnitRect {
                min: UnitPoint { x: 0.8, y: 0.0 },
                max: UnitPoint { x: 0.2, y: 1.0 },
            },
            UnitRect {
                max: UnitPoint { x: 1.1, y: 1.0 },
                ..UnitRect::FULL
            },
        ] {
            assert!(PixelCrop::from_selection(region, (8, 8), MediaKind::Image).is_none());
        }
        assert!(PixelCrop::from_selection(UnitRect::FULL, (0, 8), MediaKind::Image).is_none());
        assert!(PixelCrop::from_selection(UnitRect::FULL, (1, 8), MediaKind::Video).is_none());
        assert!(PixelCrop::from_selection(UnitRect::FULL, (8, 8), MediaKind::Audio).is_none());
    }

    #[test]
    fn snapped_selection_round_trips_on_supported_texture_sizes() {
        for side in [1, 3, 8, 99, 1001, 16_384] {
            for kind in [MediaKind::Image, MediaKind::Video] {
                for start in 0..100 {
                    let region = UnitRect {
                        min: UnitPoint {
                            x: start as f32 / 100.0,
                            y: 0.0,
                        },
                        max: UnitPoint {
                            x: (start + 1) as f32 / 100.0,
                            y: 1.0,
                        },
                    };
                    if let Some(crop) = PixelCrop::from_selection(region, (side, side), kind) {
                        assert_eq!(
                            PixelCrop::from_selection(
                                crop.unit_rect((side, side)),
                                (side, side),
                                kind
                            ),
                            Some(crop)
                        );
                        assert!(crop.x + crop.width <= side && crop.y + crop.height <= side);
                        assert!(crop.width > 0 && crop.height > 0);
                        if kind == MediaKind::Video {
                            assert_eq!((crop.x | crop.y | crop.width | crop.height) % 2, 0);
                        }
                    }
                }
            }
        }
    }
}
