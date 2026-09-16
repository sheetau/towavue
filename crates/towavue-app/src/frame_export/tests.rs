use super::*;
use std::sync::mpsc;
use towavue_core::PlaybackRange;
use winit::platform::windows::EventLoopBuilderExtWindows;

fn advance(session: &mut PlaybackSession) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.pending_video_time().is_none() {
        assert!(Instant::now() < deadline, "frame arrival");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(session.advance_pending());
}

#[test]
fn current_frame_export_keeps_clicked_picture_edits_save_state_and_owner_guards() {
    let Some(root) = crate::tests::isolated_test_root(
        "frame_export::tests::current_frame_export_keeps_clicked_picture_edits_save_state_and_owner_guards",
    ) else {
        return;
    };
    let source = root.join("source.mkv");
    assert!(
        std::process::Command::new(
            PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe")
        )
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=4:duration=2",
            "-c:v",
            "ffv1"
        ])
        .arg(&source)
        .status()
        .expect("fixture process")
        .success()
    );
    struct Trial {
        root: PathBuf,
        source: PathBuf,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let original = std::fs::read(&self.source).expect("source");
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden fixture window");
            let renderer = FrameRenderer::new(&window).expect("D3D11 fixture");
            let (tx, rx) = mpsc::channel();
            let mut app = Application::new(None, move |event| {
                let _ = tx.send(event);
            })
            .expect("app");
            let tab = app.tabs.open_new(self.source.clone(), MediaKind::Video);
            app.path = Some(self.source.clone());
            app.media_kind = Some(MediaKind::Video);
            app.state = PlaybackState::Paused;
            let definition = *command_definitions()
                .iter()
                .find(|command| command.id == CommandId::ExportFrame)
                .expect("frame command");
            assert!(
                !definition.is_enabled(app.command_context()),
                "no frame yet"
            );
            let mut session = PlaybackSession::open(
                &self.source,
                renderer.graphics_device(),
                0.0,
                1.0,
                PlaybackRange::default(),
                |_| {},
            )
            .expect("session");
            session.set_paused(true).expect("paused");
            advance(&mut session);
            app.session = Some(session);
            assert!(definition.is_enabled(app.command_context()));
            assert!(
                shortcuts::defaults().get(CommandId::ExportFrame).is_none(),
                "no new default shortcut"
            );
            let previous = self.root.join("previous.mp4");
            std::fs::write(&previous, b"previous video export").expect("old export");
            app.export_paths.insert(tab, previous.clone());
            let history = app.edits.entry(tab).or_default();
            history.push(EditOperation::SetVolume(0.5), MediaKind::Video);
            history.mark_saved();
            history.push(
                EditOperation::Crop(PixelCrop {
                    x: 2,
                    y: 4,
                    width: 18,
                    height: 16,
                }),
                MediaKind::Video,
            );
            history.push(EditOperation::RotateClockwise, MediaKind::Video);
            let view = app.image_view;
            let intent = app
                .capture_frame_export()
                .expect("capture command-time picture");
            let time = intent.frame.source_time();
            let operations = intent.operations.clone();
            app.pending_dialog = Some(DialogIntent::ExportFrame(intent));
            advance(app.session.as_mut().expect("session"));
            assert_ne!(
                app.session
                    .as_ref()
                    .expect("session")
                    .current_source_video_time(),
                Some(time)
            );
            app.edits
                .get_mut(&tab)
                .expect("history")
                .push(EditOperation::FlipHorizontal, MediaKind::Video);
            let history = app.edits[&tab].clone();
            let current = app
                .session
                .as_ref()
                .expect("session")
                .current_source_video_time();
            let target = self.root.join("frame.png");
            app.finish_dialog(Ok(Some(target.clone())));
            let export = app.active_export.as_ref().expect("frame worker");
            assert_eq!(export.options.output, ExportOutput::VideoFrame);
            assert_eq!(
                export.request.operations, operations,
                "dialog retains the clicked edit snapshot"
            );
            assert!(app.capture_frame_export().is_none(), "one export at a time");
            crate::audio_export_tests::drain_export(&mut app, &rx);
            assert!(app.export_error.is_none());
            assert_eq!(
                std::fs::read(&target).expect("frame PNG"),
                towavue_runtime_windows::edited_video_frame_png(
                    &self.source,
                    time,
                    &operations,
                    &|| false
                )
                .expect("clicked picture")
            );
            assert_eq!(app.edits[&tab], history);
            assert_eq!(app.export_paths[&tab], previous);
            assert_eq!(app.image_view, view);
            assert_eq!(app.state, PlaybackState::Paused);
            assert_eq!(
                app.session
                    .as_ref()
                    .expect("session")
                    .current_source_video_time(),
                current
            );
            assert!(
                app.status_message
                    .as_ref()
                    .expect("status")
                    .0
                    .contains("video save state unchanged")
            );
            assert!(app.pending_guard.is_none());
            let late_target = self.root.join("late-cancel.png");
            app.pending_dialog = Some(DialogIntent::ExportFrame(
                app.capture_frame_export().expect("late-cancel capture"),
            ));
            app.finish_dialog(Ok(Some(late_target.clone())));
            loop {
                if let AppEvent::Export(event) = rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("export completion")
                {
                    if matches!(event, ExportEvent::Finished(_)) {
                        app.handle_ui_action(UiAction::CancelExport);
                        app.handle_export_event(event);
                        break;
                    }
                    app.handle_export_event(event);
                }
            }
            assert!(late_target.exists(), "publication won before cancellation");
            assert_eq!(app.edits[&tab], history);
            assert_eq!(app.export_paths[&tab], previous);
            assert!(
                app.status_message
                    .as_ref()
                    .expect("cancel race status")
                    .0
                    .contains("completed before cancellation")
            );
            for result in [Ok(None), Err(DialogError::OwnerUnavailable)] {
                app.pending_dialog = Some(DialogIntent::ExportFrame(
                    app.capture_frame_export().expect("capture"),
                ));
                app.finish_dialog(result);
                assert!(app.active_export.is_none());
            }
            let cancelled_target = self.root.join("must-not-exist.png");
            for stale_generation in [false, true] {
                let intent = app
                    .capture_frame_export()
                    .expect("capture before owner change");
                app.pending_dialog = Some(DialogIntent::ExportFrame(intent));
                let other = if stale_generation {
                    app.media_generation += 1;
                    None
                } else {
                    Some(
                        app.tabs
                            .open_new(self.root.join("other.mkv"), MediaKind::Video),
                    )
                };
                app.finish_dialog(Ok(Some(cancelled_target.clone())));
                assert!(app.active_export.is_none());
                assert!(!cancelled_target.exists());
                if let Some(other) = other {
                    app.tabs.activate(tab);
                    app.tabs.close(other);
                }
            }
            assert_eq!(app.edits[&tab], history);
            assert_eq!(
                std::fs::read(&self.source).expect("source unchanged"),
                original
            );
            drop(app);
            drop(renderer);
            drop(window);
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial { root, source };
    EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("owned event loop")
        .run_app(&mut trial)
        .expect("native trial");
}
