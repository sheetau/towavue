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

fn click_preview_transport<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    tab: TabId,
) {
    let active = app.tabs.active().expect("foreground tab").id;
    let path = app
        .tabs
        .tabs()
        .iter()
        .find(|item| item.id == tab)
        .expect("preview tab")
        .target
        .current_path()
        .to_owned();
    let history = app.edits[&tab].clone();
    assert_eq!(
        app.preview_transport(tab, &path)
            .expect("loaded transport")
            .state,
        PlaybackState::Paused
    );
    let previous_context = app.ui_context.take();
    for density in [1.0, 1.25, 2.0] {
        let context = fonts::test_context();
        context.enable_accesskit();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        app.ui_context = Some(context.clone());
        let mut time = 0.0;
        let mut frame = |app: &mut Application<N>, point, pressed: Option<bool>| {
            time += 0.01;
            let mut events = vec![egui::Event::PointerMoved(point)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                });
            }
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    focused: true,
                    time: Some(time),
                    events,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    app.draw_top_bar(ui, &mut actions);
                },
            );
            let mut count = 0;
            for (index, action) in actions.iter().enumerate() {
                if !actions[..index].contains(action) {
                    app.handle_ui_action(action.clone());
                    count += 1;
                }
            }
            (output, count)
        };
        let node_center = |output: &egui::FullOutput, label: &str| {
            let bounds = output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("tree")
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap_or_else(|| panic!("missing control: {label}"))
                .1
                .bounds()
                .expect("control bounds");
            egui::pos2(
                (bounds.x0 + bounds.x1) as f32 / 2.0,
                (bounds.y0 + bounds.y1) as f32 / 2.0,
            )
        };
        let card = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .flat_map(|shape| match &shape.shape {
                    egui::Shape::Vec(shapes) => shapes.as_slice(),
                    shape => std::slice::from_ref(shape),
                })
                .find_map(|shape| match shape {
                    egui::Shape::Rect(rect)
                        if (240.0..245.0).contains(&rect.rect.width())
                            && rect.rect.height() > 50.0
                            && rect.fill == chrome::FLOATING_BACKGROUND =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("tab card remains open")
        };
        // Let the active tab's automatic scroll-to-visible animation settle
        // before hovering another tab; actual source movement closes its card.
        for _ in 0..50 {
            frame(app, egui::pos2(900.0, 400.0), None);
        }
        let (output, _) = frame(app, egui::pos2(900.0, 400.0), None);
        let source = node_center(&output, &display_name(&path));
        frame(app, source, None);
        let (output, _) = frame(app, source, None);
        let bounds = card(&output);
        let caption_top = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text)
                    if text.galley.text().starts_with("Preview near ")
                        || text.galley.text() == path.display().to_string() =>
                {
                    Some(text.pos.y)
                }
                _ => None,
            })
            .expect("caption below the fitted thumbnail");
        let point = egui::pos2(bounds.center().x, (bounds.top() + caption_top - 6.0) * 0.5);
        frame(app, point, None);
        let (output, _) = frame(app, point, None);
        let mut button = node_center(&output, "Play");
        for (label, state) in [
            ("Pause", PlaybackState::Playing),
            ("Play", PlaybackState::Paused),
        ] {
            frame(app, button, None);
            assert_eq!(frame(app, button, Some(true)).1, 0);
            assert_eq!(
                frame(app, button, Some(false)).1,
                1,
                "preview click: density={density}, next={label}, point={button:?}, active={}",
                active == tab
            );
            let transport = app.preview_transport(tab, &path).expect("same media");
            assert_eq!(transport.state, state);
            let (output, count) = frame(app, button, None);
            assert_eq!(count, 0);
            assert_eq!(card(&output), bounds, "transport must not move its card");
            button = node_center(&output, label);
            assert_eq!(app.tabs.active().expect("unchanged foreground").id, active);
            assert_eq!(app.edits[&tab], history);
        }
        let (output, _) = frame(app, button, None);
        let bar = node_center(&output, "Preview playback position (seconds)");
        frame(app, bar, None);
        assert_eq!(frame(app, bar, Some(true)).1, 0);
        assert_eq!(frame(app, bar, Some(false)).1, 1);
        let transport = app.preview_transport(tab, &path).expect("seek owner");
        let duration = transport.duration.expect("known duration");
        let session = if app.displayed_tab == Some(tab) {
            app.session.as_ref()
        } else {
            app.retained_playback[&tab].session.as_ref()
        }
        .expect("same tab session");
        let target = media_time(Duration::from_secs_f64(duration.as_seconds_f64() * 0.5));
        assert!((session.target().as_nanoseconds() - target.as_nanoseconds()).abs() <= 1);
        assert_eq!(transport.state, PlaybackState::Paused);
        assert_eq!(
            app.tabs.active().expect("seek does not activate").id,
            active
        );
        assert_eq!(app.edits[&tab], history);
        if app.displayed_tab != Some(tab) && transport.kind == MediaKind::Video {
            assert!(app.retained_playback[&tab].video_suspended);
        }
    }
    app.ui_context = previous_context;
}

