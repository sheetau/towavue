use super::*;

#[test]
fn video_resume_reopens_from_disk_preserves_tabs_and_rejects_delayed_delivery() {
    let Some(root) = crate::tests::isolated_test_root(
        "resume::tests::video_resume_reopens_from_disk_preserves_tabs_and_rejects_delayed_delivery",
    ) else {
        return;
    };
    let path = root.join("resume.mkv");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    assert!(
        std::process::Command::new(ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x96:rate=25:duration=5",
                "-an",
                "-c:v",
                "mpeg4"
            ])
            .arg(&path)
            .status()
            .expect("generated video")
            .success()
    );
    let second = root.join("second.mkv");
    std::fs::copy(&path, &second).expect("second fixture");
    let history = root.join("video-resume.txt");
    let (tx, rx) = std::sync::mpsc::channel();
    let seed = VideoResumeHistory::new(history.clone(), move |event| {
        tx.send(event).expect("seed event");
    })
    .expect("history");
    seed.load(0, path.clone());
    let VideoResumeEvent::Loaded { result, .. } =
        rx.recv_timeout(Duration::from_secs(5)).expect("source")
    else {
        panic!("source")
    };
    let source = result.expect("source stamp").source;
    seed.remember(
        source.clone(),
        Duration::from_secs(4),
        std::time::SystemTime::now(),
    );
    drop(seed);
    struct Trial {
        path: PathBuf,
        second: PathBuf,
        history: PathBuf,
        source: Option<VideoResumeSource>,
    }
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let original = std::fs::read(&self.path).expect("source bytes");
            let modified = std::fs::metadata(&self.path)
                .expect("source metadata")
                .modified()
                .expect("source time");
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("hidden window"),
            );
            let renderer = FrameRenderer::new(&window).expect("D3D11");
            let (tx, rx) = std::sync::mpsc::channel();
            let notify = tx.clone();
            let mut app = Application::new(None, move |event| {
                let _ = notify.send(event);
            })
            .expect("app");
            app.window = Some(window.clone());
            app.renderer = Some(renderer);
            app.resume_history = Some(
                VideoResumeHistory::new(self.history.clone(), move |event| {
                    let _ = tx.send(AppEvent::VideoResume(event));
                })
                .expect("resume worker"),
            );
            let tab = app.tabs.open_new(self.path.clone(), MediaKind::Video);
            app.load_path(self.path.clone(), MediaKind::Video);
            assert_eq!(app.state, PlaybackState::Loading);
            assert!(app.session.is_none());
            finish_open(&mut app, &rx);
            assert_eq!(
                app.session.as_ref().expect("session").target(),
                media_time(Duration::from_secs(4))
            );
            app.toggle_pause();
            let target = media_time(Duration::from_millis(1250));
            app.seek_to(target);
            resume::record(&mut app, false);
            assert_eq!(
                app.resume_owner.as_ref().expect("owner").saved,
                Duration::from_secs(4)
            );
            app.resume_owner.as_mut().expect("owner").written =
                Instant::now() - Duration::from_secs(6);
            resume::record(&mut app, false);
            assert_eq!(
                app.resume_owner.as_ref().expect("owner").saved,
                Duration::from_millis(1250)
            );
            let source = self.source.take().expect("source stamp");
            let generation = app.media_generation;
            app.handle_video_resume(VideoResumeEvent::Loaded {
                token: generation,
                path: self.path.clone(),
                result: Ok(VideoResume {
                    source: source.clone(),
                    position: Some(Duration::from_secs(4)),
                }),
            });
            assert_eq!(
                app.session.as_ref().expect("session").target(),
                target,
                "duplicate delivery cannot undo a user seek"
            );
            let other = app.tabs.open_new(self.second.clone(), MediaKind::Video);
            app.load_path(self.second.clone(), MediaKind::Video);
            assert!(app.retained_playback[&tab].resume.is_some());
            app.activate_tab(tab);
            assert_eq!(app.state, PlaybackState::Paused);
            assert_eq!(
                app.session.as_ref().expect("retained session").target(),
                target
            );
            app.activate_tab(other);
            assert_eq!(
                app.state,
                PlaybackState::Loading,
                "revisit an unfinished lookup"
            );
            app.handle_video_resume(VideoResumeEvent::Loaded {
                token: generation,
                path: self.path.clone(),
                result: Ok(VideoResume {
                    source: source.clone(),
                    position: Some(Duration::from_secs(4)),
                }),
            });
            assert!(
                app.session.is_none(),
                "old path/token cannot finish a pending open"
            );
            finish_open(&mut app, &rx);
            app.activate_tab(tab);
            let renderer = app.renderer.take().expect("renderer");
            drop(app); // Drain final writes before a new application instance.
            let (tx, rx) = std::sync::mpsc::channel();
            let notify = tx.clone();
            let mut app = Application::new(None, move |event| {
                let _ = notify.send(event);
            })
            .expect("restarted app");
            app.window = Some(window);
            app.renderer = Some(renderer);
            app.resume_history = Some(
                VideoResumeHistory::new(self.history.clone(), move |event| {
                    let _ = tx.send(AppEvent::VideoResume(event));
                })
                .expect("new resume worker"),
            );
            let tab = app.tabs.open_new(self.path.clone(), MediaKind::Video);
            app.load_path(self.path.clone(), MediaKind::Video);
            finish_open(&mut app, &rx);
            assert_eq!(
                app.session.as_ref().expect("restarted session").target(),
                target
            );
            app.toggle_pause();
            let history = app.edits.get(&tab).cloned().unwrap_or_default();
            resume::record(&mut app, true);
            assert_eq!(
                app.edits.get(&tab).cloned().unwrap_or_default(),
                history,
                "resume never edits history"
            );
            app.seek_to(media_time(Duration::from_secs(4)));
            app.remove_tab(tab, true);
            let tab = app.tabs.open_new(self.path.clone(), MediaKind::Video);
            let mut edits = EditHistory::default();
            let range = towavue_core::TimeRange::new(
                media_time(Duration::from_secs(1)),
                media_time(Duration::from_secs(2)),
            )
            .expect("deleted range");
            assert!(edits.push(
                EditOperation::Timeline(towavue_core::TimelineEdit::Delete(range)),
                MediaKind::Video,
            ));
            app.edits.insert(tab, edits.clone());
            app.load_path(self.path.clone(), MediaKind::Video);
            let deadline = Instant::now() + Duration::from_secs(10);
            while app.resume_open.is_none() {
                app.handle_app_event(
                    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .expect("resume before duration"),
                );
                assert!(
                    app.session.is_none(),
                    "timeline must be ready before playback"
                );
            }
            finish_open(&mut app, &rx);
            assert_eq!(
                app.session.as_ref().expect("edited session").target(),
                media_time(Duration::from_secs(3)),
                "original 4 s maps to edited 3 s on first open"
            );
            assert!(
                app.session
                    .as_ref()
                    .expect("edited session")
                    .timeline()
                    .is_some()
            );
            assert_eq!(app.edits[&tab].operations(), edits.operations());
            app.toggle_pause();
            app.seek_to(media_time(Duration::from_millis(3500)));
            app.remove_tab(tab, true);
            let tab = app.tabs.open_new(self.path.clone(), MediaKind::Video);
            app.load_path(self.path.clone(), MediaKind::Video);
            finish_open(&mut app, &rx);
            assert_eq!(
                app.session.as_ref().expect("unedited reopen").target(),
                media_time(Duration::from_millis(4500)),
                "closing an edited tab writes original-source time"
            );
            app.media_duration = Some(Duration::from_secs(5));
            app.seek_to(media_time(Duration::from_secs(5)));
            app.clock = Some(PlaybackClock::paused(
                media_time(Duration::from_secs(5)),
                1.0,
            ));
            app.pending_time = None;
            app.decode_finished = true;
            app.check_eof();
            assert_eq!(app.state, PlaybackState::Ended);
            assert!(!app.resume_owner.as_ref().expect("owner").natural_end);
            app.remove_tab(tab, true);
            let tab = app.tabs.open_new(self.path.clone(), MediaKind::Video);
            app.load_path(self.path.clone(), MediaKind::Video);
            finish_open(&mut app, &rx);
            assert_eq!(
                app.session.as_ref().expect("end preview reopen").target(),
                media_time(Duration::from_secs(5)),
                "a paused end-frame preview is not natural completion"
            );
            app.media_duration = Some(Duration::from_secs(5));
            app.clock = Some(PlaybackClock::paused(
                media_time(Duration::from_secs(5)),
                1.0,
            ));
            app.pending_time = None;
            app.decode_finished = true;
            app.state = PlaybackState::Playing;
            app.check_eof();
            assert_eq!(app.state, PlaybackState::Ended);
            assert!(app.resume_owner.as_ref().expect("owner").natural_end);
            app.remove_tab(tab, true);
            let tab = app.tabs.open_new(self.path.clone(), MediaKind::Video);
            app.load_path(self.path.clone(), MediaKind::Video);
            finish_open(&mut app, &rx);
            assert_eq!(
                app.session.as_ref().expect("EOF reopen").target(),
                MediaTime::ZERO,
                "original EOF persists a reset"
            );
            app.remove_tab(tab, true);
            app.resume_history.take();
            let corrupt = self.history.with_file_name("corrupt-resume.txt");
            std::fs::write(&corrupt, b"unknown version\n").expect("corrupt fixture");
            let notify = Arc::clone(&app.notify);
            app.resume_history = Some(
                VideoResumeHistory::new(corrupt.clone(), move |event| {
                    notify(AppEvent::VideoResume(event));
                })
                .expect("corrupt history worker"),
            );
            app.tabs.open_new(self.path.clone(), MediaKind::Video);
            app.load_path(self.path.clone(), MediaKind::Video);
            finish_open(&mut app, &rx);
            assert!(
                app.resume_owner.is_none(),
                "failed lookup cannot write history"
            );
            assert_eq!(
                app.session.as_ref().expect("fallback session").target(),
                MediaTime::ZERO
            );
            drop(app);
            assert_eq!(
                std::fs::read(corrupt).expect("preserved history"),
                b"unknown version\n"
            );
            assert_eq!(std::fs::read(&self.path).expect("source bytes"), original);
            assert_eq!(
                std::fs::metadata(&self.path)
                    .expect("metadata")
                    .modified()
                    .expect("mtime"),
                modified
            );
            eprintln!(
                "PASS video resume: asynchronous open, stale result, pending/live tabs, restart, edited close/reopen, EOF reset and corrupt-history fallback"
            );
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
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial {
            path,
            second,
            history,
            source: Some(source),
        })
        .expect("resume trial");
}

