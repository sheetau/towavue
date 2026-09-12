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
    ResizeVideo(crate::VideoResize),
    RotateImage(ImageRotation),
    RotateVideo(crate::VideoRotation),
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
            Self::RotateVideo(_) | Self::ResizeVideo(_) => kind == MediaKind::Video,
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
                | EditOperation::ResizeVideo(_)
                | EditOperation::RotateImage(_)
                | EditOperation::RotateVideo(_)
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
    saved_operations: Vec<EditOperation>,
    source_duration: Option<MediaTime>,
    timeline_matches_saved: bool,
}

impl Default for EditHistory {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            cursor: 0,
            saved_cursor: Some(0),
            saved_operations: Vec::new(),
            source_duration: None,
            timeline_matches_saved: false,
        }
    }
}

impl EditHistory {
    /// Bind the original source duration, never the edited timeline duration.
    pub fn set_source_duration(&mut self, duration: Option<MediaTime>) {
        let duration = duration.filter(|duration| *duration > MediaTime::ZERO);
        if self.source_duration != duration {
            self.source_duration = duration;
            self.refresh_timeline_equivalence();
        }
    }

    fn refresh_timeline_equivalence(&mut self) {
        self.timeline_matches_saved = false;
        let Some(duration) = self.source_duration else {
            return;
        };
        if !effective_edits_without_timeline(self.operations())
            .eq(effective_edits_without_timeline(&self.saved_operations))
        {
            return;
        }
        if let (Some(current), Some(saved)) = (
            crate::EditTimeline::from_operations(duration, self.operations()),
            crate::EditTimeline::from_operations(duration, &self.saved_operations),
        ) {
            self.timeline_matches_saved = current == saved;
        }
    }

    pub fn timeline(&self, source_duration: MediaTime) -> Option<crate::EditTimeline> {
        crate::EditTimeline::from_operations(source_duration, self.operations())
    }

    pub fn push(&mut self, operation: EditOperation, kind: MediaKind) -> bool {
        if matches!(operation, EditOperation::RotateImage(rotation) if rotation.tenths() == 0)
            || matches!(operation, EditOperation::RotateVideo(rotation) if rotation.tenths() == 0)
            || matches!(operation, EditOperation::ResizeVideo(resize) if resize.is_identity())
        {
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
        self.refresh_timeline_equivalence();
        true
    }

    pub fn undo(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.refresh_timeline_equivalence();
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.cursor == self.operations.len() {
            return false;
        }
        self.cursor += 1;
        self.refresh_timeline_equivalence();
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
            && !self.timeline_matches_saved
            && !effective_edits(self.operations()).eq(effective_edits(&self.saved_operations))
    }

    pub fn mark_saved(&mut self) {
        self.saved_cursor = Some(self.cursor);
        self.saved_operations = self.operations().to_vec();
        self.timeline_matches_saved = false;
    }

    pub fn mark_exported(&mut self, operations: &[EditOperation]) {
        self.saved_operations = operations.to_vec();
        self.saved_cursor = self
            .operations
            .starts_with(operations)
            .then_some(operations.len());
        self.refresh_timeline_equivalence();
    }
}

#[derive(PartialEq)]
enum EffectiveEdit {
    Orientation(u8, bool),
    Operation(EditOperation),
    Playback(PlaybackRange, f32, f32),
}

fn effective_edits(operations: &[EditOperation]) -> impl Iterator<Item = EffectiveEdit> + '_ {
    compare_edits(operations, false)
}

fn effective_edits_without_timeline(
    operations: &[EditOperation],
) -> impl Iterator<Item = EffectiveEdit> + '_ {
    compare_edits(operations, true)
}

