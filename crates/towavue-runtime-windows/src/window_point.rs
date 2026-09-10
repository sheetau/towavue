use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, WindowFromPoint};

/// Maps a source client point into a target only when that target is unobscured.
/// Coordinates are physical pixels in the caller's per-monitor DPI-aware UI thread.
pub fn unobscured_window_point(
    source: &impl HasWindowHandle,
    target: &impl HasWindowHandle,
    point: (i32, i32),
) -> Option<(i32, i32)> {
    let source = source.window_handle().ok()?;
    let target = target.window_handle().ok()?;
    let (RawWindowHandle::Win32(source), RawWindowHandle::Win32(target)) =
        (source.as_raw(), target.as_raw())
    else {
        return None;
    };
    let source = HWND(source.hwnd.get() as *mut _);
    let target = HWND(target.hwnd.get() as *mut _);
    let mut point = POINT {
        x: point.0,
        y: point.1,
    };
    // SAFETY: borrowed owners outlive these synchronous, read-only UI-thread calls.
    // POINT is stack-owned and no native handle or pointer escapes. Hit testing checks
    // the actual topmost root, not just overlapping rectangles or mouse capture.
    unsafe {
        if !ClientToScreen(source, &mut point).as_bool()
            || GetAncestor(WindowFromPoint(point), GA_ROOT) != target
            || !ScreenToClient(target, &mut point).as_bool()
        {
            return None;
        }
    }
    Some((point.x, point.y))
}
