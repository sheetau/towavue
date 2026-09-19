use super::*;
use crate::file_operations::Kind;
use std::time::SystemTime;
use towavue_core::{FolderMediaItem, FolderSnapshot, FolderSnapshotSource, ShellIdentity};
use towavue_runtime_windows::{DeleteConfirmation, FileRecycleReport};

fn snapshot(folder: &Path, paths: &[PathBuf]) -> FolderSnapshot {
    FolderSnapshot {
        folder_identity: ShellIdentity::new(vec![0]),
        folder_path: folder.to_owned(),
        items: paths
            .iter()
            .enumerate()
            .map(|(i, path)| FolderMediaItem {
                identity: ShellIdentity::new(vec![i as u8]),
                path: path.clone(),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: Vec::new(),
        source: FolderSnapshotSource::PersistedShellView,
        generation: 1,
        captured_at: SystemTime::UNIX_EPOCH,
    }
}

fn source_app(host: &mut WindowHost, source: &Path) -> WindowKey {
    let owner = *host.windows.keys().next().expect("window");
    let app = host.windows.get_mut(&owner).expect("app");
    if let Some(gallery) = app.tabs.gallery() {
        app.tabs.take_gallery(gallery);
    }
    let tab = app.tabs.open_new(source.to_owned(), MediaKind::Image);
    app.path = Some(source.to_owned());
    app.media_kind = Some(MediaKind::Image);
    app.displayed_tab = Some(tab);
    app.state = PlaybackState::Paused;
    *host.captured_events.lock().expect("events") = Some(VecDeque::new());
    owner
}

// A native dialog response is injected here. This verifies the real worker and
// host ownership boundary; it does not claim visible dialog/input evidence.
fn pending_confirmation(host: &mut WindowHost, owner: WindowKey) -> u64 {
    host.windows
        .get_mut(&owner)
        .expect("app")
        .begin_file_relocation(Kind::Delete);
    let deadline = Instant::now() + Duration::from_secs(5);
    while host.windows[&owner].file_operations.ready.is_none() {
        super::super::tests::drain_captured(host);
        assert!(Instant::now() < deadline, "source inspection");
        std::thread::sleep(Duration::from_millis(2));
    }
    let app = host.windows.get_mut(&owner).expect("app");
    let serial = app.file_operations.serial;
    let (source, action) = app.file_operations.ready.take().expect("prepared delete");
    let path = source.path().to_owned();
    host.file_operation = Some(Transaction {
        owner,
        serial,
        source: path,
        expected: source.clone(),
        waiting_since: None,
        retain_copy: false,
        action: Some((source, action)),
        suppress_confirmation: false,
    });
    for app in host.windows.values_mut() {
        app.file_operations.locked = true;
    }
    serial
}

fn wait(host: &mut WindowHost) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.file_operation.is_some() {
        super::super::tests::drain_captured(host);
        host.advance_file_operation();
        assert!(Instant::now() < deadline, "recycle completion");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn recycle_cancel_and_unavailable_confirmation_preserve_source_edits_and_preference() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::recycle_tests::recycle_cancel_and_unavailable_confirmation_preserve_source_edits_and_preference",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    std::fs::write(&source, b"owned source").expect("source");
    let mut host = WindowHost::new(None, None).expect("host");
    let owner = source_app(&mut host, &source);
    let id = host.windows[&owner].displayed_tab.expect("tab");
    host.windows
        .get_mut(&owner)
        .expect("app")
        .edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let serial = pending_confirmation(&mut host, owner);
    host.finish_delete_confirmation(
        owner,
        serial.wrapping_sub(1),
        Ok(DeleteConfirmation {
            confirmed: true,
            dont_ask_again: true,
        }),
    );
    assert!(host.file_operation.is_some());
    host.finish_delete_confirmation(
        owner,
        serial,
        Ok(DeleteConfirmation {
            confirmed: false,
            dont_ask_again: true,
        }),
    );
    assert!(host.file_operation.is_none());
    assert!(source.exists());
    assert!(host.windows[&owner].edits[&id].is_dirty());
    assert!(!host.delete_confirmation_suppressed);
    assert!(
        !host
            .delete_preference_path
            .as_ref()
            .expect("isolated preference")
            .exists()
    );
    // Even when opted out, dirty media still needs an available native owner.
    host.delete_confirmation_suppressed = true;
    let serial = pending_confirmation(&mut host, owner);
    let mut transaction = host.file_operation.take().expect("pending");
    host.windows
        .get_mut(&owner)
        .expect("app")
        .file_operations
        .ready = transaction.action.take();
    for app in host.windows.values_mut() {
        app.file_operations.locked = false;
    }
    host.start_pending_file_operation();
    assert!(host.file_operation.is_none());
    assert!(source.exists());
    assert!(host.windows[&owner].edits[&id].is_dirty());
    assert_eq!(host.windows[&owner].file_operations.serial, serial);
}

#[test]
fn recycle_failure_and_timeline_gate_preserve_source_and_dirty_history() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::recycle_tests::recycle_failure_and_timeline_gate_preserve_source_and_dirty_history",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&source);
    let mut host = WindowHost::new(None, None).expect("host");
    let owner = source_app(&mut host, &source);
    let app = host.windows.get_mut(&owner).expect("app");
    let id = app.displayed_tab.expect("tab");
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    app.timeline_open = true;
    app.begin_file_relocation(Kind::Delete);
    assert!(app.file_operations.pending.is_none());
    app.timeline_open = false;
    let original = std::fs::read(&source).expect("original bytes");
    let serial = pending_confirmation(&mut host, owner);
    let writer = std::fs::OpenOptions::new()
        .write(true)
        .open(&source)
        .expect("competing writer");
    host.finish_delete_confirmation(
        owner,
        serial,
        Ok(DeleteConfirmation {
            confirmed: true,
            dont_ask_again: true,
        }),
    );
    wait(&mut host);
    drop(writer);
    assert_eq!(std::fs::read(&source).expect("original survives"), original);
    let app = &host.windows[&owner];
    assert_eq!(app.path.as_ref(), Some(&source));
    assert!(app.edits[&id].is_dirty());
    assert!(!app.file_operations.busy());
    assert!(!host.delete_confirmation_suppressed);
    assert!(
        !host
            .delete_preference_path
            .as_ref()
            .expect("preference")
            .exists()
    );
}

#[test]
fn recycle_completion_retains_duplicate_documents_edits_and_navigation() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::recycle_tests::recycle_completion_retains_duplicate_documents_edits_and_navigation",
    ) else {
        return;
    };
    let source = root.join("middle.bmp");
    crate::tab_transfer::tests::bitmap(&source);
    let expected = FileOperationSource::capture(&source).expect("source identity");
    let retained =
        towavue_runtime_windows::RetainedSource::capture(&expected).expect("retained bytes");
    let next = root.join("next.bmp");
    crate::tab_transfer::tests::bitmap(&next);
    let mut host = WindowHost::new(None, None).expect("host");
    host.add_application(None).expect("second");
    let keys: Vec<_> = host.windows.keys().copied().collect();
    let mut ids = Vec::new();
    for key in &keys {
        let app = host.windows.get_mut(key).expect("app");
        let background = app.tabs.open_new(source.clone(), MediaKind::Image);
        let active = app.tabs.open_new(source.clone(), MediaKind::Image);
        app.path = Some(source.clone());
        app.media_kind = Some(MediaKind::Image);
        app.displayed_tab = Some(active);
        app.state = PlaybackState::Paused;
        for id in [background, active] {
            app.edits
                .entry(id)
                .or_default()
                .push(EditOperation::FlipHorizontal, MediaKind::Image);
            app.export_paths.insert(id, root.join("export.bmp"));
        }
        app.closed_tabs
            .push_back(closed_tabs::ClosedTab::Media(source.clone(), 0));
        ids.push((background, active));
    }
    let report = FileRecycleReport {
        retained_source: Some(retained),
        before: snapshot(&root, &[source.clone(), next.clone()]),
        after: Some(snapshot(&root, std::slice::from_ref(&next))),
    };
    std::fs::remove_file(&source).expect("simulate completed recycle");
    for (key, (background, active)) in keys.iter().zip(&ids) {
        let app = host.windows.get_mut(key).expect("app");
        app.finish_file_recycling(&expected, &report);
        assert_eq!(app.tabs.active_id(), Some(*active));
        assert_eq!(app.path.as_ref(), Some(&source));
        for id in [background, active] {
            assert_eq!(
                app.tabs
                    .get_mut(*id)
                    .expect("same tab")
                    .target
                    .current_path(),
                source
            );
            assert!(app.edits[id].is_dirty());
            assert!(app.export_paths.contains_key(id));
            assert!(app.deleted_sources.contains_key(id));
            assert_eq!(
                std::fs::read(app.source_backings[id].original_path()).expect("retained bytes"),
                std::fs::read(&next).expect("same fixture")
            );
        }
        assert!(app.closed_tabs.is_empty());
        assert_eq!(app.deleted_path_prefix(), "(deleted) ");
        assert!(app.command_context().media_kind.is_some());
        assert!(
            !towavue_core::command_definitions()
                .iter()
                .find(|command| command.id == CommandId::DeleteFile)
                .expect("delete command")
                .is_enabled(app.command_context())
        );
        assert_eq!(
            app.folder_snapshot
                .as_ref()
                .expect("real listing")
                .items
                .len(),
            1
        );
        assert_eq!(
            app.navigation_snapshot()
                .expect("held position")
                .items
                .len(),
            2
        );
        app.navigate(true, true);
        assert!(
            app.pending_guard.is_some(),
            "leaving still protects dirty edits"
        );
        assert!(!app.exit_requested);
    }
}