fn finish_open<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: &std::sync::mpsc::Receiver<AppEvent>,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.session.is_none() && app.state == PlaybackState::Loading {
        let remaining = deadline.saturating_duration_since(Instant::now());
        app.handle_app_event(events.recv_timeout(remaining).expect("open completion"));
    }
    assert!(
        app.session.is_some(),
        "open succeeded: {:?}",
        app.playback_error
    );
}

#[test]
fn resume_positions_use_original_time_and_only_reset_at_original_eof() {
    let time = |seconds: i64| MediaTime::from_nanoseconds(seconds * 1_000_000_000);
    let duration = Some(Duration::from_secs(10));
    let range = towavue_core::PlaybackRange {
        start: time(2),
        end: Some(time(8)),
    };
    let plan = EditTimeline::new(time(10), range).expect("trimmed timeline");
    assert_eq!(
        source_position(Some(&plan), time(3), duration, PlaybackState::Playing),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        source_position(Some(&plan), time(6), duration, PlaybackState::Ended),
        Some(Duration::from_secs(8)),
        "trim EOF is not original EOF"
    );
    assert_eq!(
        source_position(None, time(10), duration, PlaybackState::Ended),
        Some(Duration::ZERO)
    );
    assert_eq!(
        source_position(None, time(10), duration, PlaybackState::Paused),
        Some(Duration::from_secs(10)),
        "explicit end preview stays at that position"
    );
    assert_eq!(
        source_position(Some(&plan), time(7), duration, PlaybackState::Playing),
        None
    );
    assert_eq!(
        source_position(None, time(-1), duration, PlaybackState::Playing),
        None
    );
}
