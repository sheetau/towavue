use super::*;

fn snapshot(paths: &[PathBuf]) -> FolderSnapshot {
    FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![0]),
        folder_path: paths[0].parent().expect("fixture folder").to_owned(),
        items: paths
            .iter()
            .enumerate()
            .map(|(index, path)| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![index as u8]),
                path: path.clone(),
                kind: MediaKind::from_path(path).expect("media"),
            })
            .collect(),
        sort_columns: Vec::new(),
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 42,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    }
}

#[test]
fn queued_filmstrip_windows_validate_source_identity_and_coalesce_duplicate_actions() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::opening_tests::queued_filmstrip_windows_validate_source_identity_and_coalesce_duplicate_actions",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let key = *host.windows.keys().next().expect("window");
    let app = host.windows.get_mut(&key).expect("app");
    let paths = [root.join("source.png"), root.join("target.png")];
    let tab = app.tabs.open_new(paths[0].clone(), MediaKind::Image);
    app.folder_snapshot = Some(snapshot(&paths));
    app.filmstrip_open = true;
    app.handle_ui_action(UiAction::OpenWindow(paths[1].clone(), 41));
    app.handle_ui_action(UiAction::OpenWindow(root.join("foreign.png"), 42));
    assert!(app.pending_window_open.is_none());
    app.palette_open = true;
    app.handle_ui_action(UiAction::OpenWindow(paths[1].clone(), 42));
    assert!(app.pending_window_open.is_none());
    app.palette_open = false;
    app.handle_ui_action(UiAction::OpenWindow(paths[1].clone(), 42));
    app.handle_ui_action(UiAction::OpenWindow(paths[0].clone(), 42));
    let request = app.pending_window_open.take().expect("one queued action");
    assert_eq!(request.path, paths[1]);
    assert!(app.window_open_request_is_current(&request));
    app.filmstrip_open = false;
    assert!(!app.window_open_request_is_current(&request));
    app.filmstrip_open = true;
    app.media_generation += 1;
    assert!(!app.window_open_request_is_current(&request));
    app.media_generation -= 1;
    app.tabs.open_new(paths[0].clone(), MediaKind::Image);
    assert!(!app.window_open_request_is_current(&request));
    app.tabs.activate(tab);
    app.folder_snapshot.as_mut().expect("snapshot").generation += 1;
    assert!(!app.window_open_request_is_current(&request));
    app.folder_snapshot.as_mut().expect("snapshot").generation -= 1;
    app.export_error = Some("modal".into());
    assert!(!app.window_open_request_is_current(&request));
    app.export_error = None;
    assert!(
        host.open_filmstrip_window_with(key, &request, false, |_, _| panic!(
            "renderer unavailable"
        ))
        .is_err()
    );
    assert_eq!(host.windows.len(), 1);
    host.windows.get_mut(&key).expect("source").exit_requested = true;
    assert!(
        host.open_filmstrip_window_with(key, &request, false, |_, _| panic!("closed source"))
            .expect("ignore closed")
            .is_none()
    );
    host.remove_closed();
    assert!(
        host.open_filmstrip_window_with(key, &request, false, |_, _| panic!("removed source"))
            .expect("ignore removed")
            .is_none()
    );
}

