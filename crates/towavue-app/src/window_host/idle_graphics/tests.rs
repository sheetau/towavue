use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

fn blocked(host: &mut WindowHost, now: Instant) {
    assert_eq!(host.trim_idle_graphics(now), ControlFlow::Wait);
    assert!(host.idle_graphics.is_none());
}

fn seed(app: &mut WindowApplication, path: PathBuf, color: [u8; 4]) -> egui::TextureId {
    let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
    app.displayed_tab = Some(tab);
    app.path = Some(path.clone());
    app.media_kind = Some(MediaKind::Image);
    app.next_media_instance();
    app.fullscreen = true;
    let decoded = Arc::new(DecodedImage {
        format: "test",
        frames: vec![towavue_runtime_windows::DecodedImageFrame {
            width: 1024,
            height: 1024,
            rgba: color.repeat(1024 * 1024),
            delay: Duration::ZERO,
        }],
    });
    app.image = Some(
        app.image_texture_cache
            .load(app.ui_context.as_ref().expect("context"), &path, decoded)
            .expect("original"),
    );
    app.render_frame();
    app.image.as_ref().expect("image").texture.id()
}

fn pixels(app: &mut WindowApplication) -> [u8; 4] {
    app.render_frame();
    let width = app.window.as_ref().expect("window").inner_size().width as usize;
    let pixels = app
        .renderer
        .as_mut()
        .expect("renderer")
        .verification_surface_rgba()
        .expect("GPU pixels");
    let center = ((pixels.len() / 4 / width / 2) * width + width / 2) * 4;
    pixels[center..center + 4].try_into().expect("RGBA")
}

