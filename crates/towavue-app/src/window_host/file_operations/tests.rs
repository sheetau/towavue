use super::*;
use crate::file_operations::{Kind, Pending};
use std::sync::mpsc;

fn choose(host: &mut WindowHost, owner: WindowKey, kind: Kind, target: PathBuf) {
    let path = host.windows[&owner].path.clone().expect("source path");
    choose_at(host, owner, kind, path, target);
}

fn choose_at(host: &mut WindowHost, owner: WindowKey, kind: Kind, path: PathBuf, target: PathBuf) {
    let app = host.windows.get_mut(&owner).expect("owner");
    let (send, receive) = mpsc::channel();
    towavue_runtime_windows::inspect_file_operation_source(path.clone(), move |result| {
        send.send(result).expect("snapshot receiver");
    })
    .expect("snapshot worker");
    let source = receive
        .recv_timeout(Duration::from_secs(5))
        .expect("snapshot callback")
        .expect("source identity");
    app.file_operations.serial += 1;
    app.file_operations.pending = Some(Pending {
        serial: app.file_operations.serial,
        tab: app.tabs.active_id().expect("source tab"),
        origin_path: app.path.clone().expect("displayed origin"),
        path,
        instance: app.media_generation,
        kind,
        source: Some(source),
    });
    app.pending_dialog = Some(DialogIntent::RelocateFile);
    app.finish_dialog(Ok(Some(target)));
    assert!(app.file_operations.ready.is_some());
    host.start_pending_file_operation();
    assert!(
        host.file_operation.is_some(),
        "{:?}",
        host.windows[&owner].status_message
    );
    let serial = host.windows[&owner].file_operations.serial;
    host.finish_host_file_operation(
        owner,
        serial.wrapping_sub(1),
        Err("obsolete completion".into()),
    );
    assert!(
        host.file_operation.is_some(),
        "obsolete result cannot unlock the transaction"
    );
    for app in host.windows.values_mut() {
        assert!(app.modal_input_blocked());
        app.request_guarded(GuardedAction::Exit);
        assert!(!app.exit_requested);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.file_operation.is_some() {
        super::super::tests::drain_captured(host);
        assert!(Instant::now() < deadline, "file operation deadline");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(host.windows.values().all(|app| !app.file_operations.locked));
    assert!(host.windows[&owner].file_operations.pending.is_none());
}

#[test]
fn relocation_updates_all_tab_owners_without_discarding_edits_and_collision_rolls_back() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::tests::relocation_updates_all_tab_owners_without_discarding_edits_and_collision_rolls_back",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    let target = root.join("renamed.bmp");
    let folder = root.join("other");
    std::fs::create_dir(&folder).expect("destination");
    std::fs::write(&source, b"owned bytes; UI state test").expect("source");
    let mut host = WindowHost::new(None, None).expect("host");
    let owner = *host.windows.keys().next().expect("first");
    host.add_application(None).expect("second");
    *host.captured_events.lock().expect("events") = Some(VecDeque::new());
    for app in host.windows.values_mut() {
        let id = app.tabs.open_new(source.clone(), MediaKind::Image);
        app.path = Some(source.clone());
        app.media_kind = Some(MediaKind::Image);
        app.displayed_tab = Some(id);
        app.state = PlaybackState::Paused;
        app.image_view.zoom = ZoomMode::Custom(1.7);
        app.image_view.pan = (12.0, -8.0);
        app.edits
            .entry(id)
            .or_default()
            .push(EditOperation::FlipHorizontal, MediaKind::Image);
        app.export_paths.insert(id, root.join("export.bmp"));
        app.source_versions.insert(
            id,
            Some(FileOperationSource::capture(&source).expect("loaded version")),
        );
        app.closed_tabs
            .push_back(closed_tabs::ClosedTab::Media(source.clone(), 1));
    }
    let before: Vec<_> = host
        .windows
        .values()
        .map(|app| {
            (
                app.tabs.active_id(),
                app.tabs.tab_ids().collect::<Vec<_>>(),
                app.edits.clone(),
                app.image_view,
                app.media_generation,
                app.export_paths.clone(),
            )
        })
        .collect();
    choose(&mut host, owner, Kind::Rename, target.clone());
    assert!(!source.exists());
    assert_eq!(
        std::fs::read(&target).expect("renamed bytes"),
        b"owned bytes; UI state test"
    );
    for (app, (active, order, edits, view, instance, exports)) in host.windows.values().zip(&before)
    {
        assert_eq!(app.path.as_ref(), Some(&target));
        assert_eq!(
            app.source_versions.get(&active.expect("active")),
            Some(&Some(
                FileOperationSource::capture(&target).expect("moved version")
            ))
        );
        assert_eq!(app.tabs.active_id(), *active);
        assert_eq!(app.tabs.tab_ids().collect::<Vec<_>>(), *order);
        assert_eq!(app.edits, *edits);
        assert_eq!(app.image_view, *view);
        assert_eq!(app.export_paths, *exports);
        assert_ne!(app.media_generation, *instance);
        assert!(
            matches!(app.closed_tabs.back(), Some(closed_tabs::ClosedTab::Media(path, 1)) if path == &target)
        );
    }
    choose(&mut host, owner, Kind::Move, folder.clone());
    let moved = folder.join("renamed.bmp");
    assert!(!target.exists());
    assert!(
        host.windows
            .values()
            .all(|app| app.path.as_ref() == Some(&moved))
    );
    for app in host.windows.values() {
        assert_eq!(
            app.source_versions.get(&app.tabs.active_id().expect("tab")),
            Some(&Some(
                FileOperationSource::capture(&moved).expect("moved version")
            ))
        );
    }
    let occupied = folder.join("occupied.bmp");
    std::fs::write(&occupied, b"keep destination").expect("collision fixture");
    let instances: Vec<_> = host
        .windows
        .values()
        .map(|app| app.media_generation)
        .collect();
    choose(&mut host, owner, Kind::Rename, occupied.clone());
    assert_eq!(
        std::fs::read(&occupied).expect("destination"),
        b"keep destination"
    );
    assert_eq!(
        std::fs::read(&moved).expect("source remains"),
        b"owned bytes; UI state test"
    );
    for (app, instance) in host.windows.values().zip(instances) {
        assert_eq!(app.path.as_ref(), Some(&moved));
        assert_eq!(app.media_generation, instance);
        assert!(!app.file_operations.busy());
    }
    std::fs::write(
        &moved,
        b"externally changed bytes after the loaded document",
    )
    .expect("external change");
    choose(&mut host, owner, Kind::Rename, folder.join("external.bmp"));
    for app in host.windows.values() {
        assert_eq!(
            app.source_versions.get(&app.tabs.active_id().expect("tab")),
            Some(&None),
            "moving an externally changed file must not authorize it for existing edits"
        );
    }
}

#[test]
fn cancelled_or_stale_destination_never_submits_a_mutation() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::tests::cancelled_or_stale_destination_never_submits_a_mutation",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    std::fs::write(&source, b"untouched").expect("source");
    let mut app = Application::new(None, |_| {}).expect("app");
    let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
    app.path = Some(source.clone());
    app.media_kind = Some(MediaKind::Image);
    app.displayed_tab = Some(tab);
    for stale in [false, true] {
        app.file_operations.pending = Some(Pending {
            serial: 1,
            tab,
            path: source.clone(),
            origin_path: source.clone(),
            instance: app.media_generation,
            kind: Kind::Rename,
            source: None,
        });
        app.pending_dialog = Some(DialogIntent::RelocateFile);
        if stale {
            app.media_generation += 1;
        }
        app.finish_dialog(if stale {
            Ok(Some(root.join("wrong.bmp")))
        } else {
            Ok(None)
        });
        assert!(app.file_operations.pending.is_none() && app.file_operations.ready.is_none());
        assert_eq!(
            std::fs::read(&source).expect("source remains"),
            b"untouched"
        );
    }
}

