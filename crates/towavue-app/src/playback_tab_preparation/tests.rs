use super::*;
use std::sync::mpsc;

type App = Application<Box<dyn Fn(AppEvent) + Send + Sync>>;

fn app() -> (App, mpsc::Receiver<AppEvent>) {
    let (sender, receiver) = mpsc::channel();
    let mut app = Application::new(
        None,
        Box::new(move |event| {
            let _ = sender.send(event);
        }) as Box<dyn Fn(AppEvent) + Send + Sync>,
    )
    .expect("app");
    app.ui_context = Some(fonts::test_context());
    (app, receiver)
}

fn fixtures(root: &Path) -> (PathBuf, PathBuf) {
    let video = root.join("preview.mp4");
    let audio = root.join("a.wav");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let output = std::process::Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=64x48:r=10",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo",
            "-t",
            "4",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
        ])
        .arg(&video)
        .args(["-map", "1:a", "-t", "4", "-c:a", "pcm_s16le"])
        .arg(&audio)
        .output()
        .expect("owned silent media");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::copy(&audio, root.join("b.wav")).expect("second audio track");
    (video, audio)
}

fn finish(app: &mut App, receiver: &mpsc::Receiver<AppEvent>, id: TabId) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        while let Ok(event) = receiver.try_recv() {
            app.handle_app_event(event);
        }
        app.finish_audio_folder_loads();
        let saved = &app.retained_playback[&id];
        assert!(saved.error.is_none(), "{:?}", saved.error);
        if saved.duration.is_some()
            && (saved.kind != MediaKind::Audio
                || app.audio_queues[&id]
                    .order
                    .next(&saved.path, false)
                    .is_some())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "metadata/order ready: {:?}",
            saved.status
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn frame(app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
    let context = app.ui_context.clone().expect("context");
    context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 576.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let mut actions = Vec::new();
            app.draw_ui(ui, &mut actions);
            assert!(actions.is_empty(), "hover must not dispatch transport");
        },
    )
}

#[test]
fn unopened_hover_prepares_metadata_and_audio_order_without_playback() {
    let Some(root) = crate::tests::isolated_test_root(
        "playback_tab_preparation::tests::unopened_hover_prepares_metadata_and_audio_order_without_playback",
    ) else {
        return;
    };
    let (video, audio) = fixtures(&root);
    let (mut app, receiver) = app();
    let gallery = app.tabs.active_id().expect("Gallery");
    let foreground_generation = app.media_generation;
    for (path, kind) in [(video, MediaKind::Video), (audio, MediaKind::Audio)] {
        let id = app.tabs.open_new(path.clone(), kind);
        app.tabs.activate(gallery);
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let output = frame(&mut app, vec![]);
        let name = path.file_name().expect("name").to_string_lossy();
        let point = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == name => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .expect("background tab label");
        for _ in 0..4 {
            frame(&mut app, vec![egui::Event::PointerMoved(point)]);
        }
        assert!(
            app.retained_playback.contains_key(&id),
            "production hover wiring"
        );
        finish(&mut app, &receiver, id);
        let card = app.preview_transport(id, &path).expect("ready card");
        assert!(card.enabled);
        assert_eq!(card.state, PlaybackState::Paused);
        assert_eq!(card.position, MediaTime::ZERO);
        assert!(card.duration.expect("duration").as_seconds_f64() >= 3.9);
        assert_eq!(card.next, kind == MediaKind::Audio);
        assert_eq!(app.tabs.active_id(), Some(gallery));
        assert_eq!(app.media_generation, foreground_generation);
        assert!(app.path.is_none() && app.session.is_none());
        assert!(app.retained_playback[&id].session.is_none());
        assert_eq!(app.retained_playback[&id].state, PlaybackState::Loading);
        assert_eq!(
            app.retained_playback[&id].resume.is_some(),
            kind == MediaKind::Video
        );
        app.prepare_playback_tab(id);
        assert_eq!(app.retained_playback[&id].instance, card.instance);
        assert!(!app.duration_workers.contains_key(&card.instance));
        app.close_tab_unchecked(id);
        app.finish_playback_tab_preparation(
            id,
            card.instance,
            path.clone(),
            Ok(Duration::from_secs(9)),
            None,
        );
        assert!(!app.retained_playback.contains_key(&id));
        assert!(app.preview_transport(id, &path).is_none());
    }
}

