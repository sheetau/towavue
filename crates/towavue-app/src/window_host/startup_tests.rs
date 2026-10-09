use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "hidden ordinary-app startup control"]
fn ordinary_startup_preserves_ui_and_shared_job_after_first_close() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::startup_tests::ordinary_startup_preserves_ui_and_shared_job_after_first_close",
    ) else {
        return;
    };
    let source = root.join("startup.bmp");
    crate::tab_transfer::tests::bitmap(&source);
    let source = canonical_shell_path(&source).expect("fixture identity");
    struct Trial {
        host: WindowHost,
        first: WindowKey,
        second: WindowKey,
        job: Arc<towavue_runtime_windows::StartupDevice>,
        source: PathBuf,
        completed: bool,
        expected: Option<Vec<u8>>,
        deadline: Instant,
    }
    impl ApplicationHandler<Event> for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.host.start_pending(event_loop, false);
            assert_eq!(self.host.windows.len(), 2);
            for app in self.host.windows.values_mut() {
                assert!(app.renderer.is_none() && app.startup_surface.is_some());
                assert_eq!(
                    app.window.as_ref().expect("window").is_visible(),
                    Some(false)
                );
                app.render_frame();
                assert!(!app.startup_failed);
            }
            let app = self.host.windows.get_mut(&self.second).expect("second");
            let deadline = Instant::now() + Duration::from_secs(10);
            while app.image_loading || app.pending_folder.is_some() {
                app.finish_folder_load();
                app.finish_image_load();
                assert!(Instant::now() < deadline, "image load deadline");
                std::thread::sleep(Duration::from_millis(2));
            }
            app.render_frame();
            assert!(app.renderer.is_none(), "controlled worker still gated");
            assert!(app.image_error.is_none());
            let decoded = towavue_runtime_windows::decode_image(&self.source).expect("original");
            assert!(*app.image.as_ref().expect("image").decoded == decoded);
            assert!(
                app.image_sequence.awaiting.is_none(),
                "ordinary CPU presentation gate"
            );
            let expected = app
                .startup_surface
                .as_ref()
                .expect("CPU surface")
                .verification_rgba();

            self.host
                .windows
                .get_mut(&self.first)
                .expect("first")
                .exit_requested = true;
            self.host.remove_closed();
            assert_eq!(self.host.windows.len(), 1);
            self.expected = Some(expected);
            self.job.release();
        }
        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
            if !matches!(event, Event::StartupGraphics) {
                self.host.route(event);
                return;
            }
            self.host.route(event);
            let expected = self
                .expected
                .take()
                .expect("ready event after first-window close");
            let app = self.host.windows.get_mut(&self.second).expect("survivor");
            app.finish_startup_graphics(false).expect("handoff");
            assert!(
                app.startup_surface.is_some(),
                "GDI retained until GPU presentation"
            );
            let renderer = app.renderer.as_mut().expect("GPU surface");
            assert!(
                self.job.matches(&renderer.graphics_device()),
                "same host device"
            );
            let actual = renderer.verification_surface_rgba().expect("GPU pixels");
            assert_eq!(actual.len(), expected.len());
            let error = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .expect("frame");
            let count = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
            if error > 2 {
                let root = self.source.parent().expect("fixture root");
                std::fs::write(root.join("cpu.rgba"), &expected).expect("CPU evidence");
                std::fs::write(root.join("gpu.rgba"), &actual).expect("GPU evidence");
            }
            assert!(
                error <= 2,
                "app UI maximum channel difference {error}, channels {count}"
            );
            // Flip-discard replaces the writable back buffer on Present. Compare
            // the rendered buffer first, then exercise native visibility publication.
            app.present_startup_or_gpu().expect("handoff publication");
            assert!(app.startup_surface.is_none() && app.startup_device.is_none());
            app.render_frame();
            assert!(app.playback_error.is_none() && app.image_error.is_none());
            let device = app.renderer.as_ref().expect("GPU").graphics_device();
            let third = self
                .host
                .add_application(Some(self.source.clone()))
                .expect("later window");
            self.host.start_pending(event_loop, false);
            let app = self.host.windows.get_mut(&third).expect("later app");
            assert!(app.startup_surface.is_none());
            assert!(self.job.matches(&device));
            assert!(
                self.job
                    .matches(&app.renderer.as_ref().expect("later GPU").graphics_device())
            );
            self.host.start_pending(event_loop, false);
            assert!(
                self.host.startup_device.is_none(),
                "host retires completed job"
            );
            for app in self.host.windows.values_mut() {
                app.exit_requested = true;
            }
            self.host.remove_closed();
            eprintln!(
                "APP_STARTUP HANDOFF full_ui_max_error={error} differing_channels={count} first_closed=true shared_device=true host_event=true"
            );
            exercise_large_and_video(
                event_loop,
                self.source.parent().expect("fixture root"),
                self.host.proxy.clone().expect("proxy"),
            );
            self.completed = true;
            eprintln!(
                "APP_STARTUP PASS full_ui_max_error={error} differing_channels={count} first_closed=true shared_device=true cpu_original_completed=true later_window=true hidden=true"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            assert!(
                Instant::now() < self.deadline,
                "host startup notification deadline"
            );
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.deadline));
        }
    }
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    configure_mouse_input(&mut builder);
    let event_loop = builder.build().expect("event loop");
    let proxy = event_loop.create_proxy();
    let mut host = WindowHost::new(None, Some(proxy.clone())).expect("host");
    let job = Arc::new(towavue_runtime_windows::StartupDevice::controlled(
        move || {
            let _ = proxy.send_event(Event::StartupGraphics);
        },
    ));
    host.startup_started = true;
    host.startup_device = Some(Arc::clone(&job));
    let first = *host.windows.keys().next().expect("first");
    host.windows.get_mut(&first).expect("first").startup_device = Some(Arc::clone(&job));
    let second = host.add_application(Some(source.clone())).expect("second");
    let mut trial = Trial {
        host,
        first,
        second,
        job,
        source,
        completed: false,
        expected: None,
        deadline: Instant::now() + Duration::from_secs(30),
    };
    event_loop.run_app(&mut trial).expect("native app trial");
    assert!(trial.completed);
}

