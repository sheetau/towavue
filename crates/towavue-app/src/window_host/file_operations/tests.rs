use super::*;
use crate::file_operations::{Kind, Pending};
use std::sync::mpsc;

fn choose(host: &mut WindowHost, owner: WindowKey, kind: Kind, target: PathBuf) {
    let app = host.windows.get_mut(&owner).expect("owner");
    let path = app.path.clone().expect("source path");
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
        for (key, id, position, state, view, instance) in &retained {
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