#[test]
fn shared_device_trims_once_only_after_all_empty_frames_and_reopens() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::idle_graphics::tests::shared_device_trims_once_only_after_all_empty_frames_and_reopens",
    ) else {
        return;
    };
    struct Trial {
        root: PathBuf,
        proxy: EventLoopProxy<Event>,
        waiting: Option<(WindowHost, Instant)>,
    }
    impl ApplicationHandler<Event> for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let mut host = WindowHost::new(None, Some(self.proxy.clone())).expect("host");
            let first = *host.windows.keys().next().expect("first");
            let second = host.add_application(None).expect("second");
            host.start_pending(event_loop, false);
            if host.windows.values().any(|app| app.renderer.is_none()) {
                eprintln!("SKIP idle graphics: shared hardware D3D11 surfaces unavailable");
                event_loop.exit();
                return;
            }
            let now = Instant::now();
            let first_texture = seed(
                host.windows.get_mut(&first).expect("first"),
                self.root.join("first.png"),
                [30, 80, 160, 255],
            );
            seed(
                host.windows.get_mut(&second).expect("second"),
                self.root.join("second.png"),
                [90, 160, 30, 255],
            );
            let first_pixels = pixels(host.windows.get_mut(&first).expect("first"));
            assert_eq!(first_pixels, [30, 80, 160, 255]);
            let app = host.windows.get_mut(&second).expect("second");
            app.close_tab_unchecked(app.tabs.active().expect("second tab").id);
            app.render_frame();
            blocked(&mut host, now);
            assert_eq!(
                pixels(host.windows.get_mut(&first).expect("surviving window")),
                first_pixels
            );
            assert!(
                host.windows[&first]
                    .renderer
                    .as_ref()
                    .expect("renderer")
                    .verification_managed_textures()
                    .iter()
                    .any(|(id, _)| *id == first_texture)
            );

            let app = host.windows.get_mut(&first).expect("first");
            app.close_tab_unchecked(app.tabs.active().expect("last tab").id);
            blocked(&mut host, now);
            host.windows.get_mut(&first).expect("first").render_frame();
            host.prepare_wait();
            let deadline = host
                .idle_graphics
                .as_ref()
                .expect("host scheduler arms trim")
                .deadline;
            let now = deadline - Duration::from_secs(1);
            assert_eq!(
                host.trim_idle_graphics(now),
                ControlFlow::WaitUntil(deadline)
            );
            assert_eq!(
                host.trim_idle_graphics(deadline - Duration::from_millis(1)),
                ControlFlow::WaitUntil(deadline)
            );

            // A same-window reopen cancels the timer even before any new GPU frame.
            for kind in [MediaKind::Image, MediaKind::Audio, MediaKind::Video] {
                let app = host.windows.get_mut(&second).expect("second");
                let tab = app.tabs.open_new(self.root.join("pending"), kind);
                app.state = PlaybackState::Playing;
                blocked(&mut host, deadline);
                let app = host.windows.get_mut(&second).expect("second");
                app.close_tab_unchecked(tab);
                blocked(&mut host, deadline);
                host.windows
                    .get_mut(&second)
                    .expect("second")
                    .render_frame();
                assert_eq!(
                    host.trim_idle_graphics(now),
                    ControlFlow::WaitUntil(deadline)
                );
            }
            for mode in 0..8 {
                let app = host.windows.get_mut(&first).expect("first");
                match mode {
                    0 => app.pending_folder = Some((0, FolderIntent::Open)),
                    1 => app.initial_path = Some(self.root.join("pending.png")),
                    2 => app.export_error = Some("Pending error dialog".into()),
                    3 => {
                        app.graphics_recovery_request = Some(GraphicsRecoveryRequest {
                            position: MediaTime::ZERO,
                            state: PlaybackState::Paused,
                            retry: false,
                        })
                    }
                    4 => app.palette_open = true,
                    5 => app.grid_open = true,
                    6 => app.filmstrip_open = true,
                    _ => egui::Popup::open_id(
                        app.ui_context.as_ref().expect("context"),
                        "idle-menu".into(),
                    ),
                }
                blocked(&mut host, deadline);
                let app = host.windows.get_mut(&first).expect("first");
                app.pending_folder = None;
                app.initial_path = None;
                app.export_error = None;
                app.graphics_recovery_request = None;
                app.palette_open = false;
                app.grid_open = false;
                app.filmstrip_open = false;
                egui::Popup::close_all(app.ui_context.as_ref().expect("context"));
                assert_eq!(
                    host.trim_idle_graphics(now),
                    ControlFlow::WaitUntil(deadline)
                );
            }
            host.windows.get_mut(&first).expect("first").graphics_epoch += 1;
            blocked(&mut host, deadline);
            host.windows.get_mut(&first).expect("first").render_frame();
            assert_eq!(
                host.trim_idle_graphics(now),
                ControlFlow::WaitUntil(deadline)
            );
            let third = host.add_application(None).expect("new empty window");
            host.start_pending(event_loop, false);
            host.windows.get_mut(&third).expect("third").render_frame();
            assert_eq!(
                host.trim_idle_graphics(deadline),
                ControlFlow::WaitUntil(deadline + Duration::from_secs(1))
            );
            host.windows.get_mut(&third).expect("third").exit_requested = true;
            host.remove_closed();
            assert_eq!(
                host.trim_idle_graphics(now),
                ControlFlow::WaitUntil(deadline)
            );
            // Even if an entire open/close cycle occurs between host polls, its frame token changes.
            let app = host.windows.get_mut(&first).expect("first");
            let tab = app
                .tabs
                .open_new(self.root.join("fast.png"), MediaKind::Image);
            app.close_tab_unchecked(tab);
            app.render_frame();
            let fresh_deadline = deadline + Duration::from_secs(1);
            assert_eq!(
                host.trim_idle_graphics(deadline),
                ControlFlow::WaitUntil(fresh_deadline)
            );
            let before = host.windows[&first]
                .renderer
                .as_ref()
                .expect("renderer")
                .verification_memory()
                .expect("memory before idle");
            assert_eq!(host.trim_idle_graphics(fresh_deadline), ControlFlow::Wait);
            let after = host.windows[&first]
                .renderer
                .as_ref()
                .expect("renderer")
                .verification_memory()
                .expect("memory after idle");
            let mib = |bytes: u64| bytes as f64 / 1048576.0;
            eprintln!(
                "HOST_IDLE_MEMORY private_before_after_mib={:.1}/{:.1} gpu_local_nonlocal_before_after_mib={:?}/{:?}; production host trim, test-controlled time, two hidden windows",
                mib(before.private_bytes),
                mib(after.private_bytes),
                before
                    .gpu_local_nonlocal
                    .as_ref()
                    .ok()
                    .map(|usage| usage.map(mib)),
                after
                    .gpu_local_nonlocal
                    .as_ref()
                    .ok()
                    .map(|usage| usage.map(mib))
            );
            assert!(host.idle_graphics.as_ref().expect("idle state").attempted);
            assert_eq!(
                host.trim_idle_graphics(fresh_deadline + Duration::from_secs(10)),
                ControlFlow::Wait
            );
            assert!(host.idle_graphics.as_ref().expect("idle state").attempted);
            // A real upload/readback after production Trim must preserve both hosted surfaces.
            for (key, color) in [(first, [210, 60, 20, 255]), (second, [20, 150, 210, 255])] {
                let app = host.windows.get_mut(&key).expect("reopen window");
                seed(app, self.root.join(format!("reopen-{}.png", key.0)), color);
                assert_eq!(pixels(app), color);
            }
            blocked(&mut host, fresh_deadline + Duration::from_secs(20));
            eprintln!(
                "PASS idle graphics: two shared-device surfaces, surviving original pixels, no trim with any media tab, final-close frame receipt, delayed once-only trim, cancellation and GPU reopen"
            );
            for app in host.windows.values_mut() {
                app.close_tab_unchecked(app.tabs.active().expect("reopened tab").id);
                app.render_frame();
            }
            self.waiting = Some((host, Instant::now()));
        }
        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            if let Some((host, _)) = &mut self.waiting {
                host.window_event(event_loop, id, event);
            }
        }
        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
            if let Some((host, _)) = &mut self.waiting {
                host.user_event(event_loop, event);
            }
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            let Some((host, started)) = &mut self.waiting else {
                return;
            };
            host.about_to_wait(event_loop);
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "real idle timer timeout"
            );
            if host
                .idle_graphics
                .as_ref()
                .is_some_and(|idle| idle.attempted)
            {
                assert!(started.elapsed() >= Duration::from_secs(1));
                eprintln!(
                    "PASS idle graphics real event-loop wake: {:.3}s",
                    started.elapsed().as_secs_f64()
                );
                event_loop.set_control_flow(ControlFlow::Poll);
                event_loop.exit();
            }
        }
    }
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    let event_loop = builder.build().expect("event loop");
    let proxy = event_loop.create_proxy();
    event_loop
        .run_app(&mut Trial {
            root,
            proxy,
            waiting: None,
        })
        .expect("trial");
}