fn exercise_retained_preview_seek<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: &mpsc::Receiver<AppEvent>,
    tab: TabId,
) {
    let active = app.tabs.active_id();
    let saved = &app.retained_playback[&tab];
    let (path, instance, kind) = (saved.path.clone(), saved.instance, saved.kind);
    let duration = media_time(saved.duration.expect("source duration"));
    let quarter = MediaTime::from_nanoseconds(duration.as_nanoseconds() / 4);
    let three_quarters = MediaTime::from_nanoseconds(duration.as_nanoseconds() * 3 / 4);
    let keep = towavue_core::TimeRange::new(quarter, three_quarters).expect("keep range");
    let history = app.edits.get_mut(&tab).expect("history");
    history.set_source_duration(Some(duration));
    history.push(
        EditOperation::Timeline(towavue_core::TimelineEdit::Keep(keep)),
        kind,
    );
    let history = history.clone();
    let plan = history.timeline(duration).expect("edited timeline");
    let end = plan.duration();
    let target = MediaTime::from_nanoseconds(end.as_nanoseconds() / 2);
    let selection = towavue_core::TimeRange::new(
        MediaTime::from_nanoseconds(end.as_nanoseconds() / 4),
        MediaTime::from_nanoseconds(end.as_nanoseconds() * 3 / 4),
    )
    .expect("edited selection");
    let saved = app
        .retained_playback
        .get_mut(&tab)
        .expect("background owner");
    saved
        .session
        .as_mut()
        .expect("session")
        .seek_with_timeline(MediaTime::ZERO, 1.0, plan.clone(), true)
        .expect("prepare edited source");
    saved.state = PlaybackState::Paused;
    saved.clock = Some(PlaybackClock::paused(MediaTime::ZERO, 1.0));
    saved.time_selection = Some(selection);
    saved.playback_selection = None;
    app.handle_preview_seek(tab, instance, &path, target);
    assert_eq!(app.retained_playback[&tab].state, PlaybackState::Paused);
    assert!(app.retained_playback[&tab].playback_selection.is_none());
    app.handle_preview_transport(tab, instance, path.clone(), CommandId::TogglePause);
    app.handle_preview_seek(tab, instance, &path, target);
    assert_eq!(
        app.retained_playback[&tab].playback_selection,
        Some(selection)
    );
    assert_eq!(app.retained_playback[&tab].state, PlaybackState::Playing);
    assert_eq!(
        app.retained_playback[&tab]
            .session
            .as_ref()
            .expect("selected session")
            .range()
            .end,
        Some(selection.end())
    );
    app.handle_preview_seek(tab, instance, &path, MediaTime::ZERO);
    assert!(app.retained_playback[&tab].playback_selection.is_none());
    assert_eq!(
        app.retained_playback[&tab]
            .session
            .as_ref()
            .expect("full plan")
            .timeline(),
        Some(&plan)
    );
    assert_eq!(
        app.retained_playback[&tab]
            .session
            .as_ref()
            .expect("full range")
            .range()
            .end,
        Some(end)
    );
    wait(app, events, |app| {
        app.retained_playback[&tab].position() > media_time(Duration::from_millis(40))
    });
    assert_eq!(app.retained_playback[&tab].state, PlaybackState::Playing);
    app.handle_preview_seek(tab, instance, &path, end);
    assert_eq!(app.retained_playback[&tab].state, PlaybackState::Paused);
    let generation = app.retained_playback[&tab]
        .session
        .as_ref()
        .expect("end preview")
        .generation();
    app.handle_preview_seek(tab, instance.wrapping_add(1), &path, MediaTime::ZERO);
    app.handle_preview_seek(
        tab,
        instance,
        &path.with_extension("stale"),
        MediaTime::ZERO,
    );
    app.palette_open = true;
    app.handle_preview_seek(tab, instance, &path, MediaTime::ZERO);
    app.palette_open = false;
    assert_eq!(
        app.retained_playback[&tab]
            .session
            .as_ref()
            .expect("same generation")
            .generation(),
        generation
    );
    assert_eq!(
        app.retained_playback[&tab]
            .session
            .as_ref()
            .expect("same target")
            .target(),
        end
    );
    app.retained_playback
        .get_mut(&tab)
        .expect("ended fixture")
        .state = PlaybackState::Ended;
    app.handle_preview_seek(tab, instance, &path, target);
    assert_eq!(app.retained_playback[&tab].state, PlaybackState::Paused);
    assert_eq!(
        app.retained_playback[&tab]
            .session
            .as_ref()
            .expect("reopened end preview")
            .target(),
        target
    );
    assert_eq!(
        app.retained_playback[&tab]
            .session
            .as_ref()
            .expect("same edit mapping")
            .timeline(),
        Some(&plan)
    );
    assert_eq!(app.edits[&tab], history);
    assert_eq!(app.tabs.active_id(), active);
    if kind == MediaKind::Video {
        assert!(app.retained_playback[&tab].video_suspended);
    }
}

