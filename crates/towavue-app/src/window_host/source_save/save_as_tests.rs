use super::*;
use towavue_runtime_windows::SaveAsTarget;

fn chosen(host: &mut WindowHost, owner: WindowKey, id: TabId, source: &Path, target: &Path) {
    let selected = SaveAsTarget::capture(target).expect("simulated native acceptance");
    assert!(host.windows.get_mut(&owner).expect("owner").start_save_as(
        id,
        source.to_owned(),
        MediaKind::Image,
        selected,
        None
    ));
}
fn ready(host: &mut WindowHost, owner: WindowKey) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        drain(host);
        let app = &host.windows[&owner];
        if app
            .source_save
            .save_as
            .as_ref()
            .is_some_and(|pending| pending.prepared.is_some())
        {
            return;
        }
        assert!(app.export_error.is_none(), "{:?}", app.export_error);
        assert!(Instant::now() < deadline, "Save as preparation");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn flip(host: &mut WindowHost, owner: WindowKey, id: TabId) {
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
}

#[test]
fn save_as_adopts_only_the_exporter_and_preserves_both_originals_through_undo_and_save() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::save_as::save_as_adopts_only_the_exporter_and_preserves_both_originals_through_undo_and_save",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    let target = root.join("destination.bmp");
    crate::tab_transfer::tests::bitmap(&source);
    crate::tab_transfer::tests::bitmap(&target);
    let original = pixels(&source);
    let mut bytes = std::fs::read(&target).expect("destination");
    bytes[54..60].copy_from_slice(&[0, 255, 0, 255, 255, 255]);
    std::fs::write(&target, bytes).expect("different destination pixels");
    let previous_destination = pixels(&target);
    let (mut host, owner, id) = setup(&source);
    let source_window = host.add_application(None).expect("source peer window");
    let source_peer = attach(&mut host, source_window, &source);
    let destination_window = host.add_application(None).expect("destination peer window");
    let destination_peer = attach(&mut host, destination_window, &target);
    flip(&mut host, owner, id);
    chosen(&mut host, owner, id, &source, &target);
    ready(&mut host, owner);
    assert_eq!(
        pixels(&target),
        previous_destination,
        "preparation remains unpublished"
    );
    finish(&mut host, owner);
    let app = &host.windows[&owner];
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    assert_eq!(app.path.as_ref(), Some(&target));
    assert_eq!(app.tabs.active().expect("tab").id, id);
    assert_eq!(
        app.tabs.active().expect("tab").target.current_path(),
        target
    );
    assert!(!app.edits[&id].is_dirty());
    assert_ne!(pixels(&target), original);
    assert_eq!(
        pixels(app.media_input_for(Some(id), &target).path()),
        original
    );
    assert_eq!(host.windows[&source_window].path.as_ref(), Some(&source));
    assert!(
        !host.windows[&source_window]
            .source_backings
            .contains_key(&source_peer)
    );
    let peer = &host.windows[&destination_window];
    assert_eq!(
        pixels(peer.media_input_for(Some(destination_peer), &target).path()),
        previous_destination
    );
    assert!(peer.edits[&destination_peer].is_dirty());
    let app = host.windows.get_mut(&owner).expect("owner");
    app.dispatch(CommandId::Undo);
    assert!(app.edits[&id].is_dirty());
    assert!(app.edits[&id].operations().is_empty());
    assert_eq!(
        app.tabs.active().expect("same tab").target.current_path(),
        target
    );
    app.dispatch(CommandId::Save);
    finish(&mut host, owner);
    assert_eq!(
        pixels(&target),
        original,
        "Undo Save reads the first original"
    );
    assert_eq!(
        pixels(&source),
        original,
        "Save as never mutates the original path"
    );
    assert!(!host.windows[&owner].edits[&id].is_dirty());
    assert_eq!(
        pixels(
            host.windows[&destination_window]
                .media_input_for(Some(destination_peer), &target)
                .path()
        ),
        previous_destination
    );
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .dispatch(CommandId::Redo);
    let next = root.join("again.png");
    chosen(&mut host, owner, id, &target, &next);
    finish(&mut host, owner);
    let app = &host.windows[&owner];
    assert_eq!(app.path.as_ref(), Some(&next));
    assert!(!app.edits[&id].is_dirty());
    assert_ne!(pixels(&next), original);
    assert_eq!(
        pixels(app.media_input_for(Some(id), &next).path()),
        original
    );
}

