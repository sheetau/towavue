use super::*;
use towavue_runtime_windows::FileOperationSource;

#[test]
fn video_export_quality_save_uses_the_started_preset_and_retains_original_when_global_choice_changes()
 {
    use std::os::windows::process::CommandExt;
    use towavue_runtime_windows::VideoExportQuality;
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::video_export_quality_save_uses_the_started_preset_and_retains_original_when_global_choice_changes",
    ) else {
        return;
    };
    let source = root.join("source.mp4");
    let fixture = std::process::Command::new(
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe"),
    )
    .creation_flags(0x0800_0000)
    .args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=96x64:rate=8:duration=1",
        "-c:v",
        "mpeg4",
    ])
    .arg(&source)
    .output()
    .expect("fixture");
    assert!(
        fixture.status.success(),
        "{}",
        String::from_utf8_lossy(&fixture.stderr)
    );
    let original = std::fs::read(&source).expect("original");
    let (mut host, owner, id) = setup(&source);
    let app = host.windows.get_mut(&owner).expect("owner");
    app.tabs
        .get_mut(id)
        .expect("tab")
        .target
        .set_current_path(source.clone(), MediaKind::Video);
    app.media_kind = Some(MediaKind::Video);
    app.dispatch(CommandId::ExportQualityBalanced);
    app.dispatch(CommandId::Save);
    let export = app
        .active_export
        .as_ref()
        .expect("quality-only Save encodes");
    assert_eq!(export.request.target, source);
    assert!(export.request.operations.is_empty());
    assert_eq!(export.options.video_quality, VideoExportQuality::Balanced);
    app.dispatch(CommandId::ExportQualitySmaller);
    assert_eq!(app.video_export_quality(), VideoExportQuality::Smaller);
    assert_eq!(
        app.active_export
            .as_ref()
            .expect("unchanged worker options")
            .options
            .video_quality,
        VideoExportQuality::Balanced
    );
    finish(&mut host, owner);
    let app = host.windows.get_mut(&owner).expect("owner");
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    assert_ne!(std::fs::read(&source).expect("saved video"), original);
    assert!(
        app.edits[&id].is_dirty(),
        "the currently selected output differs from the saved snapshot"
    );
    app.dispatch(CommandId::Save);
    assert_eq!(
        app.active_export
            .as_ref()
            .expect("next Save")
            .options
            .video_quality,
        VideoExportQuality::Smaller
    );
    finish(&mut host, owner);
    let app = &host.windows[&owner];
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    assert!(!app.edits[&id].is_dirty());
    assert_eq!(
        std::fs::read(app.media_input_for(Some(id), &source).path()).expect("retained original"),
        original
    );
}

fn attach(host: &mut WindowHost, owner: WindowKey, source: &Path) -> TabId {
    let app = host.windows.get_mut(&owner).expect("window");
    let id = app.tabs.open_new(source.to_owned(), MediaKind::Image);
    app.path = Some(source.to_owned());
    app.displayed_tab = Some(id);
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    app.source_versions.insert(
        id,
        Some(FileOperationSource::capture(source).expect("loaded source")),
    );
    id
}
fn drain(host: &mut WindowHost) {
    super::super::tests::drain_captured(host);
}
fn finish(host: &mut WindowHost, owner: WindowKey) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        drain(host);
        host.advance_source_save();
        if host.source_save.is_none() && host.windows[&owner].active_export.is_none() {
            break;
        }
        assert!(Instant::now() < deadline, "save completion");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        host.windows
            .values()
            .all(|app| !app.file_operations.locked && !app.source_save.frozen)
    );
}
fn prepared(host: &mut WindowHost, owner: WindowKey) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        drain(host);
        if host.windows[&owner]
            .source_save
            .pending
            .as_ref()
            .is_some_and(|pending| pending.prepared.is_some())
        {
            return;
        }
        assert!(
            host.windows[&owner].export_error.is_none(),
            "{:?}",
            host.windows[&owner].export_error
        );
        assert!(Instant::now() < deadline, "save preparation");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn pixels(path: &Path) -> Vec<u8> {
    towavue_runtime_windows::decode_image(path)
        .expect("bitmap")
        .frames[0]
        .rgba
        .to_vec()
}
fn setup(source: &Path) -> (WindowHost, WindowKey, TabId) {
    let mut host = WindowHost::new(None, None).expect("host");
    *host.captured_events.lock().expect("events") = Some(VecDeque::new());
    let owner = *host.windows.keys().next().expect("window");
    let id = attach(&mut host, owner, source);
    (host, owner, id)
}

