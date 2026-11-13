use super::*;

/// Same fixed cells and centered gutters as Gallery. Captions and late preview
/// sizes never participate in layout or change the scroll extent.
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
        let (columns, cell) = gallery_cells(width, inset.height());
        Self {
            columns,
            cell,
            left: screen.left() + side - inset.left(),
            viewport: inset.height(),
        }
    }

    pub fn pitch(self) -> f32 {
        self.cell + GALLERY_GAP
    }

    pub fn height(self, count: usize) -> f32 {
        GALLERY_GAP + count.div_ceil(self.columns) as f32 * self.pitch() + 16.0
    }

    pub fn offset(self, index: usize, count: usize) -> f32 {
        (GALLERY_GAP + (index / self.columns) as f32 * self.pitch() + self.cell * 0.5
            - self.viewport * 0.5)
            .clamp(0.0, (self.height(count) - self.viewport).max(0.0))
    }

    pub fn rect(self, index: usize, origin: egui::Pos2, size: Vec2) -> Rect {
        let cell = Rect::from_min_size(
            origin
                + egui::vec2(
                    self.left + (index % self.columns) as f32 * self.pitch(),
                    GALLERY_GAP + (index / self.columns) as f32 * self.pitch(),
                ),
            Vec2::splat(self.cell),
        );
        gallery_thumbnail(cell, size, 1.0)
    }

    pub fn visible(self, viewport: Rect, count: usize) -> Range<usize> {
        // Include the caption band even when its thumbnail is just above view.
        let first = ((viewport.top() - GALLERY_GAP - 40.0) / self.pitch())
            .floor()
            .max(0.0) as usize;
        let last = ((viewport.bottom() - GALLERY_GAP) / self.pitch())
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
