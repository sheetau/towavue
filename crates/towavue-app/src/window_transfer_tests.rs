use super::*;

#[test]
fn playback_origins_follow_live_owners_without_forwarding_chains() {
    let Some(_) = crate::tests::isolated_test_root(
        "window_host::transfer_tests::playback_origins_follow_live_owners_without_forwarding_chains",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let first = *host.windows.keys().next().expect("first");
    let second = host.add_application(None).expect("second");
    for key in [first, second] {
        let app = host.windows.get_mut(&key).expect("app");
        app.media_generation = 7;
        app.playback_origin = Some((key, 7));
    }
    assert_eq!(host.playback_owner((first, 7)), Some((first, 7)));
    assert_eq!(host.playback_owner((second, 7)), Some((second, 7)));
    let app = host.windows.get_mut(&first).expect("source");
    app.path = Some(PathBuf::from("routing-only.mp4"));
    app.media_kind = Some(MediaKind::Video);
    let mut saved = app.take_playback_tab_state();
    saved.instance = 11;
    let app = host.windows.get_mut(&second).expect("destination");
    let id = app.tabs.open_new(saved.path.clone(), MediaKind::Video);
    app.retained_playback.insert(id, saved);
    host.windows.get_mut(&first).expect("source").exit_requested = true;
    host.remove_closed();
    assert_eq!(host.playback_owner((first, 7)), Some((second, 11)));
    assert_eq!(host.playback_owner((second, 7)), Some((second, 7)));
    let third = host.add_application(None).expect("third");
    let mut saved = host
        .windows
        .get_mut(&second)
        .expect("second")
        .retained_playback
        .remove(&id)
        .expect("moved");
    saved.instance = 29;
    let app = host.windows.get_mut(&third).expect("third");
    app.media_generation = saved.instance;
    app.playback_origin = saved.origin;
    assert_eq!(host.playback_owner((first, 7)), Some((third, 29)));
    app_close(&mut host, third);
    assert_eq!(host.playback_owner((first, 7)), None);
    assert_eq!(host.playback_owner((WindowKey(u64::MAX), 7)), None);
}

fn app_close(host: &mut WindowHost, key: WindowKey) {
    host.windows.get_mut(&key).expect("window").exit_requested = true;
    host.remove_closed();
}

fn assert_frame(
    app: &mut WindowApplication,
    position: MediaTime,
    frame: MediaTime,
    generation: PlaybackGeneration,
) {
    assert_eq!(app.state, PlaybackState::Paused);
    assert_eq!(app.current_position(), position);
    assert_eq!(app.generation, generation, "live session was not reopened");
    assert_eq!(
        app.session.as_ref().expect("session").current_video_time(),
        Some(frame)
    );
    // The accessibility fixture renders at 640x480. Restore the owned HWND's
    // surface size before normal rendering uses that window's larger UI viewport.
    let size = app.window.as_ref().expect("window").inner_size();
    app.renderer
        .as_mut()
        .expect("renderer")
        .resize_surface(size.width, size.height)
        .expect("native surface size");
    app.render_frame();
    assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
    let metrics = app.session.as_ref().expect("session").metrics();
    assert!(metrics.hardware_frame_count > 0);
    assert_eq!(metrics.cpu_transfer_count, 0);
}

pub(super) fn exercise(host: &mut WindowHost, event_loop: &ActiveEventLoop) {
    let keys: Vec<_> = host.windows.keys().copied().collect();
    let source = keys[0];
    let target = keys[1];
    let target_active = host.windows[&target].tabs.active().expect("target tab").id;
    let app = host.windows.get_mut(&source).expect("source");
    let id = app.tabs.active().expect("source tab").id;
    // The outer event loop is paused during this helper, so its asynchronous
    // duration result may still be queued. Exercise bounded background suspension.
    app.media_duration = Some(Duration::from_secs(3));
    let old_edits = app.edits.remove(&id);
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Video);
    let edits = app.edits[&id].clone();
    let export = app
        .path
        .as_ref()
        .expect("path")
        .with_file_name("unsaved-target.mp4");
    app.export_paths.insert(id, export.clone());
    let audio_options = AudioExportOptions {
        normalize_peak: true,
        channels: towavue_runtime_windows::AudioChannels::Mono,
    };
    let mut metadata_options = MetadataExportOptions::default();
    metadata_options
        .set(
            towavue_runtime_windows::MetadataField::Title,
            Some("Moved title".into()),
        )
        .expect("metadata");
    app.audio_export_settings.insert(id, audio_options);
    app.metadata_export_settings
        .insert(id, metadata_options.clone());
    app.filmstrip_open = true;
    tab_focus::tests::hardware_focus(app, "Play / replay", true);
    app.time_selection = Some(
        towavue_core::TimeRange::new(MediaTime::ZERO, MediaTime::from_nanoseconds(900_000_000))
            .expect("range"),
    );
    let selection = app.time_selection;
    let position = app.current_position();
    let frame = app
        .session
        .as_ref()
        .expect("session")
        .current_video_time()
        .expect("frame");
    let generation = app.generation;
    let origin = app.playback_origin.expect("origin");
    let request = app.tab_detach_request(id).expect("request");
    let before_tabs = app.tabs.tabs().to_vec();
    assert!(
        host.detach_tab_with(source, &request, false, |_, _| Err(
            "injected startup failure".into()
        ))
        .is_err()
    );
    assert_eq!(host.windows.len(), 2);
    assert_eq!(host.windows[&source].tabs.tabs(), before_tabs);
    assert_eq!(host.windows[&source].edits[&id], edits);
    assert!(
        host.detach_tab_with(source, &request, false, |app, device| {
            app.start_on_device(event_loop, Some(device), false)
                .expect("staged hidden HWND");
            Err("injected post-start failure".into())
        })
        .is_err()
    );
    assert_eq!(host.windows.len(), 2);
    let app = host.windows.get_mut(&source).expect("source");
    app.media_generation += 1;
    assert!(
        app.validate_tab_transfer(&request).is_err(),
        "same-path reload invalidates a queued request"
    );
    app.media_generation -= 1;
    let export_request = ExportRequest {
        source: app.path.clone().expect("source"),
        target: app.path.clone().expect("source"),
        kind: MediaKind::Video,
        operations: Vec::new(),
        hardware_encode: false,
    };
    // Same-source rejection makes this busy-state fixture incapable of writing output.
    app.active_export = Some(ActiveExport {
        job: ExportJob::start(export_request.clone(), |_| {}).expect("rejected fixture job"),
        tab: id,
        progress: export_progress::ExportProgress::new(
            &export_request,
            &ExportOptions::default(),
            None,
        ),
        request: export_request,
        options: ExportOptions::default(),
        encoded: Duration::ZERO,
        analyzing_audio: false,
        cancelling: false,
        continuation: None,
    });
    assert!(
        app.validate_tab_transfer(&request).is_err(),
        "cannot move an exporting tab"
    );
    let other = app
        .tabs
        .tabs()
        .iter()
        .find(|tab| tab.id != id)
        .expect("neighbor")
        .id;
    app.active_export.as_mut().expect("export").tab = other;
    assert!(
        app.validate_tab_transfer(&request).is_ok(),
        "another tab's export need not block transfer"
    );
    app.active_export.take();
    assert_frame(
        host.windows.get_mut(&source).expect("source"),
        position,
        frame,
        generation,
    );
    host.windows.get_mut(&target).expect("target").export_error = Some("modal test".into());
    assert!(host.move_tab(source, target, &request, 0).is_err());
    host.windows.get_mut(&target).expect("target").export_error = None;
    assert!(host.move_tab(source, target, &request, usize::MAX).is_err());
    assert!(host.move_tab(source, source, &request, 0).is_err());
    let moved = host
        .move_tab(source, target, &request, 0)
        .expect("move dirty live video");
    assert!(host.windows[&source].pending_guard.is_none());
    assert!(!host.windows[&source].edits.contains_key(&id));
    let app = host.windows.get_mut(&target).expect("target");
    assert_eq!(app.tabs.tabs()[0].id, moved);
    assert_eq!(app.edits[&moved], edits);
    assert_eq!(app.export_paths[&moved], export);
    assert_eq!(app.audio_export_settings[&moved], audio_options);
    assert_eq!(app.metadata_export_settings[&moved], metadata_options);
    assert_eq!(app.time_selection, selection);
    assert!(app.filmstrip_open);
    tab_focus::tests::hardware_focus(app, "Play / replay", false);
    assert_frame(app, position, frame, generation);
    let instance = app.media_generation;
    assert_eq!(host.playback_owner(origin), Some((target, instance)));
    assert!(
        host.move_tab(source, target, &request, 0).is_err(),
        "stale tab request"
    );
    let back_request = host.windows[&target]
        .tab_detach_request(moved)
        .expect("return request");
    let returned = host
        .move_tab(target, source, &back_request, 1)
        .expect("return live tab");
    host.windows
        .get_mut(&target)
        .expect("target")
        .activate_tab(target_active);
    assert_frame(
        host.windows.get_mut(&source).expect("source"),
        position,
        frame,
        generation,
    );

    // Exercise the same pending action used by an outside-strip drag. No window
    // becomes visible and no foreground input or external application is used.
    host.windows
        .get_mut(&source)
        .expect("source")
        .close_filmstrip();
    host.windows
        .get_mut(&source)
        .expect("source")
        .handle_ui_action(UiAction::DropTab(
            returned,
            egui::pos2(-20.0, 90.0),
            egui::vec2(100.0, 15.0),
        ));
    host.update_tab_drops_with(event_loop, false, |_, _, _| None);
    assert_eq!(host.windows.len(), 3);
    let detached = *host
        .windows
        .keys()
        .find(|key| !keys.contains(key))
        .expect("new window");
    let app = host.windows.get_mut(&detached).expect("detached");
    assert_eq!(
        app.window.as_ref().expect("window").is_visible(),
        Some(false)
    );
    assert_frame(app, position, frame, generation);
    let detached_id = app.tabs.active().expect("tab").id;
    assert_eq!(app.edits[&detached_id], edits);
    let request = app.tab_detach_request(detached_id).expect("request");
    let returned = host
        .move_tab(detached, source, &request, 1)
        .expect("return detached tab");
    assert!(host.windows[&detached].tabs.tabs().is_empty());
    assert!(host.windows[&detached].path.is_none());
    assert!(host.windows[&detached].tabs.welcome().is_some());
    app_close(host, detached);
    let app = host.windows.get_mut(&source).expect("source");
    assert_frame(app, position, frame, generation);
    if let Some(history) = old_edits {
        app.edits.insert(returned, history);
    } else {
        app.edits.remove(&returned);
    }
    app.export_paths.remove(&returned);
    app.audio_export_settings.remove(&returned);
    app.metadata_export_settings.remove(&returned);
    app.time_selection = None;
    app.filmstrip_open = false;
    let other = app
        .tabs
        .tabs()
        .iter()
        .find(|tab| tab.id != returned)
        .expect("neighbor")
        .id;
    app.activate_tab(other);
    let request = app
        .tab_detach_request(returned)
        .expect("background request");
    assert!(app.retained_playback[&returned].video_suspended);
    let moved = host
        .move_tab(source, target, &request, 0)
        .expect("move background video");
    assert_eq!(
        host.windows[&source]
            .tabs
            .active()
            .expect("unchanged active tab")
            .id,
        other
    );
    assert_frame(
        host.windows.get_mut(&target).expect("target"),
        position,
        frame,
        generation,
    );
    let request = host.windows[&target]
        .tab_detach_request(moved)
        .expect("return request");
    host.move_tab(target, source, &request, 1)
        .expect("return background tab");
    host.windows
        .get_mut(&target)
        .expect("target")
        .activate_tab(target_active);
    exercise_audio(host, event_loop, source);
    exercise_images(host, event_loop, source, target);
    eprintln!(
        "PASS live video transfer: dirty state, exact paused frame/clock, unchanged session generation, shared-device draw with no CPU transfer, repeat moves, hidden detached HWND, Welcome source, failed startup/modal/stale/gap guards"
    );
}

