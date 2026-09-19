use super::*;

fn host() -> (WindowHost, WindowKey, WindowKey) {
    let mut host = WindowHost::new(None, None).expect("host");
    *host.captured_events.lock().expect("events") = Some(VecDeque::new());
    let first = *host.windows.keys().next().expect("first");
    let second = host.add_application(None).expect("second");
    for app in host.windows.values_mut() {
        app.state = PlaybackState::Paused;
    }
    host.updates.enabled = true;
    (host, first, second)
}

fn attach(host: &mut WindowHost, key: WindowKey, path: &Path, dirty: bool) -> TabId {
    let app = host.windows.get_mut(&key).expect("window");
    let id = app.tabs.open_new(path.to_owned(), MediaKind::Image);
    app.path = Some(path.to_owned());
    app.displayed_tab = Some(id);
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    app.ui_context = Some(crate::fonts::test_context());
    app.source_versions.insert(
        id,
        Some(towavue_runtime_windows::FileOperationSource::capture(path).expect("source")),
    );
    if dirty {
        app.edits
            .entry(id)
            .or_default()
            .push(EditOperation::FlipHorizontal, MediaKind::Image);
    }
    id
}

fn ready(host: &mut WindowHost, phase: UpdatePhase, startup: bool) {
    host.update_event(UpdateEvent::Ready {
        version: "1.0.1".parse().expect("version"),
        phase,
        startup,
    });
}

fn choose(host: &mut WindowHost, key: WindowKey, action: Action) {
    host.windows
        .get_mut(&key)
        .expect("window")
        .handle_update_action(action);
    crate::window_host::tests::drain_captured(host);
}

#[test]
fn update_all_window_discard_waits_for_helper_ack_and_cancellation_preserves_edits() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::updates::tests::update_all_window_discard_waits_for_helper_ack_and_cancellation_preserves_edits",
    ) else {
        return;
    };
    let path = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&path);
    let (mut host, first, second) = host();
    let id = attach(&mut host, second, &path, true);
    ready(&mut host, UpdatePhase::Ready, false);
    choose(&mut host, first, Action::Install);
    let token = host.updates.attempt.as_ref().expect("guarding").token;
    assert!(host.windows[&first].update_is_held());
    let approved = host.windows.get_mut(&first).expect("approved peer");
    assert!(!approved.dialog_input_blocked_for_update_save(Some(token)));
    assert!(approved.dialog_input_blocked_for_update_save(Some(token + 1)));
    approved.about_open = true;
    assert!(
        approved.dialog_input_blocked_for_update_save(Some(token)),
        "ordinary dialogs still block publication"
    );
    approved.about_open = false;
    assert!(
        matches!(host.windows[&second].pending_guard, Some(GuardedAction::UpdateExit(t)) if t == token)
    );
    assert!(host.windows.values().all(|app| !app.exit_requested));
    host.windows
        .get_mut(&second)
        .expect("second")
        .resolve_guard(GuardDecision::Cancel);
    host.advance_update();
    host.clear_stale_update_guards();
    assert!(host.updates.attempt.is_none());
    assert!(
        host.windows
            .values()
            .all(|app| !app.exit_requested && app.update_close.is_none())
    );
    assert!(host.windows[&second].edits[&id].is_dirty());
    host.update_event(UpdateEvent::HandoffReady(token));
    assert!(host.updates.attempt.is_none());
    crate::window_host::tests::drain_captured(&mut host);
    host.update_event(UpdateEvent::Cancelled(token));

    ready(&mut host, UpdatePhase::Failed, false);
    choose(&mut host, first, Action::Install);
    host.windows
        .get_mut(&second)
        .expect("second")
        .resolve_guard(GuardDecision::Discard);
    host.advance_update();
    let next = host.updates.attempt.as_ref().expect("preparing");
    assert!(next.step == Step::Preparing);
    let next = next.token;
    assert_ne!(next, token);
    host.update_event(UpdateEvent::HandoffReady(token));
    assert_eq!(
        host.updates
            .attempt
            .as_ref()
            .expect("ignore stale helper")
            .token,
        next
    );
    assert!(host.windows.values().all(|app| !app.exit_requested));
    host.update_event(UpdateEvent::HandoffReady(next));
    assert!(host.update_committing());
    choose(&mut host, first, Action::Cancel);
    assert!(
        host.update_committing(),
        "commit point cannot be reversed by a late click"
    );
    host.update_event(UpdateEvent::Committed(token));
    assert!(host.windows.values().all(|app| !app.exit_requested));
    host.update_event(UpdateEvent::Committed(next));
    assert!(host.windows.values().all(|app| app.exit_requested));
}

