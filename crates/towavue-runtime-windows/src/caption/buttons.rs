//! Win32 owns caption gestures; the application paints their shared state.
use super::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, IsWindowEnabled, ReleaseCapture, SetCapture, TME_LEAVE, TME_NONCLIENT,
    TRACKMOUSEEVENT, TrackMouseEvent,
};

#[derive(Clone, Copy)]
struct Layout {
    bounds: egui::Rect,
    client: RECT,
    dpi: u32,
    maximized: bool,
}

#[derive(Default)]
pub(super) struct State {
    layout: Cell<Option<Layout>>,
    hovered: Cell<Option<CaptionAction>>,
    pressed: Cell<Option<CaptionAction>>,
}

impl State {
    pub(super) fn set_bounds(&self, handle: HWND, bounds: egui::Rect) {
        let mut client = RECT::default();
        // SAFETY: the caption owner retains this UI-thread HWND. All outputs are local.
        unsafe {
            if bounds.is_finite()
                && bounds.is_positive()
                && GetClientRect(handle, &mut client).is_ok()
            {
                // Native pointer coordinates are integer pixels; remove scale roundoff.
                let bounds = egui::Rect::from_min_max(
                    egui::pos2(bounds.left().round(), bounds.top().round()),
                    egui::pos2(bounds.right().round(), bounds.bottom().round()),
                );
                self.layout.set(Some(Layout {
                    bounds,
                    client,
                    dpi: GetDpiForWindow(handle),
                    maximized: IsZoomed(handle).as_bool(),
                }));
            }
        }
    }

    pub(super) fn bounds(&self, handle: HWND) -> egui::Rect {
        let mut client = RECT::default();
        // SAFETY: synchronous queries on the callback/owner's live, same-thread HWND.
        unsafe {
            let _ = GetClientRect(handle, &mut client);
            let dpi = GetDpiForWindow(handle);
            let maximized = IsZoomed(handle).as_bool();
            if let Some(layout) = self.layout.get().filter(|layout| {
                layout.client == client && layout.dpi == dpi && layout.maximized == maximized
            }) {
                return layout.bounds;
            }
            let scale = dpi as f32 / 96.0;
            let top = if maximized {
                (GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
                    + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)) as f32
            } else {
                0.0
            };
            egui::Rect::from_min_max(
                egui::pos2((client.right as f32 - 108.0 * scale).max(0.0), top),
                egui::pos2(client.right as f32, top + 32.0 * scale - 1.0),
            )
        }
    }

    fn hover(&self, action: Option<CaptionAction>, state: &CaptionState) {
        if self.hovered.replace(action) != action {
            redraw(state);
        }
    }

    pub(super) fn clear(&self, handle: HWND, state: &CaptionState) {
        self.hover(None, state);
        if self.pressed.take().is_some() {
            // SAFETY: release only capture owned by this window; no foreign input is modified.
            unsafe {
                if GetCapture() == handle {
                    let _ = ReleaseCapture();
                }
            }
            redraw(state);
        }
    }
}

fn redraw(state: &CaptionState) {
    if let Some(window) = state.window.upgrade() {
        window.request_redraw();
    }
}

const ACTIONS: [CaptionAction; 3] = [
    CaptionAction::Minimize,
    CaptionAction::ToggleMaximize,
    CaptionAction::Close,
];

fn rect(bounds: egui::Rect, index: usize) -> egui::Rect {
    let width = bounds.width() / 3.0;
    egui::Rect::from_min_max(
        egui::pos2(bounds.left() + width * index as f32, bounds.top()),
        egui::pos2(bounds.left() + width * (index + 1) as f32, bounds.bottom()),
    )
}

fn command(handle: HWND, action: CaptionAction) -> u32 {
    match action {
        CaptionAction::Minimize => SC_MINIMIZE,
        // SAFETY: scalar query of the same-thread retained window.
        CaptionAction::ToggleMaximize if unsafe { IsZoomed(handle).as_bool() } => SC_RESTORE,
        CaptionAction::ToggleMaximize => SC_MAXIMIZE,
        CaptionAction::Close => SC_CLOSE,
    }
}

fn enabled(handle: HWND, action: CaptionAction) -> bool {
    // The system menu caches disabled Restore/Maximize flags until it is opened.
    // It is not the capability source for independently drawn caption controls.
    // SAFETY: scalar queries of the retained same-thread window.
    unsafe {
        if !IsWindowEnabled(handle).as_bool() {
            return false;
        }
        let style = WINDOW_STYLE(GetWindowLongPtrW(handle, GWL_STYLE) as u32);
        match action {
            CaptionAction::Minimize => style.contains(WS_MINIMIZEBOX),
            CaptionAction::ToggleMaximize => style.contains(WS_MAXIMIZEBOX),
            CaptionAction::Close => {
                GetClassLongPtrW(handle, GCL_STYLE) & CS_NOCLOSE.0 as usize == 0
            }
        }
    }
}

