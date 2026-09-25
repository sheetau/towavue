use super::*;
use crate::SavedWindowPlacement;

impl NativeCaption {
    /// Snapshot only normal placement. Fullscreen retains its pre-entry snapshot;
    /// minimizing never becomes a next-launch visibility instruction.
    pub fn saved_placement(&self) -> Option<SavedWindowPlacement> {
        if self.state.fullscreen.get() {
            return self.windowed_placement.get();
        }
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        // SAFETY: retained same-thread HWND; native output stays on this stack.
        unsafe {
            GetWindowPlacement(self.handle, &mut placement).ok()?;
            let rect = placement.rcNormalPosition;
            let saved = SavedWindowPlacement {
                bounds: [rect.left, rect.top, rect.right, rect.bottom],
                dpi: GetDpiForWindow(self.handle),
                maximized: IsZoomed(self.handle).as_bool()
                    || (IsIconic(self.handle).as_bool()
                        && placement.flags.0 & WPF_RESTORETOMAXIMIZED.0 != 0),
            };
            saved.valid().then_some(saved)
        }
    }

    /// Restore a not-yet-shown window before surface creation. The caller restores
    /// maximization only at its normal show boundary, since winit shows on maximize.
    pub fn restore_placement(
        &self,
        saved: SavedWindowPlacement,
        minimum: winit::dpi::LogicalSize<u32>,
    ) -> windows::core::Result<()> {
        // SAFETY: all native calls are synchronous on the retained window thread.
        // Persisted workspace coordinates go only to SetWindowPlacement. The
        // later clamp uses newly queried screen coordinates, never that record.
        unsafe {
            if !saved.valid()
                || IsWindowVisible(self.handle).as_bool()
                || self.state.fullscreen.get()
            {
                return Err(windows::core::Error::from_hresult(
                    windows::Win32::Foundation::E_INVALIDARG,
                ));
            }
            let [left, top, right, bottom] = saved.bounds;
            let placement = WINDOWPLACEMENT {
                length: size_of::<WINDOWPLACEMENT>() as u32,
                showCmd: SW_HIDE.0 as u32,
                rcNormalPosition: RECT {
                    left,
                    top,
                    right,
                    bottom,
                },
                ..Default::default()
            };
            SetWindowPlacement(self.handle, &placement)?;
            let dpi = GetDpiForWindow(self.handle);
            let mut outer = RECT::default();
            let mut client = RECT::default();
            GetWindowRect(self.handle, &mut outer)?;
            GetClientRect(self.handle, &mut client)?;
            let monitor = MonitorFromRect(&outer, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(monitor, &mut info).ok()?;
            let mut work = info.rcWork;
            // Keep the visible frame in the work area while allowing its native
            // invisible resize border outside. Otherwise edge-aligned windows
            // drift inward on every launch. Query thickness at the restored DPI;
            // if DWM cannot report it, conservatively keep the entire outer frame.
            let mut visible_border = 0_u32;
            if DwmGetWindowAttribute(
                self.handle,
                windows::Win32::Graphics::Dwm::DWMWA_VISIBLE_FRAME_BORDER_THICKNESS,
                (&mut visible_border as *mut u32).cast(),
                size_of::<u32>() as u32,
            )
            .is_ok()
            {
                let side =
                    ((outer.right - outer.left - client.right) / 2 - visible_border as i32).max(0);
                let bottom =
                    (outer.bottom - outer.top - client.bottom - visible_border as i32).max(0);
                work.left -= side;
                work.right += side;
                work.bottom += bottom;
            }
            let scale = |extent: i32| {
                ((i64::from(extent) * i64::from(dpi) + i64::from(saved.dpi) / 2)
                    / i64::from(saved.dpi)) as i32
            };
            let minimum = minimum.to_physical::<u32>(f64::from(dpi) / 96.0);
            let width = scale(right - left)
                .max(minimum.width as i32 + outer.right - outer.left - client.right)
                .min(work.right - work.left);
            let height = scale(bottom - top)
                .max(minimum.height as i32 + outer.bottom - outer.top - client.bottom)
                .min(work.bottom - work.top);
            SetWindowPos(
                self.handle,
                None,
                outer.left.clamp(work.left, work.right - width),
                outer.top.clamp(work.top, work.bottom - height),
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