#[test]
fn update_cancel_before_ready_event_and_stale_save_continuations_never_close_windows() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::updates::tests::update_cancel_before_ready_event_and_stale_save_continuations_never_close_windows",
    ) else {
        return;
    };
    let (mut host, first, _) = host();
    ready(&mut host, UpdatePhase::Ready, false);
    choose(&mut host, first, Action::Install);
    let token = host.updates.attempt.as_ref().expect("preparing").token;
    // Deliberately route worker readiness before the queued UI cancellation.
    host.windows
        .get_mut(&first)
        .expect("first")
        .handle_update_action(Action::Cancel);
    host.update_event(UpdateEvent::HandoffReady(token));
    assert!(!host.update_committing());
    crate::window_host::tests::drain_captured(&mut host);
    host.update_event(UpdateEvent::Cancelled(token));
    let app = host.windows.get_mut(&first).expect("first");
    app.request_guarded(GuardedAction::UpdateExit(token));
    app.perform_guarded(GuardedAction::UpdateExit(token));
    assert!(!app.exit_requested && app.pending_guard.is_none());
    app.pending_guard = Some(GuardedAction::UpdateExit(token));
    host.clear_stale_update_guards();
    assert!(host.windows[&first].pending_guard.is_none());
    // The OS Close button cancels the attempt even while a dirty guard is open.
    let path = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&path);
    attach(&mut host, first, &path, true);
    ready(&mut host, UpdatePhase::Ready, false);
    choose(&mut host, first, Action::Install);
    host.windows
        .get_mut(&first)
        .expect("first")
        .request_guarded(GuardedAction::Exit);
    host.advance_update();
    host.clear_stale_update_guards();
    assert!(host.updates.attempt.is_none());
    assert!(host.windows.values().all(|app| !app.exit_requested));
}

#[test]
fn update_next_launch_runs_only_before_primary_startup_and_failures_require_a_choice() {
    let Some(_root) = crate::tests::isolated_test_root(
        "window_host::updates::tests::update_next_launch_runs_only_before_primary_startup_and_failures_require_a_choice",
    ) else {
        return;
    };
    let (mut host, first, _) = host();
    ready(&mut host, UpdatePhase::Ready, false);
    choose(&mut host, first, Action::NextLaunch);
    assert!(host.updates.deferring && host.updates.attempt.is_none());
    host.update_event(UpdateEvent::Deferred);
    assert!(host.updates.notice.is_none());
    assert!(host.windows.values().all(|app| !app.exit_requested));
    ready(&mut host, UpdatePhase::NextLaunch, true);
    assert!(
        host.updates.attempt.is_none(),
        "a forwarded launch already released the startup gate"
    );
    for app in host.windows.values_mut() {
        app.update_notice = None;
    }
    host.updates.startup = true;
    ready(&mut host, UpdatePhase::Failed, true);
    assert!(!host.updates.startup && host.updates.attempt.is_none());
    assert!(host.updates.notice.expect("explicit retry notice").failed);
    for app in host.windows.values_mut() {
        app.update_notice = None;
    }
    host.updates.startup = true;
    ready(&mut host, UpdatePhase::NextLaunch, true);
    assert!(host.updates.startup);
    assert!(
        host.updates
            .attempt
            .as_ref()
            .is_some_and(|a| a.step == Step::Preparing)
    );
}

