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
            let shown = app.status_message.as_ref().expect("success notice").1;
            assert_eq!(app.export_notice_target(shown), Some(target.as_path()));
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
            // Exercise the same command/dialog path after a real native source
            // replacement. The host save handoff is simulated here; Ctrl+S is
            // deliberately outside this retained-input regression.
            drop(app.session.take());
            let prepared = towavue_runtime_windows::prepare_source_save(
                towavue_runtime_windows::FileOperationSource::capture(&self.source)
                    .expect("loaded target version"),
                ExportRequest {
                    source: self.source.clone(),
                    target: self.source.clone(),
                    kind: MediaKind::Video,
                    operations: vec![EditOperation::Crop(PixelCrop {
                        x: 0,
                        y: 0,
                        width: 32,
                        height: 48,
                    })],
                    hardware_encode: false,
                },
                ExportOptions::default(),
                &std::sync::atomic::AtomicBool::new(false),
                &|_| {},
                &|_| {},
            )
            .expect("prepare source replacement");
            let (saved_tx, saved_rx) = mpsc::channel();
            towavue_runtime_windows::commit_source_save(prepared, move |result| {
                saved_tx.send(result).ok();
            })
            .expect("publication worker");
            let saved = saved_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("source publication")
                .expect("saved source");
            let backup = saved.original_path().to_owned();
            let saved_bytes = std::fs::read(&self.source).expect("saved video");
            assert_ne!(saved_bytes, original);
            app.source_versions
                .insert(tab, Some(saved.current_source().clone()));
            app.source_backings.insert(tab, saved.into());
            app.displayed_tab = Some(tab);
            let mut session = PlaybackSession::open_input(
                app.media_input(&self.source),
                renderer.graphics_device(),
                0.0,
                1.0,
                PlaybackRange::default(),
                true,
                |_| {},
            )
            .expect("retained source session");
            advance(&mut session);
            assert_eq!(session.video_geometry(), Some((64, 48, 1.0)));
            app.session = Some(session);
            let intent = app.capture_frame_export().expect("retained frame command");
            assert_eq!(intent.input.logical_path(), self.source);
            assert_eq!(intent.frame.source_path(), backup);
            let expected = towavue_runtime_windows::edited_video_frame_png(
                &backup,
                intent.frame.source_time(),
                &intent.operations,
                &|| false,
            )
            .expect("original frame with current edits");
            let retained_target = self.root.join("retained-frame.png");
            intent.finish(&mut app, Ok(Some(retained_target.clone())));
            assert_eq!(
                app.active_export
                    .as_ref()
                    .expect("frame job")
                    .request
                    .source,
                self.source,
                "completion ownership stays logical"
            );
            crate::audio_export_tests::drain_export(&mut app, &rx);
            assert!(app.export_error.is_none(), "{:?}", app.export_error);
            assert_eq!(std::fs::read(retained_target).expect("frame PNG"), expected);
            app.capture_frame_export()
                .expect("source protection capture")
                .finish(&mut app, Ok(Some(self.source.clone())));
            crate::audio_export_tests::drain_export(&mut app, &rx);
            assert!(app.export_error.take().is_some());
            assert_eq!(
                std::fs::read(&self.source).expect("protected source"),
                saved_bytes
            );
            assert_eq!(app.edits[&tab], history);
            assert_eq!(app.export_paths[&tab], previous);
            // Changing only the retained input while the dialog is open invalidates
            // the intent even when the logical path and media generation match.
            let intent = app.capture_frame_export().expect("input ownership capture");
            app.source_backings.remove(&tab);
            let stale = self.root.join("stale-input.png");
            intent.finish(&mut app, Ok(Some(stale.clone())));
            assert!(app.active_export.is_none());
            assert!(!stale.exists());
            assert!(backup.exists(), "session still owns the retained original");
            drop(app);
            let deadline = Instant::now() + Duration::from_secs(5);
            while backup.exists() {
                assert!(Instant::now() < deadline, "frame input cleanup");
                std::thread::sleep(Duration::from_millis(2));
            }
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
