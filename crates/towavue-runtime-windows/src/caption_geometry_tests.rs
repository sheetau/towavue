use super::*;
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmFlush};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Geometry {
    dpi: u32,
    maximized: bool,
    outer_right_gap: i32,
    visible_right_gap: i32,
    client_right_gap: i32,
    visible_top_gap: i32,
    button_size: [(i32, i32); 3],
    client_size: (i32, i32),
}

fn geometry(window: &Window, name: &str, state: &str) -> Geometry {
    let RawWindowHandle::Win32(raw) = window.window_handle().expect("window handle").as_raw()
    else {
        panic!("Windows fixture");
    };
    let handle = HWND(raw.hwnd.get() as *mut _);
    let mut outer = RECT::default();
    let mut visible = RECT::default();
    let mut client = RECT::default();
    let (info, origin) = titlebar_info(handle).expect("native button rectangles");
    // SAFETY: all queries address an owned live window on this event-loop thread;
    // outputs are separate stack values and no pointers survive the native calls.
    let result = unsafe {
        assert_ne!(GetForegroundWindow(), handle, "reference must not activate");
        GetWindowRect(handle, &mut outer).expect("outer bounds");
        GetClientRect(handle, &mut client).expect("client bounds");
        DwmGetWindowAttribute(
            handle,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut visible as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
        .expect("visible bounds");
        assert_eq!(info.rgstate[5] & 0x18000, 0, "visible close button");
        let close = info.rgrect[5];
        assert!(close.right > close.left && close.bottom > close.top);
        Geometry {
            dpi: GetDpiForWindow(handle),
            maximized: IsZoomed(handle).as_bool(),
            outer_right_gap: outer.right - close.right,
            visible_right_gap: visible.right - close.right,
            client_right_gap: origin.x + client.right - close.right,
            visible_top_gap: close.top - visible.top,
            button_size: [2, 3, 5].map(|index| {
                let rect = info.rgrect[index];
                (rect.right - rect.left, rect.bottom - rect.top)
            }),
            client_size: (client.right, client.bottom),
        }
    };
    eprintln!(
        "CAPTION_REFERENCE {name} {state}: {result:?}; outer={outer:?}; visible={visible:?}; client_origin={origin:?}; close={:?}",
        info.rgrect[5]
    );
    result
}

#[test]
#[ignore = "briefly displays owned standard/custom windows across monitors; native API diagnostics, no physical input"]
fn compare_native_reference_geometry_and_fullscreen_restore() {
    const NAME: &str =
        "caption::geometry_tests::compare_native_reference_geometry_and_fullscreen_restore";
    if std::env::var_os("TOWAVUE_CAPTION_REFERENCE_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", NAME, "--include-ignored", "--nocapture"])
            .env("TOWAVUE_CAPTION_REFERENCE_CHILD", "1")
            .output()
            .expect("isolated native reference trial");
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    struct Trial;
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            for custom in [false, true] {
                let name = if custom {
                    "towavue-frame"
                } else {
                    "standard-frame"
                };
                let window = Arc::new(
                    event_loop
                        .create_window(
                            Window::default_attributes()
                                .with_title(format!("towavue owned caption reference: {name}"))
                                .with_visible(false)
                                .with_active(false)
                                .with_inner_size(winit::dpi::LogicalSize::new(720.0, 300.0)),
                        )
                        .expect("owned reference window"),
                );
                let RawWindowHandle::Win32(raw) = window.window_handle().expect("handle").as_raw()
                else {
                    panic!("Windows fixture");
                };
                let handle = HWND(raw.hwnd.get() as *mut _);
                // SAFETY: change only this test window; retain the standard frame
                // style while suppressing activation during the visible trial.
                unsafe {
                    let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
                    SetWindowLongPtrW(handle, GWL_EXSTYLE, style | WS_EX_NOACTIVATE.0 as isize);
                }
                let caption = custom
                    .then(|| NativeCaption::new(window.clone()).expect("custom native frame"));
                let monitors: Vec<_> = window.available_monitors().collect();
                for monitor in monitors {
                    let position = monitor.position();
                    window.set_outer_position(winit::dpi::PhysicalPosition::new(
                        position.x + 40,
                        position.y + 40,
                    ));
                    let size = winit::dpi::LogicalSize::new(720.0, 300.0);
                    if let Some(caption) = &caption {
                        caption
                            .resize_client(size.to_physical(window.scale_factor()))
                            .expect("matching client size");
                    } else {
                        let _ = window.request_inner_size(size);
                    }
                    // SAFETY: this trial owns the HWND and runs on its window thread.
                    unsafe {
                        let _ = ShowWindow(handle, SW_SHOWNOACTIVATE);
                        DwmFlush().expect("DWM settled");
                    }
                    let baseline = geometry(&window, name, "normal-before");
                    assert!(!baseline.maximized);
                    for maximized in [false, true] {
                        if maximized {
                            window.set_maximized(true);
                            unsafe {
                                DwmFlush().expect("maximized DWM");
                            }
                        }
                        let before = geometry(
                            &window,
                            name,
                            if maximized {
                                "maximized-before"
                            } else {
                                "normal-before-cycle"
                            },
                        );
                        assert_eq!(before.maximized, maximized);
                        for cycle in 0..3 {
                            let _guard = caption
                                .as_ref()
                                .and_then(NativeCaption::suppress_transitions);
                            if let Some(caption) = &caption {
                                caption.set_fullscreen(true);
                                // Commit the fullscreen native state before restoring;
                                // back-to-back calls alone could mask compositor state.
                                unsafe {
                                    DwmFlush().expect("fullscreen DWM");
                                }
                                caption.set_fullscreen(false);
                            } else {
                                window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(
                                    window.current_monitor(),
                                )));
                                unsafe {
                                    DwmFlush().expect("fullscreen DWM");
                                }
                                window.set_fullscreen(None);
                            }
                            let immediate = geometry(
                                &window,
                                name,
                                &format!("restore-immediate-max{maximized}-cycle{cycle}"),
                            );
                            unsafe {
                                DwmFlush().expect("restored DWM");
                            }
                            let settled = geometry(
                                &window,
                                name,
                                &format!("restore-settled-max{maximized}-cycle{cycle}"),
                            );
                            assert_eq!(immediate.maximized, maximized);
                            assert_eq!(settled.maximized, maximized);
                            assert_eq!(
                                settled.button_size, before.button_size,
                                "restored native button dimensions"
                            );
                        }
                    }
                    window.set_maximized(false);
                    unsafe {
                        DwmFlush().expect("normal DWM");
                    }
                    let restored = geometry(&window, name, "normal-after-maximize-fullscreen");
                    assert!(!restored.maximized);
                    assert_eq!(
                        restored.button_size, baseline.button_size,
                        "return to normal button size"
                    );
                    unsafe {
                        let _ = ShowWindow(handle, SW_HIDE);
                    }
                }
                drop(caption);
                drop(window);
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
    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("native reference loop");
    event_loop
        .run_app(&mut Trial)
        .expect("native reference trial");
}