fn draw_image(app: &mut WindowApplication, original: &Arc<DecodedImage>, frame: usize) {
    let size = app.window.as_ref().expect("window").inner_size();
    app.renderer
        .as_mut()
        .expect("renderer")
        .resize_surface(size.width, size.height)
        .expect("native surface");
    app.render_frame();
    assert!(app.image_error.is_none(), "{:?}", app.image_error);
    assert!(!app.image_loading);
    let image = app.image.as_ref().expect("image");
    assert!(Arc::ptr_eq(&image.decoded, original));
    assert_eq!(image.frame_index, frame);
}

fn exercise_images(
    host: &mut WindowHost,
    event_loop: &ActiveEventLoop,
    source: WindowKey,
    target: WindowKey,
) {
    let target_active = host.windows[&target]
        .tabs
        .active()
        .expect("target video")
        .id;
    let app = host.windows.get_mut(&source).expect("source");
    let source_active = app.tabs.active().expect("source video").id;
    let path = app
        .path
        .as_ref()
        .expect("owned fixture")
        .with_file_name("memory-only-animation.png");
    let original = tab_transfer::tests::decoded(true);
    let id = tab_transfer::tests::install(app, path, Arc::clone(&original));
    app.image_view.selection = Some(UnitRect {
        min: UnitPoint { x: 0.0, y: 0.0 },
        max: UnitPoint { x: 0.5, y: 1.0 },
    });
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let history = app.edits[&id].clone();
    let image = app.image.as_mut().expect("animation");
    image.frame_index = 1;
    image.next_frame_at = Some(Instant::now() + Duration::from_secs(60));
    image
        .texture
        .set(color_image(&original.frames[1]), TextureOptions::LINEAR);
    let deadline = image.next_frame_at;
    tab_focus::tests::hardware_focus(app, "Crop preview", true);
    let request = app.tab_detach_request(id).expect("image request");
    let context = host.windows[&target]
        .ui_context
        .clone()
        .expect("target context");
    let limit = context.input(|input| input.max_texture_side);
    context.input_mut(|input| input.max_texture_side = 1);
    assert!(host.move_tab(source, target, &request, 0).is_err());
    context.input_mut(|input| input.max_texture_side = limit);
    assert_eq!(host.windows[&source].edits[&id], history);
    draw_image(host.windows.get_mut(&source).expect("source"), &original, 1);
    let keys: Vec<_> = host.windows.keys().copied().collect();
    host.windows
        .get_mut(&source)
        .expect("source")
        .handle_ui_action(UiAction::DropTab(
            id,
            egui::pos2(-20.0, 90.0),
            egui::vec2(100.0, 15.0),
        ));
    host.update_tab_drops_with(event_loop, false, |_, _, _| None);
    let detached = *host
        .windows
        .keys()
        .find(|key| !keys.contains(key))
        .expect("detached image HWND");
    let app = host.windows.get_mut(&detached).expect("detached");
    let moved = app.tabs.active().expect("image tab").id;
    assert_eq!(
        app.window.as_ref().expect("window").is_visible(),
        Some(false)
    );
    assert_eq!(app.edits[&moved], history);
    assert!(app.pending_guard.is_none());
    assert_eq!(app.image.as_ref().expect("image").next_frame_at, deadline);
    tab_focus::tests::hardware_focus(app, "Crop preview", false);
    draw_image(app, &original, 1);
    app.recover_graphics_device(MediaTime::ZERO);
    host.recover_pending_graphics();
    draw_image(
        host.windows.get_mut(&detached).expect("recovered image"),
        &original,
        1,
    );
    let request = host.windows[&detached]
        .tab_detach_request(moved)
        .expect("image return");
    let moved = host
        .move_tab(detached, target, &request, 0)
        .expect("move image into video window");
    app_close(host, detached);
    let app = host.windows.get_mut(&target).expect("target");
    draw_image(app, &original, 1);
    app.activate_tab(target_active);
    let request = app.tab_detach_request(moved).expect("background image");
    let returned = host
        .move_tab(target, source, &request, 0)
        .expect("move retained image");
    let app = host.windows.get_mut(&source).expect("source");
    draw_image(app, &original, 1);
    assert_eq!(app.edits[&returned], history);
    app.request_guarded(GuardedAction::CloseTab(returned));
    assert!(
        app.pending_guard.is_some(),
        "normal close still protects transferred edits"
    );
    app.resolve_guard(GuardDecision::Cancel);
    assert!(app.edits[&returned].is_dirty());
    app.request_guarded(GuardedAction::CloseTab(returned));
    app.resolve_guard(GuardDecision::Discard);
    app.activate_tab(source_active);
    assert_eq!(host.windows.len(), keys.len());
    eprintln!(
        "PASS live image transfer: hidden native detach and repeat moves, active/retained animation pixels retained in memory, current frame/deadline/dirty history/UIA focus, texture-stage rollback, shared-device recovery and ordinary close guard"
    );
}