#[test]
fn update_approved_peer_is_invalidated_by_real_source_save_and_active_exports_delay_install() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::updates::tests::update_approved_peer_is_invalidated_by_real_source_save_and_active_exports_delay_install",
    ) else {
        return;
    };
    let path = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&path);
    let before = std::fs::read(&path).expect("original");
    let (mut host, first, second) = host();
    let first_id = attach(&mut host, first, &path, true);
    attach(&mut host, second, &path, false);
    ready(&mut host, UpdatePhase::Ready, false);
    choose(&mut host, first, Action::Install);
    assert!(host.windows[&second].update_is_held());
    host.windows
        .get_mut(&first)
        .expect("first")
        .resolve_guard(GuardDecision::Save);
    assert!(host.windows[&first].active_export.is_some());
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.source_save.is_some() || host.windows[&first].active_export.is_some() {
        assert!(Instant::now() < deadline, "save publication");
        crate::window_host::tests::drain_captured(&mut host);
        host.advance_source_save();
        host.advance_update();
        std::thread::sleep(Duration::from_millis(2));
    }
    host.advance_update();
    host.clear_stale_update_guards();
    assert!(
        host.updates.attempt.is_none(),
        "peer edits changed after its approval"
    );
    assert!(
        host.windows
            .values()
            .all(|app| !app.exit_requested && app.export_error.is_none())
    );
    assert!(!host.windows[&first].edits[&first_id].is_dirty());
    assert_ne!(std::fs::read(&path).expect("saved"), before);
    let token = host.updates.cancelling.expect("cancel worker");
    host.update_event(UpdateEvent::Cancelled(token));
    let app = host.windows.get_mut(&first).expect("first");
    app.edits
        .get_mut(&first_id)
        .expect("history")
        .push(EditOperation::FlipVertical, MediaKind::Image);
    assert!(app.save_source(None));
    ready(&mut host, UpdatePhase::Ready, false);
    // The other window may show the notification, but its Install choice must
    // not approve or terminate an export running in this first window.
    choose(&mut host, second, Action::Install);
    assert!(host.updates.attempt.is_none());
    while host.source_save.is_some() || host.windows[&first].active_export.is_some() {
        assert!(Instant::now() < deadline, "second save publication");
        crate::window_host::tests::drain_captured(&mut host);
        host.advance_source_save();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn update_ready_notice_and_prepare_cancel_render_at_supported_densities() {
    let Some(_root) = crate::tests::isolated_test_root(
        "window_host::updates::tests::update_ready_notice_and_prepare_cancel_render_at_supported_densities",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    for density in [1.0, 1.25, 2.0] {
        for preparing in [false, true] {
            let context = crate::fonts::test_context();
            context.global_style_mut(chrome::style);
            context.set_pixels_per_point(density);
            app.update_notice = (!preparing).then_some(Notice {
                version: "1.0.1".parse().expect("version"),
                failed: true,
            });
            app.update_close = preparing.then_some(Close {
                token: 1,
                approved: Some(app.edits.clone()),
                committing: false,
            });
            assert!(app.modal_input_blocked());
            for pass in 0..3 {
                let mut actions = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(600.0, 360.0),
                        )),
                        events: if pass == 2 {
                            vec![egui::Event::Key {
                                key: egui::Key::Escape,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            }]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                    |_| app.draw_update(&context, &mut actions),
                );
                assert!(!output.shapes.is_empty());
                assert_eq!(
                    actions
                        .iter()
                        .any(|a| matches!(a, UiAction::Update(Action::Cancel))),
                    preparing && pass == 2
                );
            }
        }
    }
}

