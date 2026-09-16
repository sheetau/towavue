use super::*;
use std::sync::mpsc;
use winit::platform::windows::EventLoopBuilderExtWindows;

fn advance<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: &mpsc::Receiver<AppEvent>,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app
        .session
        .as_ref()
        .expect("session")
        .video_refresh_pending()
    {
        for event in events.try_iter() {
            app.handle_app_event(event);
        }
        app.load_next_frame();
        app.advance_media();
        assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
        assert!(
            Instant::now() < deadline,
            "fresh frame must become available"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn present<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>) {
    let renderer = app.renderer.as_mut().expect("renderer");
    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
    let drawn = app
        .session
        .as_mut()
        .expect("session")
        .draw_current(
            renderer,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 240.0)),
            towavue_runtime_windows::VideoOrientation::default().source_uv(),
        )
        .expect("draw fresh media");
    assert!(drawn);
    renderer.present_surface().expect("native presentation");
    app.record_seek_presentation(drawn);
}

#[test]
fn held_video_seeks_wait_for_presented_results_without_backlog_or_discrete_input_loss() {
    let Some(root) = crate::tests::isolated_test_root(
        "seek_repeat_tests::native::held_video_seeks_wait_for_presented_results_without_backlog_or_discrete_input_loss",
    ) else {
        return;
    };
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let mut paths = Vec::new();
    for codec in ["copy", "mpeg4"] {
        for audio in [false, true] {
            let path = root.join(format!("{codec}-{audio}.mp4"));
            let mut command = std::process::Command::new(&ffmpeg);
            command
                .args(["-v", "error", "-stream_loop", "9", "-i"])
                .arg(&source);
            command.args(["-c:v", codec]);
            if audio {
                command.args(["-c:a", "copy"]);
            } else {
                command.arg("-an");
            }
            assert!(
                command
                    .arg(&path)
                    .status()
                    .expect("owned fixture")
                    .success()
            );
            paths.push((path, audio));
        }
    }
    struct Trial {
        paths: Vec<(PathBuf, bool)>,
        completed: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_visible(false)
                            .with_inner_size(winit::dpi::PhysicalSize::new(320, 240)),
                    )
                    .expect("owned hidden window"),
            );
            for (path, audio) in &self.paths {
                let original = std::fs::read(path).expect("source bytes");
                for playing in [false, true] {
                    let mut renderer = FrameRenderer::new(&window).expect("D3D11 renderer");
                    let size = window.inner_size();
                    renderer
                        .resize_surface(size.width, size.height)
                        .expect("native surface");
                    let (sender, events) = mpsc::channel();
                    let mut app = Application::new(None, move |event| {
                        let _ = sender.send(event);
                    })
                    .expect("app");
                    app.window = Some(window.clone());
                    app.renderer = Some(renderer);
                    app.shortcuts = shortcuts::defaults();
                    app.tabs.open_new(path.clone(), MediaKind::Video);
                    app.media_kind = Some(MediaKind::Video);
                    app.set_playback_volume(0.0);
                    app.load_path(path.clone(), MediaKind::Video);
                    assert_eq!(app.session.as_ref().expect("session").has_audio(), *audio);
                    if !playing {
                        app.toggle_pause();
                    }
                    app.seek_to(MediaTime::ZERO);
                    advance(&mut app, &events);
                    present(&mut app);
                    for key in ["Right", "Right", "Left", "Right"] {
                        let generation = app.generation;
                        app.repeat_media_shortcut(key.parse().expect("repeat key"));
                        assert_eq!(app.generation, generation.next());
                        let target = app.session.as_ref().expect("session").target();
                        let accepted = app.generation;
                        let notice = app.relative_seek_notice;
                        assert!(app.pending_seek_started.is_some());
                        for _ in 0..50 {
                            app.repeat_media_shortcut(key.parse().expect("held key"));
                            assert_eq!(
                                app.generation, accepted,
                                "no replacement before a frame is presented"
                            );
                        }
                        assert_eq!(
                            app.relative_seek_notice, notice,
                            "ignored repeats are not accepted distance"
                        );
                        advance(&mut app, &events);
                        let session = app.session.as_ref().expect("session");
                        let frame = session.current_video_time().expect("new frame");
                        assert!(
                            frame >= target
                                && frame.as_seconds_f64() - target.as_seconds_f64() < 0.2
                        );
                        let presented = session.metrics().presented_frame_count;
                        app.record_seek_presentation(false);
                        app.repeat_media_shortcut(key.parse().expect("undrawn repeat"));
                        assert_eq!(
                            app.generation, accepted,
                            "advancing a frame alone is not presentation"
                        );
                        present(&mut app);
                        assert!(app.pending_seek_started.is_none());
                        assert_eq!(
                            app.session
                                .as_ref()
                                .expect("session")
                                .metrics()
                                .presented_frame_count,
                            presented + 1
                        );
                        for _ in 0..3 {
                            app.advance_media();
                        }
                        assert_eq!(
                            app.generation, accepted,
                            "release has no deferred seek backlog"
                        );
                    }
                    // Separate key presses remain immediate even before the last result arrives.
                    for _ in 0..3 {
                        let generation = app.generation;
                        app.process_shortcut("Left".parse().expect("discrete press"));
                        assert_eq!(app.generation, generation.next());
                    }
                    assert!(app.edits.values().all(|history| !history.is_dirty()));
                    assert_eq!(
                        app.state,
                        if playing {
                            PlaybackState::Playing
                        } else {
                            PlaybackState::Paused
                        }
                    );
                }
                assert_eq!(std::fs::read(path).expect("unchanged source"), original);
            }
            self.completed = true;
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop");
    let mut trial = Trial {
        paths,
        completed: false,
    };
    event_loop.run_app(&mut trial).expect("native seek trial");
    assert!(trial.completed);
}
