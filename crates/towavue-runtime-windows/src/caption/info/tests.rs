use super::*;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;

fn check(caption: &NativeCaption, stage: &str) {
    // SAFETY: flush compositor work for this owned native comparison only.
    unsafe {
        DwmFlush().expect("compositor");
    }
    let cached = caption
        .state
        .info
        .read(caption.handle)
        .expect("cached query");
    let native = titlebar_info(caption.handle).expect("native reference");
    assert_eq!(cached.1, native.1, "{stage}: client origin");
    for index in [2, 3, 5] {
        assert_eq!(
            cached.0.rgrect[index], native.0.rgrect[index],
            "{stage}: button {index}"
        );
        assert_eq!(
            cached.0.rgstate[index] & 0x18001,
            native.0.rgstate[index] & 0x18001,
            "{stage}: state {index}"
        );
    }
    eprintln!("PASS caption snapshot: {stage}");
}

fn check_reentrant_invalidation(caption: &NativeCaption) {
    const ID: usize = 0x7476_6963;
    struct Hook {
        handle: HWND,
        state: Rc<CaptionState>,
        fired: Cell<bool>,
    }
    impl Drop for Hook {
        fn drop(&mut self) {
            // SAFETY: this guard is dropped before its retained fixture. Remove
            // the callback before freeing its same-thread boxed state.
            unsafe {
                if !RemoveWindowSubclass(self.handle, Some(callback), ID).as_bool() {
                    // Do not release storage still reachable by a native callback.
                    std::process::abort();
                }
            }
        }
    }
    unsafe extern "system" fn callback(
        handle: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _: usize,
        data: usize,
    ) -> LRESULT {
        // SAFETY: the boxed Hook remains alive until this subclass is removed.
        let hook = unsafe { &*(data as *const Hook) };
        if message == WM_GETTITLEBARINFOEX && !hook.fired.replace(true) {
            hook.state.info.invalidate();
        }
        unsafe { DefSubclassProc(handle, message, wparam, lparam) }
    }
    let hook = Box::new(Hook {
        handle: caption.handle,
        state: caption.state.clone(),
        fired: Cell::new(false),
    });
    // SAFETY: the box has a stable address; callbacks and destruction share the
    // fixture's thread, and Drop removes the hook before releasing the box.
    assert!(
        unsafe {
            SetWindowSubclass(
                caption.handle,
                Some(callback),
                ID,
                (&*hook as *const Hook) as usize,
            )
        }
        .as_bool()
    );
    caption.state.info.invalidate();
    let _ = caption
        .state
        .info
        .read(caption.handle)
        .expect("reentrant query");
    assert!(hook.fired.get());
    assert!(
        caption.state.info.saved.get().is_none(),
        "do not publish across a native invalidation"
    );
    drop(hook);
    check(caption, "after reentrant invalidation");
    assert!(caption.state.info.saved.get().is_some());
}

