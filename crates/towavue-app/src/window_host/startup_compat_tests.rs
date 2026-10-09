use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "hidden startup transfer compatibility control"]
fn pending_cpu_windows_transfer_without_losing_originals_or_edits() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::startup_compat_tests::pending_cpu_windows_transfer_without_losing_originals_or_edits",
    ) else {
        return;
    };
    let path = root.join("original.bmp");
    tab_transfer::tests::bitmap(&path);
    let path = canonical_shell_path(&path).expect("fixture identity");
    struct Trial {
        path: PathBuf,
        proxy: EventLoopProxy<Event>,
        complete: bool,
    }
    impl ApplicationHandler<Event> for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            exercise_move(event_loop, &self.path, &self.proxy);
            exercise_detach(event_loop, &self.path, &self.proxy);
            exercise_filmstrip(event_loop, &self.path, &self.proxy);
            exercise_failure(event_loop, &self.path, &self.proxy);
            exercise_zero_size(event_loop, &self.path, &self.proxy);
            exercise_prepared(event_loop, &self.path, &self.proxy);
            self.complete = true;
            eprintln!(
                "STARTUP_COMPAT PASS hidden=true pending_move=true pending_detach=true pending_filmstrip=true invalid_requests_do_not_wait=true originals_and_edits_retained=true failed_destination_rollback=true shared_device=true"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    configure_mouse_input(&mut builder);
    let event_loop = builder.build().expect("event loop");
    let mut trial = Trial {
        path,
        proxy: event_loop.create_proxy(),
        complete: false,
    };
    event_loop
        .run_app(&mut trial)
        .expect("hidden native control");
    assert!(trial.complete);
}

fn pending_host(
    event_loop: &ActiveEventLoop,
    proxy: &EventLoopProxy<Event>,
) -> (
    WindowHost,
    WindowKey,
    Arc<towavue_runtime_windows::StartupDevice>,
) {
    let mut host = WindowHost::new(None, Some(proxy.clone())).expect("host");
    let source = *host.windows.keys().next().expect("source");
    let job = Arc::new(towavue_runtime_windows::StartupDevice::controlled(|| {}));
    host.startup_started = true;
    host.startup_device = Some(Arc::clone(&job));
    host.windows
        .get_mut(&source)
        .expect("source")
        .startup_device = Some(Arc::clone(&job));
    host.start_pending(event_loop, false);
    assert_pending(&host, source);
    (host, source, job)
}

fn assert_pending(host: &WindowHost, key: WindowKey) {
    let app = &host.windows[&key];
    assert!(app.renderer.is_none() && app.startup_surface.is_some());
    assert_eq!(
        app.window.as_ref().expect("window").is_visible(),
        Some(false)
    );
}

fn install(
    host: &mut WindowHost,
    key: WindowKey,
    path: &Path,
) -> (TabId, Arc<DecodedImage>, EditHistory) {
    let app = host.windows.get_mut(&key).expect("app");
    let original = tab_transfer::tests::decoded(false);
    let id = tab_transfer::tests::install(app, path.to_owned(), Arc::clone(&original));
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let history = app.edits[&id].clone();
    app.render_frame();
    assert!(app.renderer.is_none() && !app.startup_failed);
    (id, original, history)
}

fn assert_image(
    app: &WindowApplication,
    id: TabId,
    original: &Arc<DecodedImage>,
    history: &EditHistory,
) {
    assert_eq!(app.tabs.active_id(), Some(id));
    assert!(Arc::ptr_eq(
        &app.image.as_ref().expect("image").decoded,
        original
    ));
    assert_eq!(&app.edits[&id], history);
}

fn assert_device(
    host: &mut WindowHost,
    key: WindowKey,
    job: &towavue_runtime_windows::StartupDevice,
) {
    let app = host.windows.get_mut(&key).expect("app");
    assert!(job.matches(&app.renderer.as_ref().expect("shared GPU").graphics_device()));
    assert!(
        app.ui_context
            .as_ref()
            .expect("context")
            .input(|i| i.max_texture_side)
            > 2048
    );
    app.render_frame();
    assert!(
        app.startup_surface.is_none() && app.playback_error.is_none() && app.image_error.is_none()
    );
    assert_eq!(
        app.window.as_ref().expect("window").is_visible(),
        Some(false)
    );
}

