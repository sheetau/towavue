use super::*;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "briefly shows owned windows for maximize/minimize/fullscreen; native placement evidence"]
fn native_window_placement_preserves_normal_bounds_and_clamps_restore() {
    const NAME: &str = "caption::placement::tests::native_window_placement_preserves_normal_bounds_and_clamps_restore";
    if std::env::var_os("TOWAVUE_PLACEMENT_CHILD").is_none() {
        let output =
            crate::hidden_test_command(std::env::current_exe().expect("owned placement fixture"))
                .args(["--exact", NAME, "--include-ignored", "--nocapture"])
                .env("TOWAVUE_PLACEMENT_CHILD", "1")
                .output()
                .expect("owned placement fixture");
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
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_visible(false)
                            .with_active(false)
                            .with_title("towavue owned placement fixture")
                            .with_inner_size(winit::dpi::LogicalSize::new(680, 440)),
                    )
                    .expect("owned placement fixture"),
            );
            let caption = NativeCaption::new(window.clone()).expect("owned placement fixture");
            let minimum = winit::dpi::LogicalSize::new(420, 260);
            for monitor in window.available_monitors() {
                let pos = monitor.position();
                window
                    .set_outer_position(winit::dpi::PhysicalPosition::new(pos.x + 40, pos.y + 40));
                caption
                    .resize_client(
                        winit::dpi::LogicalSize::new(680, 440).to_physical(window.scale_factor()),
                    )
                    .expect("owned placement fixture");
                let original = caption.saved_placement().expect("owned placement fixture");
                for _ in 0..5 {
                    caption
                        .restore_placement(original, minimum)
                        .expect("owned placement fixture");
                    assert_eq!(
                        window.is_visible(),
                        Some(false),
                        "restoration must stay hidden"
                    );
                    assert_eq!(
                        caption.saved_placement(),
                        Some(original),
                        "no repeated workspace-coordinate drift"
                    );
                }
                window.set_visible(true);
                assert!(
                    caption.restore_placement(original, minimum).is_err(),
                    "visible owner cannot be repositioned by startup restore"
                );
                window.set_maximized(true);
                let maximized = caption.saved_placement().expect("owned placement fixture");
                assert!(maximized.maximized);
                assert_eq!(maximized.bounds, original.bounds);
                window.set_minimized(true);
                assert_eq!(
                    caption.saved_placement(),
                    Some(maximized),
                    "minimized maximized owner retains restore mode"
                );
                window.set_minimized(false);
                window.set_maximized(true);
                assert_eq!(caption.saved_placement(), Some(maximized));
                caption.set_fullscreen(true);
                assert_eq!(
                    caption.saved_placement(),
                    Some(maximized),
                    "fullscreen must retain normal placement"
                );
                caption.set_fullscreen(false);
                window.set_maximized(false);
                window.set_minimized(true);
                assert!(
                    !caption
                        .saved_placement()
                        .expect("owned placement fixture")
                        .maximized
                );
                window.set_minimized(false);
                window.set_visible(false);
                caption
                    .restore_placement(maximized, minimum)
                    .expect("owned placement fixture");
                assert_eq!(window.is_visible(), Some(false));
                assert_eq!(
                    caption
                        .saved_placement()
                        .expect("owned placement fixture")
                        .bounds,
                    original.bounds
                );
                window.set_maximized(true);
                assert!(
                    caption
                        .saved_placement()
                        .expect("owned placement fixture")
                        .maximized,
                    "deferred show restores maximize"
                );
                window.set_maximized(false);
                window.set_visible(false);

                // Match the visible left/bottom edges to the work area. The
                // invisible resize frame may legitimately extend outside it.
                window.set_visible(true);
                unsafe {
                    use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmFlush};
                    DwmFlush().expect("settled fixture");
                    let mut outer = RECT::default();
                    let mut visible = RECT::default();
                    GetWindowRect(caption.handle, &mut outer).expect("outer bounds");
                    DwmGetWindowAttribute(
                        caption.handle,
                        DWMWA_EXTENDED_FRAME_BOUNDS,
                        (&mut visible as *mut RECT).cast(),
                        size_of::<RECT>() as u32,
                    )
                    .expect("visible bounds");
                    let mut info = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    GetMonitorInfoW(MonitorFromRect(&outer, MONITOR_DEFAULTTONEAREST), &mut info)
                        .ok()
                        .expect("work area");
                    let position = winit::dpi::PhysicalPosition::new(
                        info.rcWork.left - (visible.left - outer.left),
                        info.rcWork.bottom - (outer.bottom - outer.top)
                            + (outer.bottom - visible.bottom),
                    );
                    window.set_outer_position(position);
                }
                let at_edge = caption.saved_placement().expect("edge placement");
                window.set_visible(false);
                caption
                    .restore_placement(at_edge, minimum)
                    .expect("restore edge placement");
                assert_eq!(
                    caption.saved_placement(),
                    Some(at_edge),
                    "no invisible-frame inset at screen edges"
                );
                caption
                    .restore_placement(original, minimum)
                    .expect("restore baseline");

                let mut changed_dpi = original;
                changed_dpi.dpi *= 2;
                caption
                    .restore_placement(changed_dpi, winit::dpi::LogicalSize::new(1, 1))
                    .expect("owned placement fixture");
                let scaled = caption.saved_placement().expect("owned placement fixture");
                for (end, start) in [(2, 0), (3, 1)] {
                    assert!(
                        (2 * (scaled.bounds[end] - scaled.bounds[start])
                            - (original.bounds[end] - original.bounds[start]))
                            .abs()
                            <= 1
                    );
                }
                eprintln!(
                    "PLACEMENT dpi={} normal={:?} scaled={:?}",
                    original.dpi, original.bounds, scaled.bounds
                );
                caption
                    .restore_placement(original, minimum)
                    .expect("owned placement fixture");
            }
            for bounds in [
                [100_000, 100_000, 100_100, 100_100],
                [-100_000, -100_000, -90_000, -90_000],
            ] {
                let saved = SavedWindowPlacement {
                    bounds,
                    dpi: 96,
                    maximized: false,
                };
                caption
                    .restore_placement(saved, minimum)
                    .expect("owned placement fixture");
                window.set_visible(true);
                // SAFETY: only synchronous queries of this retained same-thread fixture HWND.
                unsafe {
                    use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmFlush};
                    DwmFlush().expect("settled visible bounds");
                    let mut outer = RECT::default();
                    GetWindowRect(caption.handle, &mut outer).expect("owned placement fixture");
                    let mut info = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    GetMonitorInfoW(MonitorFromRect(&outer, MONITOR_DEFAULTTONEAREST), &mut info)
                        .ok()
                        .expect("owned placement fixture");
                    let mut visible = RECT::default();
                    DwmGetWindowAttribute(
                        caption.handle,
                        DWMWA_EXTENDED_FRAME_BOUNDS,
                        (&mut visible as *mut RECT).cast(),
                        size_of::<RECT>() as u32,
                    )
                    .expect("visible frame");
                    assert!(visible.left >= info.rcWork.left && visible.top >= info.rcWork.top);
                    assert!(
                        visible.right <= info.rcWork.right && visible.bottom <= info.rcWork.bottom
                    );
                }
                let inner = window.inner_size().to_logical::<f64>(window.scale_factor());
                assert!(inner.width >= 420.0 && inner.height >= 260.0);
                window.set_visible(false);
                assert_eq!(window.is_visible(), Some(false));
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
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("owned placement fixture")
        .run_app(&mut Trial)
        .expect("owned placement fixture");
}