#[test]
fn save_as_background_owner_and_later_edits_keep_foreground_and_dirty_state() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::save_as::save_as_background_owner_and_later_edits_keep_foreground_and_dirty_state",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    let other = root.join("foreground.bmp");
    let target = root.join("saved.png");
    crate::tab_transfer::tests::bitmap(&source);
    crate::tab_transfer::tests::bitmap(&other);
    let (mut host, owner, id) = setup(&source);
    flip(&mut host, owner, id);
    chosen(&mut host, owner, id, &source, &target);
    ready(&mut host, owner);
    flip(&mut host, owner, id);
    let foreground = attach(&mut host, owner, &other);
    finish(&mut host, owner);
    let app = &host.windows[&owner];
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    assert_eq!(app.tabs.active().expect("foreground").id, foreground);
    assert_eq!(app.path.as_ref(), Some(&other));
    assert_eq!(
        app.tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == id)
            .expect("background exporter")
            .target
            .current_path(),
        target
    );
    assert!(
        app.edits[&id].is_dirty(),
        "later edits differ from the saved snapshot"
    );
    assert_eq!(app.edits[&id].operations().len(), 2);
    assert!(!app.source_backings.contains_key(&foreground));
}

#[test]
fn save_as_rejects_late_collisions_stale_peers_and_cancelled_or_changed_owners() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::save_as::save_as_rejects_late_collisions_stale_peers_and_cancelled_or_changed_owners",
    ) else {
        return;
    };
    for mode in 0..4 {
        let source = root.join(format!("source-{mode}.bmp"));
        let target = root.join(format!("target-{mode}.bmp"));
        crate::tab_transfer::tests::bitmap(&source);
        let (mut host, owner, id) = setup(&source);
        flip(&mut host, owner, id);
        let original = pixels(&source);
        if mode == 1 {
            crate::tab_transfer::tests::bitmap(&target);
            let other = host.add_application(None).expect("peer");
            attach(&mut host, other, &target);
            let mut bytes = std::fs::read(&target).expect("target");
            bytes[54..60].copy_from_slice(&[0, 255, 0, 255, 255, 255]);
            std::fs::write(&target, bytes).expect("newer version than peer");
        }
        chosen(&mut host, owner, id, &source, &target);
        ready(&mut host, owner);
        match mode {
            0 => std::fs::write(&target, b"late competing file").expect("late arrival"),
            1 => {}
            2 => host
                .windows
                .get_mut(&owner)
                .expect("owner")
                .handle_ui_action(UiAction::CancelExport),
            _ => {
                host.windows
                    .get_mut(&owner)
                    .expect("owner")
                    .tabs
                    .get_mut(id)
                    .expect("tab")
                    .target
                    .set_current_path(root.join("changed.bmp"), MediaKind::Image);
            }
        }
        let before = std::fs::read(&target).ok();
        finish(&mut host, owner);
        let app = &host.windows[&owner];
        assert!(app.edits[&id].is_dirty());
        assert!(!app.source_backings.contains_key(&id));
        assert_eq!(pixels(&source), original);
        assert_eq!(std::fs::read(&target).ok(), before);
        assert_ne!(
            app.tabs.active().expect("tab").target.current_path(),
            target
        );
    }
}