pub(crate) fn exercise(host: &mut WindowHost) {
    let owner = *host.windows.keys().next().expect("source window");
    let source = host.windows[&owner].path.clone().expect("video source");
    let moved = source.with_file_name("relocated-video.mp4");
    // Earlier transfer checks can leave the first frame queued for presentation.
    // Establish an actual displayed reference before testing exact-frame recovery.
    for app in host.windows.values_mut() {
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.media_kind == Some(MediaKind::Video)
            && app
                .session
                .as_ref()
                .is_some_and(|session| session.current_video_time().is_none())
        {
            app.load_next_frame();
            app.render_frame();
            assert!(Instant::now() < deadline, "reference video frame");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let retained: Vec<_> = host
        .windows
        .iter()
        .flat_map(|(key, app)| {
            app.retained_playback
                .iter()
                .filter(|(_, saved)| saved.path == source)
                .map(|(id, saved)| {
                    (
                        *key,
                        *id,
                        saved.position(),
                        saved.state,
                        saved.view,
                        saved.instance,
                    )
                })
        })
        .collect();
    assert!(
        !retained.is_empty(),
        "real background source owners are required"
    );
    let before: Vec<_> = host
        .windows
        .iter()
        .map(|(key, app)| {
            (
                *key,
                app.current_position(),
                app.state,
                app.image_view,
                app.edits.clone(),
                app.session
                    .as_ref()
                    .and_then(PlaybackSession::current_video_time),
                app.media_generation,
            )
        })
        .collect();
    for target in [moved.clone(), source.clone()] {
        choose(host, owner, Kind::Rename, target.clone());
        let version =
            FileOperationSource::capture(&target).expect("relocated native source version");
        for (key, id, position, state, view, instance) in &retained {
            assert_eq!(
                host.windows[key].source_versions.get(id),
                Some(&Some(version.clone())),
                "retained native playback follows the verified source version"
            );
            let saved = &host.windows[key].retained_playback[id];
            assert_eq!(saved.path, target);
            assert_eq!(saved.state, *state);
            assert_eq!(saved.view, *view);
            assert_ne!(saved.instance, *instance);
            assert!(saved.recovery_position.is_none());
            if *state != PlaybackState::Playing {
                assert_eq!(saved.position(), *position);
            }
        }
        for (key, position, state, view, edits, frame, _) in &before {
            let app = host.windows.get_mut(key).expect("window");
            if app.path.as_ref() == Some(&target) && app.session.is_some() {
                assert_eq!(
                    app.source_versions
                        .get(&app.displayed_tab.expect("active playback tab")),
                    Some(&Some(version.clone())),
                    "active native playback retains its verified document version"
                );
            }
            assert_eq!(app.current_position(), *position);
            assert_eq!(app.state, *state);
            assert_eq!(app.image_view, *view);
            assert_eq!(app.edits, *edits);
            let deadline = Instant::now() + Duration::from_secs(5);
            while app
                .session
                .as_ref()
                .is_some_and(|session| session.current_video_time().is_none())
            {
                app.load_next_frame();
                app.render_frame();
                assert!(Instant::now() < deadline, "reopened video frame");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(
                app.session
                    .as_ref()
                    .and_then(PlaybackSession::current_video_time),
                *frame
            );
        }
    }
    for (key, _, _, _, _, _, old_instance) in before {
        let app = host.windows.get_mut(&key).expect("window");
        let duration = app.media_duration;
        app.handle_app_event(AppEvent::Duration(
            source.clone(),
            old_instance,
            Ok(Duration::from_secs(999)),
        ));
        assert_eq!(
            app.media_duration, duration,
            "pre-move metadata cannot affect the new owner"
        );
    }
    eprintln!(
        "PASS native file relocation: shared-device video owners release readers across rename round trips; active paused frames and edits/views, retained background positions/views, and fresh metadata ownership are verified"
    );
}

#[test]
fn inactive_relocation_keeps_displayed_owner_and_refreshes_the_current_folder() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::tests::inactive_relocation_keeps_displayed_owner_and_refreshes_the_current_folder",
    ) else {
        return;
    };
    let current = root.join("current.bmp");
    let source = root.join("inactive.bmp");
    let renamed = root.join("renamed.bmp");
    let destination = root.join("other");
    std::fs::create_dir(&destination).expect("owned destination");
    std::fs::write(&current, b"current document").expect("current");
    std::fs::write(&source, b"inactive document").expect("inactive");
    let mut host = WindowHost::new(None, None).expect("host");
    let owner = *host.windows.keys().next().expect("window");
    *host.captured_events.lock().expect("events") = Some(VecDeque::new());
    let app = host.windows.get_mut(&owner).expect("app");
    let active = app.tabs.open_new(current.clone(), MediaKind::Image);
    let background = app.tabs.open_new(source.clone(), MediaKind::Image);
    app.tabs.activate(active);
    app.path = Some(current.clone());
    app.displayed_tab = Some(active);
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    app.image_view.zoom = ZoomMode::Custom(2.0);
    app.edits
        .entry(active)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let history = app.edits.clone();
    let generation = app.media_generation;
    choose_at(
        &mut host,
        owner,
        Kind::Rename,
        source.clone(),
        renamed.clone(),
    );
    assert!(!source.exists() && renamed.exists());
    assert_eq!(
        host.windows[&owner]
            .tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == background)
            .expect("background")
            .target
            .current_path(),
        renamed
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    while !host.windows[&owner]
        .folder_snapshot
        .as_ref()
        .is_some_and(|s| s.items.iter().any(|item| item.path == renamed))
    {
        super::super::tests::drain_captured(&mut host);
        assert!(
            Instant::now() < deadline,
            "refreshed Shell order after inactive rename"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    choose_at(
        &mut host,
        owner,
        Kind::Move,
        renamed.clone(),
        destination.clone(),
    );
    assert!(!renamed.exists() && destination.join("renamed.bmp").exists());
    let deadline = Instant::now() + Duration::from_secs(8);
    while !host.windows[&owner]
        .folder_snapshot
        .as_ref()
        .is_some_and(|s| {
            s.items.iter().any(|item| item.path == current)
                && s.items.iter().all(|item| item.path != renamed)
        })
    {
        super::super::tests::drain_captured(&mut host);
        assert!(
            Instant::now() < deadline,
            "refreshed Shell order after inactive move"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let app = &host.windows[&owner];
    assert_eq!(app.tabs.active_id(), Some(active));
    assert_eq!(app.path.as_ref(), Some(&current));
    assert_eq!(app.media_generation, generation);
    assert_eq!(app.image_view.zoom, ZoomMode::Custom(2.0));
    assert_eq!(app.edits, history);
    assert_eq!(app.state, PlaybackState::Paused);
    assert!(!app.image_loading);
    assert_eq!(
        std::fs::read(current).expect("current preserved"),
        b"current document"
    );
}

#[test]
#[ignore = "recycles one owned unopened fixture through the host and native Shell"]
fn inactive_recycling_keeps_current_media_and_refreshes_filmstrip_membership() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::file_operations::tests::inactive_recycling_keeps_current_media_and_refreshes_filmstrip_membership",
    ) else {
        return;
    };
    let current = root.join("current.bmp");
    let source = root.join("unopened.bmp");
    std::fs::write(&current, b"current document").expect("current");
    std::fs::write(&source, b"owned recycled fixture").expect("unopened");
    let mut host = WindowHost::new(None, None).expect("host");
    let owner = *host.windows.keys().next().expect("window");
    *host.captured_events.lock().expect("events") = Some(VecDeque::new());
    // No confirmation window or persistent preference write in this owned trial.
    host.delete_confirmation_suppressed = true;
    let app = host.windows.get_mut(&owner).expect("app");
    let active = app.tabs.open_new(current.clone(), MediaKind::Image);
    app.path = Some(current.clone());
    app.displayed_tab = Some(active);
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    app.filmstrip_open = true;
    app.image_view.zoom = ZoomMode::Custom(2.0);
    app.edits
        .entry(active)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let history = app.edits.clone();
    let generation = app.media_generation;
    app.begin_file_relocation_at(Kind::Delete, source.clone());
    assert_eq!(
        app.file_operations.pending.as_ref().expect("request").path,
        source
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.windows[&owner].file_operations.ready.is_none() {
        super::super::tests::drain_captured(&mut host);
        assert!(Instant::now() < deadline, "source inspection");
        std::thread::sleep(Duration::from_millis(2));
    }
    host.start_pending_file_operation();
    assert!(host.file_operation.is_some());
    while host.file_operation.is_some()
        || !host.windows[&owner]
            .folder_snapshot
            .as_ref()
            .is_some_and(|snapshot| {
                snapshot.items.iter().any(|item| item.path == current)
                    && snapshot.items.iter().all(|item| item.path != source)
            })
    {
        super::super::tests::drain_captured(&mut host);
        assert!(
            Instant::now() < deadline,
            "native recycle and refreshed Shell listing"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let app = &host.windows[&owner];
    assert!(!source.exists());
    assert_eq!(app.tabs.active_id(), Some(active));
    assert_eq!(app.path.as_ref(), Some(&current));
    assert_eq!(app.media_generation, generation);
    assert_eq!(app.edits, history);
    assert_eq!(app.image_view.zoom, ZoomMode::Custom(2.0));
    assert!(app.filmstrip_open && !app.image_loading && !app.file_operations.busy());
    assert_eq!(
        std::fs::read(current).expect("current retained"),
        b"current document"
    );
    eprintln!(
        "PASS inactive recycle: exact owned target removed, current document/view/edits retained, real Shell listing refreshed; no dialog or physical input"
    );
}