fn recover_failed_playback<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: &mpsc::Receiver<AppEvent>,
) {
    let tab = app.tabs.active().expect("failed tab").id;
    let history = app.edits[&tab].clone();
    let path = app.path.clone();
    let view = app.image_view;
    let selection = app.time_selection;
    let error = app.playback_error.clone().expect("original playback error");
    let position = app.current_position();
    let epoch = app.graphics_epoch;
    let generation = app.generation;
    app.recover_graphics_device(position);
    assert!(app.renderer.is_some());
    assert!(app.graphics_epoch > epoch);
    assert_ne!(app.generation, generation, "pipeline rebuilt on new device");
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        service(app, events);
        assert_eq!(app.state, PlaybackState::Faulted);
        assert_eq!(app.playback_error.as_deref(), Some(error.as_str()));
        assert_eq!(app.current_position(), position);
        assert_eq!(app.edits[&tab], history);
        assert_eq!(app.path, path);
        assert_eq!(app.image_view, view);
        assert_eq!(app.time_selection, selection);
        if app.media_kind == Some(MediaKind::Audio) || app.pending_time.is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "recovered failed video frame");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        app.clock
            .as_ref()
            .expect("stopped clock")
            .paused_at
            .is_some()
    );
    if let Some((_, shown)) = &mut app.status_message {
        *shown = Instant::now() - STATUS_MESSAGE_DURATION;
    }
    assert_eq!(
        app.status_notice(),
        Some(format!("Could not play media: {error}")),
        "the failure stays visible after the temporary notice expires"
    );
}