#[test]
fn save_as_same_path_uses_source_save_and_native_result_keeps_the_selection_snapshot() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::save_as::save_as_same_path_uses_source_save_and_native_result_keeps_the_selection_snapshot",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    let target = root.join("saved.png");
    crate::tab_transfer::tests::bitmap(&source);
    let (mut host, owner, id) = setup(&source);
    flip(&mut host, owner, id);
    chosen(&mut host, owner, id, &source, &source);
    assert!(host.windows[&owner].source_save.pending.is_some());
    finish(&mut host, owner);
    assert!(!host.windows[&owner].edits[&id].is_dirty());
    let app = host.windows.get_mut(&owner).expect("owner");
    app.pending_dialog = Some(DialogIntent::Export {
        tab: id,
        source: source.clone(),
        kind: MediaKind::Image,
        generation: app.media_generation,
        output: ExportOutput::Media,
        continuation: None,
    });
    let chosen = SaveAsTarget::capture(&target).expect("native acceptance snapshot");
    std::fs::write(&target, b"arrived after dialog acceptance").expect("late file");
    app.finish_save_as_dialog(Ok(Some(chosen)));
    finish(&mut host, owner);
    let app = &host.windows[&owner];
    assert!(app.export_error.is_some());
    assert_eq!(app.path.as_ref(), Some(&source));
    assert_eq!(
        std::fs::read(&target).expect("competing file"),
        b"arrived after dialog acceptance"
    );
}