fn compare_edits(
    mut operations: &[EditOperation],
    skip_timeline: bool,
) -> impl Iterator<Item = EffectiveEdit> + '_ {
    let state = EditState::from_operations(operations);
    std::iter::from_fn(move || {
        // Global playback settings are applied once; raster/timeline operations remain barriers.
        let (mut turns, mut reflected) = (0_u8, false);
        while let Some((operation, rest)) = operations.split_first() {
            match operation {
                EditOperation::Timeline(_) if skip_timeline => {}
                EditOperation::SetVolume(_)
                | EditOperation::SetRate(_)
                | EditOperation::SetTrimStart(_)
                | EditOperation::SetTrimEnd(_) => {}
                EditOperation::RotateClockwise => turns = (turns + 1) % 4,
                EditOperation::RotateCounterclockwise => turns = (turns + 3) % 4,
                EditOperation::FlipHorizontal => {
                    turns = (4 - turns) % 4;
                    reflected = !reflected;
                }
                EditOperation::FlipVertical => {
                    turns = (6 - turns) % 4;
                    reflected = !reflected;
                }
                _ => {
                    if turns != 0 || reflected {
                        return Some(EffectiveEdit::Orientation(turns, reflected));
                    }
                    operations = rest;
                    return Some(EffectiveEdit::Operation(*operation));
                }
            }
            operations = rest;
        }
        (turns != 0 || reflected).then_some(EffectiveEdit::Orientation(turns, reflected))
    })
    .chain(std::iter::once(EffectiveEdit::Playback(
        if skip_timeline {
            PlaybackRange::default()
        } else {
            state.playback_range()
        },
        state.volume,
        state.rate,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_timeline_volume_matches_original_content_with_known_duration() {
        let time = MediaTime::from_nanoseconds;
        let range = crate::TimeRange::new(time(100), time(300)).expect("range");
        let mut history = EditHistory::default();
        history.push(
            EditOperation::Timeline(crate::TimelineEdit::SetVolume(range, 0.5)),
            MediaKind::Audio,
        );
        history.push(
            EditOperation::Timeline(crate::TimelineEdit::SetVolume(range, 1.0)),
            MediaKind::Audio,
        );
        assert_eq!(
            history.timeline(time(1000)),
            crate::EditTimeline::from_operations(time(1000), &[])
        );
        assert!(history.is_dirty(), "duration is not known yet");
        history.set_source_duration(Some(time(1000)));
        assert!(
            !history.is_dirty(),
            "restored playback content must not remain dirty"
        );
        assert_eq!(history.operations().len(), 2);
        history.undo();
        assert!(history.is_dirty());
        history.redo();
        assert!(!history.is_dirty());
        for duration in [None, Some(time(0)), Some(time(-1)), Some(time(200))] {
            history.set_source_duration(duration);
            assert!(
                history.is_dirty(),
                "unknown/invalid/too-short source duration"
            );
        }
        history.set_source_duration(Some(time(1000)));
        assert!(!history.is_dirty());
    }

    #[test]
    fn timeline_equivalence_tracks_duration_rounding_export_and_branch_changes() {
        let time = MediaTime::from_nanoseconds;
        let range = |start, end| crate::TimeRange::new(time(start), time(end)).expect("range");
        let mut history = EditHistory::default();
        history.set_source_duration(Some(time(1000)));
        history.push(EditOperation::SetTrimEnd(time(1000)), MediaKind::Audio);
        assert!(!history.is_dirty(), "explicit original EOF");
        history.set_source_duration(Some(time(1001)));
        assert!(history.is_dirty(), "one nanosecond was trimmed");
        history.set_source_duration(Some(time(1000)));
        for edit in [
            crate::TimelineEdit::Stretch(range(100, 300), time(400)),
            crate::TimelineEdit::Stretch(range(100, 500), time(200)),
        ] {
            history.push(EditOperation::Timeline(edit), MediaKind::Audio);
        }
        assert!(!history.is_dirty(), "exact stretch restoration");
        history.undo();
        assert!(history.is_dirty());
        history.push(
            EditOperation::Timeline(crate::TimelineEdit::Stretch(range(100, 500), time(201))),
            MediaKind::Audio,
        );
        assert!(history.is_dirty(), "one nanosecond is not equivalent");
        history.undo();
        history.push(
            EditOperation::Timeline(crate::TimelineEdit::Stretch(range(100, 500), time(200))),
            MediaKind::Audio,
        );
        assert!(!history.is_dirty());
        assert!(!history.redo());
        history.mark_saved();
        history.push(
            EditOperation::Timeline(crate::TimelineEdit::Keep(range(100, 800))),
            MediaKind::Audio,
        );
        assert!(history.is_dirty());
        let exported = [
            EditOperation::SetTrimStart(time(100)),
            EditOperation::SetTrimEnd(time(800)),
        ];
        history.mark_exported(&exported);
        assert!(
            !history.is_dirty(),
            "same source segment through a different edit path"
        );
        history.undo();
        assert!(
            history.is_dirty(),
            "saved content is the export, not the current cursor"
        );
        history.redo();
        assert!(!history.is_dirty());
        for edit in [
            crate::TimelineEdit::Delete(range(0, 1)),
            crate::TimelineEdit::SetVolume(range(0, 100), 0.5),
        ] {
            history.push(EditOperation::Timeline(edit), MediaKind::Audio);
            assert!(history.is_dirty());
            history.undo();
            assert!(!history.is_dirty());
        }
        history.mark_exported(&[
            EditOperation::SetTrimStart(time(101)),
            EditOperation::SetTrimEnd(time(801)),
        ]);
        assert!(
            history.is_dirty(),
            "same length but different source samples"
        );
        history.mark_exported(&[EditOperation::Timeline(crate::TimelineEdit::Keep(range(
            0, 2000,
        )))]);
        assert!(
            history.is_dirty(),
            "invalid saved plan cannot establish equivalence"
        );
    }

    #[test]
    fn global_playback_settings_return_to_saved_content_without_erasing_undo() {
        for kind in [MediaKind::Audio, MediaKind::Video] {
            for (changed, restored) in [
                (EditOperation::SetVolume(0.5), EditOperation::SetVolume(1.0)),
                (EditOperation::SetRate(2.0), EditOperation::SetRate(1.0)),
                (
                    EditOperation::SetTrimStart(MediaTime::from_nanoseconds(100)),
                    EditOperation::SetTrimStart(MediaTime::ZERO),
                ),
            ] {
                let mut history = EditHistory::default();
                history.push(changed, kind);
                assert!(history.is_dirty());
                history.push(restored, kind);
                assert!(!history.is_dirty(), "{kind:?}: {restored:?}");
                assert_eq!(history.operations(), &[changed, restored]);
                assert!(history.undo());
                assert!(history.is_dirty());
                assert!(history.redo());
                assert!(!history.is_dirty());
            }
        }
    }

    #[test]
    fn global_playback_equivalence_preserves_export_snapshots_and_edit_boundaries() {
        let time = MediaTime::from_nanoseconds;
        let range = crate::TimeRange::new(time(100), time(200)).expect("range");
        let crop = EditOperation::Crop(PixelCrop {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        });
        let timeline = EditOperation::Timeline(crate::TimelineEdit::SetVolume(range, 0.5));
        let exported = [
            EditOperation::SetVolume(0.5),
            crop,
            timeline,
            EditOperation::SetRate(2.0),
            EditOperation::SetTrimEnd(time(900)),
        ];
        let mut history = EditHistory::default();
        for operation in [
            EditOperation::SetRate(0.5),
            crop,
            EditOperation::SetTrimEnd(time(500)),
            timeline,
            EditOperation::SetVolume(0.2),
        ] {
            history.push(operation, MediaKind::Video);
        }
        history.mark_exported(&exported);
        assert!(
            history.is_dirty(),
            "export snapshot is not the live revision"
        );
        for operation in [
            EditOperation::SetVolume(0.5),
            EditOperation::SetRate(2.0),
            EditOperation::SetTrimEnd(time(900)),
        ] {
            history.push(operation, MediaKind::Video);
        }
        assert!(!history.is_dirty());
        history.undo();
        assert!(history.is_dirty());
        history.push(EditOperation::SetTrimEnd(time(900)), MediaKind::Video);
        assert!(
            !history.is_dirty(),
            "equivalent branch matches the actual export"
        );
        assert!(!history.redo());
        for operation in [
            crop,
            timeline,
            EditOperation::Timeline(crate::TimelineEdit::Stretch(range, time(200))),
        ] {
            history.push(operation, MediaKind::Video);
            assert!(
                history.is_dirty(),
                "raster and timeline operations still matter"
            );
            history.undo();
            assert!(!history.is_dirty());
        }
        history.push(
            EditOperation::SetVolume(f32::from_bits(0.5_f32.to_bits() + 1)),
            MediaKind::Video,
        );
        assert!(history.is_dirty(), "no approximate float equality");
        history.undo();
        history.push(EditOperation::SetVolume(f32::NAN), MediaKind::Video);
        assert!(
            history.is_dirty(),
            "invalid value cannot establish equivalence"
        );
        let mut clamped = EditHistory::default();
        clamped.push(EditOperation::SetVolume(2.0), MediaKind::Audio);
        clamped.mark_saved();
        clamped.push(EditOperation::SetVolume(3.0), MediaKind::Audio);
        assert!(!clamped.is_dirty(), "use the same clamp as playback/export");
    }

    #[test]
    fn reversible_orientations_return_to_saved_content_without_erasing_undo() {
        for length in 0..=6 {
            for mut word in 0..4_usize.pow(length) {
                let mut history = EditHistory::default();
                let mut point = (1, 2);
                for _ in 0..length {
                    let operation = match word % 4 {
                        0 => {
                            point = (-point.1, point.0);
                            EditOperation::RotateClockwise
                        }
                        1 => {
                            point = (point.1, -point.0);
                            EditOperation::RotateCounterclockwise
                        }
                        2 => {
                            point.0 = -point.0;
                            EditOperation::FlipHorizontal
                        }
                        _ => {
                            point.1 = -point.1;
                            EditOperation::FlipVertical
                        }
                    };
                    word /= 4;
                    assert!(history.push(operation, MediaKind::Image));
                }
                assert_eq!(
                    history.is_dirty(),
                    point != (1, 2),
                    "{:?}",
                    history.operations()
                );
                assert_eq!(history.operations().len(), length as usize);
                if length > 0 {
                    assert!(history.undo());
                    assert!(history.redo());
                }
            }
        }
    }

    #[test]
    fn saved_content_survives_branching_but_raster_boundaries_do_not_cancel() {
        let mut history = EditHistory::default();
        history.push(EditOperation::RotateClockwise, MediaKind::Image);
        history.mark_saved();
        history.undo();
        for _ in 0..3 {
            history.push(EditOperation::RotateCounterclockwise, MediaKind::Image);
        }
        assert!(!history.is_dirty());
        assert!(!history.redo());
        history.push(EditOperation::FlipHorizontal, MediaKind::Image);
        assert!(history.is_dirty());
        history.undo();
        assert!(!history.is_dirty());
        let resize =
            EditOperation::Resize(ImageResize::new(2, 3, ResampleFilter::Nearest).expect("resize"));
        let exported = [EditOperation::RotateClockwise, resize];
        history.mark_exported(&exported);
        assert!(history.is_dirty());
        history.push(resize, MediaKind::Image);
        assert!(!history.is_dirty());
        history.push(EditOperation::RotateCounterclockwise, MediaKind::Image);
        assert!(
            history.is_dirty(),
            "rotation cannot cross a raster boundary"
        );
        assert!(
            !effective_edits(&[
                EditOperation::RotateClockwise,
                resize,
                EditOperation::RotateCounterclockwise
            ])
            .eq(effective_edits(&[resize]))
        );
    }

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
