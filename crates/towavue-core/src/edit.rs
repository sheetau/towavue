use crate::{MediaKind, MediaTime, PixelCrop};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResampleFilter {
    Nearest,
    Bilinear,
    Bicubic,
    Lanczos,
}

/// A bounded raster edit, distinct from display zoom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageResize {
    width: u32,
    height: u32,
    pub filter: ResampleFilter,
}

impl ImageResize {
    pub fn new(width: u32, height: u32, filter: ResampleFilter) -> Option<Self> {
        (width > 0
            && height > 0
            && width <= 16384
            && height <= 16384
            && u64::from(width) * u64::from(height) <= 128 * 1024 * 1024)
            .then_some(Self {
                width,
                height,
                filter,
            })
    }

    pub fn size(self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// A clockwise raster rotation with a validated enclosing canvas.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageRotation {
    tenths: i16,
    source_size: (u32, u32),
    size: (u32, u32),
}

impl ImageRotation {
    pub fn new(tenths: i16, source_size: (u32, u32)) -> Option<Self> {
        if !(-1800..=1800).contains(&tenths) {
            return None;
        }
        ImageResize::new(source_size.0, source_size.1, ResampleFilter::Nearest)?;
        let size = match tenths {
            -900 | 900 => (source_size.1, source_size.0),
            -1800 | 0 | 1800 => source_size,
            _ => {
                let (sin, cos) = (f64::from(tenths) * std::f64::consts::PI / 1800.0).sin_cos();
                (
                    (f64::from(source_size.0) * cos.abs() + f64::from(source_size.1) * sin.abs())
                        .ceil() as u32,
                    (f64::from(source_size.0) * sin.abs() + f64::from(source_size.1) * cos.abs())
                        .ceil() as u32,
                )
            }
        };
        ImageResize::new(size.0, size.1, ResampleFilter::Nearest)?;
        Some(Self {
            tenths,
            source_size,
            size,
        })
    }

    pub fn tenths(self) -> i16 {
        self.tenths
    }
    pub fn source_size(self) -> (u32, u32) {
        self.source_size
    }
    pub fn size(self) -> (u32, u32) {
        self.size
    }
}

/// A source-time half-open playback interval; no end means natural EOF.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlaybackRange {
    pub start: MediaTime,
    pub end: Option<MediaTime>,
}

impl PlaybackRange {
    pub fn contains(self, position: MediaTime) -> bool {
        position >= self.start && self.end.is_none_or(|end| position < end)
    }

