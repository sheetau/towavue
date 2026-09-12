//! Opt-in OS-delivery control, independent of media, egui, caption hooks and Shell workers.

use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "visible 90-second native-input control; requires keys/clicks from an operator or supported UI tool"]
fn native_input_delivery_reaches_a_plain_winit_window() {
    struct Probe {
        window: Option<Window>,
        deadline: Instant,
        keys: usize,
        clicks: usize,
        wheels: usize,
    }

    impl Probe {
        fn title(&self) -> String {
            format!(
                "towavue input control — keys={} clicks={} wheels={}",
                self.keys, self.clicks, self.wheels
            )
        }
    }

    impl ApplicationHandler for Probe {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.window = Some(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_title(self.title())
                            .with_inner_size(LogicalSize::new(640, 360)),
                    )
                    .expect("visible control window"),
            );
            eprintln!("INPUT CONTROL READY: send a key, click and wheel inside this window");
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::KeyboardInput {
                    event,
                    is_synthetic: false,
                    ..
                } if event.state == ElementState::Pressed => self.keys += 1,
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                } => self.clicks += 1,
                WindowEvent::MouseWheel { .. } => self.wheels += 1,
                WindowEvent::CloseRequested => event_loop.exit(),
                _ => return,
            }
            // Only event counts are recorded, never typed text or pointer coordinates.
            let title = self.title();
            self.window
                .as_ref()
                .expect("control window")
                .set_title(&title);
            eprintln!("{title}");
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if Instant::now() >= self.deadline {
                event_loop.exit();
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.deadline));
            }
        }
    }

    let mut probe = Probe {
        window: None,
        deadline: Instant::now() + Duration::from_secs(90),
        keys: 0,
        clicks: 0,
        wheels: 0,
    };
    EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("control event loop")
        .run_app(&mut probe)
        .expect("native input trial");
    assert!(
        probe.keys > 0 && probe.clicks > 0 && probe.wheels > 0,
        "input delivery was not demonstrated: {}",
        probe.title()
    );
}
