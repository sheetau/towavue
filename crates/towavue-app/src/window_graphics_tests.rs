use super::*;

#[test]
fn hosted_graphics_retry_is_deferred_and_late_loss_does_not_retry_failed_graphics() {
    let Some(_) = crate::tests::isolated_test_root(
        "window_host::graphics_tests::hosted_graphics_retry_is_deferred_and_late_loss_does_not_retry_failed_graphics",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let app = host.windows.values_mut().next().expect("window");
    let epoch = app.graphics_epoch;
    app.state = PlaybackState::Paused;
    let position = MediaTime::from_nanoseconds(400_000_000);
    app.recover_graphics_device(position);
    assert!(
        app.graphics_recovery_request.is_none(),
        "no renderer: old loss is ignored"
    );
    app.request_graphics_recovery(position, true);
    app.request_graphics_recovery(MediaTime::ZERO, true);
    let request = app.graphics_recovery_request.expect("deferred request");
    assert_eq!(request.position, position);
    assert_eq!(request.state, PlaybackState::Paused);
    assert!(request.retry);
    assert_eq!(app.graphics_epoch, epoch, "no per-window device rebuild");
    app.exit_requested = true;
    host.recover_pending_graphics_with(|_, _| panic!("closed window must not create graphics"));
    assert!(
        host.windows
            .values()
            .all(|app| app.graphics_recovery_request.is_none())
    );
}

struct Snapshot {
    position: MediaTime,
    frame_time: MediaTime,
    generation: PlaybackGeneration,
    epoch: u64,
    edits: BTreeMap<TabId, EditHistory>,
    retained: Vec<(TabId, MediaTime, PlaybackGeneration)>,
}

fn snapshot(app: &WindowApplication) -> Snapshot {
    Snapshot {
        position: app.current_position(),
        frame_time: app
            .session
            .as_ref()
            .expect("session")
            .current_video_time()
            .expect("current frame"),
        generation: app.generation,
        epoch: app.graphics_epoch,
        edits: app.edits.clone(),
        retained: app
            .retained_playback
            .iter()
            .map(|(id, saved)| {
                (
                    *id,
                    saved.position(),
                    saved
                        .session
                        .as_ref()
                        .expect("retained session")
                        .generation(),
                )
            })
            .collect(),
    }
}

fn current_video_loss(session: &PlaybackSession, reason: &str) -> PlaybackEvent {
    // Video suspension has its own generation, distinct from the audio/session
    // generation. Use the public acceptance contract to select a valid injected event.
    let mut generation = PlaybackGeneration::INITIAL;
    for _ in 0..128 {
        let event = PlaybackEvent::DeviceRemoved(generation, reason.into());
        if session.accepts_event(&event) {
            return event;
        }
        generation = generation.next();
    }
    panic!("fixture exceeded its bounded video generation count");
}

fn draw_ready(app: &mut WindowApplication) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.load_next_frame();
        app.render_frame();
        assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
        let metrics = app.session.as_ref().expect("session").metrics();
        if metrics.presented_frame_count > 0 {
            assert!(metrics.hardware_frame_count > 0);
            assert_eq!(metrics.cpu_transfer_count, 0);
            break;
        }
        assert!(Instant::now() < deadline, "shared recovery frame deadline");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn assert_restored(app: &mut WindowApplication, saved: &Snapshot, attempts: u64) {
    draw_ready(app);
    assert_eq!(app.state, PlaybackState::Paused);
    assert_eq!(app.current_position(), saved.position);
    assert_eq!(
        app.session.as_ref().expect("session").current_video_time(),
        Some(saved.frame_time)
    );
    assert_eq!(app.generation, saved.generation.next());
    assert_eq!(app.graphics_epoch, saved.epoch + attempts);
    assert_eq!(app.edits, saved.edits);
    assert!(app.queued_recovery.is_none() && app.graphics_recovery_request.is_none());
    for (id, position, generation) in &saved.retained {
        let retained = &app.retained_playback[id];
        assert_eq!(retained.position(), *position);
        assert_eq!(retained.state, PlaybackState::Paused);
        assert_eq!(
            retained
                .session
                .as_ref()
                .expect("retained session")
                .generation(),
            generation.next()
        );
        assert!(retained.recovery_position.is_none());
        assert_eq!(retained.graphics_epoch, app.graphics_epoch);
    }
}

fn cross_draw(host: &mut WindowHost, first: WindowKey, second: WindowKey) {
    let mut source = host.windows.remove(&first).expect("source window");
    let renderer = host
        .windows
        .get_mut(&second)
        .expect("destination")
        .renderer
        .as_mut()
        .expect("renderer");
    assert!(
        source
            .session
            .as_mut()
            .expect("hardware session")
            .draw_current(
                renderer,
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(160.0, 96.0)),
                source.video_uv,
            )
            .expect("hardware frame draws on the other window's device")
    );
    host.windows.insert(first, source);
}