#[test]
fn source_save_dispatch_preserves_undo_and_coordinates_repeated_saves_across_baselines() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::source_save_dispatch_preserves_undo_and_coordinates_repeated_saves_across_baselines",
    ) else {
        return;
    };
    let path = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&path);
    let original = pixels(&path);
    let (mut host, owner, id) = setup(&path);
    let other = host.add_application(None).expect("other window");
    let other_id = attach(&mut host, other, &path);
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .dispatch(CommandId::Save);
    assert!(
        host.windows[&owner].active_export.is_none(),
        "unedited media must not be recompressed"
    );
    assert_eq!(pixels(&path), original);
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .dispatch(CommandId::Save);
    prepared(&mut host, owner);
    assert_eq!(pixels(&path), original, "preparation cannot overwrite");
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .edits
        .get_mut(&id)
        .expect("history")
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let read_path = path.clone();
    host.windows[&owner].thumbnail_worker.submit(move |_| {
        let _reader = std::fs::File::open(read_path).expect("owned reader");
        ready_tx.send(()).expect("reader ready");
        release_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("reader release");
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("reader started");
    host.begin_source_publication(owner);
    host.advance_source_save();
    assert!(
        host.source_save
            .as_ref()
            .expect("waiting for reader")
            .prepared
            .is_some()
    );
    assert_eq!(
        pixels(&path),
        original,
        "cancelled worker must actually release its reader before publication"
    );
    for app in host.windows.values_mut() {
        assert!(app.modal_input_blocked());
        app.request_guarded(GuardedAction::Exit);
        assert!(!app.exit_requested);
        app.handle_ui_action(UiAction::CancelExport);
        assert!(
            app.active_export
                .as_ref()
                .is_none_or(|export| !export.cancelling),
            "accepted publication cannot cancel"
        );
    }
    release_tx.send(()).expect("release cancelled reader");
    finish(&mut host, owner);
    assert!(
        host.windows[&owner].export_error.is_none(),
        "{:?}",
        host.windows[&owner].export_error
    );
    assert_ne!(pixels(&path), original);
    let flipped = pixels(&path);
    assert!(
        host.windows[&owner].edits[&id].is_dirty(),
        "edits added during preparation were not saved"
    );
    assert!(
        host.windows
            .get_mut(&owner)
            .expect("owner")
            .edits
            .get_mut(&id)
            .expect("history")
            .undo()
    );
    assert!(!host.windows[&owner].edits[&id].is_dirty());
    assert!(host.windows[&other].edits[&other_id].is_dirty());
    let first_original = host.windows[&owner].source_backings[&id]
        .original_path()
        .to_owned();
    assert_eq!(
        host.windows[&other].source_backings[&other_id].original_path(),
        first_original
    );
    // A document opened after the first save has a different original baseline.
    let newer = host.add_application(None).expect("newer window");
    let newer_id = attach(&mut host, newer, &path);
    let app = host.windows.get_mut(&owner).expect("owner");
    assert!(app.edits.get_mut(&id).expect("history").undo());
    assert!(app.edits[&id].is_dirty());
    app.dispatch(CommandId::Save);
    finish(&mut host, owner);
    assert!(host.windows[&owner].export_error.is_none());
    assert_eq!(
        pixels(&path),
        original,
        "repeated Save applies edits once to the earliest original"
    );
    assert_eq!(
        host.windows[&owner].source_backings[&id].original_path(),
        first_original
    );
    assert!(
        !host.windows[&other].edits[&other_id].is_dirty(),
        "same original with empty edits now matches"
    );
    assert!(
        host.windows[&newer].edits[&newer_id].is_dirty(),
        "newer baseline cannot compare equal operation arrays"
    );
    assert_eq!(
        pixels(host.windows[&newer].source_backings[&newer_id].original_path()),
        flipped
    );
    let app = host.windows.get_mut(&newer).expect("newer");
    app.dispatch(CommandId::Save);
    finish(&mut host, newer);
    assert!(host.windows[&newer].export_error.is_none());
    assert_eq!(pixels(&path), flipped);
    assert!(!host.windows[&newer].edits[&newer_id].is_dirty());
    assert!(host.windows[&owner].edits[&id].is_dirty());
    assert_eq!(
        host.windows[&owner].source_versions[&id],
        Some(FileOperationSource::capture(&path).expect("fresh target"))
    );
    drop(host);
    let deadline = Instant::now() + Duration::from_secs(5);
    while first_original.exists() {
        assert!(Instant::now() < deadline, "last owner cleanup");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn source_save_cancel_and_stale_publication_keep_source_and_dirty_history() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::source_save_cancel_and_stale_publication_keep_source_and_dirty_history",
    ) else {
        return;
    };
    let path = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&path);
    let original = std::fs::read(&path).expect("source");
    let (mut host, owner, id) = setup(&path);
    let app = host.windows.get_mut(&owner).expect("owner");
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    app.dispatch(CommandId::Save);
    prepared(&mut host, owner);
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .handle_ui_action(UiAction::CancelExport);
    finish(&mut host, owner);
    assert_eq!(std::fs::read(&path).expect("cancelled source"), original);
    assert!(host.windows[&owner].edits[&id].is_dirty());
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .dispatch(CommandId::Save);
    prepared(&mut host, owner);
    std::fs::write(&path, b"external replacement after preparation").expect("owned external edit");
    let external = std::fs::read(&path).expect("external bytes");
    finish(&mut host, owner);
    assert_eq!(
        std::fs::read(&path).expect("preserved external edit"),
        external
    );
    assert!(host.windows[&owner].export_error.is_some());
    assert!(host.windows[&owner].edits[&id].is_dirty());
    assert!(host.windows[&owner].source_backings.is_empty());
    let app = host.windows.get_mut(&owner).expect("owner");
    assert!(!app.edits[&id].source_available());
    assert!(!app.start_export(
        id,
        path.clone(),
        MediaKind::Image,
        root.join("untrusted.bmp"),
        None,
        ExportOutput::Media
    ));
    assert!(!root.join("untrusted.bmp").exists());
}

