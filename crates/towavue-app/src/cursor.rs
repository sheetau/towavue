use std::time::{Duration, Instant};

const IDLE_DELAY: Duration = Duration::from_secs(2);

pub fn set_native(window: &winit::window::Window, icon: egui::CursorIcon) {
    if let Some(icon) = native_icon(icon) {
        window.set_cursor_visible(true);
        window.set_cursor(icon);
    } else {
        window.set_cursor_visible(false);
    }
}

// egui-winit's translation is private and its cursor update skips captured pointers outside
// the window. Keep the same mapping for host-owned drag feedback and normal-cursor restoration.
fn native_icon(icon: egui::CursorIcon) -> Option<winit::window::CursorIcon> {
    use egui::CursorIcon as E;
    use winit::window::CursorIcon as W;
    Some(match icon {
        E::None => return None,
        E::Default => W::Default,
        E::ContextMenu => W::ContextMenu,
        E::Help => W::Help,
        E::PointingHand => W::Pointer,
        E::Progress => W::Progress,
        E::Wait => W::Wait,
        E::Cell => W::Cell,
        E::Crosshair => W::Crosshair,
        E::Text => W::Text,
        E::VerticalText => W::VerticalText,
        E::Alias => W::Alias,
        E::Copy => W::Copy,
        E::Move => W::Move,
        E::NoDrop => W::NoDrop,
        E::NotAllowed => W::NotAllowed,
        E::Grab => W::Grab,
        E::Grabbing => W::Grabbing,
        E::AllScroll => W::AllScroll,
        E::ResizeHorizontal => W::EwResize,
        E::ResizeNeSw => W::NeswResize,
        E::ResizeNwSe => W::NwseResize,
        E::ResizeVertical => W::NsResize,
        E::ResizeEast => W::EResize,
        E::ResizeSouthEast => W::SeResize,
        E::ResizeSouth => W::SResize,
        E::ResizeSouthWest => W::SwResize,
        E::ResizeWest => W::WResize,
        E::ResizeNorthWest => W::NwResize,
        E::ResizeNorth => W::NResize,
        E::ResizeNorthEast => W::NeResize,
        E::ResizeColumn => W::ColResize,
        E::ResizeRow => W::RowResize,
        E::ZoomIn => W::ZoomIn,
        E::ZoomOut => W::ZoomOut,
    })
}

#[derive(Default)]
pub struct ViewingCursor {
    pub hidden: bool,
    pub deadline: Option<Instant>,
}

impl ViewingCursor {
    pub fn activity(&mut self) {
        self.deadline = None;
        self.hidden = false;
    }

    pub fn update(&mut self, now: Instant, eligible: bool) {
        if !eligible {
            self.activity();
        } else if !self.hidden {
            let deadline = *self.deadline.get_or_insert(now + IDLE_DELAY);
            if now >= deadline {
                self.hidden = true;
                self.deadline = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_feedback_and_restoration_cover_all_standard_ui_cursors() {
        for icon in egui::CursorIcon::ALL {
            assert_eq!(native_icon(icon).is_none(), icon == egui::CursorIcon::None);
        }
        for (ui, native) in [
            (egui::CursorIcon::Move, winit::window::CursorIcon::Move),
            (egui::CursorIcon::NoDrop, winit::window::CursorIcon::NoDrop),
            (
                egui::CursorIcon::PointingHand,
                winit::window::CursorIcon::Pointer,
            ),
            (
                egui::CursorIcon::ResizeNeSw,
                winit::window::CursorIcon::NeswResize,
            ),
        ] {
            assert_eq!(native_icon(ui), Some(native));
        }
    }

    #[test]
    fn cursor_hides_once_and_activity_or_ineligibility_restarts_the_delay() {
        let now = Instant::now();
        let mut cursor = ViewingCursor::default();
        cursor.update(now, false);
        assert_eq!(cursor.deadline, None);
        cursor.update(now, true);
        assert_eq!(cursor.deadline, Some(now + IDLE_DELAY));
        cursor.update(now + IDLE_DELAY / 2, true);
        assert!(!cursor.hidden);
        cursor.update(now + IDLE_DELAY, true);
        assert!(cursor.hidden);
        assert_eq!(cursor.deadline, None);
        cursor.update(now + IDLE_DELAY * 2, true);
        assert!(cursor.hidden);
        assert_eq!(cursor.deadline, None);
        cursor.activity();
        assert!(!cursor.hidden);
        cursor.update(now + IDLE_DELAY * 3, true);
        assert_eq!(cursor.deadline, Some(now + IDLE_DELAY * 4));
        cursor.update(now + IDLE_DELAY * 4, false);
        assert!(!cursor.hidden);
        assert_eq!(cursor.deadline, None);
    }
}
