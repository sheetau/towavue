use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

fn app() -> Application<Box<dyn Fn(AppEvent) + Send + Sync>> {
    Application::new(
        None,
        Box::new(|_| {}) as Box<dyn Fn(AppEvent) + Send + Sync>,
    )
    .expect("video track verification")
}

fn advance(app: &mut Application<Box<dyn Fn(AppEvent) + Send + Sync>>, size: (u32, u32), fps: f64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let session = app.session.as_mut().expect("video track verification");
    while session.pending_video_time().is_none() {
        assert!(Instant::now() < deadline, "selected video did not arrive");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(session.advance_pending());
    let geometry = session.video_geometry().expect("video track verification");
    assert_eq!((geometry.0, geometry.1), size);
    assert_eq!(session.source_video_frame_rate(), Some(fps));
    assert!(session.current_video_snapshot().is_some());
}

#[test]
fn video_tracks_guard_edits_and_keep_selection_through_paused_playback_and_transfer() {
    let Some(root) = crate::tests::isolated_test_root(
        "video_tracks::tests::video_tracks_guard_edits_and_keep_selection_through_paused_playback_and_transfer",
    ) else {
        return;
    };
    let source = root.join("tracks.mkv");
    let output = crate::tests::hidden_command(
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("video track verification"))
            .join("bin/ffmpeg.exe"),
    )
    .args([
        "-v",
        "error",
        "-n",
        "-f",
        "lavfi",
        "-i",
        "color=red:size=160x96:rate=10:duration=3",
        "-f",
        "lavfi",
        "-i",
        "color=blue:size=128x80:rate=25:duration=3",
        "-map",
        "0:v",
        "-map",
        "1:v",
        "-c:v",
        "libopenh264",
        "-threads",
        "1",
        "-disposition:v:0",
        "default",
        "-disposition:v:1",
        "0",
    ])
    .arg(&source)
    .output()
    .expect("video track verification");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    struct Trial(PathBuf);
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("video track verification"),
            );
            let mut app = app();
            app.renderer = Some(FrameRenderer::new(&window).expect("video track verification"));
            app.window = Some(window.clone());
            let context = fonts::test_context();
            context.enable_accesskit();
            context.global_style_mut(chrome::style);
            app.ui_context = Some(context.clone());
            let tab = app.tabs.open_new(self.0.clone(), MediaKind::Video);
            app.load_path(self.0.clone(), MediaKind::Video);
            assert_eq!(
                app.state,
                PlaybackState::Playing,
                "{:?}",
                app.playback_error
            );
            app.media_duration = Some(Duration::from_secs(3));
            app.toggle_pause();
            app.seek_to(MediaTime::from_nanoseconds(700_000_000));
            advance(&mut app, (160, 96), 10.0);
            app.push_edit(EditOperation::FlipHorizontal);
            let history = app.edits[&tab].clone();
            let track = VideoTrackId::from_index(1);
            app.select_video_track(track);
            assert!(matches!(
                app.pending_guard,
                Some(GuardedAction::VideoTrack(..))
            ));
            assert_eq!(app.video_selection(), None);
            app.resolve_guard(GuardDecision::Cancel);
            assert_eq!(app.edits[&tab], history);
            assert_eq!(app.video_selection(), None);
            app.select_video_track(track);
            app.resolve_guard(GuardDecision::Discard);
            assert_eq!(app.video_selection(), Some(track));
            assert!(!app.edits[&tab].is_dirty());
            assert!(
                !app.edits
                    .get_mut(&tab)
                    .expect("video track verification")
                    .undo()
            );
            assert!(
                !app.edits
                    .get_mut(&tab)
                    .expect("video track verification")
                    .redo()
            );
            assert_eq!(app.state, PlaybackState::Paused);
            assert_eq!(
                app.session
                    .as_ref()
                    .expect("video track verification")
                    .target(),
                MediaTime::from_nanoseconds(700_000_000)
            );
            assert!(
                app.session
                    .as_ref()
                    .expect("video track verification")
                    .current_video_snapshot()
                    .is_none()
            );
            advance(&mut app, (128, 80), 25.0);
            let snapshot = app
                .session
                .as_ref()
                .expect("session")
                .current_video_snapshot()
                .expect("blue snapshot");
            let generation = app.generation;
            app.switch_video_track(
                tab,
                app.media_generation.wrapping_sub(1),
                self.0.clone(),
                VideoTrackId::from_index(0),
            );
            assert_eq!(
                app.generation, generation,
                "stale controls cannot switch a new document"
            );
            app.select_video_track(VideoTrackId::from_index(99));
            assert_eq!(app.generation, generation);
            app.timeline_open = true;
            for density in [1.0, 1.25, 2.0] {
                context.set_pixels_per_point(density);
                let result = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(320.0, 240.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
                    },
                );
                let tree = result
                    .platform_output
                    .accesskit_update
                    .expect("video track verification");
                let row = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("V2"))
                    .expect("selected V2 control");
                let bounds = row.1.bounds().expect("video track verification");
                assert_eq!(bounds.y1 - bounds.y0, 24.0);
                assert!(bounds.x0 >= 0.0 && bounds.x1 <= 320.0, "{bounds:?}");
            }
            assert!(app.begin_palette_options(CommandId::CycleVideoTrack));
            assert_eq!(
                app.palette.options_command(),
                Some(CommandId::CycleVideoTrack)
            );
            app.cancel_command_overlay();
            let request = app.tab_detach_request(tab).expect("transfer request");
            let transfer = app.take_tab_transfer(&request, None);
            let (send, events) = std::sync::mpsc::channel();
            let mut destination = Application::new(
                None,
                Box::new(move |event| {
                    let _ = send.send(event);
                }) as Box<dyn Fn(AppEvent) + Send + Sync>,
            )
            .expect("destination");
            destination.renderer = app.renderer.take();
            destination.window = Some(window);
            destination.ui_context = Some(context);
            let moved = destination.accept_tab_transfer(transfer, 0);
            assert_eq!(
                destination.video_selection_for(Some(moved), &self.0),
                Some(track)
            );
            assert_eq!(
                destination
                    .session
                    .as_ref()
                    .expect("video track verification")
                    .video_selection(),
                Some(track)
            );
            destination.select_video_track(VideoTrackId::from_index(0));
            advance(&mut destination, (160, 96), 10.0);
            let png = self.0.with_file_name("selected-frame.png");
            towavue_runtime_windows::export_video_frame(&snapshot, &png, &[])
                .expect("retained selected frame");
            let image = towavue_runtime_windows::decode_image(&png).expect("frame PNG");
            assert_eq!((image.frames[0].width, image.frames[0].height), (128, 80));
            assert!(image.frames[0].rgba[2] > 200 && image.frames[0].rgba[0] < 30);
            destination.push_edit(EditOperation::FlipHorizontal);
            destination.select_video_track(track);
            destination.resolve_guard(GuardDecision::Save);
            let export = destination
                .active_export
                .as_ref()
                .expect("guard starts source save");
            assert_eq!(
                export.request.video_track,
                Some(VideoTrackId::from_index(0))
            );
            assert!(
                matches!(export.continuation, Some(GuardedAction::VideoTrack(_, _, _, id)) if id == track)
            );
            crate::source_save::tests::finish(&mut destination, &events);
            assert!(
                destination.export_error.is_none(),
                "{:?}",
                destination.export_error
            );
            assert!(destination.pending_guard.is_none());
            assert_eq!(
                destination.video_selection(),
                Some(track),
                "saved continuation selects the retained source stream"
            );
            assert!(destination.source_backings.contains_key(&moved));
            advance(&mut destination, (128, 80), 25.0);
            let saved = destination
                .preview_cache
                .thumbnail(&self.0, Duration::ZERO, 160)
                .expect("saved file preview");
            assert!(
                saved.rgba[0] > 200 && saved.rgba[2] < 30,
                "saved file contains the previously selected red video"
            );
            drop(destination);
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: winit::window::WindowId,
            _: WindowEvent,
        ) {
        }
    }
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("video track verification")
        .run_app(&mut Trial(source))
        .expect("video track verification");
}
