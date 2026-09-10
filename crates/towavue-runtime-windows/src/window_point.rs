use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, WindowFromPoint};

/// Returns the nearest monitor's work rectangle in physical virtual-screen coordinates.
/// Call on the per-monitor DPI-aware UI thread.
pub fn monitor_work_area(point: (i32, i32)) -> Option<(i32, i32, i32, i32)> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: both inputs are owned scalar/stack data. The borrowed monitor handle
    // is used only for this synchronous query; no native handle or pointer escapes.
    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: point.0,
                y: point.1,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
    }
    let rect = info.rcWork;
    Some((rect.left, rect.top, rect.right, rect.bottom))
}

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
