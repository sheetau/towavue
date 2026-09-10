use super::*;

fn marker(text: &str) -> AppEvent {
    AppEvent::FileRevealed(Err(std::io::Error::other(text)))
}

fn status(app: &WindowApplication) -> Option<&str> {
    app.status_message.as_ref().map(|(text, _)| text.as_str())
}

fn has_accessibility_tree(app: &mut WindowApplication) -> bool {
    let window = app.window.as_ref().expect("native window");
    let input = app
        .ui_state
        .as_mut()
        .expect("UI state")
        .take_egui_input(window);
    let context = app.ui_context.clone().expect("context");
    let output = context.run_ui(input, |ui| {
        ui.label("Scoped accessibility probe");
    });
    let has_tree = output.platform_output.accesskit_update.is_some();
    app.renderer
        .as_mut()
        .expect("renderer")
        .render_ui(&context, output)
        .expect("upload probe texture changes");
    has_tree
}

#[test]
fn notifications_stay_with_their_window_and_closed_keys_are_not_reused() {
    let Some(_) = crate::tests::isolated_test_root(
        "window_host::tests::notifications_stay_with_their_window_and_closed_keys_are_not_reused",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let first = *host.windows.keys().next().expect("first");
    let second = host.add_application(None).expect("second");
    host.route(Event::Window(first, marker("first")));
    host.route(Event::Window(second, marker("second")));
    assert!(
        status(&host.windows[&first])
            .expect("status")
            .ends_with("first")
    );
    assert!(
        status(&host.windows[&second])
            .expect("status")
            .ends_with("second")
    );
    host.windows.get_mut(&first).expect("first").exit_requested = true;
    host.route(Event::Window(first, marker("late before removal")));
    assert!(
        status(&host.windows[&first])
            .expect("status")
            .ends_with("first")
    );
    host.prepare_wait();
    assert!(!host.windows.contains_key(&first));
    let third = host.add_application(None).expect("replacement");
    assert!(third > second && second > first);
    host.route(Event::Window(first, marker("late after removal")));
    host.route(Event::Window(WindowKey(u64::MAX), marker("unknown")));
    assert!(status(&host.windows[&third]).is_none());
    assert!(
        status(&host.windows[&second])
            .expect("status")
            .ends_with("second")
    );
}

#[test]
fn waiting_uses_the_earliest_window_deadline_regardless_of_iteration_order() {
    let now = Instant::now();
    let near = now + Duration::from_secs(10);
    let far = now + Duration::from_secs(20);
    let choices = [
        ControlFlow::Wait,
        ControlFlow::Poll,
        ControlFlow::WaitUntil(near),
        ControlFlow::WaitUntil(far),
    ];
    for a in choices {
        for b in choices {
            for c in choices {
                let flow = earliest_wait(earliest_wait(a, b), c);
                assert_eq!(flow, earliest_wait(c, earliest_wait(b, a)));
                let expected = if [a, b, c].contains(&ControlFlow::Poll) {
                    ControlFlow::Poll
                } else {
                    [a, b, c]
                        .into_iter()
                        .filter_map(|flow| match flow {
                            ControlFlow::WaitUntil(at) => Some(at),
                            _ => None,
                        })
                        .min()
                        .map_or(ControlFlow::Wait, ControlFlow::WaitUntil)
                };
                assert_eq!(flow, expected);
            }
        }
    }
}

#[test]
fn close_guards_and_repaint_deadlines_are_window_local() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::tests::close_guards_and_repaint_deadlines_are_window_local",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let first = *host.windows.keys().next().expect("first");
    let second = host.add_application(None).expect("second");
    let now = Instant::now();
    let near = now + Duration::from_secs(10);
    let far = now + Duration::from_secs(20);
    for (key, deadline) in [(first, near), (second, far)] {
        let app = host.windows.get_mut(&key).expect("window");
        app.state = PlaybackState::Paused;
        app.ui_repaint_at = Some(deadline);
    }
    assert_eq!(host.prepare_wait(), ControlFlow::WaitUntil(near));
    let app = host.windows.get_mut(&first).expect("first");
    let tab = app
        .tabs
        .open_new(root.join("memory-only.png"), MediaKind::Image);
    let mut history = EditHistory::default();
    history.push(EditOperation::FlipHorizontal, MediaKind::Image);
    app.edits.insert(tab, history);
    app.request_guarded(GuardedAction::Exit);
    assert!(app.pending_guard.is_some() && !app.exit_requested);
    host.prepare_wait();
    assert_eq!(host.windows.len(), 2);
    let app = host.windows.get_mut(&first).expect("first");
    app.resolve_guard(GuardDecision::Cancel);
    assert!(app.edits[&tab].is_dirty() && !app.exit_requested);
    app.request_guarded(GuardedAction::Exit);
    app.resolve_guard(GuardDecision::Discard);
    assert_eq!(host.prepare_wait(), ControlFlow::WaitUntil(far));
    assert_eq!(host.windows.len(), 1);
    host.windows
        .get_mut(&second)
        .expect("second")
        .request_guarded(GuardedAction::Exit);
    assert_eq!(host.prepare_wait(), ControlFlow::Poll);
    assert!(host.windows.is_empty());
}