fn exercise_move(event_loop: &ActiveEventLoop, path: &Path, proxy: &EventLoopProxy<Event>) {
    let (mut host, source, job) = pending_host(event_loop, proxy);
    let target = host.add_application(None).expect("destination");
    host.start_pending(event_loop, false);
    let (id, original, history) = install(&mut host, source, path);
    let request = host.windows[&source]
        .tab_detach_request(id)
        .expect("pending CPU request");
    assert!(host.move_tab(source, target, &request, usize::MAX).is_err());
    host.windows.get_mut(&target).expect("target").export_error = Some("modal control".into());
    assert!(host.move_tab(source, target, &request, 0).is_err());
    host.windows.get_mut(&target).expect("target").export_error = None;
    host.windows
        .get_mut(&source)
        .expect("source")
        .media_generation += 1;
    assert!(host.move_tab(source, target, &request, 0).is_err());
    host.windows
        .get_mut(&source)
        .expect("source")
        .media_generation -= 1;
    assert_pending(&host, source);
    assert_pending(&host, target);
    assert_image(&host.windows[&source], id, &original, &history);
    assert!(
        job.get(false).is_none(),
        "invalid requests never released or waited for device"
    );
    job.release();
    let moved = host
        .move_tab(source, target, &request, 0)
        .expect("move while CPU startup is pending");
    assert_image(&host.windows[&target], moved, &original, &history);
    assert!(
        !host.windows[&source]
            .tabs
            .tabs()
            .iter()
            .any(|tab| tab.id == id)
    );
    assert_device(&mut host, source, &job);
    assert_device(&mut host, target, &job);
}

fn exercise_detach(event_loop: &ActiveEventLoop, path: &Path, proxy: &EventLoopProxy<Event>) {
    let (mut host, source, job) = pending_host(event_loop, proxy);
    let (id, original, history) = install(&mut host, source, path);
    let request = host.windows[&source]
        .tab_detach_request(id)
        .expect("request");
    job.release();
    let result = host.detach_tab_with(source, &request, false, |app, device| {
        assert!(job.matches(&device));
        app.start_on_device(event_loop, Some(device), false)
            .expect("hidden destination");
        Err("injected destination startup failure".into())
    });
    assert_eq!(
        result.expect_err("injected error"),
        "injected destination startup failure"
    );
    assert_eq!(host.windows.len(), 1);
    assert_image(&host.windows[&source], id, &original, &history);
    assert_device(&mut host, source, &job);

    // A separate host exercises successful detach directly from the CPU state.
    let (mut host, source, job) = pending_host(event_loop, proxy);
    let (id, original, history) = install(&mut host, source, path);
    let request = host.windows[&source]
        .tab_detach_request(id)
        .expect("request");
    job.release();
    let target = host
        .detach_tab_with(source, &request, false, |app, device| {
            assert!(job.matches(&device));
            app.start_on_device(event_loop, Some(device), false)
                .map_err(|error| error.to_string())
        })
        .expect("detach pending CPU image");
    let moved = host.windows[&target].tabs.active_id().expect("moved tab");
    assert_image(&host.windows[&target], moved, &original, &history);
    assert_device(&mut host, target, &job);
}

