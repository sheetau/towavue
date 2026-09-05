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
