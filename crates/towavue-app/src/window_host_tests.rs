use super::*;

#[test]
fn video_export_quality_is_shared_across_existing_and_new_windows_without_affecting_images_or_audio()
 {
    use towavue_runtime_windows::VideoExportQuality;
    let Some(_root) = crate::tests::isolated_test_root(
        "window_host::tests::video_export_quality_is_shared_across_existing_and_new_windows_without_affecting_images_or_audio",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    *host.captured_events.lock().expect("events") = Some(VecDeque::new());
    let first = *host.windows.keys().next().expect("first");
    let second = host.add_application(None).expect("second");
    assert_eq!(
        host.windows[&first].video_export_quality(),
        VideoExportQuality::High
    );
    host.windows
        .get_mut(&first)
        .expect("first")
        .set_video_export_quality(VideoExportQuality::Balanced);
    drain_captured(&mut host);
    assert_eq!(
        host.windows[&second].video_export_quality(),
        VideoExportQuality::Balanced
    );
    let third = host.add_application(None).expect("third");
    assert_eq!(
        host.windows[&third].video_export_quality(),
        VideoExportQuality::Balanced
    );
    host.windows
        .get_mut(&second)
        .expect("second")
        .set_video_export_quality(VideoExportQuality::Smaller);
    drain_captured(&mut host);
    for app in host.windows.values() {
        assert_eq!(
            app.effective_video_export_quality(MediaKind::Video, ExportOutput::Media),
            VideoExportQuality::Smaller
        );
        for (kind, output) in [
            (MediaKind::Image, ExportOutput::Media),
            (MediaKind::Audio, ExportOutput::Media),
            (MediaKind::Video, ExportOutput::AudioOnly),
            (MediaKind::Video, ExportOutput::VideoFrame),
        ] {
            assert_eq!(
                app.effective_video_export_quality(kind, output),
                VideoExportQuality::High
            );
        }
        assert!(app.edits.values().all(|history| !history.is_dirty()));
    }
}

#[test]
fn last_window_closes_while_shell_retirement_keeps_the_event_loop_available() {
    let Some(_root) = crate::tests::isolated_test_root(
        "window_host::tests::last_window_closes_while_shell_retirement_keeps_the_event_loop_available",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    // Keep a separate idle worker alive so retirement cannot win the scheduling
    // race. No Explorer UI, private media or deliberately blocked COM call.
    let pending = FolderOrderProvider::new().expect("controlled Shell lifetime");
    host.windows
        .values_mut()
        .next()
        .expect("window")
        .exit_requested = true;
    assert!(matches!(host.prepare_wait(), ControlFlow::WaitUntil(_)));
    assert!(host.windows.is_empty(), "closing the window must not wait");
    assert!(towavue_runtime_windows::shell_workers_pending());

    let reopened = host
        .add_application(None)
        .expect("launch during retirement");
    host.prepare_wait();
    assert!(host.windows.contains_key(&reopened));
    host.windows
        .get_mut(&reopened)
        .expect("new window")
        .exit_requested = true;
    host.prepare_wait();
    assert!(host.windows.is_empty());
    drop(pending);

    let deadline = Instant::now() + Duration::from_secs(5);
    while towavue_runtime_windows::shell_workers_pending() {
        assert!(Instant::now() < deadline, "Shell workers must finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(host.prepare_wait(), ControlFlow::Poll);
}

#[test]
fn new_tabs_inherit_last_listening_volume_across_windows_without_rewriting_existing_tabs() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::tests::new_tabs_inherit_last_listening_volume_across_windows_without_rewriting_existing_tabs",
    ) else {
        return;
    };
    let paths = ["first.mp4", "second.mp4", "third.mp4", "fourth.mp4"].map(|name| root.join(name));
    for path in &paths {
        std::fs::write(path, []).expect("owned unopened media");
    }
    let mut host = WindowHost::new(None, None).expect("host");
    let first_window = *host.windows.keys().next().expect("first window");
    let first = {
        let app = host.windows.get_mut(&first_window).expect("first app");
        app.open_external(paths[0].clone(), true);
        assert_eq!(app.playback_volume(), 0.5);
        app.set_playback_volume(0.37);
        app.tabs.active().expect("first tab").id
    };
    let second_window = host.add_application(None).expect("second window");
    let second = {
        let app = host.windows.get_mut(&second_window).expect("second app");
        app.open_external(paths[1].clone(), true);
        assert_eq!(app.playback_volume(), 0.37);
        app.set_playback_volume(1.2);
        app.tabs.active().expect("second tab").id
    };
    {
        let app = host.windows.get_mut(&first_window).expect("first app");
        assert_eq!(app.playback_volume(), 0.37, "existing tab stays unchanged");
        app.open_external(paths[2].clone(), true);
        assert_eq!(app.playback_volume(), 1.2);
        app.toggle_playback_mute();
        assert_eq!(app.playback_volume(), 0.0);
        app.filmstrip_open = true;
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: vec![towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![1]),
                path: paths[3].clone(),
                kind: MediaKind::Video,
            }],
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        let original = app.tabs.active().expect("original tab").id;
        app.handle_ui_action(UiAction::OpenFilmstripMedia(paths[3].clone(), true));
        assert!(
            app.filmstrip_open,
            "background creation preserves the source overlay"
        );
        let background = app.tabs.tabs().last().expect("background tab").id;
        assert_ne!(background, app.tabs.active().expect("active tab").id);
        app.set_playback_volume(0.8);
        app.activate_tab(background);
        assert!(
            !app.filmstrip_open,
            "fresh background tab starts without an overlay"
        );
        app.activate_tab(original);
        assert!(app.filmstrip_open, "source tab restores its own overlay");
        app.activate_tab(background);
        assert!(
            !app.filmstrip_open,
            "destination retains its closed overlay"
        );
        assert_eq!(app.playback_volume(), 0.0, "new media remains muted");
        app.toggle_playback_mute();
        assert_eq!(
            app.playback_volume(),
            1.2,
            "inherit the nonzero restore level"
        );
        app.tabs.activate(first);
        assert_eq!(app.playback_volume(), 0.37);
        assert!(app.edits.values().all(|history| !history.is_dirty()));
        assert_eq!(app.edit_state().volume, 1.0);
    }
    {
        let app = host.windows.get_mut(&second_window).expect("second app");
        assert_eq!(app.tabs.active().expect("tab").id, second);
        assert_eq!(app.playback_volume(), 1.2);
        // Activating an older tab is not a volume adjustment.
        app.open_external(paths[0].clone(), true);
        assert_eq!(app.playback_volume(), 1.2);
    }
    let fresh_host = WindowHost::new(None, None).expect("independent launch");
    assert_eq!(
        fresh_host
            .windows
            .values()
            .next()
            .expect("app")
            .playback_volume(),
        0.5
    );
}

