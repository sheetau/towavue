use std::{marker::PhantomData, rc::Rc};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, POINT, RECT, SIZE},
        Graphics::Gdi::{
            AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
            CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP,
            HDC, HGDIOBJ, SelectObject,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GetWindowRect, HWND_TOPMOST, SW_HIDE, SWP_NOACTIVATE,
            SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow,
            WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
            WS_POPUP,
        },
    },
    core::w,
};

/// A UI-thread-owned, input-transparent drag decoration. It never owns capture,
/// focus, a message loop, a graphics device, or a reference to the source window.
/// Layered + transparent also excludes it from native point picking.
pub struct DragBadge {
    window: HWND,
    position: Option<(i32, i32, bool)>,
    _thread: PhantomData<Rc<()>>,
}

impl DragBadge {
    pub fn new() -> windows::core::Result<Self> {
        // SAFETY: the predefined static class needs no callback-owned state. The
        // resulting window belongs exclusively to this UI thread and stays hidden.
        let window = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST,
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )?
        };
        Ok(Self {
            window,
            position: None,
            _thread: PhantomData,
        })
    }

    /// Replace the bounded top-down premultiplied RGBA decoration. User32 copies
    /// the pixels before return, so the temporary DIB can be released immediately.
    pub fn set_image(&mut self, size: u32, rgba: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        if !(1..=256).contains(&size)
            || rgba.len() != size as usize * size as usize * 4
            || rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[..3].iter().any(|c| *c > p[3]))
        {
            return Err("Invalid drag badge pixels".into());
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        // SAFETY: bounded DIB dimensions and stack-owned description. The returned
        // objects remain on this thread; Surface restores selection before disposal.
        let surface = unsafe {
            let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let dc = CreateCompatibleDC(None);
            if dc.0.is_null() {
                let error = windows::core::Error::from_thread();
                let _ = DeleteObject(bitmap.into());
                return Err(error.into());
            }
            let previous = SelectObject(dc, bitmap.into());
            if previous.0.is_null() {
                let error = windows::core::Error::from_thread();
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(dc);
                return Err(error.into());
            }
            Surface {
                bitmap,
                dc,
                previous,
            }
        };
        // SAFETY: this unshared DIB contains exactly the validated RGBA byte count;
        // only this thread writes it, before the synchronous native copy.
        let pixels = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), rgba.len()) };
        for (source, target) in rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(pixels.as_chunks_mut::<4>().0)
        {
            target.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
        }
        let mut rect = RECT::default();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        // SAFETY: window and selected DIB are alive on their creating UI thread;
        // all pointers refer to stack data and no borrowed storage escapes.
        unsafe {
            GetWindowRect(self.window, &mut rect)?;
            UpdateLayeredWindow(
                self.window,
                None,
                Some(&POINT {
                    x: rect.left,
                    y: rect.top,
                }),
                Some(&SIZE {
                    cx: size as i32,
                    cy: size as i32,
                }),
                Some(surface.dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )?;
        }
        Ok(())
    }

    /// Native geometry for owned-window verification; no HWND or pixels escape.
    #[cfg(any(test, feature = "render-verification"))]
    pub fn verification_bounds(&self) -> windows::core::Result<(i32, i32, i32, i32)> {
        let mut rect = RECT::default();
        // SAFETY: read this wrapper's live UI-thread window into owned stack data.
        unsafe {
            GetWindowRect(self.window, &mut rect)?;
        }
        Ok((
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        ))
    }

    pub fn move_to(&mut self, position: (i32, i32), visible: bool) -> windows::core::Result<()> {
        if self.position == Some((position.0, position.1, visible)) {
            return Ok(());
        }
        // SAFETY: uniquely owned UI-thread window; never activate or alter focus.
        unsafe {
            if !visible {
                let _ = ShowWindow(self.window, SW_HIDE);
            }
            SetWindowPos(
                self.window,
                Some(HWND_TOPMOST),
                position.0,
                position.1,
                0,
                0,
                SWP_NOACTIVATE
                    | SWP_NOSIZE
                    | if visible {
                        SWP_SHOWWINDOW
                    } else {
                        Default::default()
                    },
            )?;
        }
        self.position = Some((position.0, position.1, visible));
        Ok(())
    }
}

