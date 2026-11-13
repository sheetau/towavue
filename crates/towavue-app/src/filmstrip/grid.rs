use super::*;

/// Filmstrip's original thumbnail diameter and horizontal pitch, arranged in
/// rows inside Gallery's side gutters. Reserve room for four caption lines.
pub(super) const ROW_GAP: f32 = 108.0;

/// Captions and late preview sizes never change the scroll extent.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Grid {
    pub columns: usize,
    pub cell: f32,
    pub left: f32,
    pub viewport: f32,
}

impl Grid {
    pub fn new(screen: Rect, inset: Rect) -> Self {
        let side = gallery_side_inset(screen.width());
        let width = (screen.width() - 2.0 * side).max(1.0);
        let cell = layout::DIAMETER.min(width);
        let rows = (inset.height() / (cell + ROW_GAP)).ceil() as usize + 2;
        let columns = (((width + layout::GAP) / (cell + layout::GAP)).floor() as usize)
            .max(1)
            .min((VISIBLE_PREVIEW_LIMIT / rows.max(1)).max(1));
        let used = columns as f32 * cell + columns.saturating_sub(1) as f32 * layout::GAP;
        Self {
            columns,
            cell,
            left: screen.left() + side + (width - used) * 0.5 - inset.left(),
            viewport: inset.height(),
        }
    }

    pub fn pitch(self) -> f32 {
        self.cell + ROW_GAP
    }

    pub fn padding(self) -> f32 {
        ROW_GAP
    }

    pub fn height(self, count: usize) -> f32 {
        2.0 * self.padding()
            + self.cell
            + count.div_ceil(self.columns).saturating_sub(1) as f32 * self.pitch()
    }

    pub fn offset(self, index: usize, count: usize) -> f32 {
        (self.padding() + (index / self.columns) as f32 * self.pitch() + self.cell * 0.5
            - self.viewport * 0.5)
            .clamp(0.0, (self.height(count) - self.viewport).max(0.0))
    }

    pub fn rect(self, index: usize, origin: egui::Pos2, size: Vec2) -> Rect {
        let cell = Rect::from_min_size(
            origin
                + egui::vec2(
                    self.left + (index % self.columns) as f32 * (self.cell + layout::GAP),
                    self.padding() + (index / self.columns) as f32 * self.pitch(),
                ),
            Vec2::splat(self.cell),
        );
        gallery_thumbnail(cell, size, 1.0)
    }

    pub fn visible(self, viewport: Rect, count: usize) -> Range<usize> {
        // Include the caption band even when its thumbnail is just above view.
        let first = ((viewport.top() - self.padding() - 90.0) / self.pitch())
            .floor()
            .max(0.0) as usize;
        let last = ((viewport.bottom() - self.padding()) / self.pitch())
            .ceil()
            .max(0.0) as usize
            + 1;
        let start = first.saturating_mul(self.columns).min(count);
        start
            ..last
                .saturating_mul(self.columns)
                .min(count)
                .min(start + VISIBLE_PREVIEW_LIMIT)
    }
}

pub(super) fn gallery_cells(width: f32, height: f32) -> (usize, f32) {
    let mut columns =
        (((width + GALLERY_GAP) / (GALLERY_DIAMETER + GALLERY_GAP)).floor() as usize).max(1);
    while columns > 1 {
        let cell = (width - (columns - 1) as f32 * GALLERY_GAP) / columns as f32;
        let rows = (height / (cell + GALLERY_GAP)).ceil() as usize + 1;
        if columns.saturating_mul(rows) <= VISIBLE_PREVIEW_LIMIT {
            break;
        }
        columns -= 1;
    }
    (
        columns,
        ((width - (columns - 1) as f32 * GALLERY_GAP) / columns as f32).max(1.0),
    )
}
