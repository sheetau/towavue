use super::*;

#[test]
fn transport_routes_current_tabs_and_preserves_unsaved_guards() {
    let Some(root) = crate::tests::isolated_test_root(
        "taskbar::tests::transport_routes_current_tabs_and_preserves_unsaved_guards",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let paths = [root.join("a.mp4"), root.join("b.mp4"), root.join("c.mp4")];
    let tab = app.tabs.open_new(paths[1].clone(), MediaKind::Video);
    app.path = Some(paths[1].clone());
    app.media_kind = Some(MediaKind::Video);
    app.state = PlaybackState::Paused;
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![0]),
        folder_path: root,
        items: paths
            .iter()
            .map(|path| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(
                    path.to_string_lossy().as_bytes().to_vec(),
                ),
                path: path.clone(),
                kind: MediaKind::Video,
            })
            .collect(),
        sort_columns: vec![],
        source: FolderSnapshotSource::PersistedShellView,
        generation: 1,
        captured_at: std::time::SystemTime::now(),
    });
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Video);
    app.sync_taskbar_transport();
    let revision = app.taskbar_ui.revision;
    for (action, target) in [
        (TaskbarAction::Previous, &paths[0]),
        (TaskbarAction::Next, &paths[2]),
    ] {
        app.handle_app_event(AppEvent::TaskbarClick(revision, action));
        assert!(
            matches!(&app.pending_guard, Some(GuardedAction::Navigate(path)) if path == target)
        );
        assert_eq!(app.path.as_ref(), Some(&paths[1]));
        assert!(!app.taskbar_transport().expect("transport").play_pause);
        app.resolve_guard(GuardDecision::Cancel);
    }
    app.media_generation += 1;
    app.handle_app_event(AppEvent::TaskbarClick(revision, TaskbarAction::Next));
    assert!(
        app.pending_guard.is_none(),
        "reject changed media even before next sync"
    );
    app.sync_taskbar_transport();
    app.handle_app_event(AppEvent::TaskbarClick(revision, TaskbarAction::Next));
    assert!(app.pending_guard.is_none());
    let revision = app.taskbar_ui.revision;
    app.tabs.open_new(paths[0].clone(), MediaKind::Video);
    app.handle_app_event(AppEvent::TaskbarClick(revision, TaskbarAction::Next));
    assert!(
        app.pending_guard.is_none(),
        "queued click must not follow tab activation"
    );
    app.sync_taskbar_transport();
    app.tabs.activate(tab);
    app.sync_taskbar_transport();
    app.handle_app_event(AppEvent::TaskbarClick(revision, TaskbarAction::Next));
    assert!(
        app.pending_guard.is_none(),
        "A to B to A rejects the old revision"
    );
    for kind in [Some(MediaKind::Video), Some(MediaKind::Audio)] {
        app.media_kind = kind;
        for state in [
            PlaybackState::Playing,
            PlaybackState::Paused,
            PlaybackState::Ended,
            PlaybackState::Loading,
            PlaybackState::Faulted,
        ] {
            app.state = state;
            let transport = app.taskbar_transport().expect("transport");
            assert_eq!(transport.play_pause, state.after_play_pause().is_some());
            assert_eq!(transport.playing, state == PlaybackState::Playing);
        }
    }
    for kind in [Some(MediaKind::Image), None] {
        app.media_kind = kind;
        assert!(app.taskbar_transport().is_none());
    }
}

#[test]
fn bundled_transport_icons_keep_alpha_contrast_and_distinct_shapes_at_each_density() {
    for density in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let side = (32.0 * density) as u32;
        let images = icon_pixels(side);
        for (index, pixels) in images.iter().enumerate() {
            assert_eq!(pixels.len(), (side * side * 4) as usize);
            assert!(
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] == 255 && p[3] == 255),
                "white fill {index}"
            );
            assert!(
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] == 0 && p[3] > 0),
                "black outline {index}"
            );
            assert!(
                pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0),
                "transparent background"
            );
            assert!(
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[0] == p[1] && p[1] == p[2] && p[0] <= p[3])
            );
            for other in &images[index + 1..] {
                assert_ne!(pixels, other);
            }
        }
        TaskbarIcons::new(side, images.each_ref().map(|image| image.as_slice()))
            .expect("native icons");
    }
}

pub(crate) fn playback_round_trip<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
) {
    app.sync_taskbar_transport();
    let revision = app.taskbar_ui.revision;
    let generation = app.generation;
    assert_eq!(app.state, PlaybackState::Playing);
    app.handle_app_event(AppEvent::TaskbarClick(revision, TaskbarAction::PlayPause));
    assert_eq!(app.state, PlaybackState::Paused);
    app.handle_app_event(AppEvent::TaskbarClick(revision, TaskbarAction::PlayPause));
    assert_eq!(app.state, PlaybackState::Playing);
    assert_eq!(
        app.generation, generation,
        "pause does not restart the media"
    );
}