    pub fn play_target(self, position: MediaTime) -> MediaTime {
        if self.contains(position) {
            position
        } else {
            self.start
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditOperation {
    Timeline(crate::TimelineEdit),
    Resize(ImageResize),
    RotateImage(ImageRotation),
    Crop(PixelCrop),
    RotateClockwise,
    RotateCounterclockwise,
    FlipHorizontal,
    FlipVertical,
    SetTrimStart(MediaTime),
    SetTrimEnd(MediaTime),
    SetVolume(f32),
    SetRate(f32),
}

impl EditOperation {
    pub fn applies_to(self, kind: MediaKind) -> bool {
        match self {
            Self::Timeline(_) => matches!(kind, MediaKind::Video | MediaKind::Audio),
            Self::Resize(_) | Self::RotateImage(_) => kind == MediaKind::Image,
            Self::Crop(_)
            | Self::RotateClockwise
            | Self::RotateCounterclockwise
            | Self::FlipHorizontal
            | Self::FlipVertical => matches!(kind, MediaKind::Image | MediaKind::Video),
            Self::SetTrimStart(_) | Self::SetTrimEnd(_) | Self::SetRate(_) => {
                matches!(kind, MediaKind::Video | MediaKind::Audio)
            }
            Self::SetVolume(_) => matches!(kind, MediaKind::Video | MediaKind::Audio),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditState {
    pub quarter_turns: u8,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub trim_start: Option<MediaTime>,
    pub trim_end: Option<MediaTime>,
    pub volume: f32,
    pub rate: f32,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            quarter_turns: 0,
            flip_horizontal: false,
            flip_vertical: false,
            trim_start: None,
            trim_end: None,
            volume: 1.0,
            rate: 1.0,
        }
    }
}

impl EditState {
    pub fn playback_range(&self) -> PlaybackRange {
        PlaybackRange {
            start: self.trim_start.unwrap_or(MediaTime::ZERO),
            end: self.trim_end,
        }
    }

    pub fn from_operations(operations: &[EditOperation]) -> Self {
        let mut state = Self::default();
        for operation in operations {
            match *operation {
                EditOperation::Crop(_)
                | EditOperation::Resize(_)
                | EditOperation::RotateImage(_)
                | EditOperation::Timeline(_) => {}
                EditOperation::RotateClockwise => {
                    state.quarter_turns = (state.quarter_turns + 1) % 4;
                }
                EditOperation::RotateCounterclockwise => {
                    state.quarter_turns = (state.quarter_turns + 3) % 4;
                }
                EditOperation::FlipHorizontal => state.flip_horizontal ^= true,
                EditOperation::FlipVertical => state.flip_vertical ^= true,
                EditOperation::SetTrimStart(time) => state.trim_start = Some(time),
                EditOperation::SetTrimEnd(time) => state.trim_end = Some(time),
                EditOperation::SetVolume(volume) => state.volume = volume.clamp(0.0, 2.0),
                EditOperation::SetRate(rate) => state.rate = rate.clamp(0.25, 4.0),
            }
        }
        state
    }

    pub fn trim_is_valid(&self, duration: Option<MediaTime>) -> bool {
        let start = self.trim_start.unwrap_or(MediaTime::ZERO);
        let end = self.trim_end.or(duration);
        start >= MediaTime::ZERO
            && end.is_none_or(|end| start < end)
            && duration.is_none_or(|duration| {
                start < duration && self.trim_end.is_none_or(|end| end <= duration)
            })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditHistory {
    operations: Vec<EditOperation>,
    cursor: usize,
    saved_cursor: Option<usize>,
}

impl Default for EditHistory {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            cursor: 0,
            saved_cursor: Some(0),
        }
    }
}

impl EditHistory {
    pub fn timeline(&self, source_duration: MediaTime) -> Option<crate::EditTimeline> {
        crate::EditTimeline::from_operations(source_duration, self.operations())
    }

    pub fn push(&mut self, operation: EditOperation, kind: MediaKind) -> bool {
        if matches!(operation, EditOperation::RotateImage(rotation) if rotation.tenths() == 0) {
            return false;
        }
        if !operation.applies_to(kind) {
            return false;
        }
        if self.cursor < self.operations.len() {
            self.operations.truncate(self.cursor);
            if self.saved_cursor.is_some_and(|saved| saved > self.cursor) {
                self.saved_cursor = None;
            }
        }
        self.operations.push(operation);
        self.cursor += 1;
        true
    }

    pub fn undo(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.cursor == self.operations.len() {
            return false;
        }
        self.cursor += 1;
        true
    }

    pub fn operations(&self) -> &[EditOperation] {
        &self.operations[..self.cursor]
    }

    pub fn state(&self) -> EditState {
        EditState::from_operations(self.operations())
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_cursor != Some(self.cursor)
    }

    pub fn mark_saved(&mut self) {
        self.saved_cursor = Some(self.cursor);
    }

    pub fn mark_exported(&mut self, operations: &[EditOperation]) {
        self.saved_cursor = self
            .operations
            .starts_with(operations)
            .then_some(operations.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_rotation_bounds_contain_the_source_and_keep_exact_quarter_turns() {
        for size in [(1, 1), (2, 3), (91, 73), (3000, 4000)] {
            for angle in -1800..=1800 {
                let rotation = ImageRotation::new(angle, size).expect("bounded angle");
                assert_eq!(rotation.tenths(), angle);
                assert_eq!(rotation.source_size(), size);
                if angle % 900 == 0 {
                    assert_eq!(
                        rotation.size(),
                        if angle.abs() == 900 {
                            (size.1, size.0)
                        } else {
                            size
                        }
                    );
                } else {
                    let (sin, cos) = (f64::from(angle) * std::f64::consts::PI / 1800.0).sin_cos();
                    let mut extent = (0.0_f64, 0.0_f64);
                    for x in [-0.5, 0.5] {
                        for y in [-0.5, 0.5] {
                            let (x, y) = (x * f64::from(size.0), y * f64::from(size.1));
                            extent.0 = extent.0.max((x * cos - y * sin).abs() * 2.0);
                            extent.1 = extent.1.max((x * sin + y * cos).abs() * 2.0);
                        }
                    }
                    let out = rotation.size();
                    assert!(f64::from(out.0) >= extent.0 && f64::from(out.0) < extent.0 + 1.0);
                    assert!(f64::from(out.1) >= extent.1 && f64::from(out.1) < extent.1 + 1.0);
                }
            }
        }
        for angle in [-1801, 1801, i16::MIN, i16::MAX] {
            assert!(ImageRotation::new(angle, (100, 100)).is_none());
        }
        for size in [
            (0, 1),
            (1, 0),
            (16385, 1),
            (16384, 16384),
            (u32::MAX, u32::MAX),
        ] {
            assert!(ImageRotation::new(0, size).is_none());
        }
        assert!(
            ImageRotation::new(450, (10000, 10000)).is_none(),
            "expanded area exceeds raster budget"
        );
        assert!(
            ImageRotation::new(450, (16384, 1)).is_none(),
            "ceil pushes the expanded canvas over the area limit"
        );
        assert!(ImageRotation::new(450, (16000, 1)).is_some());
    }

    #[test]
    fn image_rotation_is_undoable_and_identity_does_not_replace_the_redo_branch() {
        let mut history = EditHistory::default();
        let rotation =
            EditOperation::RotateImage(ImageRotation::new(-317, (200, 100)).expect("angle"));
        for kind in [MediaKind::Video, MediaKind::Audio] {
            assert!(!history.push(rotation, kind));
        }
        assert!(history.push(rotation, MediaKind::Image));
        assert!(history.is_dirty());
        history.mark_saved();
        assert!(history.undo());
        let before = history.clone();
        assert!(!history.push(
            EditOperation::RotateImage(ImageRotation::new(0, (200, 100)).expect("identity")),
            MediaKind::Image
        ));
        assert_eq!(history, before);
        assert!(history.redo());
        assert!(!history.is_dirty());
        assert_eq!(history.operations(), &[rotation]);
        assert_eq!(
            history.state(),
            EditState::default(),
            "raster rotation is not a transport setting"
        );
    }

    #[test]
    fn resize_is_bounded_image_only_and_preserves_saved_history() {
        for (width, height) in [
            (0, 1),
            (1, 0),
            (16385, 1),
            (16384, 16384),
            (u32::MAX, u32::MAX),
        ] {
            assert!(ImageResize::new(width, height, ResampleFilter::Lanczos).is_none());
        }
        let operation = EditOperation::Resize(
            ImageResize::new(800, 600, ResampleFilter::Lanczos).expect("valid dimensions"),
        );
        let mut history = EditHistory::default();
        assert!(!history.push(operation, MediaKind::Video));
        assert!(!history.push(operation, MediaKind::Audio));
        assert!(history.push(operation, MediaKind::Image));
        history.mark_saved();
        assert!(history.push(EditOperation::RotateClockwise, MediaKind::Image));
        assert!(history.undo());
        assert_eq!(history.operations(), &[operation]);
        assert!(!history.is_dirty());
        assert!(history.undo());
        assert!(history.is_dirty());
        assert!(history.redo());
        assert!(!history.is_dirty());
    }

    #[test]
    fn trim_playback_keeps_source_time_and_restarts_outside_half_open_range() {
        let range = PlaybackRange {
            start: MediaTime::from_nanoseconds(2_000_000_000),
            end: Some(MediaTime::from_nanoseconds(5_000_000_000)),
        };
        for (nanos, accepted) in [
            (0, false),
            (2_000_000_000, true),
            (4_999_999_999, true),
            (5_000_000_000, false),
            (8_000_000_000, false),
        ] {
            let position = MediaTime::from_nanoseconds(nanos);
            assert_eq!(range.contains(position), accepted);
            assert_eq!(
                range.play_target(position),
                if accepted { position } else { range.start }
            );
        }
        assert!(PlaybackRange::default().contains(MediaTime::from_nanoseconds(i64::MAX)));
    }

    #[test]
    fn trim_validation_includes_implicit_boundaries_and_source_duration() {
        let time = |seconds: i64| MediaTime::from_nanoseconds(seconds * 1_000_000_000);
        for (start, end, known, expected) in [
            (None, None, None, true),
            (Some(-1), None, None, false),
            (None, Some(0), None, false),
            (Some(2), Some(2), None, false),
            (Some(3), Some(2), None, false),
            (Some(2), Some(3), None, true),
            (Some(10), None, Some(10), false),
            (None, Some(11), Some(10), false),
            (None, Some(10), Some(10), true),
            (Some(2), None, Some(10), true),
            (Some(2), Some(5), Some(10), true),
        ] {
            let state = EditState {
                trim_start: start.map(time),
                trim_end: end.map(time),
                ..Default::default()
            };
            assert_eq!(state.trim_is_valid(known.map(time)), expected, "{state:?}");
        }
    }

    #[test]
    fn background_export_marks_only_the_exported_revision_saved() {
        let mut history = EditHistory::default();
        history.push(EditOperation::RotateClockwise, MediaKind::Image);
        let exported = history.operations().to_vec();
        history.push(EditOperation::FlipHorizontal, MediaKind::Image);
        history.mark_exported(&exported);
        assert!(history.is_dirty());
        history.undo();
        assert!(!history.is_dirty());
        history.redo();
        assert!(history.is_dirty());
    }

    #[test]
    fn background_export_does_not_mark_a_replaced_branch_saved() {
        let mut history = EditHistory::default();
        history.push(EditOperation::RotateClockwise, MediaKind::Image);
        let exported = history.operations().to_vec();
        history.undo();
        history.push(EditOperation::FlipVertical, MediaKind::Image);
        history.mark_exported(&exported);
        assert!(history.is_dirty());
        history.undo();
        assert!(history.is_dirty());
    }

    #[test]
    fn undo_redo_and_saved_cursor_track_dirty_state() {
        let mut history = EditHistory::default();
        assert!(!history.is_dirty());
        assert!(history.push(EditOperation::RotateClockwise, MediaKind::Image));
        assert!(history.is_dirty());
        history.mark_saved();
        assert!(!history.is_dirty());
        assert!(history.undo());
        assert!(history.is_dirty());
        assert!(history.redo());
        assert!(!history.is_dirty());
    }

    #[test]
    fn branching_after_undo_discards_redo_and_saved_identity() {
        let mut history = EditHistory::default();
        history.push(EditOperation::RotateClockwise, MediaKind::Image);
        history.push(EditOperation::FlipHorizontal, MediaKind::Image);
        history.mark_saved();
        history.undo();
        history.push(EditOperation::FlipVertical, MediaKind::Image);

        assert!(!history.redo());
        assert!(history.is_dirty());
        assert!(history.state().flip_vertical);
        assert!(!history.state().flip_horizontal);
    }

    #[test]
    fn media_kind_rejects_inapplicable_edits() {
        let mut history = EditHistory::default();

        assert!(!history.push(EditOperation::SetVolume(0.5), MediaKind::Image));
        assert!(!history.is_dirty());
    }
}
