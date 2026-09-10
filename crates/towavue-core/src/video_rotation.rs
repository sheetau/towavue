use crate::{ImageResize, ImageRotation, ResampleFilter};

/// Ordered video rotation: square-pixel resampling, rotation, then right/bottom padding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoRotation {
    source_size: (u32, u32),
    source_pixel_aspect: f32,
    rotation: ImageRotation,
    size: (u32, u32),
}

impl VideoRotation {
    pub fn new(tenths: i16, source_size: (u32, u32), source_pixel_aspect: f32) -> Option<Self> {
        ImageResize::new(source_size.0, source_size.1, ResampleFilter::Bilinear)?;
        if !source_pixel_aspect.is_finite() || source_pixel_aspect <= 0.0 {
            return None;
        }
        // Expand the compressed display axis instead of dropping source detail.
        let aspect = f64::from(source_pixel_aspect);
        let square_size = (
            (f64::from(source_size.0) * aspect.max(1.0)).round() as u32,
            (f64::from(source_size.1) / aspect.min(1.0)).round() as u32,
        );
        let rotation = ImageRotation::new(tenths, square_size)?;
        let raster_size = rotation.size();
        let size = (
            raster_size.0.next_multiple_of(2),
            raster_size.1.next_multiple_of(2),
        );
        ImageResize::new(size.0, size.1, ResampleFilter::Bilinear)?;
        Some(Self {
            source_size,
            source_pixel_aspect,
            rotation,
            size,
        })
    }

    pub fn tenths(self) -> i16 {
        self.rotation.tenths()
    }
    pub fn source_size(self) -> (u32, u32) {
        self.source_size
    }
    pub fn source_pixel_aspect(self) -> f32 {
        self.source_pixel_aspect
    }
    pub fn square_size(self) -> (u32, u32) {
        self.rotation.source_size()
    }
    pub fn raster_size(self) -> (u32, u32) {
        self.rotation.size()
    }
    pub fn size(self) -> (u32, u32) {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditHistory, EditOperation, MediaKind};

    #[test]
    fn video_rotation_preserves_display_aspect_and_bounds_every_angle_and_padding() {
        for (size, aspect, square) in [
            ((64, 48), 2.0, (128, 48)),
            ((64, 48), 0.5, (64, 96)),
            ((65, 49), 1.0, (65, 49)),
            ((65, 49), 1.5, (98, 49)),
            ((65, 49), 0.75, (65, 65)),
            ((1, 1), 1.0, (1, 1)),
        ] {
            for angle in -1800..=1800 {
                let value = VideoRotation::new(angle, size, aspect).expect("rotation");
                assert_eq!(value.source_size(), size);
                assert_eq!(value.source_pixel_aspect(), aspect);
                assert_eq!(value.square_size(), square);
                assert_eq!(
                    value.raster_size(),
                    ImageRotation::new(angle, square).expect("geometry").size()
                );
                assert_eq!(value.size().0 % 2, 0);
                assert_eq!(value.size().1 % 2, 0);
                assert!(
                    value.size().0 >= value.raster_size().0
                        && value.size().0 - value.raster_size().0 <= 1
                );
                assert!(
                    value.size().1 >= value.raster_size().1
                        && value.size().1 - value.raster_size().1 <= 1
                );
            }
        }
        for aspect in [
            0.0,
            -1.0,
            f32::NAN,
            f32::INFINITY,
            f32::MIN_POSITIVE,
            f32::MAX,
        ] {
            assert!(VideoRotation::new(317, (64, 48), aspect).is_none());
        }
        for (angle, size, aspect) in [
            (1801, (64, 48), 1.0),
            (-1801, (64, 48), 1.0),
            (0, (0, 1), 1.0),
            (317, (10000, 10000), 1.0),
            (0, (10000, 100), 2.0),
            (0, (100, 10000), 0.5),
        ] {
            assert!(VideoRotation::new(angle, size, aspect).is_none());
        }
        assert_eq!(
            VideoRotation::new(0, (16383, 8192), 1.0)
                .expect("bounded padding")
                .size(),
            (16384, 8192)
        );
    }

    #[test]
    fn video_rotation_history_is_video_only_and_zero_preserves_the_redo_branch() {
        let rotation =
            EditOperation::RotateVideo(VideoRotation::new(317, (64, 48), 2.0).expect("value"));
        let mut history = EditHistory::default();
        for kind in [MediaKind::Image, MediaKind::Audio] {
            assert!(!history.push(rotation, kind));
        }
        assert!(history.push(rotation, MediaKind::Video));
        history.mark_saved();
        assert!(history.undo());
        let before = history.clone();
        assert!(!history.push(
            EditOperation::RotateVideo(VideoRotation::new(0, (64, 48), 2.0).expect("identity")),
            MediaKind::Video
        ));
        assert_eq!(history, before);
        assert!(history.redo());
        assert!(!history.is_dirty());
    }
}
