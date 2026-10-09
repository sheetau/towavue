use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "hidden large-original startup failure and retry regression"]
fn large_original_returns_after_startup_device_retry() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::startup_recovery_tests::large_original_returns_after_startup_device_retry",
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
    std::fs::write(&path, bytes).expect("owned original");
    let path = canonical_shell_path(&path).expect("identity");
    let original = towavue_runtime_windows::decode_image(&path).expect("reference original");
    struct Trial {
        host: WindowHost,
        job: Arc<towavue_runtime_windows::StartupDevice>,
        original: DecodedImage,
        complete: bool,
    }
    impl ApplicationHandler<Event> for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.host.start_pending(event_loop, false);
            self.job.release();
            let key = *self.host.windows.keys().next().expect("app");
            let app = self.host.windows.get_mut(&key).expect("app");
            let deadline = Instant::now() + Duration::from_secs(10);
            while !app.startup_failed || app.pending_folder.is_some() {
                app.finish_folder_load();
                app.finish_image_load();
                assert!(
                    Instant::now() < deadline,
                    "initial failure and folder deadline"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(app.renderer.is_none() && app.startup_surface.is_some());
            assert!(app.image.is_none());
            let id = app.displayed_tab.expect("original tab");
            let history = app.edits.get(&id).cloned();
            let request_generation = app.image_generation;
            eprintln!(
                "LARGE_RETRY BEFORE image_loading={} image_error={:?} state={:?}",
                app.image_loading, app.image_error, app.state
            );
            app.native_prompt = app.queued_recovery.take();
            assert!(matches!(
                app.native_prompt,
                Some(FallbackPrompt::Recovery { .. })
            ));
            app.finish_native_prompt(Ok(PromptResponse::Retry));
            self.host.recover_pending_graphics();
            let app = self.host.windows.get_mut(&key).expect("recovered");
            assert!(app.renderer.is_some() && !app.startup_failed);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                app.finish_folder_load();
                app.finish_image_load();
                app.render_frame();
                if app.image.is_some() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "Retry did not restore the loaded original: error={:?}, loading={}, state={:?}",
                    app.image_error,
                    app.image_loading,
                    app.state
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(*app.image.as_ref().expect("restored original").decoded == self.original);
            assert!(app.image_error.is_none() && app.playback_error.is_none());
            assert_eq!(app.displayed_tab, Some(id));
            assert_eq!(app.edits.get(&id), history.as_ref());
            assert_eq!(
                app.image_generation, request_generation,
                "no new decode request"
            );
            assert!(!app.image_loading && app.image_sequence.awaiting.is_none());
            assert_eq!(
                app.window.as_ref().expect("window").is_visible(),
                Some(false)
            );
            self.complete = true;
            eprintln!(
                "LARGE_RETRY PASS original_4097x2_exact=true same_tab_and_history=true same_request_generation=true normal_presentation_completed=true hidden=true"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    configure_mouse_input(&mut builder);
    let event_loop = builder.build().expect("event loop");
    let mut host = WindowHost::new(Some(path), Some(event_loop.create_proxy())).expect("host");
    let job = Arc::new(towavue_runtime_windows::StartupDevice::controlled_failure(
        || {},
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
        complete: false,
    };
    event_loop
        .run_app(&mut trial)
        .expect("hidden retry control");
    assert!(trial.complete);
}