#[test]
fn source_save_ambiguous_publication_keeps_edits_and_rejects_untrusted_input() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::source_save_ambiguous_publication_keeps_edits_and_rejects_untrusted_input",
    ) else {
        return;
    };
    let path = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&path);
    let (mut host, owner, id) = setup(&path);
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .dispatch(CommandId::Save);
    prepared(&mut host, owner);
    host.begin_source_publication(owner);
    let serial = host.source_save.as_ref().expect("publication").serial;
    // Controlled delivery of an ambiguous native outcome, not an induced OS failure.
    host.finish_source_publication(
        owner,
        serial,
        Err(towavue_runtime_windows::SourceSaveError::RecoveryRequired {
            message: "controlled ambiguous replacement".into(),
            directory: root.clone(),
        }
        .into()),
    );
    let app = &host.windows[&owner];
    assert!(
        app.export_error
            .as_ref()
            .expect("recovery details")
            .contains("controlled ambiguous replacement")
    );
    assert!(app.edits[&id].is_dirty());
    assert!(!app.edits[&id].source_available());
    assert_eq!(app.source_versions[&id], None);
    assert_eq!(
        app.edits[&id].operations(),
        &[EditOperation::FlipHorizontal]
    );
    assert!(!app.file_operations.locked && !app.source_save.frozen);
}