fn exercise_large_and_video(
    event_loop: &ActiveEventLoop,
    root: &Path,
    proxy: EventLoopProxy<Event>,
) {
    let source = root.join("large.bmp");
    let width = 4097_u32;
    let stride = (width * 3).div_ceil(4) * 4;
    let length = 54 + stride * 2;
    let mut bytes = vec![0_u8; length as usize];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&length.to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&width.to_le_bytes());
    bytes[22..26].copy_from_slice(&2_u32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&(stride * 2).to_le_bytes());
    for (i, byte) in bytes[54..].iter_mut().enumerate() {
        *byte = (i % 251) as u8;
    }
    std::fs::write(&source, bytes).expect("large original");
    let original = towavue_runtime_windows::decode_image(&source).expect("large decoded original");
    let mut host = WindowHost::new(Some(source), Some(proxy.clone())).expect("large host");
    let job = Arc::new(towavue_runtime_windows::StartupDevice::controlled(|| {}));
    host.startup_started = true;
    host.startup_device = Some(Arc::clone(&job));
    host.windows
        .values_mut()
        .next()
        .expect("large app")
        .startup_device = Some(Arc::clone(&job));
    host.start_pending(event_loop, false);
    let app = host.windows.values_mut().next().expect("large app");
    assert!(app.renderer.is_none());
    assert_eq!(
        app.ui_context
            .as_ref()
            .expect("context")
            .input(|i| i.max_texture_side),
        2048
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.image_loader.is_idle() || app.pending_folder.is_some() {
        app.finish_folder_load();
        assert!(Instant::now() < deadline, "large load deadline");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(job.get(false).is_none());
    job.release();
    // This branch tests consumption after readiness. The separate gated test
    // proves the UI returns before readiness and resumes via notification.
    while job.get(false).is_none() {
        assert!(Instant::now() < deadline, "large device readiness deadline");
        std::thread::sleep(Duration::from_millis(2));
    }
    app.finish_image_load();
    assert!(
        app.renderer.is_some(),
        "oversized provisional image obtains same ready device"
    );
    assert!(job.matches(&app.renderer.as_ref().expect("GPU").graphics_device()));
    assert!(*app.image.as_ref().expect("full large image").decoded == original);
    assert!(app.image_error.is_none());
    app.render_frame();
    assert!(app.startup_surface.is_none() && app.image_sequence.awaiting.is_none());
    assert_eq!(
        app.window.as_ref().expect("window").is_visible(),
        Some(false)
    );
    app.exit_requested = true;
    host.remove_closed();
    drop(host);
    drop(job);

    let video = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/generated/benchmark-seek-v1/h264-1080.mp4");
    assert!(video.is_file(), "required existing generated video");
    let mut host = WindowHost::new(
        Some(canonical_shell_path(&video).expect("video path")),
        Some(proxy),
    )
    .expect("video host");
    *host.captured_events.lock().expect("captured video events") = Some(VecDeque::new());
    let job = Arc::new(towavue_runtime_windows::StartupDevice::controlled(|| {}));
    host.startup_started = true;
    host.startup_device = Some(Arc::clone(&job));
    host.windows
        .values_mut()
        .next()
        .expect("video app")
        .startup_device = Some(Arc::clone(&job));
    host.start_pending(event_loop, false);
    assert!(
        host.windows.values().all(|app| app.session.is_none()),
        "existing asynchronous resume lookup precedes playback"
    );
    let lookup_deadline = Instant::now() + Duration::from_secs(10);
    let mut received_resume = false;
    while host.windows.values().all(|app| app.session.is_none()) {
        let events: Vec<_> = host
            .captured_events
            .lock()
            .expect("captured video events")
            .as_mut()
            .expect("capture enabled")
            .drain(..)
            .collect();
        for event in events {
            if matches!(event, Event::Window(_, AppEvent::VideoResume(_))) {
                assert!(
                    job.get(false).is_none(),
                    "native creation remains gated until lookup"
                );
                received_resume = true;
                job.release();
            }
            host.route(event);
        }
        assert!(
            Instant::now() < lookup_deadline,
            "initial resume lookup deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(received_resume);
    let app = host.windows.values_mut().next().expect("video app");
    assert!(
        app.renderer.is_some() && app.session.is_some(),
        "initial video waits same job"
    );
    assert!(
        job.matches(
            &app.renderer
                .as_ref()
                .expect("video renderer")
                .graphics_device()
        )
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.load_next_frame();
        app.render_frame();
        assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
        let metrics = app.session.as_ref().expect("video session").metrics();
        if metrics.presented_frame_count > 0 {
            assert!(metrics.hardware_frame_count > 0);
            assert_eq!(metrics.cpu_transfer_count, 0);
            break;
        }
        assert!(Instant::now() < deadline, "initial video frame deadline");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(app.startup_surface.is_none());
    assert_eq!(
        app.window.as_ref().expect("video window").is_visible(),
        Some(false)
    );
    app.exit_requested = true;
    host.remove_closed();
    eprintln!(
        "APP_STARTUP EXTRA large_original_exact=true provisional_limit_wait=true initial_video_shared_device=true hardware_video=true hidden=true"
    );
}

#[test]
#[ignore = "hidden startup completion must not wait for a gated device"]
fn large_completion_keeps_cpu_ui_live_until_device_notification() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::startup_tests::large_completion_keeps_cpu_ui_live_until_device_notification",
    ) else {
        return;
    };
    let path = root.join("large.bmp");
    let width = 4097_u32;
    let stride = (width * 3).div_ceil(4) * 4;
    let length = 54 + stride * 2;
    let mut bytes = vec![0_u8; length as usize];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&length.to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&width.to_le_bytes());
    bytes[22..26].copy_from_slice(&2_u32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&(stride * 2).to_le_bytes());
    for (index, byte) in bytes[54..].iter_mut().enumerate() {
        *byte = (index % 251) as u8;
    }
    std::fs::write(&path, bytes).expect("fixture");
    let path = canonical_shell_path(&path).expect("identity");
    let original = towavue_runtime_windows::decode_image(&path).expect("original reference");
    struct Trial {
        host: WindowHost,
        job: Arc<towavue_runtime_windows::StartupDevice>,
        original: DecodedImage,
        generation: Option<u64>,
        complete: bool,
        deadline: Instant,
    }
    impl ApplicationHandler<Event> for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.host.start_pending(event_loop, false);
            let app = self.host.windows.values_mut().next().expect("app");
            // A watchdog makes a reintroduced blocking recv fail instead of
            // hanging the test process indefinitely. Success is determined by
            // the still-closed gate, not a machine-speed latency threshold.
            let (cancel, wait) = std::sync::mpsc::channel();
            let job = Arc::clone(&self.job);
            let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let watch_released = Arc::clone(&released);
            let watchdog = std::thread::spawn(move || {
                if wait.recv_timeout(Duration::from_secs(10)).is_err() {
                    watch_released.store(true, std::sync::atomic::Ordering::SeqCst);
                    job.release();
                }
            });
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.pending_folder.is_some() || !app.image_loader.is_idle() {
                app.finish_folder_load();
                assert!(
                    Instant::now() < deadline,
                    "original and Shell ordering deadline"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            self.generation = Some(app.image_generation);
            assert!(app.image_loading);
            for _ in 0..3 {
                app.finish_image_load();
                app.render_frame();
                assert!(app.renderer.is_none() && app.startup_surface.is_some());
                assert!(app.image.is_none() && app.image_loading);
                assert!(app.image_error.is_none() && !app.startup_failed);
                assert_eq!(Some(app.image_generation), self.generation);
                assert!(self.job.get(false).is_none(), "device gate remains closed");
            }
            assert!(
                !released.load(std::sync::atomic::Ordering::SeqCst),
                "UI waited for the watchdog to release the device"
            );
            cancel.send(()).expect("stop watchdog");
            watchdog.join().expect("watchdog");
            assert!(
                !app.startup_surface
                    .as_ref()
                    .expect("CPU surface")
                    .verification_rgba()
                    .is_empty()
            );
            eprintln!(
                "STARTUP_NONBLOCKING held_original=true cpu_painted=true device_still_gated=true"
            );
            self.job.release();
        }
        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
            let ready = matches!(event, Event::StartupGraphics);
            self.host.route(event);
            if !ready {
                return;
            }
            let app = self.host.windows.values_mut().next().expect("app");
            app.render_frame();
            assert!(app.renderer.is_some() && app.startup_surface.is_none());
            assert!(*app.image.as_ref().expect("retained original").decoded == self.original);
            assert_eq!(
                Some(app.image_generation),
                self.generation,
                "no second decode request"
            );
            assert!(!app.image_loading && app.image_sequence.awaiting.is_none());
            assert!(app.image_error.is_none() && app.playback_error.is_none());
            assert!(
                self.job
                    .matches(&app.renderer.as_ref().expect("renderer").graphics_device())
            );
            assert_eq!(
                app.window.as_ref().expect("window").is_visible(),
                Some(false)
            );
            self.complete = true;
            eprintln!(
                "STARTUP_NONBLOCKING PASS original_exact=true same_request=true same_device=true notification_resumed=true hidden=true"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            assert!(
                Instant::now() < self.deadline,
                "device notification deadline"
            );
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.deadline));
        }
    }
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    configure_mouse_input(&mut builder);
    let event_loop = builder.build().expect("event loop");
    let proxy = event_loop.create_proxy();
    let mut host = WindowHost::new(Some(path), Some(proxy.clone())).expect("host");
    let job = Arc::new(towavue_runtime_windows::StartupDevice::controlled(
        move || {
            let _ = proxy.send_event(Event::StartupGraphics);
        },
    ));
    host.startup_started = true;
    host.startup_device = Some(Arc::clone(&job));
    host.windows
        .values_mut()
        .next()
        .expect("app")
        .startup_device = Some(Arc::clone(&job));
    let mut trial = Trial {
        host,
        job,
        original,
        generation: None,
        complete: false,
        deadline: Instant::now() + Duration::from_secs(30),
    };
    event_loop
        .run_app(&mut trial)
        .expect("nonblocking native trial");
    assert!(trial.complete);
}