fn exercise_audio(host: &mut WindowHost, event_loop: &ActiveEventLoop, destination: WindowKey) {
    let path = host.windows[&destination]
        .path
        .as_ref()
        .expect("fixture")
        .with_file_name("transfer-silence.wav");
    let output = std::process::Command::new("ffmpeg.exe")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo",
            "-t",
            "3",
        ])
        .arg(&path)
        .output()
        .expect("generated silence");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let source = host.add_application(None).expect("audio source");
    host.start_pending(event_loop, false);
    let app = host.windows.get_mut(&source).expect("audio source");
    let id = app.tabs.open_new(path.clone(), MediaKind::Audio);
    app.load_path(path, MediaKind::Audio);
    app.media_duration = Some(Duration::from_secs(3));
    if app.session.is_none() {
        eprintln!(
            "SKIP live audio transfer: audio session unavailable: {:?}",
            app.playback_error
        );
        app_close(host, source);
        return;
    }
    app.dispatch(CommandId::CycleAudioRepeat);
    app.dispatch(CommandId::ToggleAudioShuffle);
    let modes = app.audio_mode();
    let generation = app.generation;
    let origin = app.playback_origin.expect("audio origin");
    let request = app.tab_detach_request(id).expect("audio request");
    let gap = host.windows[&destination].tabs.tabs().len();
    let moved = host
        .move_tab(source, destination, &request, gap)
        .expect("move playing audio");
    app_close(host, source);
    let app = host
        .windows
        .get_mut(&destination)
        .expect("audio destination");
    assert_eq!(app.state, PlaybackState::Playing);
    assert_eq!(app.generation, generation);
    assert_eq!(app.audio_mode(), modes);
    let instance = app.media_generation;
    let finished = app.session.as_ref().expect("audio").decode_finished();
    app.decode_finished = !finished;
    assert_eq!(host.playback_owner(origin), Some((destination, instance)));
    host.route(Event::Window(
        origin.0,
        AppEvent::Playback(origin.1, PlaybackEvent::DecodeFinished(generation)),
    ));
    let app = host
        .windows
        .get_mut(&destination)
        .expect("audio destination");
    assert_eq!(
        app.decode_finished,
        app.session.as_ref().expect("audio").decode_finished(),
        "queued notification is delivered after the origin window closes"
    );
    app.toggle_pause();
    assert_eq!(app.state, PlaybackState::Paused);
    // Pause is queued to WASAPI, not acknowledged by toggle_pause. Match the
    // runtime audio probe's settling interval, then prove stability before moving.
    std::thread::sleep(Duration::from_millis(100));
    let position = app.current_position();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        app.current_position(),
        position,
        "audio must stop before transfer"
    );
    let request = app.tab_detach_request(moved).expect("paused audio request");
    let detached = host
        .detach_tab(
            event_loop,
            destination,
            &request,
            false,
            winit::dpi::PhysicalPosition::new(100, 80),
            egui::Vec2::ZERO,
        )
        .expect("detach paused audio");
    let app = host.windows.get_mut(&detached).expect("detached audio");
    assert_eq!(app.state, PlaybackState::Paused);
    assert_eq!(app.current_position(), position);
    assert_eq!(app.generation, generation);
    assert_eq!(app.audio_mode(), modes);
    assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
    app_close(host, detached);
    assert_eq!(host.playback_owner(origin), None);
    eprintln!(
        "PASS live audio transfer: generated silence only, playing/paused session and repeat/shuffle preserved, queued playback delivery after origin HWND removal, repeated detach and final owner removal"
    );
}