#[test]
#[ignore = "requires native hidden windows and H264 D3D11VA; generated silent video only"]
fn native_host_routes_workers_and_keeps_other_windows_alive_after_close() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::tests::native_host_routes_workers_and_keeps_other_windows_alive_after_close",
    ) else {
        return;
    };
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let path = root.join("silent.mp4");
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    let output = std::process::Command::new("ffmpeg.exe")
        .args(["-v", "error", "-i"])
        .arg(source)
        .args(["-map", "0:v:0", "-c", "copy", "-an"])
        .arg(&path)
        .output()
        .expect("silent generated fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    struct Trial {
        host: WindowHost,
        path: PathBuf,
        received: BTreeSet<WindowKey>,
        closed: Option<WindowKey>,
        deadline: Instant,
        completed: bool,
        skipped: bool,
    }
    impl ApplicationHandler<Event> for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let started = Instant::now();
            let probe = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("owned hardware probe window"),
            );
            match FrameRenderer::new(&probe) {
                Ok(renderer) => renderer.release_surface(),
                Err(error) => {
                    eprintln!("SKIP native window host: D3D11 unavailable: {error}");
                    self.skipped = true;
                    event_loop.exit();
                    return;
                }
            }
            drop(probe);
            self.host.add_application(None).expect("second application");
            self.host.start_pending(event_loop, false);
            eprintln!("Native host startup elapsed: {:?}", started.elapsed());
            for app in self.host.windows.values_mut() {
                assert_eq!(
                    app.window.as_ref().expect("started window").is_visible(),
                    Some(false)
                );
                assert!(app.native_prompt.is_none());
                app.tabs.open_new(self.path.clone(), MediaKind::Video);
                app.load_path(self.path.clone(), MediaKind::Video);
                let notify = Arc::clone(&app.notify);
                std::thread::spawn(move || notify(marker("scoped worker")))
                    .join()
                    .expect("worker");
            }
            eprintln!("Native host sessions started: {:?}", started.elapsed());
        }
        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
            let marker_key = match &event {
                Event::Window(key, AppEvent::FileRevealed(_)) => Some(*key),
                _ => None,
            };
            self.host.user_event(event_loop, event);
            if let Some(key) = marker_key {
                if Some(key) == self.closed {
                    assert!(!self.host.windows.contains_key(&key));
                } else {
                    assert!(
                        status(&self.host.windows[&key])
                            .expect("worker status")
                            .ends_with("scoped worker")
                    );
                    self.received.insert(key);
                }
            }
        }
        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            assert!(
                Instant::now() < self.deadline,
                "native host window event deadline: {event:?}"
            );
            self.host.window_event(event_loop, id, event);
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if self.completed || self.skipped {
                event_loop.set_control_flow(ControlFlow::Poll);
                event_loop.exit();
                return;
            }
            assert!(
                Instant::now() < self.deadline,
                "native host deadline: received {:?}, closed {:?}, media {:?}",
                self.received,
                self.closed,
                self.host
                    .windows
                    .iter()
                    .map(|(key, app)| (
                        key,
                        app.state,
                        app.pending_time,
                        app.session.as_ref().map(PlaybackSession::metrics)
                    ))
                    .collect::<Vec<_>>()
            );
            // Hidden HWNDs do not receive the ordinary paint wakeup. Keep real
            // worker/event-loop routing, but explicitly render only these owned windows.
            let ids: Vec<_> = self
                .host
                .windows
                .values()
                .filter_map(|app| app.window.as_ref().map(|window| window.id()))
                .collect();
            for id in ids {
                self.host
                    .window_event(event_loop, id, WindowEvent::RedrawRequested);
            }
            for app in self.host.windows.values_mut() {
                assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
            }
            if self.closed.is_none()
                && self.received.len() == 2
                && self.host.windows.values().all(|app| {
                    app.session
                        .as_ref()
                        .is_some_and(|session| session.metrics().presented_frame_count > 0)
                })
            {
                let keys: Vec<_> = self.host.windows.keys().copied().collect();
                for app in self.host.windows.values_mut() {
                    let metrics = app.session.as_ref().expect("session").metrics();
                    if metrics.hardware_frame_count == 0 {
                        eprintln!(
                            "SKIP native window host: decoder selected software, not D3D11VA"
                        );
                        self.skipped = true;
                        event_loop.exit();
                        return;
                    }
                    assert_eq!(metrics.cpu_transfer_count, 0);
                    if app.state == PlaybackState::Playing {
                        app.toggle_pause();
                    }
                }
                let first = keys[0];
                let second = keys[1];
                assert_eq!(
                    self.host.windows[&first].media_generation,
                    self.host.windows[&second].media_generation,
                    "window-local session identities deliberately collide"
                );
                assert_eq!(
                    self.host.windows[&first].generation,
                    self.host.windows[&second].generation
                );
                let first_id = self.host.windows[&first]
                    .window
                    .as_ref()
                    .expect("window")
                    .id();
                let second_id = self.host.windows[&second]
                    .window
                    .as_ref()
                    .expect("window")
                    .id();
                assert_ne!(first_id, second_id);
                for app in self.host.windows.values() {
                    app.ui_context
                        .as_ref()
                        .expect("context")
                        .disable_accesskit();
                }
                self.host.user_event(
                    event_loop,
                    accesskit_winit::Event {
                        window_id: first_id,
                        window_event: accesskit_winit::WindowEvent::InitialTreeRequested,
                    }
                    .into(),
                );
                assert!(has_accessibility_tree(
                    self.host.windows.get_mut(&first).expect("first")
                ));
                assert!(!has_accessibility_tree(
                    self.host.windows.get_mut(&second).expect("second")
                ));
                self.host.user_event(
                    event_loop,
                    accesskit_winit::Event {
                        window_id: second_id,
                        window_event: accesskit_winit::WindowEvent::InitialTreeRequested,
                    }
                    .into(),
                );
                assert!(has_accessibility_tree(
                    self.host.windows.get_mut(&second).expect("second")
                ));
                let survivor_generation = self.host.windows[&second].generation;
                let survivor_time = self.host.windows[&second].current_position();
                let app = self.host.windows.get_mut(&first).expect("first");
                let tab = app.tabs.active().expect("tab").id;
                app.edits
                    .entry(tab)
                    .or_default()
                    .push(EditOperation::FlipHorizontal, MediaKind::Video);
                self.host
                    .window_event(event_loop, first_id, WindowEvent::CloseRequested);
                let app = self.host.windows.get_mut(&first).expect("first");
                assert!(app.pending_guard.is_some() && !app.exit_requested);
                app.resolve_guard(GuardDecision::Cancel);
                assert!(app.edits[&tab].is_dirty());
                let late = Arc::clone(&app.notify);
                self.host
                    .window_event(event_loop, first_id, WindowEvent::CloseRequested);
                self.host
                    .windows
                    .get_mut(&first)
                    .expect("first")
                    .resolve_guard(GuardDecision::Discard);
                self.host.prepare_wait();
                assert_eq!(self.host.windows.len(), 1);
                assert!(!event_loop.exiting());
                self.host
                    .window_event(event_loop, first_id, WindowEvent::CloseRequested);
                let survivor = self.host.windows.get_mut(&second).expect("survivor");
                survivor
                    .ui_context
                    .as_ref()
                    .expect("context")
                    .disable_accesskit();
                self.host.user_event(
                    event_loop,
                    accesskit_winit::Event {
                        window_id: first_id,
                        window_event: accesskit_winit::WindowEvent::InitialTreeRequested,
                    }
                    .into(),
                );
                let survivor = self.host.windows.get_mut(&second).expect("survivor");
                assert!(!has_accessibility_tree(survivor));
                survivor.render_frame();
                assert!(survivor.playback_error.is_none());
                assert_eq!(survivor.generation, survivor_generation);
                assert_eq!(survivor.current_position(), survivor_time);
                assert_eq!(
                    survivor
                        .session
                        .as_ref()
                        .expect("session")
                        .metrics()
                        .cpu_transfer_count,
                    0
                );
                self.closed = Some(first);
                let third = self
                    .host
                    .add_application(None)
                    .expect("replacement application");
                assert!(third > second);
                self.host.start_pending(event_loop, false);
                let notify = Arc::clone(&self.host.windows[&third].notify);
                std::thread::spawn(move || {
                    late(marker("stale"));
                    notify(marker("scoped worker"));
                })
                .join()
                .expect("late and replacement worker");
            } else if self.closed.is_some() && self.received.len() == 3 {
                assert_eq!(self.host.windows.len(), 2);
                assert!(
                    self.host
                        .windows
                        .values()
                        .all(|app| status(app).is_none_or(|text| !text.ends_with("stale")))
                );
                let ids: Vec<_> = self
                    .host
                    .windows
                    .values()
                    .map(|app| app.window.as_ref().expect("window").id())
                    .collect();
                for id in ids {
                    self.host
                        .window_event(event_loop, id, WindowEvent::CloseRequested);
                }
                self.host.about_to_wait(event_loop);
                assert!(self.host.windows.is_empty() && event_loop.exiting());
                assert_eq!(event_loop.control_flow(), ControlFlow::Poll);
                self.completed = true;
                eprintln!(
                    "PASS native window host: scoped real worker/playback notifications and native-ID accessibility, two hidden shared-device windows, dirty close/cancel/discard, surviving video frame/generation, monotonic replacement identity, stale worker ignored and last-window exit"
                );
                return;
            }
            self.host.about_to_wait(event_loop);
            event_loop.set_control_flow(earliest_wait(
                event_loop.control_flow(),
                ControlFlow::WaitUntil(self.deadline),
            ));
        }
    }
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    configure_mouse_input(&mut builder);
    let event_loop = builder.build().expect("event loop");
    let host = WindowHost::new(None, Some(event_loop.create_proxy())).expect("host");
    let mut trial = Trial {
        host,
        path,
        received: BTreeSet::new(),
        closed: None,
        deadline: Instant::now() + Duration::from_secs(15),
        completed: false,
        skipped: false,
    };
    event_loop.run_app(&mut trial).expect("native host trial");
    assert!(trial.completed || trial.skipped);
}