fn exercise_filmstrip(event_loop: &ActiveEventLoop, path: &Path, proxy: &EventLoopProxy<Event>) {
    let (mut host, source, job) = pending_host(event_loop, proxy);
    let (id, original, history) = install(&mut host, source, path);
    let app = host.windows.get_mut(&source).expect("source");
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![0]),
        folder_path: path.parent().expect("folder").to_owned(),
        items: vec![towavue_core::FolderMediaItem {
            identity: towavue_core::ShellIdentity::new(vec![1]),
            path: path.to_owned(),
            kind: MediaKind::Image,
        }],
        sort_columns: Vec::new(),
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 42,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    });
    app.filmstrip_open = true;
    app.request_filmstrip_window(
        path.to_owned(),
        42,
        egui::pos2(30.0, 70.0),
        egui::vec2(10.0, 10.0),
    );
    let request = app
        .pending_window_open
        .take()
        .expect("pending filmstrip request");
    job.release();
    let target = host
        .open_filmstrip_window_with(source, &request, false, |app, device| {
            assert!(job.matches(&device));
            app.start_on_device(event_loop, Some(device), false)
                .map_err(|error| error.to_string())
        })
        .expect("new filmstrip window")
        .expect("current request");
    assert_eq!(host.windows[&target].path.as_deref(), Some(path));
    assert!(!host.windows[&source].filmstrip_open);
    assert_image(&host.windows[&source], id, &original, &history);
    assert_device(&mut host, source, &job);
    assert_device(&mut host, target, &job);
}

fn exercise_failure(event_loop: &ActiveEventLoop, path: &Path, proxy: &EventLoopProxy<Event>) {
    let (mut host, source, old_job) = pending_host(event_loop, proxy);
    let target = host.add_application(None).expect("second pending window");
    host.start_pending(event_loop, false);
    let (id, original, history) = install(&mut host, source, path);
    let expected = host.windows[&source]
        .startup_surface
        .as_ref()
        .expect("CPU frame")
        .verification_rgba();
    let request = host.windows[&source]
        .tab_detach_request(id)
        .expect("request");
    let job = Arc::new(towavue_runtime_windows::StartupDevice::controlled_failure(
        || {},
    ));
    for app in host.windows.values_mut() {
        app.startup_device = Some(Arc::clone(&job));
    }
    host.startup_device = Some(Arc::clone(&job));
    drop(old_job); // Cancels and joins the still-gated unused creation worker.
    job.release();
    assert!(host.move_tab(source, target, &request, 0).is_err());
    assert!(
        host.windows
            .get_mut(&target)
            .expect("target")
            .finish_startup_graphics(true)
            .is_err()
    );
    assert_image(&host.windows[&source], id, &original, &history);
    assert!(
        host.windows[&source]
            .startup_surface
            .as_ref()
            .expect("held frame")
            .verification_rgba()
            == expected
    );
    for key in [source, target] {
        let app = host.windows.get_mut(&key).expect("app");
        assert!(app.startup_failed && app.renderer.is_none());
        assert!(matches!(
            app.queued_recovery,
            Some(FallbackPrompt::Recovery { .. })
        ));
        assert!(
            app.native_prompt.is_none(),
            "hidden control never shows a dialog"
        );
        // Inject the response at the same native-dialog completion boundary.
        app.native_prompt = app.queued_recovery.take();
        app.finish_native_prompt(Ok(if key == source {
            PromptResponse::Cancel
        } else {
            PromptResponse::Retry
        }));
    }
    assert!(host.windows[&source].graphics_recovery_request.is_none());
    assert!(host.windows[&target].graphics_recovery_request.is_some());
    let mut new_devices = 0;
    let mut reused_devices = 0;
    host.recover_pending_graphics_with(|app, device| {
        if device.is_none() {
            new_devices += 1;
        } else {
            reused_devices += 1;
        }
        app.create_graphics_surface(device)
    });
    assert_eq!((new_devices, reused_devices), (1, 1));
    assert!(host.startup_device.is_none());
    for app in host.windows.values_mut() {
        assert!(
            !app.startup_failed && app.startup_surface.is_none() && app.queued_recovery.is_none()
        );
        let limit = app.renderer.as_ref().expect("recovered").max_texture_side();
        app.render_frame();
        assert_eq!(
            app.ui_context
                .as_ref()
                .expect("context")
                .input(|input| input.max_texture_side),
            limit
        );
        assert_eq!(
            app.window.as_ref().expect("window").is_visible(),
            Some(false)
        );
    }
    assert_image(&host.windows[&source], id, &original, &history);
    eprintln!(
        "STARTUP_COMPAT FAILURE held_cpu_pixels_exact=true edits_retained=true cancel_and_retry=true shared_recovery=true input_cap_restored=true"
    );
}