#[test]
fn save_as_native_video_adopts_destination_and_resumes_original_input() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::save_as::save_as_native_video_adopts_destination_and_resumes_original_input",
    ) else {
        return;
    };
    let source = root.join("source.mp4");
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
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "mpeg4"
        ])
        .arg(&source)
        .status()
        .expect("fixture video")
        .success()
    );
    struct Trial {
        source: PathBuf,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let renderer = FrameRenderer::new(&window).expect("D3D11");
            let (mut host, owner, id) = setup(&self.source);
            let other = host.add_application(None).expect("other owner");
            let other_id = attach(&mut host, other, &self.source);
            for (key, tab) in [(owner, id), (other, other_id)] {
                let app = host.windows.get_mut(&key).expect("app");
                app.tabs
                    .get_mut(tab)
                    .expect("tab")
                    .target
                    .set_current_path(self.source.clone(), MediaKind::Video);
                app.media_kind = Some(MediaKind::Video);
                let mut session = PlaybackSession::open_input(
                    app.media_input(&self.source),
                    renderer.graphics_device(),
                    0.0,
                    1.0,
                    Default::default(),
                    true,
                    |_| {},
                )
                .expect("native session");
                let position = if key == owner {
                    MediaTime::ZERO
                } else {
                    MediaTime::from_nanoseconds(500_000_000)
                };
                if position != MediaTime::ZERO {
                    session
                        .set_rate_at(position, 1.0, false)
                        .expect("paused seek");
                }
                let deadline = Instant::now() + Duration::from_secs(5);
                while session.pending_video_time().is_none() {
                    assert!(Instant::now() < deadline, "initial frame");
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert!(session.advance_pending());
                app.generation = session.generation();
                app.clock = Some(PlaybackClock::paused(position, 1.0));
                app.session = Some(session);
            }
            let target = self.source.with_file_name("saved.mp4");
            let original_bytes = std::fs::read(&self.source).expect("original");
            let app = host.windows.get_mut(&owner).expect("owner");
            app.edits.entry(id).or_default().push(
                EditOperation::Crop(PixelCrop {
                    x: 0,
                    y: 0,
                    width: 32,
                    height: 48,
                }),
                MediaKind::Video,
            );
            assert!(app.start_test_save_as(target.clone(), None));
            finish(&mut host, owner);
            assert!(
                host.windows[&owner].export_error.is_none(),
                "{:?}",
                host.windows[&owner].export_error
            );
            for (key, tab) in [(owner, id), (other, other_id)] {
                let app = host.windows.get_mut(&key).expect("app");
                let logical = if key == owner { &target } else { &self.source };
                assert_eq!(app.path.as_ref(), Some(logical));
                let backup = app.media_input_for(Some(tab), logical).path().to_owned();
                let session = app.session.as_mut().expect("resumed session");
                let deadline = Instant::now() + Duration::from_secs(5);
                while session.pending_video_time().is_none() {
                    assert!(Instant::now() < deadline, "resumed frame");
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert!(session.advance_pending());
                assert_eq!(session.video_geometry(), Some((64, 48, 1.0)));
                assert_eq!(
                    session
                        .current_video_snapshot()
                        .expect("frame")
                        .source_path(),
                    backup
                );
                assert_eq!(app.state, PlaybackState::Paused);
                assert_eq!(
                    app.current_position(),
                    if key == owner {
                        MediaTime::ZERO
                    } else {
                        MediaTime::from_nanoseconds(500_000_000)
                    }
                );
            }
            let mut frames = 0;
            towavue_runtime_windows::decode_file(&target, |item| {
                if let towavue_runtime_windows::DecodeOutput::Video(frame) = item {
                    assert_eq!((frame.width, frame.height), (32, 48));
                    frames += 1;
                }
                true
            })
            .expect("saved video");
            assert_eq!(frames, 8);
            assert_eq!(
                std::fs::read(&self.source).expect("original unchanged"),
                original_bytes
            );
            drop(host);
            drop(renderer);
            drop(window);
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop")
        .run_app(&mut Trial { source })
        .expect("native trial");
}

#[test]
fn save_as_background_audio_refreshes_its_destination_queue_and_preserves_repeat_and_undo() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::save_as::save_as_background_audio_refreshes_its_destination_queue_and_preserves_repeat_and_undo",
    ) else {
        return;
    };
    let source = root.join("source.wav");
    crate::audio_export::tests::tone(&source, false);
    let original = std::fs::read(&source).expect("source bytes");
    let folder = root.join("destination");
    std::fs::create_dir(&folder).expect("owned destination folder");
    let neighbor = folder.join("neighbor.wav");
    std::fs::copy(&source, &neighbor).expect("owned neighbor");
    let target = folder.join("saved.wav");
    let (mut host, owner, id) = setup(&source);
    let app = host.windows.get_mut(&owner).expect("owner");
    app.tabs
        .get_mut(id)
        .expect("audio document")
        .target
        .set_current_path(source.clone(), MediaKind::Audio);
    app.media_kind = Some(MediaKind::Audio);
    app.ensure_audio_queue();
    app.audio_queues
        .get_mut(&id)
        .expect("source queue")
        .order
        .set_repeat(towavue_core::RepeatMode::All);
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::SetVolume(0.5), MediaKind::Audio);
    let foreground = root.join("foreground.bmp");
    crate::tab_transfer::tests::bitmap(&foreground);
    let foreground_id = attach(&mut host, owner, &foreground);
    let app = host.windows.get_mut(&owner).expect("owner");
    assert!(app.start_save_as(
        id,
        source.clone(),
        MediaKind::Audio,
        SaveAsTarget::capture(&target).expect("choice"),
        None
    ));
    finish(&mut host, owner);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        drain(&mut host);
        if host.windows[&owner].audio_queues[&id]
            .order
            .next(&target, false)
            .as_ref()
            == Some(&neighbor)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fresh destination audio membership"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let app = host.windows.get_mut(&owner).expect("owner");
    assert_eq!(app.path.as_ref(), Some(&foreground));
    assert_eq!(app.tabs.active().expect("foreground").id, foreground_id);
    assert!(
        matches!(&app.tabs.get_mut(id).expect("saved audio").target, towavue_core::TabTarget::AudioFolder { folder: current, current: path } if current == &folder && path == &target)
    );
    assert_eq!(
        app.audio_queues[&id].order.repeat(),
        towavue_core::RepeatMode::All
    );
    assert!(!app.edits[&id].is_dirty());
    assert_eq!(std::fs::read(&source).expect("source unchanged"), original);
    // Seed only foreground ownership, as in the existing headless Save controls;
    // the separate native video test exercises actual reader suspension/resume.
    app.tabs.activate(id);
    app.displayed_tab = Some(id);
    app.path = Some(target.clone());
    app.media_kind = Some(MediaKind::Audio);
    app.state = PlaybackState::Paused;
    app.edits.get_mut(&id).expect("history").undo();
    assert!(app.edits[&id].is_dirty());
    assert!(app.save_source(None));
    finish(&mut host, owner);
    assert_eq!(
        crate::audio_export::tests::decoded_samples(&target),
        crate::audio_export::tests::decoded_samples(&source)
    );
    assert!(!host.windows[&owner].edits[&id].is_dirty());
}