fn run_trial(root: PathBuf, audio: bool, unknown_duration: bool, preview_controls: bool) {
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
        preview_controls: bool,
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
            if self.preview_controls {
                click_preview_transport(&mut app, first);
                app.open_external(self.root.join("image.bmp"), true);
                wait(&mut app, &events, |app| {
                    !app.image_loading && app.image.is_some()
                });
                click_preview_transport(&mut app, first);
                exercise_retained_preview_seek(&mut app, &events, first);
                if self.audio {
                    let music = open_muted(&mut app, self.root.join("tone.wav"), MediaKind::Audio);
                    wait(&mut app, &events, |app| app.media_duration.is_some());
                    app.toggle_pause();
                    click_preview_transport(&mut app, music);
                    app.open_external(self.root.join("image.bmp"), false);
                    wait(&mut app, &events, |app| {
                        !app.image_loading && app.image.is_some()
                    });
                    click_preview_transport(&mut app, music);
                    exercise_retained_preview_seek(&mut app, &events, music);
                }
                event_loop.exit();
                return;
            }
            let first_geometry = app.session.as_ref().expect("first video").video_geometry();
            let first_presented = app
                .session
                .as_ref()
                .expect("first video")
                .metrics()
                .presented_frame_count;
            assert!(first_geometry.is_some());
            // Keep the fixture larger than the viewport so its nonzero pan is valid.
            app.image_view.zoom = ZoomMode::Custom(8.0);
            app.image_view.pan = (31.0, -17.0);
            app.image_view.selection = Some(UnitRect::FULL);
            let view = app.image_view;
            let first_instance = app.media_generation;
            let first_generation = app.generation;
            tab_focus::tests::hardware_focus(&mut app, "Play / replay", true);
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
                tab_focus::tests::hardware_focus(&mut app, "Repeat off", true);
                let instance = app.media_generation;
                let focus = app
                    .ui_context
                    .as_ref()
                    .expect("UI")
                    .memory(egui::Memory::focused);
                let selection = towavue_core::TimeRange::new(
                    MediaTime::ZERO,
                    media_time(Duration::from_secs(1)),
                );
                app.time_selection = selection;
                app.image_view.zoom = ZoomMode::Custom(2.0);
                for paused in [false, true, false] {
                    if (app.state == PlaybackState::Paused) != paused {
                        app.toggle_pause();
                    }
                    // Play may re-arm the selected range; reopening the same tab
                    // must preserve the session after that intentional seek.
                    let generation = app.session.as_ref().expect("audio session").generation();
                    let state = app.state;
                    let position = app.current_position();
                    app.open_external(self.root.join("tone.wav"), false);
                    assert_eq!(app.tabs.active().expect("same audio tab").id, id);
                    assert_eq!(
                        app.media_generation, instance,
                        "external current audio must not reload"
                    );
                    assert_eq!(
                        app.session.as_ref().expect("same session").generation(),
                        generation
                    );
                    assert_eq!(app.state, state);
                    if paused {
                        assert_eq!(app.current_position(), position);
                    } else {
                        assert!(app.current_position() >= position);
                    }
                    assert_eq!(app.time_selection, selection);
                    assert_eq!(app.image_view.zoom, ZoomMode::Custom(2.0));
                    assert_eq!(
                        app.ui_context
                            .as_ref()
                            .expect("UI")
                            .memory(egui::Memory::focused),
                        focus
                    );
                }
                id
            });
            app.open_external(self.root.join("image.bmp"), true);
            let image = app.tabs.active().expect("image tab").id;
            wait(&mut app, &events, |app| {
                !app.image_loading && app.image.is_some()
            });
            tab_focus::tests::hardware_focus(&mut app, "Reading mode", true);
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
            let preview_path = app.retained_playback[&second].path.clone();
            app.handle_ui_action(UiAction::PreviewTransport(
                second,
                second_instance.wrapping_add(1),
                preview_path.clone(),
                CommandId::TogglePause,
            ));
            assert_eq!(app.retained_playback[&second].state, PlaybackState::Ended);
            app.handle_ui_action(UiAction::PreviewTransport(
                second,
                second_instance,
                preview_path.clone(),
                CommandId::TogglePause,
            ));
            assert_eq!(app.retained_playback[&second].state, PlaybackState::Playing);
            wait(&mut app, &events, |app| {
                app.retained_playback[&second].position() > media_time(Duration::from_millis(40))
            });
            app.handle_ui_action(UiAction::PreviewTransport(
                second,
                second_instance,
                preview_path,
                CommandId::TogglePause,
            ));
            assert_eq!(app.retained_playback[&second].state, PlaybackState::Paused);
            // WASAPI pause is queued, not acknowledged synchronously. Match the
            // native pause control's settling interval before checking a held clock.
            let requested_pause = app.retained_playback[&second].position();
            if self.audio {
                std::thread::sleep(Duration::from_millis(100));
                service(&mut app, &events);
            }
            let preview_paused = app.retained_playback[&second].position();
            assert!(
                preview_paused >= requested_pause
                    && preview_paused <= requested_pause.saturating_add(Duration::from_millis(100))
            );
            std::thread::sleep(Duration::from_millis(40));
            service(&mut app, &events);
            assert_eq!(app.retained_playback[&second].position(), preview_paused);
            if !self.unknown_duration {
                let selection = towavue_core::TimeRange::new(
                    preview_paused,
                    preview_paused.saturating_add(Duration::from_millis(80)),
                )
                .expect("preview selection");
                app.retained_playback
                    .get_mut(&second)
                    .expect("background video")
                    .time_selection = Some(selection);
                let path = app.retained_playback[&second].path.clone();
                app.handle_ui_action(UiAction::PreviewTransport(
                    second,
                    second_instance,
                    path.clone(),
                    CommandId::TogglePause,
                ));
                assert_eq!(
                    app.retained_playback[&second].playback_selection,
                    Some(selection)
                );
                wait(&mut app, &events, |app| {
                    app.retained_playback[&second].state == PlaybackState::Ended
                });
                assert!(app.retained_playback[&second].position() >= selection.end());
                app.handle_ui_action(UiAction::PreviewTransport(
                    second,
                    second_instance,
                    path.clone(),
                    CommandId::TogglePause,
                ));
                assert_eq!(
                    app.retained_playback[&second]
                        .session
                        .as_ref()
                        .expect("selection session")
                        .target(),
                    selection.start()
                );
                app.handle_ui_action(UiAction::PreviewTransport(
                    second,
                    second_instance,
                    path,
                    CommandId::TogglePause,
                ));
                assert_eq!(app.retained_playback[&second].state, PlaybackState::Paused);
            }
            assert_eq!(
                app.tabs.active().expect("preview keeps active image").id,
                image
            );
            assert_eq!(app.retained_playback[&first].position(), paused_position);
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
            tab_focus::tests::hardware_focus(&mut app, "Play / replay", false);
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
            app.toggle_pause();
            wait(&mut app, &events, |app| {
                app.current_position() > paused_position.saturating_add(Duration::from_millis(40))
            });
            app.fail("owned active fault".into());
            assert!(app.clock.as_ref().expect("fault clock").paused_at.is_some());
            if self.audio {
                std::thread::sleep(Duration::from_millis(80));
                service(&mut app, &events);
                let stopped = app
                    .session
                    .as_ref()
                    .expect("audio session")
                    .audio_position()
                    .expect("audio clock");
                assert!(
                    stopped.saturating_add(Duration::from_millis(200))
                        < media_time(app.media_duration.expect("video duration")),
                    "verify a pause before natural EOF"
                );
                std::thread::sleep(Duration::from_millis(80));
                service(&mut app, &events);
                assert_eq!(
                    app.session
                        .as_ref()
                        .expect("audio session")
                        .audio_position(),
                    Some(stopped),
                    "failed active media must stop WASAPI output"
                );
            }
            recover_failed_playback(&mut app, &events);
            let stopped = app.current_position();
            app.activate_tab(image);
            assert!(app.playback_error.is_none(), "image is unaffected");
            app.recover_graphics_device(MediaTime::ZERO);
            let saved = &app.retained_playback[&first];
            assert_eq!(saved.state, PlaybackState::Faulted);
            assert_eq!(saved.error.as_deref(), Some("owned active fault"));
            assert_eq!(saved.position(), stopped);
            app.activate_tab(first);
            assert_eq!(app.state, PlaybackState::Faulted);
            assert_eq!(app.playback_error.as_deref(), Some("owned active fault"));
            assert_eq!(app.current_position(), stopped);
            app.remove_tab(second, false);
            app.handle_app_event(AppEvent::Playback(
                second_instance,
                PlaybackEvent::Failed(first_generation, "closed background".into()),
            ));
            assert_eq!(app.tabs.active().expect("first").id, first);
            assert_eq!(app.playback_error.as_deref(), Some("owned active fault"));
            let new = open_muted(&mut app, self.root.join("second.mp4"), MediaKind::Video);
            assert!(
                app.media_generation > second_instance && app.media_generation > first_instance
            );
            app.fail("owned early fault".into());
            let deadline = Instant::now() + Duration::from_secs(8);
            while app.pending_time.is_none() {
                service(&mut app, &events);
                assert!(Instant::now() < deadline, "late first video frame");
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(app.state, PlaybackState::Faulted);
            assert_eq!(app.playback_error.as_deref(), Some("owned early fault"));
            assert!(
                app.clock
                    .as_ref()
                    .expect("late-frame clock")
                    .paused_at
                    .is_some(),
                "a frame arriving after failure must not restart the clock"
            );
            app.remove_tab(new, false);
            if let Some(music) = music {
                app.activate_tab(music);
                tab_focus::tests::hardware_focus(&mut app, "Repeat off", false);
                let end = app.media_duration.expect("audio duration");
                app.seek_to(media_time(end.saturating_sub(Duration::from_millis(50))));
                if app.state != PlaybackState::Playing {
                    app.toggle_pause();
                }
                let deadline = Instant::now() + Duration::from_secs(8);
                let drained = loop {
                    if let Some(event) = app
                        .session
                        .as_ref()
                        .expect("audio session")
                        .try_audio_event()
                    {
                        assert!(
                            matches!(event, AudioOutputEvent::Drained),
                            "expected normal native audio completion"
                        );
                        break event;
                    }
                    assert!(Instant::now() < deadline, "native audio drain");
                    std::thread::sleep(Duration::from_millis(5));
                };
                app.fail("owned fault before drain delivery".into());
                app.handle_audio_event(drained);
                assert!(app.audio_drained);
                assert_eq!(app.state, PlaybackState::Faulted);
                assert!(
                    app.clock
                        .as_ref()
                        .expect("drained clock")
                        .paused_at
                        .is_some(),
                    "late audio completion must not restart the failed clock"
                );
                assert_eq!(
                    app.clock.as_ref().expect("drained clock").position(),
                    app.session
                        .as_ref()
                        .expect("audio session")
                        .audio_position()
                        .expect("drained position"),
                    "constructing a stopped clock must not advance its source position"
                );
                let stopped = app.current_position();
                assert_eq!(app.current_position(), stopped);
                recover_failed_playback(&mut app, &events);
                let selection = app.time_selection.expect("audio time selection");
                app.playback_selection = Some(selection);
                app.seek_to(selection.end());
                app.fail("owned fault at playback end".into());
                assert!(
                    !app.session
                        .as_ref()
                        .expect("ended audio")
                        .range()
                        .contains(app.current_position()),
                    "exercise recovery at the exclusive playback end"
                );
                recover_failed_playback(&mut app, &events);
                app.remove_tab(music, false);
            }
            app.activate_tab(image);
            tab_focus::tests::hardware_focus(&mut app, "Reading mode", false);
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
            preview_controls,
        })
        .expect("retained playback trial");
}