#[test]
fn update_untitled_save_as_cancel_keeps_private_pixels_and_late_save_cannot_exit() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::updates::tests::update_untitled_save_as_cancel_keeps_private_pixels_and_late_save_cannot_exit",
    ) else {
        return;
    };
    let (mut host, first, second) = host();
    let app = host.windows.get_mut(&first).expect("first");
    app.ui_context = Some(crate::fonts::test_context());
    app.open_pasted_image(crate::image_paste::tests::fixture())
        .expect("owned paste");
    let id = app.tabs.active_id().expect("Untitled");
    let original = app.document_input(id).expect("retained pixels");
    ready(&mut host, UpdatePhase::Ready, false);
    choose(&mut host, first, Action::Install);
    let token = host.updates.attempt.as_ref().expect("guards").token;
    assert!(host.windows[&second].update_is_held());
    let app = host.windows.get_mut(&first).expect("first");
    // Inject the pending native picker boundary, without displaying a picker.
    app.pending_dialog = Some(DialogIntent::Export {
        tab: id,
        source: original.logical_path().to_owned(),
        kind: MediaKind::Image,
        generation: app.media_generation,
        output: ExportOutput::Media,
        continuation: app.pending_guard.take(),
    });
    app.finish_test_dialog(Ok(None));
    crate::window_host::tests::drain_captured(&mut host);
    host.advance_update();
    host.clear_stale_update_guards();
    assert!(host.updates.attempt.is_none());
    assert!(host.windows[&first].path.is_none() && host.windows[&first].edits[&id].is_dirty());
    assert_eq!(
        host.windows[&first]
            .document_input(id)
            .expect("pixels kept"),
        original
    );
    host.update_event(UpdateEvent::Cancelled(token));
    ready(&mut host, UpdatePhase::Ready, false);
    choose(&mut host, first, Action::Install);
    let next = host.updates.attempt.as_ref().expect("new guards").token;
    let target = root.join("saved.png");
    let app = host.windows.get_mut(&first).expect("first");
    let continuation = app.pending_guard.take();
    assert!(app.start_test_save_as(target.clone(), continuation));
    choose(&mut host, second, Action::Cancel);
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.source_save.is_some() || host.windows[&first].active_export.is_some() {
        assert!(Instant::now() < deadline, "accepted Save as completion");
        crate::window_host::tests::drain_captured(&mut host);
        host.advance_source_save();
        host.advance_update();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        host.windows
            .values()
            .all(|app| !app.exit_requested && app.export_error.is_none())
    );
    assert_eq!(host.windows[&first].path.as_ref(), Some(&target));
    assert!(!host.windows[&first].edits[&id].is_dirty());
    assert!(target.is_file());
    host.update_event(UpdateEvent::HandoffReady(next));
    assert!(host.updates.attempt.is_none());
}

