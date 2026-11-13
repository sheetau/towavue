use super::*;

pub(in crate::caption) fn verify_native_controls(caption: &NativeCaption) {
    let handle = caption.handle;
    let pack = |point: POINT| {
        LPARAM(((point.x as u32 & 0xffff) | ((point.y as u32 & 0xffff) << 16)) as isize)
    };
    let screen = |point: egui::Pos2| {
        let mut point = POINT {
            x: point.x as i32,
            y: point.y as i32,
        };
        // SAFETY: coordinates of this owned test HWND; stack-only output.
        assert!(unsafe { ClientToScreen(handle, &mut point) }.as_bool());
        pack(point)
    };
    let send = |message, point| {
        // SAFETY: messages target only the hidden test HWND with scalar coordinates.
        unsafe { SendMessageW(handle, message, None, Some(point)) }
    };
    for scale in [1.0, 1.25, 2.0] {
        let bounds = egui::Rect::from_min_max(
            egui::pos2(960.0 - 108.0 * scale, 0.0),
            egui::pos2(960.0, 32.0 * scale),
        );
        caption.set_controls_bounds(bounds.translate(egui::vec2(0.0001, 0.0001)));
        let buttons = caption.accessible_buttons(Language::English);
        assert_eq!(buttons.len(), 3);
        for button in buttons {
            assert!((button.bounds.width() - 36.0 * scale).abs() < 0.01);
            assert!((button.bounds.height() - 32.0 * scale).abs() < 0.01);
            for y in [button.bounds.top(), button.bounds.bottom() - 1.0] {
                assert_eq!(
                    send(
                        WM_NCHITTEST,
                        screen(egui::pos2(button.bounds.center().x, y))
                    )
                    .0,
                    hit_code(button.action) as isize
                );
            }
            send(WM_NCMOUSEMOVE, screen(button.bounds.center()));
            assert_eq!(caption.state.controls.hovered.get(), Some(button.action));
            send(WM_NCLBUTTONDOWN, screen(button.bounds.center()));
            assert_eq!(caption.state.controls.pressed.get(), Some(button.action));
            // Releasing on the media cancels instead of invoking the original control.
            send(WM_LBUTTONUP, pack(POINT { x: 100, y: 100 }));
            assert!(caption.state.controls.pressed.get().is_none());
            send(WM_NCLBUTTONDOWN, screen(button.bounds.center()));
            send(WM_CANCELMODE, LPARAM(0));
            assert!(caption.state.controls.pressed.get().is_none());
            send(WM_NCLBUTTONDOWN, screen(button.bounds.center()));
            send(
                WM_LBUTTONUP,
                pack(POINT {
                    x: button.bounds.center().x as i32,
                    y: button.bounds.center().y as i32,
                }),
            );
            // Observe and remove only our queued command before the event loop executes it.
            let mut queued = MSG::default();
            // SAFETY: this test owns the HWND and consumes only its scalar system command.
            unsafe {
                assert!(
                    PeekMessageW(
                        &mut queued,
                        Some(handle),
                        WM_SYSCOMMAND,
                        WM_SYSCOMMAND,
                        PM_REMOVE
                    )
                    .as_bool()
                );
                assert_eq!(queued.wParam.0, command(handle, button.action) as usize);
                assert!(
                    !PeekMessageW(
                        &mut queued,
                        Some(handle),
                        WM_SYSCOMMAND,
                        WM_SYSCOMMAND,
                        PM_REMOVE
                    )
                    .as_bool(),
                    "one gesture invokes once"
                );
            }
        }
    }
    // System-menu state can lag behind maximize/restore. It must not disable
    // our independently painted control or suppress its next queued command.
    // SAFETY: change only this owned fixture window and restore each native flag.
    unsafe {
        let menu = GetSystemMenu(handle, false);
        let previous = EnableMenuItem(menu, SC_MAXIMIZE, MF_BYCOMMAND | MF_GRAYED);
        assert!(enabled(handle, CaptionAction::ToggleMaximize));
        assert!(caption.accessible_buttons(Language::English)[1].enabled);
        let _ = EnableMenuItem(
            menu,
            SC_MAXIMIZE,
            MENU_ITEM_FLAGS(previous.0 as u32) | MF_BYCOMMAND,
        );
        let was_enabled = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(handle, false);
        assert!(!enabled(handle, CaptionAction::ToggleMaximize));
        assert!(!caption.accessible_buttons(Language::English)[1].enabled);
        if !was_enabled.as_bool() {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(handle, true);
        }
        let style = GetWindowLongPtrW(handle, GWL_STYLE);
        SetWindowLongPtrW(handle, GWL_STYLE, style & !(WS_MAXIMIZEBOX.0 as isize));
        assert!(!enabled(handle, CaptionAction::ToggleMaximize));
        SetWindowLongPtrW(handle, GWL_STYLE, style);
    }
    caption.state.controls.layout.set(None);
}