#[test]
fn source_save_close_guard_saves_then_leaves_and_unavailable_version_keeps_edits() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::source_save_close_guard_saves_then_leaves_and_unavailable_version_keeps_edits",
    ) else {
        return;
    };
    let path = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&path);
    let original = pixels(&path);
    let (mut host, owner, id) = setup(&path);
    let app = host.windows.get_mut(&owner).expect("owner");
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    app.source_versions.insert(id, None);
    app.dispatch(CommandId::Save);
    assert!(app.active_export.is_none());
    assert!(app.export_error.take().is_some());
    assert!(app.edits[&id].is_dirty());
    app.source_versions.insert(
        id,
        Some(FileOperationSource::capture(&path).expect("source")),
    );
    app.request_guarded(GuardedAction::Exit);
    app.resolve_guard(GuardDecision::Save);
    assert!(app.active_export.is_some());
    assert!(!app.exit_requested);
    finish(&mut host, owner);
    assert!(host.windows[&owner].exit_requested);
    assert_ne!(pixels(&path), original);
}

#[test]
fn source_save_leaving_guard_preserves_audio_options_and_replaces_only_the_source() {
    use crate::audio_export::tests::{apply, decoded_samples, setting, tone};
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::source_save_leaving_guard_preserves_audio_options_and_replaces_only_the_source",
    ) else {
        return;
    };
    let source = root.join("source.wav");
    let derivative = root.join("previous-export.wav");
    tone(&source, false);
    let (mut host, owner, id) = setup(&source);
    let app = host.windows.get_mut(&owner).expect("owner");
    app.tabs
        .get_mut(id)
        .expect("tab")
        .target
        .set_current_path(source.clone(), MediaKind::Audio);
    app.media_kind = Some(MediaKind::Audio);
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::SetVolume(0.25), MediaKind::Audio);
    app.export_paths.insert(id, derivative.clone());
    apply(app, setting());
    app.request_guarded(GuardedAction::Exit);
    app.resolve_guard(GuardDecision::Save);
    let export = app.active_export.as_ref().expect("guard save");
    assert_eq!(export.options.audio, setting());
    assert_eq!(export.request.target, source);
    assert!(!app.exit_requested);
    finish(&mut host, owner);
    assert!(
        host.windows[&owner].export_error.is_none(),
        "{:?}",
        host.windows[&owner].export_error
    );
    assert!(host.windows[&owner].exit_requested);
    assert!(!host.windows[&owner].edits[&id].is_dirty());
    assert!(!derivative.exists());
    let output = decoded_samples(&source);
    assert_eq!(output.len(), 48000);
    let peak = output
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);
    assert!((peak - 10_f32.powf(-0.05)).abs() < 0.00004);
}

#[test]
fn source_save_native_video_reopens_all_hosted_readers_on_their_retained_original() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::source_save::tests::source_save_native_video_reopens_all_hosted_readers_on_their_retained_original",
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
            app.dispatch(CommandId::Save);
            finish(&mut host, owner);
            assert!(
                host.windows[&owner].export_error.is_none(),
                "{:?}",
                host.windows[&owner].export_error
            );
            for (key, tab) in [(owner, id), (other, other_id)] {
                let app = host.windows.get_mut(&key).expect("app");
                let backup = app.source_backings[&tab].original_path().to_owned();
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
            towavue_runtime_windows::decode_file(&self.source, |item| {
                if let towavue_runtime_windows::DecodeOutput::Video(frame) = item {
                    assert_eq!((frame.width, frame.height), (32, 48));
                    frames += 1;
                }
                true
            })
            .expect("saved video");
            assert_eq!(frames, 8);
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
