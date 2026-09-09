use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_BUTTON_BOUNDS, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmDefWindowProc,
    DwmExtendFrameIntoClientArea, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BLACK_BRUSH, ClientToScreen, CombineRgn, CreateRectRgn, DeleteObject, FillRect, GetDC,
    GetStockObject, HBRUSH, HDC, InvalidateRect, RGN_DIFF, ReleaseDC, ScreenToClient, SetWindowRgn,
    ValidateRect,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;
use winit::window::Window;

const SUBCLASS_ID: usize = 0x7476_6361;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptionAction {
    Minimize,
    ToggleMaximize,
    Close,
}

/// Accessibility metadata for an OS-drawn button, in physical client coordinates.
pub struct CaptionButton {
    pub action: CaptionAction,
    pub label: &'static str,
    pub bounds: egui::Rect,
    pub enabled: bool,
}

struct CaptionState {
    fullscreen: Cell<bool>,
    drag: Cell<Option<egui::Rect>>,
}

/// A UI-thread-owned DWM frame whose client title row is drawn by the application.
pub struct NativeCaption {
    window: Arc<Window>,
    handle: HWND,
    state: Rc<CaptionState>,
}

impl NativeCaption {
    pub fn new(window: Arc<Window>) -> Result<Self, Box<dyn std::error::Error>> {
        let initial_size = window.inner_size();
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
            return Err("Native caption requires a Windows window".into());
        };
        let handle = HWND(handle.hwnd.get() as *mut _);
        // SAFETY: the retained winit window owns this handle. Neither API retains data.
        if unsafe { GetWindowThreadProcessId(handle, None) != GetCurrentThreadId() } {
            return Err("Native caption must be installed on its window thread".into());
        }
        let state = Rc::new(CaptionState {
            fullscreen: Cell::new(false),
            drag: Cell::new(None),
        });
        let callback_state = Rc::into_raw(state.clone());
        // SAFETY: the subclass owns one Rc reference, released on removal/destruction.
        // Rc and the !Send owner keep every access and removal on the window thread.
        if !unsafe {
            SetWindowSubclass(
                handle,
                Some(caption_proc),
                SUBCLASS_ID,
                callback_state as usize,
            )
        }
        .as_bool()
        {
            // SAFETY: failed registration did not take ownership of the raw Rc.
            unsafe {
                drop(Rc::from_raw(callback_state));
            }
            return Err("Could not install the native caption".into());
        }
        let caption = Self {
            window,
            handle,
            state,
        };
        // SAFETY: DWM copies this scalar attribute synchronously. Older systems may
        // reject dark mode; their native frame remains functional with its OS colors.
        unsafe {
            let dark = 1_i32;
            let _ = DwmSetWindowAttribute(
                handle,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark as *const i32).cast(),
                size_of::<i32>() as u32,
            );
        }
        caption.refresh_frame()?;
        // SAFETY: preserve the requested client dimensions after removing the old
        // title band. No activation/movement and no winit style-based size estimate.
        unsafe {
            let mut outer = RECT::default();
            let mut client = RECT::default();
            GetWindowRect(handle, &mut outer)?;
            GetClientRect(handle, &mut client)?;
            SetWindowPos(
                handle,
                None,
                0,
                0,
                outer.right - outer.left + initial_size.width as i32 - client.right,
                outer.bottom - outer.top + initial_size.height as i32 - client.bottom,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
        }
        Ok(caption)
    }

    /// Physical client coordinates; no native handle or callback escapes the runtime.
    pub fn set_drag_region(&self, region: Option<egui::Rect>) {
        self.state.drag.set(region);
    }

    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.state.fullscreen.set(fullscreen);
        self.state.drag.set(None);
        // SAFETY: same-thread, retained window; DWM copies the stack margins.
        unsafe { extend_frame(self.handle, fullscreen) };
    }

    /// Width reserved at the right of the client area, in physical pixels.
    pub fn controls_width(&self) -> f32 {
        let bounds = caption_bounds(self.handle);
        (self.window.inner_size().width as f32 - bounds.left as f32).max(0.0)
    }

    pub fn accessible_buttons(&self) -> Vec<CaptionButton> {
        let mut buttons = Vec::with_capacity(3);
        if self.state.fullscreen.get() {
            return buttons;
        }
        // SAFETY: same-thread message to the retained window. Windows writes only
        // this initialized stack structure and does not retain its address.
        unsafe {
            let mut info = TITLEBARINFOEX {
                cbSize: size_of::<TITLEBARINFOEX>() as u32,
                ..Default::default()
            };
            SendMessageW(
                self.handle,
                WM_GETTITLEBARINFOEX,
                None,
                Some(LPARAM((&mut info as *mut TITLEBARINFOEX) as isize)),
            );
            let mut origin = POINT::default();
            if !ClientToScreen(self.handle, &mut origin).as_bool() {
                return buttons;
            }
            let maximized = IsZoomed(self.handle).as_bool();
            for (index, action, label) in [
                (2, CaptionAction::Minimize, "Minimize window"),
                (
                    3,
                    CaptionAction::ToggleMaximize,
                    if maximized {
                        "Restore window"
                    } else {
                        "Maximize window"
                    },
                ),
                (5, CaptionAction::Close, "Close window"),
            ] {
                let rect = info.rgrect[index];
                // TITLEBARINFOEX uses STATE_SYSTEM_INVISIBLE/OFFSCREEN/UNAVAILABLE.
                if info.rgstate[index] & 0x18000 != 0
                    || rect.right <= rect.left
                    || rect.bottom <= rect.top
                {
                    continue;
                }
                buttons.push(CaptionButton {
                    action,
                    label,
                    bounds: egui::Rect::from_min_max(
                        egui::pos2((rect.left - origin.x) as f32, (rect.top - origin.y) as f32),
                        egui::pos2(
                            (rect.right - origin.x) as f32,
                            (rect.bottom - origin.y) as f32,
                        ),
                    ),
                    enabled: info.rgstate[index] & 1 == 0,
                });
            }
        }
        buttons
    }

    pub fn invoke(&self, action: CaptionAction) -> windows::core::Result<()> {
        // SAFETY: scalar system command queued only to our retained window. Close
        // follows winit's normal CloseRequested route; no HWND is destroyed here.
        unsafe {
            let command = match action {
                CaptionAction::Minimize => SC_MINIMIZE,
                CaptionAction::ToggleMaximize if IsZoomed(self.handle).as_bool() => SC_RESTORE,
                CaptionAction::ToggleMaximize => SC_MAXIMIZE,
                CaptionAction::Close => SC_CLOSE,
            };
            PostMessageW(
                Some(self.handle),
                WM_SYSCOMMAND,
                WPARAM(command as usize),
                LPARAM(0),
            )
        }
    }

    /// Maximized windows extend above the monitor; keep UI below that invisible strip.
    pub fn top_inset(&self) -> f32 {
        // SAFETY: synchronous scalar queries on the retained UI-thread window.
        unsafe {
            if self.state.fullscreen.get() || !IsZoomed(self.handle).as_bool() {
                return 0.0;
            }
            let dpi = GetDpiForWindow(self.handle);
            (GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
                + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)) as f32
        }
    }

    fn refresh_frame(&self) -> windows::core::Result<()> {
        // SAFETY: no movement/activation; synchronous non-client recalculation uses
        // the already installed subclass and its stable, UI-thread-owned state.
        unsafe {
            extend_frame(self.handle, self.state.fullscreen.get());
            SetWindowPos(
                self.handle,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
    }
}

fn caption_bounds(handle: HWND) -> RECT {
    let mut buttons = RECT::default();
    let mut outer = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: these APIs write only live stack values for the retained window.
    let valid = unsafe {
        DwmGetWindowAttribute(
            handle,
            DWMWA_CAPTION_BUTTON_BOUNDS,
            (&mut buttons as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
        .is_ok()
            && GetWindowRect(handle, &mut outer).is_ok()
            && ClientToScreen(handle, &mut origin).as_bool()
    };
    let mut client = RECT::default();
    // SAFETY: stack output for this UI-thread window; failure leaves an empty rect.
    unsafe {
        let _ = GetClientRect(handle, &mut client);
    }
    if valid && buttons.right > buttons.left {
        RECT {
            left: (outer.left + buttons.left - origin.x).max(0),
            top: 0,
            right: client.right,
            bottom: (outer.top + buttons.bottom - origin.y).max(0),
        }
    } else {
        // DWM documents undefined bounds while hidden/minimized. Reserve the
        // native default until the visible window supplies its actual bounds.
        // SAFETY: this synchronous query has no ownership or lifetime effects.
        let dpi = unsafe { GetDpiForWindow(handle) } as i32;
        RECT {
            left: (client.right - 154 * dpi / 96).max(0),
            top: 0,
            right: client.right,
            bottom: 32 * dpi / 96,
        }
    }
}

impl Drop for NativeCaption {
    fn drop(&mut self) {
        // SAFETY: !Send keeps teardown on the window thread; the retained Arc keeps
        // the HWND alive. Remove before dropping callback state; never destroy winit's HWND.
        if unsafe { RemoveWindowSubclass(self.handle, Some(caption_proc), SUBCLASS_ID) }.as_bool() {
            // SAFETY: successful removal releases the subclass's one strong reference.
            unsafe {
                drop(Rc::from_raw(Rc::as_ptr(&self.state)));
            }
        }
    }
}

unsafe fn extend_frame(handle: HWND, fullscreen: bool) {
    // SAFETY: caller owns this UI-thread window; DWM retains no margins pointer.
    unsafe {
        let margins = MARGINS {
            cyTopHeight: if fullscreen {
                0
            } else {
                (32 * GetDpiForWindow(handle) / 96) as i32
            },
            ..Default::default()
        };
        let _ = DwmExtendFrameIntoClientArea(handle, &margins);
    }
}

unsafe extern "system" fn caption_proc(
    handle: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: registration holds this Rc until same-thread subclass removal.
    // Only Cell accesses cross reentrant Windows calls; no mutable borrow is retained.
    let state = unsafe { &*(data as *const CaptionState) };
    // SAFETY: Windows supplies the documented message parameters for this live HWND.
    // Stack pointers are used only synchronously and are not retained by any callee.
    unsafe {
        if message == WM_NCDESTROY {
            if RemoveWindowSubclass(handle, Some(caption_proc), SUBCLASS_ID).as_bool() {
                drop(Rc::from_raw(data as *const CaptionState));
            }
        } else if message == WM_ERASEBKGND {
            let mut client = RECT::default();
            if GetClientRect(handle, &mut client).is_ok() {
                FillRect(
                    HDC(wparam.0 as *mut _),
                    &client,
                    HBRUSH(GetStockObject(BLACK_BRUSH).0),
                );
            }
            return LRESULT(1);
        } else if message == WM_PAINT {
            let result = DefSubclassProc(handle, message, wparam, lparam);
            let dc = GetDC(Some(handle));
            if !dc.is_invalid() {
                // Only the exposed caption strip uses GDI, never the media surface.
                let bounds = caption_bounds(handle);
                FillRect(dc, &bounds, HBRUSH(GetStockObject(BLACK_BRUSH).0));
                ReleaseDC(Some(handle), dc);
            }
            return result;
        } else if !state.fullscreen.get() {
            let mut dwm_result = LRESULT(0);
            let dwm_handled = matches!(message, WM_NCHITTEST | WM_NCMOUSELEAVE)
                && DwmDefWindowProc(handle, message, wparam, lparam, &mut dwm_result).as_bool();
            if message == WM_NCCALCSIZE && wparam.0 != 0 {
                let proposed_top = (*(lparam.0 as *const NCCALCSIZE_PARAMS)).rgrc[0].top;
                if IsZoomed(handle).as_bool() {
                    let dpi = GetDpiForWindow(handle);
                    let border = GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
                        + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    let rect = &mut (*(lparam.0 as *mut NCCALCSIZE_PARAMS)).rgrc[0];
                    rect.left += border;
                    rect.right -= border;
                    rect.bottom -= border;
                }
                // DwmDefWindowProc cannot hit-test caption buttons with a non-client
                // top inset. Keep it zero and expose the hidden strip as UI safe area.
                (*(lparam.0 as *mut NCCALCSIZE_PARAMS)).rgrc[0].top = proposed_top;
                return LRESULT(0);
            }
            if matches!(
                message,
                WM_ACTIVATE | WM_DPICHANGED | WM_DWMCOMPOSITIONCHANGED
            ) {
                extend_frame(handle, false);
            }
            if dwm_handled {
                return dwm_result;
            }
            if message == WM_NCHITTEST {
                let hit = DefSubclassProc(handle, message, wparam, lparam);
                if !matches!(hit.0 as u32, HTCLIENT | HTCAPTION) {
                    return hit;
                }
                let mut point = POINT {
                    x: i32::from(lparam.0 as i16),
                    y: i32::from((lparam.0 >> 16) as i16),
                };
                if ScreenToClient(handle, &mut point).as_bool() {
                    if !IsZoomed(handle).as_bool() {
                        let dpi = GetDpiForWindow(handle);
                        let border = GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
                            + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                        let mut client = RECT::default();
                        if GetClientRect(handle, &mut client).is_ok()
                            && let Some(edge) = resize_hit(point, client, border)
                        {
                            return LRESULT(edge as isize);
                        }
                    }
                    if state.drag.get().is_some_and(|rect| {
                        rect.contains(egui::pos2(point.x as f32, point.y as f32))
                    }) {
                        return LRESULT(HTCAPTION as isize);
                    }
                }
                return LRESULT(HTCLIENT as isize);
            }
        }
        DefSubclassProc(handle, message, wparam, lparam)
    }
}

pub(crate) struct CaptionSurface {
    pub(crate) handle: HWND,
    _parent: Arc<Window>,
    parent_handle: HWND,
    state: Rc<CaptionState>,
}

impl CaptionSurface {
    pub(crate) fn new(caption: &NativeCaption) -> windows::core::Result<Self> {
        // SAFETY: standard window class; no borrowed creation data. The child is
        // owned below and remains on the parent's thread for its entire lifetime.
        let handle = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!(""),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                1,
                1,
                Some(caption.handle),
                None,
                None,
                None,
            )?
        };
        let surface = Self {
            handle,
            _parent: caption.window.clone(),
            parent_handle: caption.handle,
            state: caption.state.clone(),
        };
        // SAFETY: callback retains no data and is removed by native child destruction.
        if !unsafe { SetWindowSubclass(handle, Some(surface_proc), SUBCLASS_ID, 0) }.as_bool() {
            return Err(windows::core::Error::from_thread());
        }
        Ok(surface)
    }

    pub(crate) fn resize(&self, width: u32, height: u32) -> windows::core::Result<()> {
        // SAFETY: the child belongs to this UI thread. Successful SetWindowRgn takes
        // ownership of its region; temporary and failed regions are deleted here.
        unsafe {
            SetWindowPos(
                self.handle,
                None,
                0,
                0,
                width as i32,
                height as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
            let region = CreateRectRgn(0, 0, width as i32, height as i32);
            if region.is_invalid() {
                return Err(windows::core::Error::from_thread());
            }
            if !self.state.fullscreen.get() {
                let bounds = caption_bounds(self.parent_handle);
                let cutout = CreateRectRgn(bounds.left, bounds.top, bounds.right, bounds.bottom);
                let combined = CombineRgn(Some(region), Some(region), Some(cutout), RGN_DIFF);
                let _ = DeleteObject(cutout.into());
                if combined.0 == 0 {
                    let _ = DeleteObject(region.into());
                    return Err(windows::core::Error::from_thread());
                }
            }
            if SetWindowRgn(self.handle, Some(region), true) == 0 {
                let _ = DeleteObject(region.into());
                return Err(windows::core::Error::from_thread());
            }
            // The parent's previous paint may have been clipped by the old child
            // region. Repaint the newly exposed caption after updating that region.
            let bounds = caption_bounds(self.parent_handle);
            let _ = InvalidateRect(Some(self.parent_handle), Some(&bounds), false);
        }
        Ok(())
    }
}

fn resize_hit(point: POINT, rect: RECT, border: i32) -> Option<u32> {
    let left = point.x < rect.left + border;
    let right = point.x >= rect.right - border;
    let top = point.y < rect.top + border;
    let bottom = point.y >= rect.bottom - border;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(HTTOPLEFT),
        (_, true, true, _) => Some(HTTOPRIGHT),
        (true, _, _, true) => Some(HTBOTTOMLEFT),
        (_, true, _, true) => Some(HTBOTTOMRIGHT),
        (true, _, _, _) => Some(HTLEFT),
        (_, true, _, _) => Some(HTRIGHT),
        (_, _, true, _) => Some(HTTOP),
        (_, _, _, true) => Some(HTBOTTOM),
        _ => None,
    }
}

impl Drop for CaptionSurface {
    fn drop(&mut self) {
        // SAFETY: Rc makes this owner !Send; parent outlives its child. The renderer
        // drops its swap chain before this surface and never exposes this handle.
        unsafe {
            let _ = DestroyWindow(self.handle);
        }
    }
}

unsafe extern "system" fn surface_proc(
    handle: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if message == WM_NCHITTEST {
        return LRESULT(HTTRANSPARENT as isize);
    }
    if message == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if message == WM_PAINT {
        // SAFETY: the swap chain owns every child pixel; suppress STATIC's GDI
        // background paint without scheduling another application frame.
        unsafe {
            let _ = ValidateRect(Some(handle), None);
        }
        return LRESULT(0);
    }
    // SAFETY: forward the original native message; no pointers retained.
    unsafe { DefSubclassProc(handle, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::platform::windows::EventLoopBuilderExtWindows;

    #[test]
    fn native_resize_edges_scale_and_preserve_interior_input() {
        for scale in [1, 2, 3] {
            let rect = RECT {
                left: 0,
                top: 0,
                right: 960 * scale,
                bottom: 576 * scale,
            };
            for (x, y, hit) in [
                (1, 1, HTTOPLEFT),
                (959, 1, HTTOPRIGHT),
                (1, 575, HTBOTTOMLEFT),
                (959, 575, HTBOTTOMRIGHT),
                (1, 200, HTLEFT),
                (959, 200, HTRIGHT),
                (300, 1, HTTOP),
                (300, 575, HTBOTTOM),
            ] {
                assert_eq!(
                    resize_hit(
                        POINT {
                            x: x * scale,
                            y: y * scale
                        },
                        rect,
                        8 * scale
                    ),
                    Some(hit)
                );
            }
            for (x, y) in [(20, 16), (100, 16), (300, 16), (500, 300)] {
                assert_eq!(
                    resize_hit(
                        POINT {
                            x: x * scale,
                            y: y * scale
                        },
                        rect,
                        8 * scale
                    ),
                    None
                );
            }
        }
    }

    #[test]
    fn native_caption_owns_subclass_and_preserves_winit_close_delivery() {
        // winit permits only one event loop per process, including across test threads.
        if std::env::var_os("TOWAVUE_CAPTION_TEST_CHILD").is_none() {
            let output = std::process::Command::new(
                std::env::current_exe().expect("test executable"),
            )
            .args([
                "--exact",
                "caption::tests::native_caption_owns_subclass_and_preserves_winit_close_delivery",
                "--nocapture",
            ])
            .env("TOWAVUE_CAPTION_TEST_CHILD", "1")
            .output()
            .expect("isolated caption test");
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        #[derive(Default)]
        struct Trial {
            window: Option<Arc<Window>>,
            caption: Option<NativeCaption>,
            close_received: bool,
            deadline: Option<std::time::Instant>,
        }
        impl ApplicationHandler for Trial {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                self.deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
                let window = Arc::new(
                    event_loop
                        .create_window(
                            Window::default_attributes()
                                .with_visible(false)
                                .with_inner_size(winit::dpi::PhysicalSize::new(960, 576)),
                        )
                        .expect("hidden caption test window"),
                );
                let caption = NativeCaption::new(window.clone()).expect("native frame");
                let handle = caption.handle;
                let surface = CaptionSurface::new(&caption).expect("input-transparent child");
                surface.resize(960, 576).expect("clipped surface");
                caption.set_drag_region(Some(egui::Rect::from_min_max(
                    egui::pos2(250.0, 8.0),
                    egui::pos2(700.0, 32.0),
                )));
                // SAFETY: retained hidden test window on this thread; scalar messages,
                // stack outputs and no user input, global hook or foreign window access.
                unsafe {
                    let mut client = RECT::default();
                    GetClientRect(handle, &mut client).expect("client");
                    assert_eq!((client.right, client.bottom), (960, 576));
                    assert_eq!(
                        SendMessageW(surface.handle, WM_NCHITTEST, None, None).0,
                        HTTRANSPARENT as isize
                    );
                    assert_eq!(SendMessageW(surface.handle, WM_ERASEBKGND, None, None).0, 1);
                    assert_eq!(SendMessageW(surface.handle, WM_PAINT, None, None).0, 0);
                    let mut origin = POINT::default();
                    assert!(ClientToScreen(handle, &mut origin).as_bool());
                    for (x, y, expected) in [
                        (300, 16, HTCAPTION),
                        (100, 16, HTCLIENT),
                        (1, 100, HTLEFT),
                        (300, 575, HTBOTTOM),
                    ] {
                        let packed = u32::from((origin.x + x) as u16)
                            | (u32::from((origin.y + y) as u16) << 16);
                        let point = LPARAM(packed as isize);
                        assert_eq!(
                            SendMessageW(handle, WM_NCHITTEST, None, Some(point)).0,
                            expected as isize
                        );
                    }
                    let state = caption.state.clone();
                    assert_eq!(Rc::strong_count(&state), 4);
                    drop(surface);
                    assert_eq!(Rc::strong_count(&state), 3);
                    drop(caption);
                    assert_eq!(Rc::strong_count(&state), 1);
                }
                // Reinstall for the real close-delivery check, with the same live HWND.
                self.caption = Some(NativeCaption::new(window.clone()).expect("reinstall frame"));
                self.window = Some(window);
                self.caption
                    .as_ref()
                    .expect("caption")
                    .invoke(CaptionAction::Close)
                    .expect("queue the same system close used by accessibility");
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
                if self
                    .deadline
                    .is_some_and(|deadline| std::time::Instant::now() >= deadline)
                {
                    event_loop.exit();
                }
            }
            fn window_event(
                &mut self,
                event_loop: &ActiveEventLoop,
                _: winit::window::WindowId,
                event: WindowEvent,
            ) {
                if matches!(event, WindowEvent::CloseRequested) {
                    self.close_received = true;
                    event_loop.exit();
                }
            }
        }
        let mut builder = EventLoop::builder();
        builder.with_any_thread(true);
        let event_loop = builder.build().expect("test event loop");
        let mut trial = Trial::default();
        event_loop
            .run_app(&mut trial)
            .expect("native event delivery");
        assert!(trial.close_received);
    }
}