impl Drop for DragBadge {
    fn drop(&mut self) {
        // SAFETY: !Send/!Sync ensures disposal on the window's creating thread.
        let _ = unsafe { DestroyWindow(self.window) };
    }
}

struct Surface {
    bitmap: HBITMAP,
    dc: HDC,
    previous: HGDIOBJ,
}
impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: all three handles are uniquely owned on this thread. Deselect
        // the DIB before deleting it, after UpdateLayeredWindow has copied it.
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, IsWindow, IsWindowVisible,
        WindowFromPoint,
    };

    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        SetThreadDpiAwarenessContext,
    };

    struct DpiContext(DPI_AWARENESS_CONTEXT);
    impl Drop for DpiContext {
        fn drop(&mut self) {
            // SAFETY: restore only this test thread's prior awareness context.
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }

    #[test]
    fn hidden_badge_keeps_focus_bounds_validation_and_lifetime() {
        exercise(false);
    }

    #[test]
    #[ignore = "briefly shows two small owned nonactivating windows to check native point picking"]
    fn visible_badge_passes_native_hit_testing_without_activation() {
        exercise(true);
    }

    fn exercise(visible: bool) {
        // SAFETY: all windows and reads belong to this synchronous test thread.
        // No keyboard/mouse input is generated and no external pixels are read.
        unsafe {
            let dpi = DpiContext(SetThreadDpiAwarenessContext(
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            ));
            assert!(!dpi.0.0.is_null());
            let foreground = GetForegroundWindow();
            // WindowFromPoint deliberately skips static-text controls. A button
            // class gives this read-only hit-test control a real, owned target.
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                w!("BUTTON"),
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .expect("owned target");
            let mut underneath = DragBadge {
                window: hwnd,
                position: None,
                _thread: PhantomData,
            };
            underneath
                .set_image(92, &vec![255; 92 * 92 * 4])
                .expect("underlying pixels");
            underneath
                .move_to((240, 180), visible)
                .expect("owned target placement");
            if visible {
                assert_eq!(
                    WindowFromPoint(POINT { x: 260, y: 200 }),
                    hwnd,
                    "reference target must be independently hittable before the badge exists"
                );
            }
            let mut badge = DragBadge::new().expect("badge");
            let badge_hwnd = badge.window;
            let style = GetWindowLongPtrW(badge_hwnd, GWL_EXSTYLE) as u32;
            let required = (WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_NOACTIVATE
                | WS_EX_TOOLWINDOW
                | WS_EX_TOPMOST)
                .0;
            assert_eq!(style & required, required);
            for size in [23, 46, 69] {
                let mut rgba = vec![0; size * size * 4];
                for pixel in rgba.as_chunks_mut::<4>().0 {
                    pixel.copy_from_slice(&[128, 0, 0, 128]);
                }
                badge
                    .set_image(size as u32, &rgba)
                    .expect("premultiplied decoration");
                badge.move_to((250, 190), visible).expect("badge placement");
                let mut rect = RECT::default();
                GetWindowRect(badge_hwnd, &mut rect).expect("bounds");
                assert_eq!(
                    (
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top
                    ),
                    (250, 190, size as i32, size as i32)
                );
                assert_eq!(IsWindowVisible(badge_hwnd).as_bool(), visible);
                if visible {
                    assert_eq!(
                        WindowFromPoint(POINT { x: 260, y: 200 }),
                        hwnd,
                        "layered badge must not obstruct drop target picking"
                    );
                }
                assert_eq!(GetForegroundWindow(), foreground);
            }
            assert!(badge.set_image(0, &[]).is_err());
            assert!(badge.set_image(257, &[]).is_err());
            assert!(badge.set_image(1, &[255, 0, 0, 128]).is_err());
            assert!(badge.set_image(2, &[0; 4]).is_err());
            badge
                .move_to((-1234, -567), false)
                .expect("negative screen coordinates");
            assert!(!IsWindowVisible(badge_hwnd).as_bool());
            drop(badge);
            assert!(
                !IsWindow(Some(badge_hwnd)).as_bool(),
                "cancel destroys the helper"
            );
            drop(underneath);
            assert!(!IsWindow(Some(hwnd)).as_bool());
            assert_eq!(GetForegroundWindow(), foreground);
        }
    }
}
