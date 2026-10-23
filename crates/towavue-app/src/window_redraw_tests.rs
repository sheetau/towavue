use super::*;

#[test]
#[ignore = "requires a hidden native window and D3D11; no physical input"]
fn continuous_native_pointer_input_does_not_starve_coalesced_redraws() {
    let Some(_root) = crate::tests::isolated_test_root(
        "window_host::redraw_tests::continuous_native_pointer_input_does_not_starve_coalesced_redraws",
    ) else {
        return;
    };
    use towavue_runtime_windows::post_verification_cursor_move;
    use winit::platform::windows::EventLoopBuilderExtWindows;

    struct Trial {
        host: WindowHost,
        moves: usize,
        redraws: usize,
        first_frame: Option<u64>,
        completed: bool,
        deadline: Instant,
    }
    impl ApplicationHandler<Event> for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.host.start_pending(event_loop, false);
            let app = self.host.windows.values_mut().next().expect("owner");
            assert_eq!(
                app.window.as_ref().expect("window").is_visible(),
                Some(false)
            );
            app.render_frame();
            post_verification_cursor_move(app.window.as_ref().expect("window"), 20, 100);
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
            if let Event::Window(_, AppEvent::Redraw) = &event {
                self.redraws += 1;
            }
            self.host.user_event(event_loop, event);
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            if matches!(event, WindowEvent::RedrawRequested) {
                // Exclude OS exposure/idle paint: the posted UI event must make progress.
                return;
            }
            let moved = matches!(event, WindowEvent::CursorMoved { .. });
            self.host.window_event(event_loop, id, event);
            if !moved {
                return;
            }
            self.moves += 1;
            let app = self.host.windows.values_mut().next().expect("owner");
            let frame = app.ui_context.as_ref().expect("UI").cumulative_frame_nr();
            let first = *self.first_frame.get_or_insert(frame);
            // Repeated requests in one dispatch must not flood the posted event queue.
            for _ in 0..100 {
                app.request_redraw();
            }
            if self.moves == 200 {
                assert!(
                    frame - first >= 50,
                    "paint must progress before input drains"
                );
                assert!(self.redraws <= self.moves + 10, "requests must coalesce");
                // A pending event for a retired window must be harmless.
                app.exit_requested = true;
                self.host.remove_closed();
                self.completed = true;
                event_loop.exit();
                event_loop.set_control_flow(ControlFlow::Poll);
            } else {
                // Keep a posted input message waiting at all times, ahead of idle WM_PAINT.
                post_verification_cursor_move(
                    app.window.as_ref().expect("window"),
                    20 + self.moves as i16,
                    100,
                );
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            assert!(Instant::now() < self.deadline, "native redraw deadline");
            if self.completed {
                event_loop.exit();
                event_loop.set_control_flow(ControlFlow::Poll);
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.deadline));
            }
        }
    }
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    configure_mouse_input(&mut builder);
    let event_loop = builder.build().expect("native event loop");
    let mut trial = Trial {
        host: WindowHost::new(None, Some(event_loop.create_proxy())).expect("host"),
        moves: 0,
        redraws: 0,
        first_frame: None,
        completed: false,
        deadline: Instant::now() + Duration::from_secs(15),
    };
    event_loop.run_app(&mut trial).expect("native input burst");
    assert!(trial.completed);
}