#[test]
fn background_audio_recycling_preserves_modes_and_unrelated_dirty_foreground() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::recycle_tests::background_audio_recycling_preserves_modes_and_unrelated_dirty_foreground",
    ) else {
        return;
    };
    let source = root.join("source.wav");
    std::fs::write(&source, b"owned audio document").expect("source");
    let expected = FileOperationSource::capture(&source).expect("source identity");
    let retained =
        towavue_runtime_windows::RetainedSource::capture(&expected).expect("retained bytes");
    let next = root.join("next.wav");
    let image = root.join("view.bmp");
    let mut host = WindowHost::new(None, None).expect("host");
    let owner = source_app(&mut host, &image);
    let app = host.windows.get_mut(&owner).expect("app");
    let foreground = app.displayed_tab.expect("image tab");
    app.edits
        .entry(foreground)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let audio = app.tabs.open_new(source.clone(), MediaKind::Audio);
    app.tabs.activate(foreground);
    app.ensure_audio_queue_for(audio, &source);
    let queue = &mut app.audio_queues.get_mut(&audio).expect("queue").order;
    queue.set_items(vec![source.clone(), next.clone()]);
    queue.set_repeat(towavue_core::RepeatMode::One);
    queue.toggle_shuffle(&source, 42);
    let mut before = snapshot(&root, &[source.clone(), next.clone()]);
    for item in &mut before.items {
        item.kind = MediaKind::Audio;
    }
    let mut after = before.clone();
    after.items.retain(|item| item.path != source);
    app.finish_file_recycling(
        &expected,
        &FileRecycleReport {
            retained_source: Some(retained),
            before,
            after: Some(after),
        },
    );
    assert_eq!(app.path.as_ref(), Some(&image));
    assert_eq!(app.tabs.active_id(), Some(foreground));
    assert!(app.edits[&foreground].is_dirty());
    assert_eq!(
        app.tabs
            .get_mut(audio)
            .expect("retained audio tab")
            .target
            .current_path(),
        source
    );
    let queue = &app.audio_queues[&audio].order;
    assert!(queue.shuffled());
    assert_eq!(queue.repeat(), towavue_core::RepeatMode::One);
    assert_eq!(queue.next(&next, true), Some(next.clone()));
    assert_eq!(queue.next(&source, true), Some(source.clone()));
    assert!(app.deleted_sources.contains_key(&audio));
}