pub(super) fn exercise(host: &mut WindowHost) {
    let keys: Vec<_> = host.windows.keys().copied().collect();
    let first = keys[0];
    let second = keys[1];
    for (index, key) in keys.iter().enumerate() {
        let app = host.windows.get_mut(key).expect("window");
        app.media_duration = Some(Duration::from_secs(3));
        let path = app.path.clone().expect("generated source");
        app.tabs.open_new(path.clone(), MediaKind::Video);
        app.load_path(path, MediaKind::Video);
        draw_ready(app);
        if app.state == PlaybackState::Playing {
            app.toggle_pause();
        }
        app.seek_to(MediaTime::from_nanoseconds(
            (index as i64 + 1) * 300_000_000,
        ));
        draw_ready(app);
        assert_eq!(app.retained_playback.len(), 1);
    }
    let original_edits: BTreeMap<_, _> = host
        .windows
        .iter()
        .map(|(key, app)| (*key, app.edits.clone()))
        .collect();
    for app in host.windows.values_mut() {
        let tab = app.tabs.active().expect("active tab").id;
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::FlipHorizontal, MediaKind::Video);
    }
    let before: BTreeMap<_, _> = host
        .windows
        .iter()
        .map(|(key, app)| (*key, snapshot(app)))
        .collect();
    let instance = host.windows[&first].media_generation;
    host.route(Event::Window(
        first,
        AppEvent::Playback(
            instance,
            PlaybackEvent::DeviceRemoved(before[&first].generation, "injected shared loss".into()),
        ),
    ));
    for key in &keys {
        assert_restored(host.windows.get_mut(key).expect("window"), &before[key], 1);
    }
    cross_draw(host, first, second);
    for key in &keys {
        let instance = host.windows[key].media_generation;
        host.route(Event::Window(
            *key,
            AppEvent::Playback(
                instance,
                PlaybackEvent::DeviceRemoved(before[key].generation, "stale shared loss".into()),
            ),
        ));
        assert_eq!(host.windows[key].graphics_epoch, before[key].epoch + 1);
    }
    // A retained tab's loss must also reach the host rather than rebuilding only its window.
    let before: BTreeMap<_, _> = host
        .windows
        .iter()
        .map(|(key, app)| (*key, snapshot(app)))
        .collect();
    let retained = host.windows[&second]
        .retained_playback
        .values()
        .next()
        .expect("background");
    let instance = retained.instance;
    let event = current_video_loss(
        retained.session.as_ref().expect("background session"),
        "injected background loss",
    );
    host.route(Event::Window(second, AppEvent::Playback(instance, event)));
    for key in &keys {
        assert_restored(host.windows.get_mut(key).expect("window"), &before[key], 1);
    }
    cross_draw(host, first, second);
    // Presentation failures share the same transaction, including a playing window.
    let paused = snapshot(&host.windows[&second]);
    let app = host.windows.get_mut(&first).expect("first");
    app.toggle_pause();
    assert_eq!(app.state, PlaybackState::Playing);
    let generation = app.generation;
    app.handle_render_error(RenderError::DeviceRemoved(
        "injected presentation loss".into(),
    ));
    let point = app
        .graphics_recovery_request
        .expect("host owns presentation recovery");
    host.recover_pending_graphics();
    let app = host.windows.get_mut(&first).expect("first");
    assert_eq!(app.state, PlaybackState::Playing);
    assert_eq!(app.generation, generation.next());
    assert_eq!(
        app.session.as_ref().expect("session").target(),
        point.position
    );
    draw_ready(app);
    app.toggle_pause();
    assert_restored(host.windows.get_mut(&second).expect("second"), &paused, 1);
    cross_draw(host, first, second);
    for fail_at in [1, 2] {
        let before: BTreeMap<_, _> = host
            .windows
            .iter()
            .map(|(key, app)| (*key, snapshot(app)))
            .collect();
        host.windows
            .get_mut(&first)
            .expect("first")
            .recover_graphics_device(before[&first].position);
        let mut calls = 0;
        host.recover_pending_graphics_with(|app, device| {
            calls += 1;
            assert!(app.renderer.is_none());
            assert!(
                app.session
                    .as_ref()
                    .expect("session")
                    .video_geometry()
                    .is_none()
            );
            assert!(
                app.retained_playback
                    .values()
                    .all(|saved| saved.recovery_position.is_some())
            );
            assert_eq!(device.is_none(), calls == 1);
            if calls == fail_at {
                Err("injected surface creation failure".into())
            } else {
                app.create_graphics_surface(device)
            }
        });
        assert_eq!(calls, fail_at);
        for key in &keys {
            let app = &host.windows[key];
            assert!(app.renderer.is_none());
            assert_eq!(app.state, PlaybackState::Faulted);
            assert_eq!(app.current_position(), before[key].position);
            assert_eq!(app.edits, before[key].edits);
            assert!(matches!(
                app.queued_recovery,
                Some(FallbackPrompt::Recovery { .. })
            ));
            assert_eq!(app.graphics_epoch, before[key].epoch + 1);
        }
        let stale_instance = host.windows[&first].media_generation;
        host.route(Event::Window(
            first,
            AppEvent::Playback(
                stale_instance,
                PlaybackEvent::DeviceRemoved(
                    before[&first].generation,
                    "late after failed transaction".into(),
                ),
            ),
        ));
        assert_eq!(
            host.windows[&first].graphics_epoch,
            before[&first].epoch + 1
        );
        let app = host.windows.get_mut(&second).expect("second");
        app.native_prompt = app.queued_recovery.take();
        app.finish_native_prompt(Ok(PromptResponse::Cancel));
        let app = host.windows.get_mut(&first).expect("first");
        app.native_prompt = app.queued_recovery.take();
        app.finish_native_prompt(Ok(PromptResponse::Retry));
        host.recover_pending_graphics();
        assert_restored(
            host.windows.get_mut(&first).expect("first"),
            &before[&first],
            2,
        );
        assert!(
            host.windows[&second].renderer.is_none(),
            "Cancel is not undone by another window's Retry"
        );
        assert_eq!(host.windows[&second].state, PlaybackState::Faulted);
        let healthy = snapshot(&host.windows[&first]);
        // An explicit later retry for teardown; Cancel itself did not request recovery.
        let app = host.windows.get_mut(&second).expect("second");
        app.state = PlaybackState::Paused;
        app.request_graphics_recovery(before[&second].position, true);
        host.recover_pending_graphics();
        assert_restored(
            host.windows.get_mut(&second).expect("second"),
            &before[&second],
            2,
        );
        assert_eq!(host.windows[&first].graphics_epoch, healthy.epoch);
        assert_eq!(host.windows[&first].generation, healthy.generation);
        assert_eq!(host.windows[&first].current_position(), healthy.position);
        cross_draw(host, first, second);
    }
    for key in keys {
        let app = host.windows.get_mut(&key).expect("window");
        let active = app.tabs.active().expect("active").id;
        let expected_edits = app.edits.clone();
        let background = *app.retained_playback.keys().next().expect("background");
        app.activate_tab(background);
        draw_ready(app);
        app.activate_tab(active);
        draw_ready(app);
        assert_eq!(app.edits, expected_edits);
        app.edits = original_edits[&key].clone();
    }
    eprintln!(
        "PASS shared graphics recovery: active/background loss restores both windows and retained tabs; first/second surface failures commit no partial renderer; stale loss ignored, Cancel respected, isolated Retry reuses healthy device; edits/positions retained and hardware cross-draw CPU transfers 0"
    );
}
