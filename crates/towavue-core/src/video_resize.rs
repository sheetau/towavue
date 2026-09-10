use crate::{ImageResize, ResampleFilter};

/// An ordered video resample with explicit square-pixel output and source identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoResize {
    source_size: (u32, u32),
    source_pixel_aspect: f32,
    output: ImageResize,
}

impl VideoResize {
    pub fn new(
        size: (u32, u32),
        filter: ResampleFilter,
        source_size: (u32, u32),
        source_pixel_aspect: f32,
    ) -> Option<Self> {
        ImageResize::new(source_size.0, source_size.1, filter)?;
        if size.0 < 16
            || size.1 < 16
            || !source_pixel_aspect.is_finite()
            || source_pixel_aspect <= 0.0
            || !size.0.is_multiple_of(2)
            || !size.1.is_multiple_of(2)
        {
            return None;
        }
        Some(Self {
            source_size,
            source_pixel_aspect,
            output: ImageResize::new(size.0, size.1, filter)?,
        })
    }

    pub fn source_size(self) -> (u32, u32) {
        self.source_size
    }
    pub fn source_pixel_aspect(self) -> f32 {
        self.source_pixel_aspect
    }
    pub fn size(self) -> (u32, u32) {
        self.output.size()
    }
    pub fn filter(self) -> ResampleFilter {
        self.output.filter
    }
    pub fn is_identity(self) -> bool {
        self.source_size == self.size() && self.source_pixel_aspect == 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditHistory, EditOperation, MediaKind};

    #[test]
    fn video_resize_checks_even_output_source_and_area_without_rounding() {
        for filter in [
            ResampleFilter::Nearest,
            ResampleFilter::Bilinear,
            ResampleFilter::Bicubic,
            ResampleFilter::Lanczos,
        ] {
            for (size, source, aspect) in [
                ((16, 16), (1, 1), 0.5),
                ((800, 600), (641, 479), 1.5),
                ((16384, 8192), (16, 16), 1.0),
            ] {
                let value = VideoResize::new(size, filter, source, aspect).expect("valid resample");
                assert_eq!(value.size(), size);
                assert_eq!(value.source_size(), source);
                assert_eq!(value.source_pixel_aspect(), aspect);
                assert_eq!(value.filter(), filter);
                assert!(!value.is_identity());
            }
            for size in [
                (0, 2),
                (2, 0),
                (1, 2),
                (2, 1),
                (2, 2),
                (14, 16),
                (16, 14),
                (641, 480),
                (640, 481),
                (16386, 2),
                (16384, 8194),
                (u32::MAX, u32::MAX),
            ] {
                assert!(VideoResize::new(size, filter, (64, 48), 1.0).is_none());
            }
            for source in [(0, 2), (2, 0), (16385, 1), (16384, 8193)] {
                assert!(VideoResize::new((64, 48), filter, source, 1.0).is_none());
            }
            for aspect in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                assert!(VideoResize::new((64, 48), filter, (64, 48), aspect).is_none());
            }
        }
    }

    #[test]
    fn video_resize_identity_preserves_redo_but_sar_changes_are_video_edits() {
        let mut history = EditHistory::default();
        history.push(EditOperation::FlipHorizontal, MediaKind::Video);
        history.mark_saved();
        history.push(EditOperation::FlipVertical, MediaKind::Video);
        history.undo();
        let before = history.clone();
        for filter in [
            ResampleFilter::Nearest,
            ResampleFilter::Bilinear,
            ResampleFilter::Bicubic,
            ResampleFilter::Lanczos,
        ] {
            let value = VideoResize::new((64, 48), filter, (64, 48), 1.0).expect("identity");
            assert!(value.is_identity());
            history.push(EditOperation::ResizeVideo(value), MediaKind::Video);
            assert_eq!(history, before);
        }
        let value =
            VideoResize::new((64, 48), ResampleFilter::Lanczos, (64, 48), 2.0).expect("SAR edit");
        assert!(!value.is_identity());
        for kind in [MediaKind::Image, MediaKind::Audio] {
            history.push(EditOperation::ResizeVideo(value), kind);
            assert_eq!(history, before);
        }
        history.push(EditOperation::ResizeVideo(value), MediaKind::Video);
        assert!(history.is_dirty());
        assert_eq!(
            history.operations().last(),
            Some(&EditOperation::ResizeVideo(value))
        );
        history.undo();
        assert_eq!(history.operations(), before.operations());
        history.redo();
        assert_eq!(
            history.operations().last(),
            Some(&EditOperation::ResizeVideo(value))
        );
    }
}
