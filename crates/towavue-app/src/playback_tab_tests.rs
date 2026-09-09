use super::*;
use std::sync::mpsc;
use winit::platform::windows::EventLoopBuilderExtWindows;

fn draw_video<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>) {
    let renderer = app.renderer.as_mut().expect("renderer");
    renderer
        .resize_surface(320, 240)
        .expect("size owned test surface");
    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
    assert!(
        app.session
            .as_mut()
            .expect("video session")
            .draw_current(
                renderer,
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(160.0, 96.0)),
                app.video_uv,
            )
            .expect("draw retained video on the shared device")
    );
    renderer.present_surface().expect("present video");
    app.record_seek_presentation(true);
}

fn service<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: &mpsc::Receiver<AppEvent>,
) {
    for event in events.try_iter() {
        app.handle_app_event(event);
    }
    app.poll_audio();
    app.load_next_frame();
    for saved in app.retained_playback.values_mut() {
        saved.poll();
    }
}

fn wait<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: &mpsc::Receiver<AppEvent>,
    predicate: impl Fn(&Application<N>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        service(app, events);
        if predicate(app) {
            return;
        }
        assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
        assert!(Instant::now() < deadline, "retained playback deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn open_muted<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    path: PathBuf,
    kind: MediaKind,
) -> TabId {
    let id = app.tabs.open_new(path.clone(), kind);
    let mut history = EditHistory::default();
    history.push(EditOperation::SetVolume(0.0), kind);
    history.mark_saved();
    app.edits.insert(id, history);
    app.load_path(path, kind);
    id
}

fn run_trial(root: PathBuf, audio: bool, unknown_duration: bool) {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    let video = root.join("first.mp4");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe");
    if audio {
        std::fs::copy(&source, &video).expect("owned A/V copy");
        assert!(
            std::process::Command::new(&ffmpeg)
                .args(["-v", "error", "-i"])
                .arg(&source)
                .args(["-vn", "-c:a", "pcm_s16le"])
                .arg(root.join("tone.wav"))
                .status()
                .expect("owned audio fixture")
                .success()
        );
    } else {
        assert!(
            std::process::Command::new(&ffmpeg)
                .args(["-v", "error", "-i"])
                .arg(&source)
                .args(["-an", "-c:v", "copy"])
                .arg(&video)
                .status()
                .expect("owned video-only fixture")
                .success()
        );
    }
    std::fs::copy(&video, root.join("second.mp4")).expect("second video");
    assert!(
        std::process::Command::new(&ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=red:s=16x16",
                "-frames:v",
                "1"
            ])
            .arg(root.join("image.bmp"))
            .status()
            .expect("owned image")
            .success()
    );

    struct Trial {
        root: PathBuf,
        audio: bool,
        unknown_duration: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_visible(false)
                            .with_inner_size(LogicalSize::new(640, 400)),
                    )
                    .expect("owned hidden window"),
            );
            let renderer = match FrameRenderer::new(&window) {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("SKIP retained playback: D3D11 renderer unavailable: {error}");
                    event_loop.exit();
                    return;
                }
            };
            let (sender, events) = mpsc::channel();
            let mut app = Application::new(None, move |event| {
                let _ = sender.send(event);
            })
            .expect("app");
            app.window = Some(window.clone());
            app.renderer = Some(renderer);
            app.ui_context = Some(fonts::test_context());
            let first = open_muted(&mut app, self.root.join("first.mp4"), MediaKind::Video);
            if self.audio
                && app
                    .playback_error
                    .as_ref()
                    .is_some_and(|error| error.starts_with("audio output failed:"))
            {
                eprintln!(
                    "SKIP retained playback: {}",
                    app.playback_error.as_ref().expect("audio error")
                );
                event_loop.exit();
                return;
            }
            wait(&mut app, &events, |app| {
                app.media_duration.is_some() && app.pending_time.is_some()
            });
            app.toggle_pause();
            app.seek_to(media_time(Duration::from_millis(508)));
            wait(&mut app, &events, |app| app.pending_time.is_some());
            assert!(app.frame_is_due());
            app.advance_media();
            draw_video(&mut app);
            let first_geometry = app.session.as_ref().expect("first video").video_geometry();
            let first_presented = app
                .session
                .as_ref()
                .expect("first video")
                .metrics()
                .presented_frame_count;
            assert!(first_geometry.is_some());
            app.image_view.zoom = ZoomMode::Custom(2.0);
            app.image_view.selection = Some(UnitRect::FULL);
            let view = app.image_view;
            let first_instance = app.media_generation;
            let first_generation = app.generation;
            let second = open_muted(&mut app, self.root.join("second.mp4"), MediaKind::Video);
            wait(&mut app, &events, |app| {
                app.media_duration.is_some() && app.pending_time.is_some()
            });
            let second_instance = app.media_generation;
            if self.unknown_duration {
                app.media_duration = None;
                assert!(app.duration_workers.is_empty());
            }
            let music = self.audio.then(|| {
                let id = open_muted(&mut app, self.root.join("tone.wav"), MediaKind::Audio);
                wait(&mut app, &events, |app| app.media_duration.is_some());
                id
            });
            app.open_external(self.root.join("image.bmp"), true);
            let image = app.tabs.active().expect("image tab").id;
            wait(&mut app, &events, |app| {
                !app.image_loading && app.image.is_some()
            });
            assert_eq!(app.retained_playback.len(), if self.audio { 3 } else { 2 });
            assert!(app.retained_playback[&first].video_suspended);
            assert_eq!(
                app.retained_playback[&second].video_suspended,
                !self.unknown_duration
            );
            assert!(app.background_wakeup(Instant::now()).is_some());
            let paused_position = app.retained_playback[&first].position();
            let before = app.retained_playback[&second].position();
            wait(&mut app, &events, |app| {
                app.retained_playback[&second].position()
                    > before.saturating_add(Duration::from_millis(100))
            });
            assert_eq!(app.retained_playback[&first].position(), paused_position);
            assert_eq!(
                app.retained_playback[&first]
                    .session
                    .as_ref()
                    .expect("retained session")
                    .generation(),
                first_generation
            );
            if let Some(music) = music {
                assert!(app.retained_playback[&music].position() > MediaTime::ZERO);
                let before_recovery = app.retained_playback[&music].position();
                app.recover_graphics_device(MediaTime::ZERO);
                assert!(app.renderer.is_some() && app.playback_error.is_none());
                let device = app
                    .renderer
                    .as_ref()
                    .expect("renderer")
                    .graphics_device()
                    .adapter_luid();
                for saved in app.retained_playback.values() {
                    assert!(saved.recovery_position.is_none());
                    assert_eq!(
                        saved
                            .session
                            .as_ref()
                            .expect("recovered session")
                            .metrics()
                            .adapter_luid,
                        device
                    );
                }
                assert!(app.retained_playback[&music].position() >= before_recovery);
                assert_eq!(app.retained_playback[&first].state, PlaybackState::Paused);
                assert!(Arc::ptr_eq(app.window.as_ref().expect("window"), &window));
            }
            wait(&mut app, &events, |app| {
                app.retained_playback[&second].state == PlaybackState::Ended
                    && music
                        .is_none_or(|id| app.retained_playback[&id].state == PlaybackState::Ended)
            });
            assert_eq!(app.tabs.active().expect("still image").id, image);
            assert!(app.background_wakeup(Instant::now()).is_none());
            let background_generation = app.retained_playback[&second]
                .session
                .as_ref()
                .expect("second session")
                .generation();
            app.handle_app_event(AppEvent::Playback(
                second_instance,
                PlaybackEvent::Failed(background_generation, "owned background fault".into()),
            ));
            assert_eq!(app.retained_playback[&second].state, PlaybackState::Faulted);
            assert_eq!(
                app.retained_playback[&second].error.as_deref(),
                Some("owned background fault")
            );
            assert!(app.playback_error.is_none());
            assert_eq!(app.tabs.active().expect("image remains active").id, image);
            app.activate_tab(first);
            if !self.audio {
                let session = app.session.as_ref().expect("restored video");
                assert_eq!(
                    session.video_geometry(),
                    first_geometry,
                    "immediate return geometry"
                );
                assert!(session.video_refresh_pending());
                let latency_count = app.seek_latencies.len();
                app.pending_seek_started = Some(Instant::now());
                draw_video(&mut app);
                assert_eq!(
                    app.session
                        .as_ref()
                        .expect("restored video")
                        .metrics()
                        .presented_frame_count,
                    first_presented
                );
                assert_eq!(
                    app.seek_latencies.len(),
                    latency_count,
                    "old frame is not a Seek result"
                );
                assert!(app.pending_seek_started.is_some());
            } else {
                assert!(
                    app.session
                        .as_ref()
                        .expect("recovered video")
                        .video_geometry()
                        .is_none(),
                    "device recovery discards retained surface"
                );
            }
            wait(&mut app, &events, |app| app.pending_time.is_some());
            assert!(
                app.frame_is_due(),
                "paused return accepts a new frame beyond its clock"
            );
            app.advance_media();
            assert!(
                !app.session
                    .as_ref()
                    .expect("fresh video")
                    .video_refresh_pending()
            );
            draw_video(&mut app);
            assert!(app.pending_seek_started.is_none());
            assert_eq!(app.media_generation, first_instance);
            assert_eq!(app.image_view, view);
            assert_eq!(app.state, PlaybackState::Paused);
            assert_eq!(app.current_position(), paused_position);
            assert!(
                app.session
                    .as_ref()
                    .expect("restored session")
                    .accepts_event(&PlaybackEvent::AudioReady(app.generation))
            );
            app.handle_app_event(AppEvent::Playback(
                first_instance,
                PlaybackEvent::VideoFailed(first_generation, "stale before hiding".into()),
            ));
            assert!(app.playback_error.is_none());
            app.remove_tab(second, false);
            app.handle_app_event(AppEvent::Playback(
                second_instance,
                PlaybackEvent::Failed(first_generation, "closed background".into()),
            ));
            assert_eq!(app.tabs.active().expect("first").id, first);
            assert!(app.playback_error.is_none());
            let new = open_muted(&mut app, self.root.join("second.mp4"), MediaKind::Video);
            assert!(
                app.media_generation > second_instance && app.media_generation > first_instance
            );
            app.remove_tab(new, false);
            if let Some(music) = music {
                app.remove_tab(music, false);
            }
            app.remove_tab(first, false);
            app.remove_tab(image, false);
            assert!(app.retained_playback.is_empty() && app.session.is_none());
            assert!(app.duration_workers.is_empty());
            drop(app);
            eprintln!(
                "PASS retained playback: independent state/clock/EOF/identity/close; audio={}",
                self.audio
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop")
        .run_app(&mut Trial {
            root,
            audio,
            unknown_duration,
        })
        .expect("retained playback trial");
}

#[test]
fn video_tabs_keep_paused_state_and_end_in_background() {
    if let Some(root) = tests::isolated_test_root(
        "playback_tab_tests::video_tabs_keep_paused_state_and_end_in_background",
    ) {
        run_trial(root, false, false);
    }
}

#[test]
#[ignore = "requires Windows D3D11 and a live shared WASAPI endpoint; owned media is muted"]
fn audio_video_tabs_continue_behind_images_and_recover_the_shared_device() {
    if let Some(root) = tests::isolated_test_root(
        "playback_tab_tests::audio_video_tabs_continue_behind_images_and_recover_the_shared_device",
    ) {
        run_trial(root, true, false);
    }
}

#[test]
fn unknown_duration_video_reaches_real_eof_in_background() {
    if let Some(root) = tests::isolated_test_root(
        "playback_tab_tests::unknown_duration_video_reaches_real_eof_in_background",
    ) {
        run_trial(root, false, true);
    }
}
