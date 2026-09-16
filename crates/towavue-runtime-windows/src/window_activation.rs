use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
};

/// Best-effort activation on the window's UI thread, without synthetic keyboard input.
/// Hidden/minimized windows stay unchanged. A refused foreground request is normal;
/// callers must not undo a completed tab transfer or attempt to steal focus.
pub fn activate_window(window: &impl HasWindowHandle) -> bool {
    let Ok(borrowed) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(handle) = borrowed.as_raw() else {
        return false;
    };
    let handle = HWND(handle.hwnd.get() as *mut _);
    // SAFETY: the borrowed owner keeps HWND valid for these synchronous calls.
    // Activation is limited to its owning thread. No pointer or handle is retained.
    unsafe {
        if GetWindowThreadProcessId(handle, None) != GetCurrentThreadId()
            || !IsWindowVisible(handle).as_bool()
            || IsIconic(handle).as_bool()
        {
            return false;
        }
        handle == GetForegroundWindow() || SetForegroundWindow(handle).as_bool()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raw_window_handle::{HandleError, Win32WindowHandle, WindowHandle};
    use std::num::NonZeroIsize;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
    };
    use windows::core::w;

    struct OwnedWindow(HWND);

    impl HasWindowHandle for OwnedWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = Win32WindowHandle::new(NonZeroIsize::new(self.0.0 as isize).expect("HWND"));
            // SAFETY: this owner destroys HWND only after the borrow ends.
            Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(raw)) })
        }
    }

    impl Drop for OwnedWindow {
        fn drop(&mut self) {
            // SAFETY: this test owns HWND and drops it on its creating thread.
            unsafe { DestroyWindow(self.0).expect("destroy owned hidden window") };
        }
    }

    #[test]
    fn hidden_activation_preserves_foreground_and_visibility() {
        // SAFETY: STATIC is a system class; no callbacks or borrowed creation data.
        // The window is never shown and the scoped owner releases it on this thread.
        let window = OwnedWindow(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("towavue activation test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                None,
                None,
                None,
                None,
            )
            .expect("owned hidden window")
        });
        // SAFETY: scalar queries; the window owner remains live.
        let foreground = unsafe { GetForegroundWindow() };
        assert!(!activate_window(&window));
        unsafe {
            assert!(!IsWindowVisible(window.0).as_bool());
            assert_eq!(GetForegroundWindow(), foreground);
        }
    }
}