#[test]
#[ignore = "recycles two uniquely owned tiny files through the production host and native Shell"]
fn native_recycle_completion_persists_confirmation_choice_and_does_not_close_last_window() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::recycle_tests::native_recycle_completion_persists_confirmation_choice_and_does_not_close_last_window",
    ) else {
        return;
    };
    let media = root.join("media");
    std::fs::create_dir(&media).expect("media folder");
    let source = media.join("only.bmp");
    crate::tab_transfer::tests::bitmap(&source);
    let mut host = WindowHost::new(None, None).expect("host");
    let owner = source_app(&mut host, &source);
    let serial = pending_confirmation(&mut host, owner);
    host.finish_delete_confirmation(
        owner,
        serial,
        Ok(DeleteConfirmation {
            confirmed: true,
            dont_ask_again: true,
        }),
    );
    host.finish_delete_confirmation(
        owner,
        serial,
        Ok(DeleteConfirmation {
            confirmed: true,
            dont_ask_again: true,
        }),
    );
    wait(&mut host);
    assert!(!source.exists());
    let app = &host.windows[&owner];
    assert_eq!(app.path.as_ref(), Some(&source));
    assert!(app.current_source_deleted());
    assert!(app.media_input(&source).path().exists());
    assert!(!app.exit_requested);
    assert!(!app.file_operations.busy());
    assert!(host.delete_confirmation_suppressed);
    let original = std::fs::read(app.media_input(&source).path()).expect("original retained bytes");
    host.windows
        .get_mut(&owner)
        .expect("owner")
        .dispatch(CommandId::Save);
    finish_save(&mut host, owner);
    assert_eq!(std::fs::read(&source).expect("recreated file"), original);
    assert!(!host.windows[&owner].current_source_deleted());
    let mut restarted = WindowHost::new(None, None).expect("reloaded preference");
    assert!(restarted.delete_confirmation_suppressed);
    let second = media.join("second.bmp");
    crate::tab_transfer::tests::bitmap(&second);
    let owner = source_app(&mut restarted, &second);
    restarted
        .windows
        .get_mut(&owner)
        .expect("app")
        .dispatch(CommandId::DeleteFile);
    let deadline = Instant::now() + Duration::from_secs(5);
    while restarted.windows[&owner].file_operations.ready.is_none() {
        super::super::tests::drain_captured(&mut restarted);
        assert!(Instant::now() < deadline, "second source inspection");
        std::thread::sleep(Duration::from_millis(2));
    }
    restarted.start_pending_file_operation();
    assert!(
        restarted
            .file_operation
            .as_ref()
            .is_some_and(|pending| pending.action.is_none()),
        "persisted opt-out starts the clean mutation without a dialog owner"
    );
    wait(&mut restarted);
    assert!(!second.exists());
    assert!(!restarted.windows[&owner].exit_requested);
    eprintln!(
        "PASS native recycle: two owned files, real Shell mutations and final enumeration, last window retained, duplicate confirmation ignored, opt-out survives new host construction and bypasses the next clean prompt through command dispatch; first native prompt response injected"
    );
}

