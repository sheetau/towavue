use egui::{Rect, Vec2};

#[derive(Clone, Copy)]
pub(super) enum Edge {
    Top,
    Bottom,
    Left,
    Right,
    Center,
}

impl Edge {
    pub fn nearest(bounds: Rect, viewport: Rect) -> Self {
        [
            (
                (bounds.top() - viewport.top()) / viewport.height(),
                Self::Top,
            ),
            (
                (viewport.bottom() - bounds.bottom()) / viewport.height(),
                Self::Bottom,
            ),
            (
                (bounds.left() - viewport.left()) / viewport.width(),
                Self::Left,
            ),
            (
                (viewport.right() - bounds.right()) / viewport.width(),
                Self::Right,
            ),
        ]
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .expect("four edges")
        .1
    }
}

/// Move the complete ink/outline box, rather than the font's line box. Clamp
/// the other axis too so glyph bearings and vertical forms remain visible.
pub(super) fn translation(bounds: Rect, viewport: Rect, edge: Edge, percent: Option<u16>) -> Vec2 {
    if !bounds.is_finite() || !bounds.is_positive() {
        return Vec2::ZERO;
    }
    let mut delta = Vec2::ZERO;
    if let Some(percent) = percent {
        let fraction = f32::from(percent.min(50)) / 100.0;
        match edge {
            Edge::Top => delta.y = viewport.top() + viewport.height() * fraction - bounds.top(),
            Edge::Bottom => {
                delta.y = viewport.bottom() - viewport.height() * fraction - bounds.bottom()
            }
            Edge::Left => delta.x = viewport.left() + viewport.width() * fraction - bounds.left(),
            Edge::Right => {
                delta.x = viewport.right() - viewport.width() * fraction - bounds.right()
            }
            Edge::Center => (),
        }
    }
    let moved = bounds.translate(delta);
    let clamp = |min: f32, max: f32, low: f32, high: f32| {
        if max - min > high - low {
            (low + high - min - max) * 0.5
        } else if min < low {
            low - min
        } else if max > high {
            high - max
        } else {
            0.0
        }
    };
    delta
        + egui::vec2(
            clamp(
                moved.left(),
                moved.right(),
                viewport.left(),
                viewport.right(),
            ),
            clamp(
                moved.top(),
                moved.bottom(),
                viewport.top(),
                viewport.bottom(),
            ),
        )
}