#[test]
#[ignore = "requires hidden native D3D11 windows; injected update events, no installation"]
fn native_update_startup_gate_recovers_initial_media_and_dirty_cancel_keeps_every_window() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::updates::tests::native_update_startup_gate_recovers_initial_media_and_dirty_cancel_keeps_every_window",
    ) else {
        return;
    };
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let path = root.join("initial.bmp");
    crate::tab_transfer::tests::bitmap(&path);
    struct Trial {
        host: WindowHost,
        path: PathBuf,
        deadline: Instant,
        completed: bool,
    }
    impl ApplicationHandler<Event> for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.host.updates.startup = true;
            self.host.start_pending(event_loop, false);
            assert!(
                self.host.windows.values().all(
                    |app| app.window.is_none() && app.initial_path.as_ref() == Some(&self.path)
                )
            );
            ready(&mut self.host, UpdatePhase::NextLaunch, true);
            self.host.start_pending(event_loop, false);
            assert!(
                self.host
                    .windows
                    .values()
                    .all(|app| app.window.is_none() && app.media_kind.is_none())
            );
            let token = self
                .host
                .updates
                .attempt
                .as_ref()
                .expect("startup handoff")
                .token;
            self.host.update_event(UpdateEvent::Error {
                message: "Controlled helper failure".into(),
                startup: false,
                operation: Some(token),
            });
            self.host.update_event(UpdateEvent::Cancelled(token));
            self.host.add_application(None).expect("second");
            self.host.start_pending(event_loop, false);
            assert_eq!(self.host.windows.len(), 2);
            assert!(self.host.windows.values().all(|app| {
                app.window.as_ref().expect("native window").is_visible() == Some(false)
            }));
        }
        fn user_event(&mut self, _: &ActiveEventLoop, event: Event) {
            self.host.route(event);
        }
        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            self.host.window_event(event_loop, id, event);
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            assert!(Instant::now() < self.deadline, "native update deadline");
            if !self.completed {
                let first = *self.host.windows.keys().next().expect("first");
                let app = &self.host.windows[&first];
                if app.image_loading || app.image.is_none() {
                    self.host.prepare_wait();
                    event_loop.set_control_flow(ControlFlow::WaitUntil(
                        Instant::now() + Duration::from_millis(10),
                    ));
                    return;
                }
                assert_eq!(app.path.as_ref(), Some(&self.path));
                let app = self.host.windows.get_mut(&first).expect("first");
                let id = app.tabs.active_id().expect("initial media");
                app.edits
                    .entry(id)
                    .or_default()
                    .push(EditOperation::FlipHorizontal, MediaKind::Image);
                ready(&mut self.host, UpdatePhase::Ready, false);
                self.host
                    .windows
                    .get_mut(&first)
                    .expect("first")
                    .render_frame();
                choose(&mut self.host, first, Action::Install);
                assert!(matches!(
                    self.host.windows[&first].pending_guard,
                    Some(GuardedAction::UpdateExit(_))
                ));
                self.host
                    .windows
                    .get_mut(&first)
                    .expect("first")
                    .resolve_guard(GuardDecision::Cancel);
                self.host.advance_update();
                self.host.clear_stale_update_guards();
                assert!(self.host.updates.attempt.is_none());
                assert!(
                    self.host
                        .windows
                        .values()
                        .all(|app| app.window.is_some() && !app.exit_requested)
                );
                assert!(self.host.windows[&first].edits[&id].is_dirty());
                // A launch during final retirement resumes a worker, without
                // consuming a scheduled update as a new primary launch.
                let retired_epoch = self.host.updates.worker_epoch;
                self.host.updates.shutdown_requested = true;
                self.host.update_wait();
                assert!(self.host.updates.worker_epoch > retired_epoch);
                assert!(!self.host.updates.startup);
                self.host.route(Event::Update(
                    retired_epoch,
                    UpdateEvent::Ready {
                        version: "1.0.2".parse().expect("version"),
                        phase: UpdatePhase::NextLaunch,
                        startup: true,
                    },
                ));
                assert!(self.host.updates.notice.is_none() && self.host.updates.attempt.is_none());
                for app in self.host.windows.values_mut() {
                    app.exit_requested = true;
                }
                self.completed = true;
            }
            self.host.about_to_wait(event_loop);
        }
    }
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    configure_mouse_input(&mut builder);
    let event_loop = builder.build().expect("event loop");
    let host = WindowHost::new(Some(path.clone()), Some(event_loop.create_proxy())).expect("host");
    *host.captured_events.lock().expect("events") = Some(VecDeque::new());
    let mut trial = Trial {
        host,
        path,
        deadline: Instant::now() + Duration::from_secs(20),
        completed: false,
    };
    event_loop
        .run_app(&mut trial)
        .expect("native update control");
    assert!(trial.completed);
    eprintln!(
        "PASS hidden native update: scheduled startup did not open media/windows; failed preparation restored initial media; dirty cancellation retained two native windows and edits"
    );
}