fn finish_save(host: &mut WindowHost, owner: WindowKey) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        super::super::tests::drain_captured(host);
        host.advance_source_save();
        if host.source_save.is_none() && host.windows[&owner].active_export.is_none() {
            break;
        }
        assert!(Instant::now() < deadline, "recreation completion");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn deleted_document_save_rejects_collision_and_recreates_edits_without_losing_undo() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::recycle_tests::deleted_document_save_rejects_collision_and_recreates_edits_without_losing_undo",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&source);
    let expected = FileOperationSource::capture(&source).expect("source");
    let retained = towavue_runtime_windows::RetainedSource::capture(&expected).expect("copy");
    let original = std::fs::read(&source).expect("original");
    let mut host = WindowHost::new(None, None).expect("host");
    let owner = source_app(&mut host, &source);
    let id = host.windows[&owner].displayed_tab.expect("tab");
    std::fs::remove_file(&source).expect("simulate completed recycle");
    let app = host.windows.get_mut(&owner).expect("app");
    app.finish_file_recycling(
        &expected,
        &FileRecycleReport {
            retained_source: Some(retained),
            before: snapshot(&root, std::slice::from_ref(&source)),
            after: Some(snapshot(&root, &[])),
        },
    );
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    std::fs::write(&source, b"another application's file").expect("collision");
    app.dispatch(CommandId::Save);
    finish_save(&mut host, owner);
    let app = host.windows.get_mut(&owner).expect("app");
    assert!(app.export_error.take().is_some());
    assert!(app.current_source_deleted());
    assert_eq!(
        std::fs::read(&source).expect("collision intact"),
        b"another application's file"
    );
    std::fs::remove_file(&source).expect("remove owned collision");
    app.dispatch(CommandId::Save);
    finish_save(&mut host, owner);
    let app = host.windows.get_mut(&owner).expect("app");
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    assert!(!app.current_source_deleted());
    assert!(!app.edits[&id].is_dirty());
    assert_ne!(std::fs::read(&source).expect("edited file"), original);
    assert_eq!(
        std::fs::read(app.media_input(&source).path()).expect("original input"),
        original
    );
    assert!(app.edits.get_mut(&id).expect("history").undo());
    assert!(app.edits[&id].is_dirty());
    let current = app.source_versions[&id].clone().expect("saved version");
    let earliest = app.media_input(&source).path().to_owned();
    std::fs::remove_file(&source).expect("simulate recycling saved file");
    app.finish_file_recycling(
        &current,
        &FileRecycleReport {
            retained_source: None,
            before: snapshot(&root, std::slice::from_ref(&source)),
            after: Some(snapshot(&root, &[])),
        },
    );
    assert_eq!(app.media_input(&source).path(), earliest);
    app.dispatch(CommandId::Save);
    finish_save(&mut host, owner);
    assert!(host.windows[&owner].export_error.is_none());
    assert_eq!(
        std::fs::read(&source).expect("recreated original after undo"),
        original
    );
    assert!(!host.windows[&owner].current_source_deleted());
}

