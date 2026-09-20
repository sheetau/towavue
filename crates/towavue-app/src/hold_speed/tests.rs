use super::*;

#[test]
fn held_video_progress_reaches_painted_status_and_does_not_expire_mid_gesture() {
    let Some(_root) = crate::tests::isolated_test_root(
        "hold_speed::tests::held_video_progress_reaches_painted_status_and_does_not_expire_mid_gesture",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    app.media_kind = Some(MediaKind::Video);
    app.state = PlaybackState::Playing;
    app.held_speed = Some(Held {
        token: 1,
        media: app.media_generation,
        rate: 2.0,
        was_paused: false,
    });
    for progress in [0, 50, 100] {
        app.show_hold_progress(progress);
        app.status_message.as_mut().expect("notice").1 = Instant::now() - Duration::from_secs(60);
        for _ in 0..2 {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 300.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
                },
            );
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains(&format!("{progress}% to lock 1×")))));
        }
    }
}

#[test]
fn video_hold_preserves_history_bounds_and_the_prior_transport_state() {
    run_session_trial(
        false,
        "hold_speed::tests::video_hold_preserves_history_bounds_and_the_prior_transport_state",
    );
}

#[test]
#[ignore = "requires a live shared WASAPI endpoint; generated audio is silence and muted"]
fn audio_hold_preserves_history_bounds_and_the_prior_transport_state() {
    run_session_trial(
        true,
        "hold_speed::tests::audio_hold_preserves_history_bounds_and_the_prior_transport_state",
    );
}