#[test]
fn gallery_belongs_to_empty_launches_and_last_media_close_not_transfer_staging() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::tests::gallery_belongs_to_empty_launches_and_last_media_close_not_transfer_staging",
    ) else {
        return;
    };
    let path = root.join("source.bmp");
    tab_transfer::tests::bitmap(&path);
    for initial in [None, Some(path.clone())] {
        let empty_launch = initial.is_none();
        let mut app = Application::new(initial, |_| {}).expect("app");
        assert_eq!(app.tabs.gallery().is_some(), empty_launch);
        app.open_external(path.clone(), false);
        assert_eq!(app.tabs.gallery().is_some(), empty_launch);
        let media = app.tabs.active().expect("media tab").id;
        app.close_tab_unchecked(media);
        assert!(app.tabs.gallery().is_some());
        assert!(app.tabs.tabs().is_empty());
        assert!(!app.exit_requested);
    }
    let mut host = WindowHost::new(None, None).expect("host");
    let source = *host.windows.keys().next().expect("source");
    let transfer = host
        .add_transfer_application()
        .expect("transfer destination");
    assert!(host.windows[&transfer].tabs.is_empty());
    assert!(host.windows[&source].tabs.gallery().is_some());
}

pub(super) fn drain_captured(host: &mut WindowHost) {
    loop {
        let event = host
            .captured_events
            .lock()
            .expect("test events")
            .as_mut()
            .and_then(VecDeque::pop_front);
        let Some(event) = event else { return };
        host.route(event);
    }
}

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
fn hosted_windows_share_seeded_image_previews_after_the_original_owner_closes() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::tests::hosted_windows_share_seeded_image_previews_after_the_original_owner_closes",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let first = *host.windows.keys().next().expect("first");
    let second = host.add_application(None).expect("second");
    let path = root.join("shared-seed.bmp");
    tab_transfer::tests::bitmap(&path);
    let app = host.windows.get_mut(&first).expect("first");
    app.ui_context = Some(fonts::test_context());
    app.tabs.open_new(path.clone(), MediaKind::Image);
    app.load_path(path.clone(), MediaKind::Image);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.image_loading {
        app.finish_image_load();
        assert!(Instant::now() < deadline, "seeded preview deadline");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(app.image_error.is_none());
    // Foreground delivery deliberately precedes thumbnail seeding; wait for that
    // separate contract before checking shared ownership and owner teardown.
    let shared = loop {
        if let Some(preview) = host.windows[&second]
            .preview_cache
            .cached_image(&path)
            .expect("lookup")
        {
            break preview;
        }
        assert!(Instant::now() < deadline, "shared preview seed deadline");
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(shared.source_size, (2, 1));
    host.windows.get_mut(&first).expect("first").exit_requested = true;
    host.remove_closed();
    assert_eq!(
        host.windows[&second]
            .preview_cache
            .cached_image(&path)
            .expect("lookup after close")
            .expect("shared lifetime")
            .image,
        shared.image
    );
    let third = host.add_application(None).expect("replacement");
    assert_eq!(
        host.windows[&third]
            .preview_cache
            .cached_image(&path)
            .expect("new owner lookup")
            .expect("host lifetime")
            .image,
        shared.image
    );
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
    let mut wait = host.prepare_wait();
    assert!(host.windows.is_empty());
    let deadline = Instant::now() + Duration::from_secs(5);
    while let ControlFlow::WaitUntil(retirement) = wait {
        assert!(retirement <= Instant::now() + Duration::from_millis(10));
        assert!(Instant::now() < deadline, "Shell retirement must finish");
        std::thread::sleep(Duration::from_millis(1));
        wait = host.prepare_wait();
    }
    assert_eq!(wait, ControlFlow::Poll);
}

#[test]
#[ignore = "requires a hidden native window and D3D11; verifies final event-loop shutdown"]
fn native_last_window_waits_for_shell_retirement_before_exiting() {
    let Some(_root) = crate::tests::isolated_test_root(
        "window_host::tests::native_last_window_waits_for_shell_retirement_before_exiting",
    ) else {
        return;
    };
    use winit::platform::windows::EventLoopBuilderExtWindows;

    struct Trial {
        host: WindowHost,
        pending: Option<FolderOrderProvider>,
        deadline: Instant,
        completed: bool,
    }

    impl ApplicationHandler<Event> for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.host.start_pending(event_loop, false);
            let app = self.host.windows.values_mut().next().expect("native owner");
            assert_eq!(
                app.window.as_ref().expect("native window").is_visible(),
                Some(false)
            );
            app.request_guarded(GuardedAction::Exit);
            assert!(app.exit_requested);
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
            self.host.user_event(event_loop, event);
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            self.host.window_event(event_loop, id, event);
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            assert!(Instant::now() < self.deadline, "native shutdown deadline");
            self.host.about_to_wait(event_loop);
            assert!(
                self.host.windows.is_empty(),
                "native owner closes immediately"
            );
            if self.pending.is_some() {
                assert!(
                    !event_loop.exiting(),
                    "keep the main STA alive for Shell work"
                );
                assert!(matches!(
                    event_loop.control_flow(),
                    ControlFlow::WaitUntil(_)
                ));
                self.pending.take();
            } else if event_loop.exiting() {
                assert!(!towavue_runtime_windows::shell_workers_pending());
                self.completed = true;
            }
        }
    }

    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    configure_mouse_input(&mut builder);
    let event_loop = builder.build().expect("native event loop");
    let mut trial = Trial {
        host: WindowHost::new(None, Some(event_loop.create_proxy())).expect("host"),
        pending: Some(FolderOrderProvider::new().expect("controlled Shell lifetime")),
        deadline: Instant::now() + Duration::from_secs(10),
        completed: false,
    };
    event_loop
        .run_app(&mut trial)
        .expect("native shutdown event loop");
    assert!(
        trial.completed,
        "production exit gate must finish the event loop"
    );
    eprintln!(
        "PASS native last-window shutdown: hidden owner closed before Shell retirement; event loop exited after native worker completion"
    );
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
        background_prepared: bool,
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
                if !self.background_prepared {
                    // Return to the real event loop for asynchronous resume lookups
                    // before the synchronous recovery/transfer exercise starts.
                    for app in self.host.windows.values_mut() {
                        app.media_duration = Some(Duration::from_secs(3));
                        let path = app.path.clone().expect("generated source");
                        app.tabs.open_new(path.clone(), MediaKind::Video);
                        app.load_path(path, MediaKind::Video);
                    }
                    self.background_prepared = true;
                    event_loop.set_control_flow(ControlFlow::Poll);
                    return;
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
                graphics_tests::exercise(&mut self.host);
                transfer_tests::exercise(&mut self.host, event_loop);
                // Synchronous fixture helpers cannot pump winit reentrantly.
                // Drain real resume results through route; leave other events
                // queued normally so scripted Shell snapshots stay controlled.
                *self.host.captured_events.lock().expect("test events") = Some(VecDeque::new());
                file_operations::tests::exercise(&mut self.host);
                opening_tests::exercise(&mut self.host, event_loop);
                dropping::tests::exercise(&mut self.host, event_loop);
                launch_tests::exercise(&mut self.host, event_loop);
                let pending = self
                    .host
                    .captured_events
                    .lock()
                    .expect("test events")
                    .take()
                    .expect("captured events");
                for event in pending {
                    self.host.route(event);
                }
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
        background_prepared: false,
        deadline: Instant::now() + Duration::from_secs(15),
        completed: false,
        skipped: false,
    };
    event_loop.run_app(&mut trial).expect("native host trial");
    assert!(trial.completed || trial.skipped);
}

#[test]
fn keyboard_shortcut_updates_reach_all_hosted_windows_and_cancel_old_prefixes() {
    let Some(_root) = crate::tests::isolated_test_root(
        "window_host::tests::keyboard_shortcut_updates_reach_all_hosted_windows_and_cancel_old_prefixes",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("shortcut host fixture");
    let second = host.add_application(None).expect("shortcut host fixture");
    for app in host.windows.values_mut() {
        app.entered_shortcut
            .push("Ctrl+K".parse().expect("valid test key"));
        app.prefix_started = Some(Instant::now());
    }
    let mut bindings = shortcuts::defaults();
    bindings.set(CommandId::OpenFile, "F2".parse().expect("valid test key"));
    host.route(Event::Window(
        second,
        AppEvent::ShortcutsChanged(bindings.clone()),
    ));
    for app in host.windows.values() {
        assert_eq!(app.shortcuts, bindings);
        assert!(app.entered_shortcut.is_empty() && app.prefix_started.is_none());
    }
}