#[test]
fn recycling_waits_for_real_readers_and_times_out_without_deleting() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::recycle_tests::recycling_waits_for_real_readers_and_times_out_without_deleting",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&source);
    let mut host = WindowHost::new(None, None).expect("host");
    let owner = source_app(&mut host, &source);
    let serial = pending_confirmation(&mut host, owner);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let path = source.clone();
    host.windows[&owner].thumbnail_worker.submit(move |_| {
        let _reader = std::fs::File::open(path).expect("reader");
        ready_tx.send(()).expect("ready");
        release_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("release");
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("running reader");
    host.finish_delete_confirmation(
        owner,
        serial,
        Ok(DeleteConfirmation {
            confirmed: true,
            dont_ask_again: false,
        }),
    );
    assert!(
        host.file_operation
            .as_ref()
            .expect("draining")
            .action
            .is_some()
    );
    assert!(source.exists());
    host.file_operation
        .as_mut()
        .expect("draining")
        .waiting_since = Some(Instant::now() - Duration::from_secs(11));
    host.advance_file_operation();
    assert!(host.file_operation.is_none());
    assert!(source.exists());
    assert!(!host.windows[&owner].source_save.frozen);
    assert!(host.windows[&owner].deleted_sources.is_empty());
    release_tx.send(()).expect("release owned reader");
}

#[test]
fn recycling_rejects_stale_unbacked_documents_before_mutation() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::recycle_tests::recycling_rejects_stale_unbacked_documents_before_mutation",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    crate::tab_transfer::tests::bitmap(&source);
    let mut host = WindowHost::new(None, None).expect("host");
    let owner = source_app(&mut host, &source);
    let id = host.windows[&owner].displayed_tab.expect("tab");
    host.windows
        .get_mut(&owner)
        .expect("app")
        .source_versions
        .insert(id, None);
    let serial = pending_confirmation(&mut host, owner);
    host.finish_delete_confirmation(
        owner,
        serial,
        Ok(DeleteConfirmation {
            confirmed: true,
            dont_ask_again: false,
        }),
    );
    assert!(host.file_operation.is_none());
    assert!(source.exists());
    assert!(host.windows[&owner].deleted_sources.is_empty());
}

#[test]
#[ignore = "recycles an owned video through the real host and Shell with two native paused readers"]
fn native_recycle_video_retains_hosted_readers_positions_and_recreates_edits() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::recycle_tests::native_recycle_video_retains_hosted_readers_positions_and_recreates_edits",
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
            let mut host = WindowHost::new(None, None).expect("host");
            let owner = source_app(&mut host, &self.source);
            let id = host.windows[&owner].displayed_tab.expect("tab");
            let other = host.add_application(None).expect("other owner");
            let app = host.windows.get_mut(&other).expect("other app");
            let other_id = app.tabs.open_new(self.source.clone(), MediaKind::Video);
            app.path = Some(self.source.clone());
            app.displayed_tab = Some(other_id);
            app.state = PlaybackState::Paused;
            for (key, tab) in [(owner, id), (other, other_id)] {
                let app = host.windows.get_mut(&key).expect("app");
                app.tabs
                    .get_mut(tab)
                    .expect("tab")
                    .target
                    .set_current_path(self.source.clone(), MediaKind::Video);
                app.media_kind = Some(MediaKind::Video);
                app.source_versions.insert(
                    tab,
                    Some(FileOperationSource::capture(&self.source).expect("loaded version")),
                );
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
            let serial = pending_confirmation(&mut host, owner);
            host.finish_delete_confirmation(
                owner,
                serial,
                Ok(DeleteConfirmation {
                    confirmed: true,
                    dont_ask_again: false,
                }),
            );
            wait(&mut host);
            assert!(!self.source.exists());
            for (key, tab) in [(owner, id), (other, other_id)] {
                let app = host.windows.get_mut(&key).expect("held owner");
                assert!(app.deleted_sources.contains_key(&tab));
                let session = app.session.as_mut().expect("held session");
                let deadline = Instant::now() + Duration::from_secs(5);
                while session.pending_video_time().is_none() {
                    assert!(Instant::now() < deadline, "deleted video frame");
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert!(session.advance_pending());
                assert_eq!(
                    session
                        .current_video_snapshot()
                        .expect("frame")
                        .source_path(),
                    app.source_backings[&tab].original_path()
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
            host.windows
                .get_mut(&owner)
                .expect("owner")
                .dispatch(CommandId::Save);
            finish_save(&mut host, owner);
            assert!(
                host.windows
                    .values()
                    .all(|app| app.deleted_sources.is_empty())
            );
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