fn run_session_trial(audio: bool, test: &str) {
    let Some(root) = crate::tests::isolated_test_root(test) else {
        return;
    };
    let path = root.join("hold.mkv");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe");
    let mut command = std::process::Command::new(ffmpeg);
    command.args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=160x96:rate=25:duration=4",
    ]);
    if audio {
        command.args([
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo",
            "-c:a",
            "pcm_s16le",
            "-shortest",
        ]);
    }
    assert!(
        command
            .args(["-c:v", "mpeg4"])
            .arg(&path)
            .status()
            .expect("silent fixture")
            .success()
    );
    struct Trial {
        path: PathBuf,
        audio: bool,
    }
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("hidden window"),
            );
            let renderer = match FrameRenderer::new(&window) {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("SKIP hold-speed session: D3D11 unavailable: {error}");
                    event_loop.exit();
                    return;
                }
            };
            let mut app = Application::new(None, |_| {}).expect("app");
            app.window = Some(window.clone());
            app.renderer = Some(renderer);
            let kind = if self.audio {
                MediaKind::Audio
            } else {
                MediaKind::Video
            };
            let tab = app.tabs.open_new(self.path.clone(), kind);
            app.load_path(self.path.clone(), kind);
            if app.state == PlaybackState::Faulted
                && self.audio
                && app.playback_error.as_deref().is_some_and(|error| {
                    error.starts_with("audio output failed: WASAPI output failed:")
                })
            {
                eprintln!(
                    "SKIP hold-speed session: shared WASAPI unavailable: {:?}",
                    app.playback_error
                );
                event_loop.exit();
                return;
            }
            assert_eq!(
                app.state,
                PlaybackState::Playing,
                "non-endpoint startup errors must fail: {:?}",
                app.playback_error
            );
            app.media_duration = Some(Duration::from_secs(4));
            app.push_edit(EditOperation::SetVolume(0.0));
            app.push_edit(EditOperation::SetRate(1.25));
            let time = |ms: i64| MediaTime::from_nanoseconds(ms * 1_000_000);
            let range = |a, b| towavue_core::TimeRange::new(time(a), time(b)).expect("range");
            app.push_edit(EditOperation::Timeline(towavue_core::TimelineEdit::Delete(
                range(1000, 1500),
            )));
            app.push_edit(EditOperation::Timeline(
                towavue_core::TimelineEdit::Stretch(range(250, 750), time(1000)),
            ));
            app.playback_selection = Some(range(200, 3800));
            app.seek_to(time(400));
            let history = app.edits[&tab].clone();
            let plan = app.session.as_ref().expect("session").timeline().cloned();
            let bounds = app.session.as_ref().expect("session").range();
            let context = fonts::test_context();
            app.ui_context = Some(context.clone());
            let mut ui_time = 0.0;
            status_frame(&mut app, &context, ui_time, vec![]);
            for paused in [false, true] {
                for interrupt in 0..3 {
                    app.playback_selection = Some(range(200, 3800));
                    app.seek_to(time(400));
                    if (app.state == PlaybackState::Paused) != paused {
                        app.toggle_pause();
                    }
                    ui_time += 1.0;
                    let press = status_button(true);
                    status_frame(&mut app, &context, ui_time, vec![press]);
                    assert!(app.held_speed.is_none());
                    status_frame(&mut app, &context, ui_time + 0.41, vec![]);
                    assert!(app.held_speed.is_some(), "normal play button starts a hold");
                    assert_eq!(app.state, PlaybackState::Playing);
                    assert_eq!(app.playback_rate(), 2.0);
                    let start = app.current_position();
                    let deadline = Instant::now() + Duration::from_millis(150);
                    while Instant::now() < deadline {
                        app.poll_audio();
                        app.load_next_frame();
                        app.advance_media();
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    assert!(app.current_position() > start, "held transport advances");
                    match interrupt {
                        0 => {}
                        1 => {
                            app.window_event(event_loop, window.id(), WindowEvent::Focused(false));
                        }
                        2 => app.dispatch(CommandId::SeekForward),
                        _ => unreachable!(),
                    }
                    status_frame(
                        &mut app,
                        &context,
                        ui_time + 0.7,
                        vec![status_button(false)],
                    );
                    assert!(app.held_speed.is_none());
                    assert_eq!(app.playback_rate(), 1.25);
                    if interrupt != 2 {
                        assert_eq!(
                            app.state,
                            if paused {
                                PlaybackState::Paused
                            } else {
                                PlaybackState::Playing
                            },
                            "release must not also click Play"
                        );
                        assert_eq!(app.session.as_ref().expect("session").range(), bounds);
                    }
                    assert_eq!(
                        app.session.as_ref().expect("session").timeline(),
                        plan.as_ref()
                    );
                    assert_eq!(app.edits[&tab], history);
                }
            }
            // Native release must leave a short click intact, but consume a held click.
            for long in [false, true] {
                app.seek_to(time(400));
                if app.state == PlaybackState::Playing {
                    app.toggle_pause();
                }
                ui_time += 1.0;
                status_frame(&mut app, &context, ui_time, vec![status_button(true)]);
                if long {
                    status_frame(&mut app, &context, ui_time + 0.41, vec![]);
                }
                app.window_event(
                    event_loop,
                    window.id(),
                    WindowEvent::MouseInput {
                        device_id: winit::event::DeviceId::dummy(),
                        state: ElementState::Released,
                        button: winit::event::MouseButton::Left,
                    },
                );
                status_frame(
                    &mut app,
                    &context,
                    ui_time + 0.5,
                    vec![status_button(false)],
                );
                assert_eq!(
                    app.state,
                    if long {
                        PlaybackState::Paused
                    } else {
                        PlaybackState::Playing
                    }
                );
                assert_eq!(app.playback_rate(), 1.25);
            }
            if !self.audio {
                for (timeline, fullscreen) in [(false, false), (true, false), (true, true)] {
                    app.timeline_open = timeline;
                    app.fullscreen = fullscreen;
                    app.seek_to(time(400));
                    if app.state == PlaybackState::Playing {
                        app.toggle_pause();
                    }
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while app
                        .session
                        .as_ref()
                        .expect("session")
                        .video_refresh_pending()
                        && Instant::now() < deadline
                    {
                        app.load_next_frame();
                        app.advance_media();
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    assert!(
                        app.session
                            .as_ref()
                            .expect("session")
                            .video_geometry()
                            .is_some()
                    );
                    ui_time += 1.0;
                    body_frame(&mut app, &context, ui_time, vec![]);
                    body_frame(&mut app, &context, ui_time + 0.1, vec![body_button(true)]);
                    body_frame(&mut app, &context, ui_time + 0.51, vec![]);
                    assert_eq!(
                        app.held_speed.is_some(),
                        !timeline || fullscreen,
                        "viewing surface only"
                    );
                    body_frame(&mut app, &context, ui_time + 0.7, vec![body_button(false)]);
                    assert!(app.held_speed.is_none());
                    assert_eq!(app.playback_rate(), 1.25);
                    assert_eq!(app.state, PlaybackState::Paused);
                    assert_eq!(app.edits[&tab], history);
                    for point in [egui::pos2(240.0, 150.0), egui::pos2(240.0, 1.0)] {
                        ui_time += 1.0;
                        let (cursor, targets) = body_frame(
                            &mut app,
                            &context,
                            ui_time,
                            vec![egui::Event::PointerMoved(point)],
                        );
                        let viewing = !timeline || fullscreen;
                        if viewing {
                            assert_eq!(cursor, egui::CursorIcon::PointingHand);
                            if point.y == 1.0 {
                                assert!(
                                    !app.video_rect.expect("video").contains(point),
                                    "letterbox click fixture"
                                );
                                assert!(
                                    targets.iter().any(|rect| rect.contains(point)),
                                    "volume wheel includes the letterbox margin"
                                );
                            }
                        }
                        let button = |pressed| egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        };
                        body_frame(&mut app, &context, ui_time + 0.05, vec![button(true)]);
                        assert_eq!(app.state, PlaybackState::Paused, "click waits for release");
                        body_frame(&mut app, &context, ui_time + 0.15, vec![button(false)]);
                        assert_eq!(
                            app.state,
                            if viewing {
                                PlaybackState::Playing
                            } else {
                                PlaybackState::Paused
                            }
                        );
                        body_frame(
                            &mut app,
                            &context,
                            ui_time + 0.25,
                            vec![button(true), button(false)],
                        );
                        assert_eq!(
                            app.state,
                            PlaybackState::Paused,
                            "one toggle per batched click"
                        );
                        assert_eq!(app.edits[&tab], history);
                    }
                }
                app.fullscreen = false;
                app.timeline_open = false;
                for blocked in 0..6 {
                    ui_time += 1.0;
                    body_frame(&mut app, &context, ui_time, vec![]);
                    match blocked {
                        0 => app.filmstrip_open = true,
                        1 => app.palette_open = true,
                        2 => app.grid_open = true,
                        3 => app.export_error = Some("injected modal".into()),
                        _ => {}
                    }
                    let mut press = body_button(true);
                    if blocked == 4
                        && let egui::Event::PointerButton { modifiers, .. } = &mut press
                    {
                        *modifiers = egui::Modifiers::CTRL;
                    }
                    body_frame(&mut app, &context, ui_time + 0.1, vec![press]);
                    if blocked == 5 {
                        app.window_event(event_loop, window.id(), WindowEvent::Focused(false));
                    }
                    body_frame(&mut app, &context, ui_time + 0.2, vec![body_button(false)]);
                    assert_eq!(app.state, PlaybackState::Paused, "blocked click {blocked}");
                    app.filmstrip_open = false;
                    app.palette_open = false;
                    app.grid_open = false;
                    app.export_error = None;
                }
            }
            if !self.audio {
                app.fullscreen = false;
                app.timeline_open = false;
                app.set_time_selection(None);
                app.playback_selection = None;
                for original in [1.0, 1.25, 2.0, 3.0] {
                    app.push_edit(EditOperation::SetRate(original));
                    for paused in [false, true] {
                        for commit in [false, true] {
                            app.seek_to(time(400));
                            if (app.state == PlaybackState::Paused) != paused {
                                app.toggle_pause();
                            }
                            let before = app.edits[&tab].operations().to_vec();
                            ui_time += 1.0;
                            body_frame(&mut app, &context, ui_time, vec![]);
                            body_frame(&mut app, &context, ui_time + 0.1, vec![body_button(true)]);
                            body_frame(&mut app, &context, ui_time + 0.51, vec![]);
                            let token = app.held_speed.as_ref().expect("held video").token;
                            body_frame(
                                &mut app,
                                &context,
                                ui_time + 0.6,
                                vec![egui::Event::PointerMoved(egui::pos2(240.0, 210.0))],
                            );
                            let target = if original == 2.0 { 1.0 } else { 2.0 };
                            assert!(
                                app.status_notice()
                                    .expect("progress")
                                    .contains(&format!("100% to lock {target}×"))
                            );
                            let release = egui::Event::PointerButton {
                                pos: egui::pos2(240.0, if commit { 210.0 } else { 150.0 }),
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            };
                            body_frame(&mut app, &context, ui_time + 0.7, vec![release]);
                            assert!(app.held_speed.is_none());
                            assert_eq!(
                                app.state,
                                if paused {
                                    PlaybackState::Paused
                                } else {
                                    PlaybackState::Playing
                                }
                            );
                            assert_eq!(app.playback_rate(), if commit { target } else { original });
                            assert_eq!(
                                app.edit_state().rate,
                                if commit { target } else { original }
                            );
                            let after = app.edits[&tab].operations().to_vec();
                            app.handle_hold_speed(
                                app.media_generation,
                                app.generation,
                                Action::Commit(token),
                            );
                            assert_eq!(app.edits[&tab].operations(), after);
                            if commit {
                                app.undo_edit(false);
                                assert_eq!(app.playback_rate(), original);
                                assert_eq!(app.edits[&tab].operations(), before);
                                app.undo_edit(true);
                                assert_eq!(app.playback_rate(), target);
                                app.undo_edit(false);
                            } else {
                                assert_eq!(after, before);
                                assert!(
                                    app.status_message.is_none(),
                                    "release quietly restores playback"
                                );
                            }
                        }
                    }
                }
                app.edits.insert(tab, history.clone());
                app.sync_playback_edits();
            }
            // A held press never leaks its rate into the retained background tab.
            app.seek_to(time(400));
            if app.state == PlaybackState::Paused {
                app.toggle_pause();
            }
            app.begin_hold_speed(999);
            let image = app
                .tabs
                .open_new(self.path.with_extension("png"), MediaKind::Image);
            app.load_path(self.path.with_extension("png"), MediaKind::Image);
            assert_eq!(
                app.retained_playback[&tab]
                    .session
                    .as_ref()
                    .expect("retained session")
                    .rate(),
                1.25
            );
            assert!(app.held_speed.is_none());
            app.activate_tab(tab);
            // Closing an inactive final image must drop its cache without touching this live session.
            let cached = Arc::new(DecodedImage {
                animation_plays: 0,
                format: "test",
                frames: vec![towavue_runtime_windows::DecodedImageFrame {
                    width: 2,
                    height: 1,
                    rgba: vec![255; 8],
                    delay: Duration::ZERO,
                }],
            });
            let weak = Arc::downgrade(&cached);
            app.image_texture_cache
                .load(
                    &context,
                    &self.path.with_extension("png"),
                    cached,
                    TextureOptions::LINEAR,
                )
                .expect("previous image cache");
            let instance = app.media_generation;
            let generation = app.session.as_ref().expect("live session").generation();
            let position = app.current_position();
            let state = app.state;
            app.close_tab_unchecked(image);
            assert!(app.image_texture_cache.entries.is_empty() && weak.upgrade().is_none());
            assert_eq!(app.media_generation, instance);
            assert_eq!(
                app.session
                    .as_ref()
                    .expect("same live session")
                    .generation(),
                generation
            );
            assert_eq!(app.state, state);
            assert!(app.current_position() >= position);
            assert_eq!(app.playback_rate(), 1.25);
            assert_eq!(app.edits[&tab], history);
            // Reaching EOF during a paused audition restores the rate, without restart.
            app.video_repeat = true;
            app.seek_to(time(3990));
            if app.state == PlaybackState::Playing {
                app.toggle_pause();
            }
            app.begin_hold_speed(1000);
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.state != PlaybackState::Ended && Instant::now() < deadline {
                app.poll_audio();
                app.load_next_frame();
                app.advance_media();
                app.decode_finished = app.session.as_ref().expect("session").decode_finished();
                app.check_eof();
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(app.state, PlaybackState::Ended);
            assert!(app.held_speed.is_none());
            assert_eq!(app.playback_rate(), 1.25);
            assert_eq!(app.edits[&tab], history);
            if !self.audio {
                ui_time += 1.0;
                body_frame(&mut app, &context, ui_time, vec![]);
                body_frame(
                    &mut app,
                    &context,
                    ui_time + 0.1,
                    vec![body_button(true), body_button(false)],
                );
                assert_eq!(
                    app.state,
                    PlaybackState::Playing,
                    "click replays ended video"
                );
                assert_eq!(app.edits[&tab], history);
            }
            if !self.audio {
                // Exercise the native release bridge: no redraw occurs between
                // the final cursor movement and MouseInput::Released.
                let context = fonts::test_context();
                app.ui_context = Some(context.clone());
                app.ui_state = Some(egui_winit::State::new(
                    context.clone(),
                    egui::ViewportId::ROOT,
                    window.as_ref(),
                    Some(window.scale_factor() as f32),
                    window.theme(),
                    None,
                ));
                app.fullscreen = false;
                app.timeline_open = false;
                app.seek_to(time(400));
                let size = window.inner_size();
                app.renderer
                    .as_mut()
                    .expect("renderer")
                    .resize_surface(size.width, size.height)
                    .expect("size hidden surface");
                app.window_event(event_loop, window.id(), WindowEvent::Focused(true));
                app.render_frame();
                let origin = app
                    .video_scrub_surface
                    .as_ref()
                    .expect("viewport")
                    .1
                    .center();
                let device_id = winit::event::DeviceId::dummy();
                let cursor = |position: egui::Pos2| WindowEvent::CursorMoved {
                    device_id,
                    position: winit::dpi::PhysicalPosition::new(
                        f64::from(position.x) * window.scale_factor(),
                        f64::from(position.y) * window.scale_factor(),
                    ),
                };
                let button = |state| WindowEvent::MouseInput {
                    device_id,
                    state,
                    button: winit::event::MouseButton::Left,
                };
                for commit in [false, true] {
                    app.seek_to(time(400));
                    if app.state == PlaybackState::Playing {
                        app.toggle_pause();
                    }
                    let before = app.edits[&tab].operations().to_vec();
                    app.window_event(event_loop, window.id(), cursor(origin));
                    app.window_event(event_loop, window.id(), button(ElementState::Pressed));
                    app.render_frame();
                    context.data_mut(|data| {
                        let input = data.get_temp_mut_or_default::<Input>(input_id());
                        input.press.as_mut().expect("native press").start -= 0.5;
                    });
                    app.render_frame();
                    assert!(app.held_speed.is_some(), "native hold activated");
                    let end = origin + egui::vec2(0.0, if commit { 65.0 } else { 59.0 });
                    app.window_event(event_loop, window.id(), cursor(end));
                    app.window_event(event_loop, window.id(), button(ElementState::Released));
                    assert!(
                        app.held_speed.is_none(),
                        "release consumed without later redraw"
                    );
                    assert_eq!(app.state, PlaybackState::Paused);
                    assert_eq!(app.playback_rate(), if commit { 2.0 } else { 1.25 });
                    if commit {
                        assert_eq!(app.edits[&tab].operations().len(), before.len() + 1);
                        app.undo_edit(false);
                    }
                    assert_eq!(app.edits[&tab].operations(), before);
                }
            }
            eprintln!(
                "PASS hold-speed session: audio={}, normal button hold/release, prior pause/rate, edited bounds/history, focus/Seek/tab/EOF",
                self.audio
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
        .run_app(&mut Trial { path, audio })
        .expect("hold trial");
}

fn status_button(pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui::pos2(20.0, 284.0),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn body_button(pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui::pos2(240.0, 150.0),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn body_frame<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    context: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
) -> (egui::CursorIcon, Vec<egui::Rect>) {
    let mut actions = Vec::new();
    let mut targets = Vec::new();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(480.0, 300.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| {
            app.draw_video_edit_overlay(ui, &mut targets, &mut actions);
        },
    );
    for (index, action) in actions.iter().enumerate() {
        if !actions[..index].contains(action) {
            app.handle_ui_action(action.clone());
        }
    }
    (
        output.platform_output.cursor_icon,
        targets
            .iter()
            .map(|response| response.interact_rect)
            .collect(),
    )
}

fn status_frame<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    context: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
) {
    let mut actions = Vec::new();
    let _ = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(480.0, 300.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| {
            app.draw_status_bar(ui, &mut actions, &mut Vec::new());
        },
    );
    for (index, action) in actions.iter().enumerate() {
        if !actions[..index].contains(action) {
            app.handle_ui_action(action.clone());
        }
    }
}

fn button(pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui::pos2(50.0, 50.0),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn frame(
    context: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
    enabled: bool,
) -> (Vec<Action>, bool) {
    frame_mode(context, time, events, enabled, true)
}

fn frame_mode(
    context: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
    enabled: bool,
    allow_latch: bool,
) -> (Vec<Action>, bool) {
    let mut actions = Vec::new();
    let mut clicked = false;
    let _ = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(300.0, 200.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| {
            let rect = egui::Rect::from_min_max(egui::pos2(20.0, 20.0), egui::pos2(100.0, 100.0));
            let response = ui.interact(rect, "hold-test".into(), egui::Sense::click_and_drag());
            for _ in 0..2 {
                let (action, consumed) = update(&response, enabled, allow_latch);
                if let Some(action) = action {
                    actions.push(action);
                }
                clicked |= response.clicked() && !consumed;
            }
            if context.current_pass_index() == 0 {
                context.request_discard("test duplicate layout pass");
            }
        },
    );
    (actions, clicked)
}

#[test]
fn stationary_hold_is_single_use_and_never_becomes_a_click_on_release() {
    let context = fonts::test_context();
    frame(&context, 0.0, vec![], true);
    assert!(frame(&context, 0.1, vec![button(true)], true).0.is_empty());
    assert!(frame(&context, 0.49, vec![], true).0.is_empty());
    let (actions, clicked) = frame(&context, 0.51, vec![], true);
    assert_eq!(actions.len(), 1);
    let Action::Begin(token) = actions[0] else {
        panic!("begin")
    };
    assert!(!clicked);
    assert!(frame(&context, 0.8, vec![], true).0.is_empty());
    assert_eq!(
        frame(&context, 0.9, vec![button(false)], true),
        (vec![Action::End(token)], false)
    );
    assert!(frame(&context, 1.0, vec![button(true)], true).0.is_empty());
    assert_eq!(
        frame(&context, 1.1, vec![button(false)], true),
        (vec![], true)
    );
    assert_eq!(
        frame(&context, 1.2, vec![button(true), button(false)], true).0,
        vec![]
    );
}

#[test]
fn moving_or_cancelled_presses_require_a_fresh_press_and_ignore_late_release() {
    for kind in 0..5 {
        let context = fonts::test_context();
        frame(&context, 0.0, vec![], true);
        frame(&context, 0.1, vec![button(true)], true);
        let events = match kind {
            0 => vec![
                egui::Event::PointerMoved(egui::pos2(100.0, 50.0)),
                egui::Event::PointerMoved(egui::pos2(50.0, 50.0)),
            ],
            1 => vec![egui::Event::WindowFocused(false)],
            2 => vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            _ => vec![],
        };
        if kind == 4 {
            assert!(cancel_input(&context));
        }
        assert!(frame(&context, 0.2, events, kind != 3).0.is_empty());
        assert!(frame(&context, 0.8, vec![], true).0.is_empty());
        assert!(!frame(&context, 0.9, vec![button(false)], true).1);
    }
}

#[test]
fn latched_hold_uses_final_vertical_distance_and_never_commits_cancelled_or_audio_drags() {
    for (end_y, expected) in [
        (20.0, false),
        (50.0, false),
        (109.9, false),
        (110.0, true),
        (150.0, true),
    ] {
        let context = fonts::test_context();
        frame(&context, 0.0, vec![], true);
        frame(&context, 0.1, vec![button(true)], true);
        let (actions, _) = frame(&context, 0.51, vec![], true);
        let Action::Begin(token) = actions[0] else {
            panic!("begin")
        };
        assert_eq!(
            frame(
                &context,
                0.6,
                vec![egui::Event::PointerMoved(egui::pos2(50.0, 80.0))],
                true
            )
            .0,
            [Action::Progress(token, 50)]
        );
        assert_eq!(
            frame(
                &context,
                0.7,
                vec![egui::Event::PointerMoved(egui::pos2(50.0, 150.0))],
                true
            )
            .0,
            [Action::Progress(token, 100)]
        );
        let release = egui::Event::PointerButton {
            pos: egui::pos2(50.0, end_y),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(
            frame(
                &context,
                0.8,
                vec![release, egui::Event::PointerMoved(egui::pos2(50.0, 180.0))],
                true
            ),
            (
                vec![if expected {
                    Action::Commit(token)
                } else {
                    Action::End(token)
                }],
                false
            )
        );
        assert!(frame(&context, 0.9, vec![], true).0.is_empty());
    }
    for mode in 0..4 {
        let context = fonts::test_context();
        frame(&context, 0.0, vec![], true);
        frame(&context, 0.1, vec![button(true)], true);
        let (actions, _) = frame(&context, 0.51, vec![], true);
        let Action::Begin(token) = actions[0] else {
            panic!("begin")
        };
        let mut events = vec![egui::Event::PointerMoved(egui::pos2(50.0, 150.0))];
        if mode == 1 {
            events.push(egui::Event::WindowFocused(false));
        }
        if mode == 2 {
            events.push(egui::Event::PointerGone);
        }
        assert_eq!(
            frame_mode(&context, 0.6, events, mode != 3, mode != 0).0,
            [Action::End(token)]
        );
        assert!(frame(&context, 0.7, vec![button(false)], true).0.is_empty());
    }
}