#[test]
fn video_tabs_keep_paused_state_and_end_in_background() {
    if let Some(root) = tests::isolated_test_root(
        "playback_tab_tests::video_tabs_keep_paused_state_and_end_in_background",
    ) {
        run_trial(root, false, false, false);
    }
}

#[test]
#[ignore = "requires Windows D3D11 and a live shared WASAPI endpoint; owned media is muted"]
fn audio_video_tabs_continue_behind_images_and_recover_the_shared_device() {
    if let Some(root) = tests::isolated_test_root(
        "playback_tab_tests::audio_video_tabs_continue_behind_images_and_recover_the_shared_device",
    ) {
        run_trial(root, true, false, false);
    }
}

#[test]
fn unknown_duration_video_reaches_real_eof_in_background() {
    if let Some(root) = tests::isolated_test_root(
        "playback_tab_tests::unknown_duration_video_reaches_real_eof_in_background",
    ) {
        run_trial(root, false, true, false);
    }
}

#[test]
#[ignore = "requires Windows D3D11 and a live shared WASAPI endpoint; owned media is muted"]
fn tab_preview_play_pause_keeps_active_and_background_cards_open() {
    if let Some(root) = tests::isolated_test_root(
        "playback_tab_tests::tab_preview_play_pause_keeps_active_and_background_cards_open",
    ) {
        run_trial(root, true, false, true);
    }
}

#[test]
fn tab_preview_seeks_video_only_background_without_losing_its_clock() {
    if let Some(root) = tests::isolated_test_root(
        "playback_tab_tests::tab_preview_seeks_video_only_background_without_losing_its_clock",
    ) {
        run_trial(root, false, false, true);
    }
}