#[test]
#[ignore = "requires D3D11 and shared WASAPI; generated silence in hidden owned windows"]
fn native_unopened_cards_seek_paused_play_and_keep_activation_position() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(root) = crate::tests::isolated_test_root(
        "playback_tab_preparation::tests::native_unopened_cards_seek_paused_play_and_keep_activation_position",
    ) else {
        return;
    };
    let (video, audio) = fixtures(&root);
    struct Trial {
        video: PathBuf,
        audio: PathBuf,
        completed: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let (mut app, receiver) = app();
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_visible(false)
                            .with_inner_size(LogicalSize::new(640, 480)),
                    )
                    .expect("hidden window"),
            );
            app.renderer = Some(FrameRenderer::new(&window).expect("D3D11"));
            app.window = Some(window);
            let gallery = app.tabs.active_id().expect("Gallery");
            for (path, kind) in [
                (&self.video, MediaKind::Video),
                (&self.audio, MediaKind::Audio),
            ] {
                let id = app.tabs.open_new(path.clone(), kind);
                app.tabs.activate(gallery);
                app.seed_playback_volume(id);
                if app.tab_mute_state(id) == Some(false) {
                    app.toggle_tab_mute(id);
                }
                app.prepare_playback_tab(id);
                finish(&mut app, &receiver, id);
                let instance = app.retained_playback[&id].instance;
                let target = media_time(Duration::from_millis(500));
                app.handle_preview_seek(id, instance.wrapping_add(1), path, target);
                app.palette_open = true;
                app.handle_preview_seek(id, instance, path, target);
                app.palette_open = false;
                assert!(
                    app.retained_playback[&id].session.is_none(),
                    "stale/covered card cannot start media"
                );
                app.handle_preview_seek(id, instance, path, target);
                let saved = &app.retained_playback[&id];
                assert_eq!(saved.state, PlaybackState::Paused, "{:?}", saved.error);
                assert!(!saved.prepared_only);
                assert_eq!(
                    saved
                        .session
                        .as_ref()
                        .expect("explicit seek opens paused")
                        .verification_volume(),
                    (0.0, Some(0.0))
                );
                assert_eq!(saved.position(), target);
                std::thread::sleep(Duration::from_millis(150));
                while let Ok(event) = receiver.try_recv() {
                    app.handle_app_event(event);
                }
                assert_eq!(app.retained_playback[&id].state, PlaybackState::Paused);
                assert_eq!(
                    app.retained_playback[&id].position(),
                    target,
                    "seek alone never plays"
                );
                app.handle_preview_transport(id, instance, path.clone(), CommandId::TogglePause);
                assert_eq!(app.retained_playback[&id].state, PlaybackState::Playing);
                let deadline = Instant::now() + Duration::from_secs(5);
                while app.retained_playback[&id].position() <= target {
                    assert!(Instant::now() < deadline, "explicit Play advances");
                    std::thread::sleep(Duration::from_millis(5));
                }
                app.handle_preview_transport(id, instance, path.clone(), CommandId::TogglePause);
                std::thread::sleep(Duration::from_millis(80));
                let position = app.retained_playback[&id].position();
                std::thread::sleep(Duration::from_millis(80));
                assert_eq!(app.retained_playback[&id].position(), position);
                assert_eq!(app.tabs.active_id(), Some(gallery));
                assert!(app.session.is_none());
                let generation = app.retained_playback[&id]
                    .session
                    .as_ref()
                    .expect("session")
                    .generation();
                app.activate_tab(id);
                assert_eq!(app.state, PlaybackState::Paused);
                assert_eq!(app.current_position(), position);
                assert_eq!(
                    app.session.as_ref().expect("retained session").generation(),
                    generation
                );
                app.close_tab_unchecked(id);
            }
            // Next from an unopened audio card must open that track at the tab's
            // listening gain without activating it or requiring an initial Play.
            let id = app.tabs.open_new(self.audio.clone(), MediaKind::Audio);
            app.tabs.activate(gallery);
            app.seed_playback_volume(id);
            if app.tab_mute_state(id) == Some(false) {
                app.toggle_tab_mute(id);
            }
            app.prepare_playback_tab(id);
            finish(&mut app, &receiver, id);
            let instance = app.retained_playback[&id].instance;
            app.handle_preview_transport(id, instance, self.audio.clone(), CommandId::NextMedia);
            let saved = &app.retained_playback[&id];
            assert_eq!(saved.path, self.audio.with_file_name("b.wav"));
            assert_eq!(saved.state, PlaybackState::Playing, "{:?}", saved.error);
            assert!(!saved.prepared_only);
            assert_eq!(
                saved
                    .session
                    .as_ref()
                    .expect("next track")
                    .verification_volume(),
                (0.0, Some(0.0))
            );
            assert_eq!(app.tabs.active_id(), Some(gallery));
            app.close_tab_unchecked(id);
            let (sender, history_events) = mpsc::channel();
            app.resume_history = Some(
                towavue_runtime_windows::VideoResumeHistory::new(
                    self.video.with_file_name("preview-resume.txt"),
                    move |event| {
                        let _ = sender.send(event);
                    },
                )
                .expect("isolated resume history"),
            );
            let recorded = |app: &App| {
                app.resume_history
                    .as_ref()
                    .expect("history")
                    .load(1, self.video.clone());
                let towavue_runtime_windows::VideoResumeEvent::Loaded { result, .. } =
                    history_events
                        .recv_timeout(Duration::from_secs(5))
                        .expect("history lookup")
                else {
                    panic!("resume lookup failed");
                };
                result.expect("resume result").position
            };
            for natural_end in [false, true] {
                app.resume_history.as_ref().expect("history").remember(
                    VideoResumeSource::capture(&self.video).expect("source identity"),
                    Duration::from_secs(2),
                    std::time::SystemTime::now(),
                );
                assert_eq!(recorded(&app), Some(Duration::from_secs(2)));
                let id = app.tabs.open_new(self.video.clone(), MediaKind::Video);
                app.tabs.activate(gallery);
                app.prepare_playback_tab(id);
                finish(&mut app, &receiver, id);
                resume::record(&mut app, true);
                assert_eq!(
                    recorded(&app),
                    Some(Duration::from_secs(2)),
                    "hover never overwrites resume history"
                );
                let saved = &app.retained_playback[&id];
                let instance = saved.instance;
                let target = if natural_end {
                    media_time(
                        saved
                            .duration
                            .expect("duration")
                            .saturating_sub(Duration::from_millis(100)),
                    )
                } else {
                    MediaTime::ZERO
                };
                app.handle_preview_seek(id, instance, &self.video, target);
                if natural_end {
                    app.handle_preview_transport(
                        id,
                        instance,
                        self.video.clone(),
                        CommandId::TogglePause,
                    );
                    let deadline = Instant::now() + Duration::from_secs(5);
                    loop {
                        let saved = app.retained_playback.get_mut(&id).expect("preview owner");
                        saved.poll();
                        assert!(saved.error.is_none(), "{:?}", saved.error);
                        if saved.state == PlaybackState::Ended {
                            break;
                        }
                        assert!(Instant::now() < deadline, "preview reaches natural EOF");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
                resume::record(&mut app, true);
                assert_eq!(
                    recorded(&app),
                    None,
                    "explicit zero seek or natural EOF resets an older resume point"
                );
                app.close_tab_unchecked(id);
            }
            self.completed = true;
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial {
        video,
        audio,
        completed: false,
    };
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("native trial");
    assert!(trial.completed);
    eprintln!(
        "PASS unopened video/audio cards: metadata only, guarded paused seek, muted Play/Pause, activation keeps session and position, unopened audio Next keeps mute"
    );
}