pub(super) fn snapshot(
    handle: HWND,
    state: &CaptionState,
    language: Language,
) -> Vec<CaptionButton> {
    if state.fullscreen.get() {
        return Vec::new();
    }
    // SAFETY: scalar query; caption owns the HWND on this thread.
    let maximized = unsafe { IsZoomed(handle).as_bool() };
    let bounds = state.controls.bounds(handle);
    ACTIONS
        .into_iter()
        .enumerate()
        .map(|(index, action)| CaptionButton {
            action,
            label: match action {
                CaptionAction::Minimize => Text::CaptionMinimize,
                CaptionAction::ToggleMaximize if maximized => Text::CaptionRestore,
                CaptionAction::ToggleMaximize => Text::CaptionMaximize,
                CaptionAction::Close => Text::CaptionClose,
            }
            .in_language(language),
            bounds: rect(bounds, index),
            enabled: enabled(handle, action),
            hovered: state.controls.hovered.get() == Some(action),
            pressed: state.controls.pressed.get() == Some(action),
            maximized,
        })
        .collect()
}

pub(super) fn hit(handle: HWND, state: &CaptionState, point: POINT) -> Option<CaptionAction> {
    if state.fullscreen.get() {
        return None;
    }
    let bounds = state.controls.bounds(handle);
    let point = egui::pos2(point.x as f32, point.y as f32);
    ACTIONS.into_iter().enumerate().find_map(|(index, action)| {
        let bounds = rect(bounds, index);
        (point.x >= bounds.left()
            && point.x < bounds.right()
            && point.y >= bounds.top()
            && point.y < bounds.bottom()
            && enabled(handle, action))
        .then_some(action)
    })
}

pub(super) fn hit_code(action: CaptionAction) -> u32 {
    match action {
        CaptionAction::Minimize => HTMINBUTTON,
        CaptionAction::ToggleMaximize => HTMAXBUTTON,
        CaptionAction::Close => HTCLOSE,
    }
}

pub(super) fn invoke(handle: HWND, action: CaptionAction) -> windows::core::Result<()> {
    if !enabled(handle, action) {
        return Ok(());
    }
    // SAFETY: only a scalar command is queued to our retained HWND. Close reaches
    // winit's CloseRequested and the app's unsaved-edit guard; never destroy here.
    unsafe {
        PostMessageW(
            Some(handle),
            WM_SYSCOMMAND,
            WPARAM(command(handle, action) as usize),
            LPARAM(0),
        )
    }
}

pub(super) fn message(
    handle: HWND,
    state: &CaptionState,
    message: u32,
    _: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let controls = &state.controls;
    if matches!(
        message,
        WM_CANCELMODE | WM_KILLFOCUS | WM_SIZE | WM_CAPTURECHANGED | WM_NCDESTROY
    ) {
        controls.clear(handle, state);
        return None;
    }
    if state.fullscreen.get() {
        return None;
    }
    let mut point = POINT {
        x: i32::from(lparam.0 as i16),
        y: i32::from((lparam.0 >> 16) as i16),
    };
    let nonclient = matches!(
        message,
        WM_NCMOUSEMOVE | WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK | WM_NCLBUTTONUP
    );
    // SAFETY: Windows supplies packed coordinates; the synchronous conversion
    // writes only this local point and retains nothing across callbacks.
    if nonclient && !unsafe { ScreenToClient(handle, &mut point) }.as_bool() {
        return None;
    }
    match message {
        WM_NCMOUSEMOVE => {
            controls.hover(hit(handle, state, point), state);
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE | TME_NONCLIENT,
                hwndTrack: handle,
                ..Default::default()
            };
            // SAFETY: tracking is copied by Windows for this retained HWND.
            let _ = unsafe { TrackMouseEvent(&mut tracking) };
            // Default processing preserves the Windows 11 Snap Layouts hover.
            None
        }
        WM_NCMOUSELEAVE => {
            controls.hover(None, state);
            None
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
            let action = hit(handle, state, point)?;
            controls.hover(Some(action), state);
            controls.pressed.set(Some(action));
            // SAFETY: real owned-window caption input activates/captures this HWND.
            // Capture makes outside release/cancellation deterministic. Only Cells
            // span reentrant messages; no application callback or graphics work runs here.
            unsafe {
                let _ = SetForegroundWindow(handle);
                SetCapture(handle);
            }
            redraw(state);
            Some(LRESULT(0))
        }
        WM_MOUSEMOVE if controls.pressed.get().is_some() => {
            controls.hover(hit(handle, state, point), state);
            Some(LRESULT(0))
        }
        WM_MOUSEMOVE => {
            controls.hover(None, state);
            None
        }
        WM_NCLBUTTONUP | WM_LBUTTONUP if controls.pressed.get().is_some() => {
            let pressed = controls.pressed.take();
            let released = hit(handle, state, point);
            controls.hover(released, state);
            // SAFETY: release only our own capture. The queued action cannot run
            // until this callback returns, so button state is already settled.
            unsafe {
                if GetCapture() == handle {
                    let _ = ReleaseCapture();
                }
            }
            redraw(state);
            if let Some(action) = pressed.filter(|pressed| Some(*pressed) == released)
                && let Err(error) = invoke(handle, action)
            {
                crate::diagnostic!("Caption action failed: {error}");
            }
            Some(LRESULT(0))
        }
        _ => None,
    }
}

#[cfg(test)]
pub(super) mod tests;
