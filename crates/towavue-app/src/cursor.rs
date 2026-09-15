use std::time::{Duration, Instant};

const IDLE_DELAY: Duration = Duration::from_secs(2);

const ZOOM_IN: &str = include_str!("../assets/cursors/zoom_in.rgba");
const GRABBING: &str = include_str!("../assets/cursors/hand_grabbing.rgba");

pub struct MediaCursors {
    zoom_in: winit::window::CustomCursor,
    grabbing: winit::window::CustomCursor,
}

impl MediaCursors {
    pub fn new(event_loop: &winit::event_loop::ActiveEventLoop, scale_factor: f64) -> Self {
        let size = (32.0 * scale_factor).round() as u16;
        Self {
            zoom_in: event_loop.create_custom_cursor(cursor_source(ZOOM_IN, 6, size)),
            grabbing: event_loop.create_custom_cursor(cursor_source(GRABBING, 13, size)),
        }
    }

    fn get(&self, icon: egui::CursorIcon) -> Option<&winit::window::CustomCursor> {
        match icon {
            egui::CursorIcon::ZoomIn => Some(&self.zoom_in),
            egui::CursorIcon::Grabbing => Some(&self.grabbing),
            _ => None,
        }
    }

    pub fn handles(icon: egui::CursorIcon) -> bool {
        matches!(icon, egui::CursorIcon::ZoomIn | egui::CursorIcon::Grabbing)
    }
}

fn cursor_pixels(asset: &str, hotspot: u16, size: u16) -> (Vec<u8>, u16) {
    // Text-encoded RGBA preserves Chromium's resource pixels without a new image
    // decoder. Decode/scale only at window creation or a native DPI change.
    let pixels: Vec<_> = asset
        .split_ascii_whitespace()
        .map(|pixel| {
            u32::from_str_radix(pixel, 16)
                .expect("embedded cursor pixel")
                .to_be_bytes()
        })
        .collect();
    let rgba = (0..usize::from(size))
        .flat_map(|y| {
            let pixels = &pixels;
            (0..usize::from(size)).flat_map(move |x| {
                pixels[(y * 32 / usize::from(size)) * 32 + x * 32 / usize::from(size)]
            })
        })
        .collect();
    (
        rgba,
        (f32::from(hotspot) * f32::from(size) / 32.0).round() as u16,
    )
}

fn cursor_source(asset: &str, hotspot: u16, size: u16) -> winit::window::CustomCursorSource {
    let (rgba, hotspot) = cursor_pixels(asset, hotspot, size);
    winit::window::CustomCursor::from_rgba(rgba, size, size, hotspot, hotspot)
        .expect("embedded cursor dimensions and hotspot")
}

pub fn set_native(
    window: &winit::window::Window,
    icon: egui::CursorIcon,
    media: Option<&MediaCursors>,
) {
    if let Some(cursor) = native_cursor(icon, media) {
        window.set_cursor_visible(true);
        window.set_cursor(cursor);
    } else {
        window.set_cursor_visible(false);
    }
}

fn native_cursor(
    icon: egui::CursorIcon,
    media: Option<&MediaCursors>,
) -> Option<winit::window::Cursor> {
    let native = native_icon(icon)?;
    Some(
        media
            .and_then(|media| media.get(icon))
            .map_or_else(|| native.into(), |custom| custom.clone().into()),
    )
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
    fn chromium_pixels_and_hotspots_scale_without_changing_the_artwork() {
        for (asset, hotspot, opaque, center) in [
            (ZOOM_IN, 6, 127, [0, 0, 0, 255]),
            (GRABBING, 13, 147, [240, 248, 255, 255]),
        ] {
            assert_eq!(asset.lines().count(), 32);
            assert!(
                asset
                    .lines()
                    .all(|row| row.split_ascii_whitespace().count() == 32)
            );
            let (original, actual_hotspot) = cursor_pixels(asset, hotspot, 32);
            assert_eq!(actual_hotspot, hotspot);
            assert_eq!(
                original
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] == 255)
                    .count(),
                opaque
            );
            let at = usize::from(hotspot) * 33 * 4;
            assert_eq!(&original[at..at + 4], center);
            for (size, expected_hotspot) in [
                (32, hotspot),
                (40, (hotspot * 5 + 2) / 4),
                (48, (hotspot * 3).div_ceil(2)),
                (64, hotspot * 2),
            ] {
                let (scaled, scaled_hotspot) = cursor_pixels(asset, hotspot, size);
                assert_eq!(scaled_hotspot, expected_hotspot);
                assert_eq!(scaled.len(), usize::from(size).pow(2) * 4);
                for (index, pixel) in scaled.as_chunks::<4>().0.iter().enumerate() {
                    let x = index % usize::from(size) * 32 / usize::from(size);
                    let y = index / usize::from(size) * 32 / usize::from(size);
                    assert_eq!(pixel, &original[(y * 32 + x) * 4..(y * 32 + x + 1) * 4]);
                }
                let _ = cursor_source(asset, hotspot, size);
            }
        }
    }

    #[test]
    fn native_media_cursor_creation_and_restoration_keep_host_drag_icons() {
        use winit::application::ApplicationHandler;
        use winit::event_loop::{ActiveEventLoop, EventLoop};
        use winit::platform::windows::EventLoopBuilderExtWindows;
        use winit::window::{Cursor, WindowId};

        let Some(_root) = crate::tests::isolated_test_root(
            "cursor::tests::native_media_cursor_creation_and_restoration_keep_host_drag_icons",
        ) else {
            return;
        };
        struct Trial(bool);
        impl ApplicationHandler for Trial {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                for scale in [1.0, 1.25, 1.5, 2.0] {
                    let media = MediaCursors::new(event_loop, scale);
                    let independent = MediaCursors::new(event_loop, scale);
                    // Winit represents allocation failure with one equal Failed
                    // value; independent successful native objects are distinct.
                    assert_ne!(media.zoom_in, independent.zoom_in);
                    assert_ne!(media.grabbing, independent.grabbing);
                    for icon in egui::CursorIcon::ALL {
                        let expected = match icon {
                            egui::CursorIcon::ZoomIn => Some(Cursor::Custom(media.zoom_in.clone())),
                            egui::CursorIcon::Grabbing => {
                                Some(Cursor::Custom(media.grabbing.clone()))
                            }
                            _ => native_icon(icon).map(Cursor::Icon),
                        };
                        assert_eq!(native_cursor(icon, Some(&media)), expected);
                    }
                }
                self.0 = true;
                event_loop.exit();
            }
            fn window_event(
                &mut self,
                _: &ActiveEventLoop,
                _: WindowId,
                _: winit::event::WindowEvent,
            ) {
            }
        }
        let mut trial = Trial(false);
        EventLoop::builder()
            .with_any_thread(true)
            .build()
            .expect("cursor event loop")
            .run_app(&mut trial)
            .expect("cursor creation trial");
        assert!(trial.0);
    }

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
