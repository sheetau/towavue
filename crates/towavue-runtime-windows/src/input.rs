use windows::Win32::UI::WindowsAndMessaging::{
    MSG, SendMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};
use winit::event_loop::EventLoopBuilder;
use winit::platform::windows::EventLoopBuilderExtWindows;

/// Preserves queued button coordinates before winit emits its position-less MouseInput.
pub fn configure_mouse_input<T>(builder: &mut EventLoopBuilder<T>) {
    builder.with_msg_hook(|message| {
        // SAFETY: winit invokes this hook on the event-loop thread with a live, aligned MSG
        // before dispatch. Copy it now; neither the pointer nor a reference escapes the callback.
        let message = unsafe { *message.cast::<MSG>() };
        if !message.hwnd.0.is_null() && is_client_button(message.message) {
            // SAFETY: this is the queued message's target and its scalar client coordinates.
            // Synchronous dispatch stays on the owning UI thread; no borrowed data is sent.
            // WM_MOUSEMOVE is not a button message, so this cannot recurse through the hook.
            unsafe {
                SendMessageW(
                    message.hwnd,
                    WM_MOUSEMOVE,
                    Some(message.wParam),
                    Some(message.lParam),
                );
            }
        }
        false
    });
}

fn is_client_button(message: u32) -> bool {
    matches!(
        message,
        WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | WM_RBUTTONDOWN
            | WM_RBUTTONUP
            | WM_MBUTTONDOWN
            | WM_MBUTTONUP
            | WM_XBUTTONDOWN
            | WM_XBUTTONUP
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
    use windows::Win32::UI::WindowsAndMessaging::{WM_MOUSEWHEEL, WM_NCLBUTTONDOWN, WM_PAINT};
    use winit::application::ApplicationHandler;
    use winit::dpi::PhysicalPosition;
    use winit::event::{ElementState, MouseButton, StartCause, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{Window, WindowId};

    #[test]
    fn queued_button_positions_reach_winit_before_each_button_without_cursor_motion() {
        struct Trial {
            window: Option<Window>,
            deadline: Instant,
            position: Option<PhysicalPosition<f64>>,
            received: Vec<(MouseButton, ElementState, Option<PhysicalPosition<f64>>)>,
        }
        impl ApplicationHandler for Trial {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let window = event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_visible(false)
                            .with_inner_size(winit::dpi::PhysicalSize::new(500, 500)),
                    )
                    .expect("hidden mouse-input test window");
                let RawWindowHandle::Win32(handle) =
                    window.window_handle().expect("window handle").as_raw()
                else {
                    panic!("Windows window required");
                };
                let window_handle = HWND(handle.hwnd.get() as *mut _);
                self.window = Some(window);
                for (message, state, x, y) in [
                    (WM_LBUTTONDOWN, 1, 50_i16, 60_i16),
                    (WM_LBUTTONUP, 0, 150, 160),
                    (WM_RBUTTONDOWN, 2, 200, 220),
                    (WM_RBUTTONUP, 0, -20, -30),
                ] {
                    let coordinates = u32::from(x as u16) | (u32::from(y as u16) << 16);
                    // SAFETY: the test retains this owned window until run_app returns. Only scalar
                    // mouse-message fields are queued to its UI thread; the real cursor is not moved.
                    unsafe {
                        PostMessageW(
                            Some(window_handle),
                            message,
                            WPARAM(state),
                            LPARAM(coordinates as isize),
                        )
                        .expect("queue mouse input");
                    }
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.deadline));
            }

            fn new_events(&mut self, event_loop: &ActiveEventLoop, _: StartCause) {
                if Instant::now() >= self.deadline {
                    event_loop.exit();
                }
            }

            fn window_event(
                &mut self,
                event_loop: &ActiveEventLoop,
                _: WindowId,
                event: WindowEvent,
            ) {
                match event {
                    WindowEvent::CursorMoved { position, .. } => self.position = Some(position),
                    WindowEvent::MouseInput { button, state, .. } => {
                        self.received.push((button, state, self.position));
                        if self.received.len() == 4 {
                            event_loop.exit();
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut builder = EventLoop::builder();
        builder.with_any_thread(true);
        configure_mouse_input(&mut builder);
        let event_loop = builder.build().expect("Windows test event loop");
        let mut trial = Trial {
            window: None,
            deadline: Instant::now() + Duration::from_secs(5),
            position: None,
            received: Vec::new(),
        };
        event_loop
            .run_app(&mut trial)
            .expect("dispatch queued input");
        let point = |x, y| Some(PhysicalPosition::new(x, y));
        assert_eq!(
            trial.received,
            vec![
                (MouseButton::Left, ElementState::Pressed, point(50.0, 60.0)),
                (
                    MouseButton::Left,
                    ElementState::Released,
                    point(150.0, 160.0)
                ),
                (
                    MouseButton::Right,
                    ElementState::Pressed,
                    point(200.0, 220.0)
                ),
                (
                    MouseButton::Right,
                    ElementState::Released,
                    point(-20.0, -30.0)
                ),
            ]
        );
    }

    #[test]
    fn only_client_button_messages_need_coordinate_synchronization() {
        for message in [
            WM_LBUTTONDOWN,
            WM_LBUTTONUP,
            WM_RBUTTONDOWN,
            WM_RBUTTONUP,
            WM_MBUTTONDOWN,
            WM_MBUTTONUP,
            WM_XBUTTONDOWN,
            WM_XBUTTONUP,
        ] {
            assert!(is_client_button(message));
        }
        for message in [WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCLBUTTONDOWN, WM_PAINT, 0] {
            assert!(!is_client_button(message));
        }
    }
}
