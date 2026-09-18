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
fn recycle_completion_retargets_duplicate_tabs_and_leaves_an_empty_window_open() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::recycle_tests::recycle_completion_retargets_duplicate_tabs_and_leaves_an_empty_window_open",
    ) else {
        return;
    };
    let source = root.join("middle.bmp");
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
        before: snapshot(&root, &[source.clone(), next.clone()]),
        after: Some(snapshot(&root, std::slice::from_ref(&next))),
    };
    for (key, (background, active)) in keys.iter().zip(&ids) {
        let app = host.windows.get_mut(key).expect("app");
        app.finish_file_recycling(&source, &report);
        assert_eq!(app.tabs.active_id(), Some(*active));
        assert_eq!(app.path.as_ref(), Some(&next));
        for id in [background, active] {
            assert_eq!(
                app.tabs
                    .get_mut(*id)
                    .expect("same tab")
                    .target
                    .current_path(),
                next
            );
            assert!(app.edits.get(id).is_none_or(|history| !history.is_dirty()));
            assert!(!app.export_paths.contains_key(id));
        }
        assert!(app.closed_tabs.is_empty());
        let empty = FileRecycleReport {
            before: snapshot(&root, std::slice::from_ref(&next)),
            after: Some(snapshot(&root, &[])),
        };
        app.finish_file_recycling(&next, &empty);
        assert!(app.tabs.tabs().is_empty());
        assert!(
            app.tabs.gallery().is_some(),
            "unrelated Gallery is retained"
        );
        assert!(app.path.is_none());
        assert!(app.session.is_none());
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
        &source,
        &FileRecycleReport {
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
        next
    );
    let queue = &app.audio_queues[&audio].order;
    assert!(queue.shuffled());
    assert_eq!(queue.repeat(), towavue_core::RepeatMode::One);
    assert_eq!(queue.next(&next, true), Some(next.clone()));
    assert_eq!(queue.next(&source, false), None);
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
    assert!(app.tabs.is_empty() && app.path.is_none());
    assert!(!app.exit_requested);
    assert!(!app.file_operations.busy());
    assert!(host.delete_confirmation_suppressed);
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
