use crate::{MediaKind, MediaTime, PixelCrop};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditOperation {
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
    pub fn from_operations(operations: &[EditOperation]) -> Self {
        let mut state = Self::default();
        for operation in operations {
            match *operation {
                EditOperation::Crop(_) => {}
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

    pub fn valid_trim(&self) -> Option<(MediaTime, MediaTime)> {
        match (self.trim_start, self.trim_end) {
            (Some(start), Some(end)) if start < end => Some((start, end)),
            _ => None,
        }
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
    pub fn push(&mut self, operation: EditOperation, kind: MediaKind) -> bool {
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