fn exercise_zero_size(event_loop: &ActiveEventLoop, path: &Path, proxy: &EventLoopProxy<Event>) {
    let (mut host, source, job) = pending_host(event_loop, proxy);
    let (id, original, history) = install(&mut host, source, path);
    let app = host.windows.get_mut(&source).expect("app");
    let window = app.window.clone().expect("window");
    let outer = window.outer_size();
    app.startup_surface
        .as_ref()
        .expect("surface")
        .verification_resize_hidden_parent([0, 0])
        .expect("zero-sized hidden parent");
    let zero_area = window.inner_size();
    // The native caption retains a 31-pixel inset on this host. Either zero
    // dimension exercises the application's minimized/zero-area guard.
    assert!(
        zero_area.width == 0 || zero_area.height == 0,
        "zero-area client required: {zero_area:?}"
    );
    app.startup_surface
        .as_mut()
        .expect("surface")
        .resize([0, 0])
        .expect("zero-sized CPU child");
    job.release();
    job.get(true).expect("worker completion").expect("device");
    app.finish_startup_graphics(false)
        .expect("deferred paint handoff");
    assert!(
        app.renderer.is_none(),
        "zero-size paint does not attempt handoff"
    );
    app.finish_startup_graphics(true)
        .expect("explicit operation can obtain the device");
    assert!(job.matches(&app.renderer.as_ref().expect("GPU").graphics_device()));
    assert!(
        app.startup_surface.is_some(),
        "no publication before ordinary frame"
    );
    app.startup_surface
        .as_ref()
        .expect("surface")
        .verification_resize_hidden_parent([outer.width as i32, outer.height as i32])
        .expect("restore hidden parent");
    app.surface_resize_pending = true;
    app.render_frame();
    assert!(
        app.startup_surface.is_none() && app.image_error.is_none() && app.playback_error.is_none()
    );
    assert_image(app, id, &original, &history);
    assert_eq!(window.is_visible(), Some(false));
    eprintln!(
        "STARTUP_COMPAT ZERO native_parent_size={zero_area:?} native_child_zero=true deferred_paint=true forced_device=true restored_frame=true hidden=true physical_minimize=false"
    );
}

fn exercise_prepared(event_loop: &ActiveEventLoop, path: &Path, proxy: &EventLoopProxy<Event>) {
    let video = path.with_file_name("prepared-silent.mp4");
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    let output = crate::tests::hidden_command("ffmpeg.exe")
        .args(["-v", "error", "-i"])
        .arg(&fixture)
        .args(["-map", "0:v:0", "-an", "-c:v", "copy", "-y"])
        .arg(&video)
        .output()
        .expect("silent fixture process");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (mut host, source, job) = pending_host(event_loop, proxy);
    let app = host.windows.get_mut(&source).expect("app");
    let (send, events) = std::sync::mpsc::channel();
    app.notify = Arc::new(Box::new(move |event| {
        let _ = send.send(event);
    }));
    let id = app.tabs.open_new(video, MediaKind::Video);
    app.prepare_playback_tab(id);
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.retained_playback[&id].duration.is_none() {
        assert!(Instant::now() < deadline, "real metadata callback deadline");
        let event = events
            .recv_timeout(Duration::from_secs(2))
            .expect("worker callback");
        app.handle_app_event(event);
    }
    assert!(app.renderer.is_none() && app.retained_playback[&id].session.is_none());
    assert!(
        job.get(false).is_none(),
        "hover preparation does not wait for graphics"
    );
    job.release();
    assert!(
        app.start_prepared_playback(id),
        "explicit transport waits for the existing device"
    );
    assert!(job.matches(&app.renderer.as_ref().expect("renderer").graphics_device()));
    let saved = &app.retained_playback[&id];
    assert!(!saved.prepared_only && saved.session.is_some() && saved.error.is_none());
    assert_eq!(saved.state, PlaybackState::Paused);
    assert_eq!(
        app.window.as_ref().expect("window").is_visible(),
        Some(false)
    );
    eprintln!(
        "STARTUP_COMPAT PREPARED real_metadata_callback=true hover_nonblocking=true explicit_transport=true same_device=true silent_video=true hidden=true"
    );
}