pub(super) fn finish_child(app: &mut WindowApplication) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.finish_image_load();
        app.poll_audio();
        app.load_next_frame();
        app.render_frame();
        let ready = match app.media_kind.expect("child media") {
            MediaKind::Image => !app.image_loading,
            MediaKind::Video => app
                .session
                .as_ref()
                .is_some_and(|session| session.current_video_time().is_some()),
            MediaKind::Audio => app
                .session
                .as_ref()
                .and_then(PlaybackSession::audio_position)
                .is_some_and(|position| position > MediaTime::ZERO),
        };
        if ready || app.state == PlaybackState::Faulted {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "child loading deadline: {:?} {:?}",
            app.state,
            app.playback_error
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn exercise(host: &mut WindowHost, event_loop: &ActiveEventLoop) {
    let source = *host.windows.keys().next().expect("source");
    let app = host.windows.get_mut(&source).expect("source");
    // The preceding image exercise has just reactivated this paused video.
    finish_child(app);
    let source_path = app.path.clone().expect("silent video");
    let image_path = source_path.with_file_name("filmstrip [new] 日本語.bmp");
    let bad_image = source_path.with_file_name("filmstrip-corrupt.bmp");
    let audio_path = source_path.with_file_name("transfer-silence.wav");
    tab_transfer::tests::bitmap(&image_path);
    std::fs::write(&bad_image, b"owned invalid bitmap").expect("invalid fixture");
    assert!(audio_path.exists(), "silent fixture from transfer harness");
    let paths = [
        source_path.clone(),
        image_path.clone(),
        audio_path.clone(),
        bad_image.clone(),
    ];
    let tab = app.tabs.active().expect("source tab").id;
    let old_history = app.edits.remove(&tab);
    let old_export = app
        .export_paths
        .insert(tab, source_path.with_file_name("source-export.mp4"));
    let old_snapshot = app.folder_snapshot.replace(snapshot(&paths));
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Video);
    let tabs = app.tabs.clone();
    let edits = app.edits.clone();
    let exports = app.export_paths.clone();
    let generation = app.generation;
    let instance = app.media_generation;
    let position = app.current_position();
    let window_count = host.windows.len();

    let app = host.windows.get_mut(&source).expect("source");
    app.filmstrip_open = true;
    app.handle_ui_action(UiAction::OpenWindow(image_path.clone(), 42));
    let request = app.pending_window_open.take().expect("request");
    assert!(
        host.open_filmstrip_window_with(source, &request, false, |_, _| Err(
            "injected startup failure".into()
        ))
        .is_err()
    );
    assert!(
        host.open_filmstrip_window_with(source, &request, false, |app, device| {
            app.start_on_device(event_loop, Some(device), false)
                .expect("hidden stage");
            Err("injected post-start failure".into())
        })
        .is_err()
    );
    assert_eq!(host.windows.len(), window_count);
    assert!(host.windows[&source].filmstrip_open);
    let mut audio_verified = false;
    for path in &paths {
        let app = host.windows.get_mut(&source).expect("source");
        app.filmstrip_open = true;
        app.handle_ui_action(UiAction::OpenWindow(path.clone(), 42));
        let keys: Vec<_> = host.windows.keys().copied().collect();
        host.open_pending_windows(event_loop, false);
        let child = *host
            .windows
            .keys()
            .find(|key| !keys.contains(key))
            .expect("hosted filmstrip child");
        assert_eq!(host.windows.len(), window_count + 1);
        let app = host.windows.get_mut(&child).expect("child");
        assert_eq!(
            app.window.as_ref().expect("child window").is_visible(),
            Some(false)
        );
        assert_eq!(
            app.path.as_ref(),
            Some(&canonical_shell_path(path).expect("canonical path"))
        );
        assert_eq!(app.tabs.tabs().len(), 1);
        assert!(!app.command_context().has_unsaved_edits && app.export_paths.is_empty());
        finish_child(app);
        if path == &bad_image {
            assert!(app.image_error.is_some() && app.state == PlaybackState::Faulted);
        } else if path == &audio_path
            && app.playback_error.as_ref().is_some_and(|error| {
                error.starts_with("audio output failed: WASAPI output failed:")
            })
        {
            eprintln!(
                "SKIP hosted filmstrip audio playback: shared WASAPI unavailable: {:?}",
                app.playback_error
            );
        } else {
            assert!(
                app.image_error.is_none() && app.playback_error.is_none(),
                "child errors: {:?} {:?}",
                app.image_error,
                app.playback_error
            );
            if path == &image_path {
                assert_eq!(
                    app.image.as_ref().expect("decoded image").dimensions(),
                    (2, 1)
                );
            }
            if path == &audio_path {
                audio_verified = true;
            }
        }
        if let Some(session) = &app.session {
            assert_eq!(session.metrics().cpu_transfer_count, 0);
        }
        // A source hardware frame must also draw on this newly opened child's device.
        let mut original = host.windows.remove(&source).expect("source");
        assert!(
            original
                .session
                .as_mut()
                .expect("source session")
                .draw_current(
                    host.windows
                        .get_mut(&child)
                        .expect("child")
                        .renderer
                        .as_mut()
                        .expect("renderer"),
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(160.0, 96.0)),
                    original.video_uv,
                )
                .expect("same-device cross-draw")
        );
        assert_eq!(original.tabs, tabs);
        assert_eq!(original.edits, edits);
        assert_eq!(original.export_paths, exports);
        assert_eq!(original.generation, generation);
        assert_eq!(original.media_generation, instance);
        assert_eq!(original.current_position(), position);
        assert!(!original.filmstrip_open && original.pending_guard.is_none());
        host.windows.insert(source, original);
        host.windows.get_mut(&child).expect("child").exit_requested = true;
        host.remove_closed();
    }
    let app = host.windows.get_mut(&source).expect("source");
    let missing = source_path.with_file_name("missing-filmstrip.png");
    app.folder_snapshot = Some(snapshot(&[source_path, missing.clone()]));
    app.filmstrip_open = true;
    app.handle_ui_action(UiAction::OpenWindow(missing, 42));
    host.open_pending_windows(event_loop, false);
    assert_eq!(host.windows.len(), window_count);
    let app = host.windows.get_mut(&source).expect("source");
    assert!(app.filmstrip_open);
    assert!(
        app.status_message
            .as_ref()
            .expect("failure status")
            .0
            .starts_with("Could not open new window:")
    );
    assert_eq!(app.edits, edits);
    if let Some(history) = old_history {
        app.edits.insert(tab, history);
    } else {
        app.edits.remove(&tab);
    }
    if let Some(path) = old_export {
        app.export_paths.insert(tab, path);
    } else {
        app.export_paths.remove(&tab);
    }
    app.folder_snapshot = old_snapshot;
    app.close_filmstrip();
    eprintln!(
        "PASS hosted filmstrip windows: normal action opens/loads image, silent video and corrupt media independently; silent audio playback verified={audio_verified}; same-device cross-draw; source tabs/edits/export/clock/session unchanged; startup/post-start/missing-file failures preserve filmstrip and leave no child"
    );
}