#[test]
#[ignore = "Owned visible native geometry/state comparison; no physical input"]
fn native_cached_caption_matches_geometry_and_command_changes() {
    const NAME: &str =
        "caption::info::tests::native_cached_caption_matches_geometry_and_command_changes";
    if std::env::var_os("TOWAVUE_CAPTION_INFO_CHILD").is_none() {
        let output = crate::hidden_test_command(std::env::current_exe().expect("test executable"))
            .args(["--exact", NAME, "--ignored", "--nocapture"])
            .env("TOWAVUE_CAPTION_INFO_CHILD", "1")
            .output()
            .expect("owned caption child");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        return;
    }
    struct Trial;
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_visible(false)
                            .with_active(false)
                            .with_title("towavue owned caption cache fixture")
                            .with_inner_size(winit::dpi::PhysicalSize::new(700, 450)),
                    )
                    .expect("owned window"),
            );
            let caption = NativeCaption::new(window.clone()).expect("native caption");
            check(&caption, "hidden");
            assert!(caption.state.info.saved.get().is_none());
            // SAFETY: every operation below targets only this retained fixture.
            unsafe {
                let _ = ShowWindow(caption.handle, SW_SHOWNOACTIVATE);
                check(&caption, "visible");
                assert!(caption.state.info.saved.get().is_some());
                let queries = caption.state.info.queries.get();
                for _ in 0..20 {
                    let _ = caption.controls_bounds();
                    let _ = caption.accessible_buttons(Language::English);
                }
                assert_eq!(
                    caption.state.info.queries.get(),
                    queries,
                    "stable reads use no native titlebar query"
                );
                window.set_title("towavue owned caption cache changed title");
                check(&caption, "title changed");
                assert_eq!(
                    caption.state.info.queries.get(),
                    queries,
                    "title text does not change button geometry"
                );
                check_reentrant_invalidation(&caption);
                for monitor in window.available_monitors() {
                    let position = monitor.position();
                    window.set_outer_position(winit::dpi::PhysicalPosition::new(
                        position.x + 80,
                        position.y + 100,
                    ));
                    check(&caption, "monitor and DPI movement");
                    eprintln!(
                        "PASS native caption DPI: {}",
                        GetDpiForWindow(caption.handle)
                    );
                }
                SetWindowPos(
                    caption.handle,
                    None,
                    100,
                    130,
                    730,
                    510,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
                .expect("move and resize");
                check(&caption, "moved and resized");
                let menu = GetSystemMenu(caption.handle, false);
                let _ = EnableMenuItem(menu, SC_CLOSE, MF_BYCOMMAND | MF_GRAYED);
                check(&caption, "close disabled");
                assert!(
                    !caption
                        .accessible_buttons(Language::English)
                        .iter()
                        .find(|b| b.action == CaptionAction::Close)
                        .expect("close")
                        .enabled
                );
                let _ = EnableMenuItem(menu, SC_CLOSE, MF_BYCOMMAND | MF_ENABLED);
                check(&caption, "close enabled");
                let _ = EnableWindow(caption.handle, false);
                check(&caption, "window disabled");
                let _ = EnableWindow(caption.handle, true);
                check(&caption, "window enabled");
                let style = GetWindowLongPtrW(caption.handle, GWL_STYLE);
                SetWindowLongPtrW(
                    caption.handle,
                    GWL_STYLE,
                    style & !(WS_MAXIMIZEBOX.0 as isize),
                );
                SetWindowPos(
                    caption.handle,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
                .expect("style change");
                check(&caption, "maximize style disabled");
                SetWindowLongPtrW(caption.handle, GWL_STYLE, style);
                SetWindowPos(
                    caption.handle,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
                .expect("restore style");
                check(&caption, "style restored");
                let _ = ShowWindow(caption.handle, SW_SHOWMAXIMIZED);
                check(&caption, "maximized");
                assert!(IsZoomed(caption.handle).as_bool());
                let _ = ShowWindow(caption.handle, SW_RESTORE);
                check(&caption, "restored");
                assert!(!IsZoomed(caption.handle).as_bool());
                caption.set_fullscreen(true);
                assert!(caption.accessible_buttons(Language::English).is_empty());
                caption.set_fullscreen(false);
                check(&caption, "fullscreen restored");
                for message in [WM_THEMECHANGED, WM_SETTINGCHANGE, WM_DWMCOMPOSITIONCHANGED] {
                    SendMessageW(caption.handle, message, None, None);
                    check(&caption, "native settings notification");
                }
                let _ = ShowWindow(caption.handle, SW_SHOWMINNOACTIVE);
                check(&caption, "minimized");
                assert!(caption.state.info.saved.get().is_none());
                let _ = ShowWindow(caption.handle, SW_SHOWNOACTIVATE);
                check(&caption, "restored after minimize");
                let _ = ShowWindow(caption.handle, SW_HIDE);
                check(&caption, "hidden again");
                assert!(caption.state.info.saved.get().is_none());
            }
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: winit::window::WindowId,
            _: WindowEvent,
        ) {
        }
    }
    EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("owned event loop")
        .run_app(&mut Trial)
        .expect("owned native comparison");
}
