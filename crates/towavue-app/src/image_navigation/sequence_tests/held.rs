use super::*;

fn start_at(app: &mut App, context: &egui::Context, path: &Path) {
    let decoded = app
        .image
        .as_ref()
        .expect("initial original")
        .decoded
        .clone();
    app.image = Some(ImagePresentation::from_decoded(context, path, decoded).expect("original"));
    app.path = Some(path.to_path_buf());
    app.tabs
        .active_mut()
        .expect("tab")
        .target
        .set_current_path(path.to_path_buf(), MediaKind::Image);
}

fn repeat(app: &mut App, key: &str, count: usize) {
    for _ in 0..count {
        app.repeat_media_shortcut(key.parse().expect("image key"));
    }
    app.image_generation = app.image_loader.request(Vec::new());
}

fn present(app: &mut App, context: &egui::Context) {
    app.image_generation = app.image_loader.request(Vec::new());
    complete(app);
    let token = draw(app, context).expect("full original submission");
    app.finish_image_sequence_frame(Some(token));
    app.image_generation = app.image_loader.request(Vec::new());
    app.folder_order.request(None);
}

#[test]
fn release_keeps_latest_position_from_middle_and_preserves_later_single_presses() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::held::release_keeps_latest_position_from_middle_and_preserves_later_single_presses",
    ) else {
        return;
    };
    let (mut app, context, paths) = fixture(&root);
    start_at(&mut app, &context, &paths[80]);
    app.edits
        .insert(app.tabs.active().expect("tab").id, EditHistory::default());
    let history = app.edits.clone();
    app.process_shortcut("Right".parse().expect("press"));
    repeat(&mut app, "Right", 3);
    app.process_shortcut("Left".parse().expect("single press"));
    repeat(&mut app, "Right", 2);
    app.release_image_repeats(&Key::ArrowRight, false);
    app.image_generation = app.image_loader.request(Vec::new());
    assert_eq!(app.path.as_ref(), Some(&paths[84]));
    assert_eq!(app.image_sequence.steps.len(), 3);
    assert!(
        app.image_handoff.is_some(),
        "retain the previously displayed original while loading"
    );
    assert_eq!(
        draw(&mut app, &context),
        None,
        "a held original is not the new destination"
    );
    present(&mut app, &context);
    assert_eq!(
        app.path.as_ref(),
        Some(&paths[83]),
        "the explicit Left press is not discarded"
    );
    present(&mut app, &context);
    assert_eq!(
        app.path.as_ref(),
        Some(&paths[85]),
        "the later released run takes one final hop"
    );
    present(&mut app, &context);
    assert!(app.image_sequence.awaiting.is_none() && app.image_sequence.steps.is_empty());
    assert_eq!(app.edits, history);
    for _ in 0..4 {
        let token = draw(&mut app, &context);
        app.finish_image_sequence_frame(token);
    }
    assert_eq!(
        app.path.as_ref(),
        Some(&paths[85]),
        "no post-release replay"
    );
}

#[test]
fn stalled_loading_folds_repeats_but_ready_originals_keep_every_presentation() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::held::stalled_loading_folds_repeats_but_ready_originals_keep_every_presentation",
    ) else {
        return;
    };
    for ready in [false, true] {
        let (mut app, context, paths) = fixture(&root);
        start_at(&mut app, &context, &paths[50]);
        app.process_shortcut("Right".parse().expect("press"));
        let before = Instant::now();
        repeat(&mut app, "Right", 9);
        assert!(!app.coalesce_image_repeats(before + Duration::from_millis(149)));
        if ready {
            complete(&mut app);
        }
        let folded = app.coalesce_image_repeats(Instant::now() + Duration::from_secs(1));
        assert_eq!(folded, !ready);
        if ready {
            for path in &paths[51..=60] {
                assert_eq!(app.path.as_ref(), Some(path));
                present(&mut app, &context);
            }
        } else {
            assert_eq!(app.path.as_ref(), Some(&paths[60]));
            assert!(app.image_sequence.steps.is_empty());
            assert!(app.image_handoff.is_some());
            present(&mut app, &context);
        }
        app.release_image_repeats(&Key::ArrowRight, false);
        assert!(app.image_sequence.awaiting.is_none() && app.image_sequence.steps.is_empty());
    }
}

#[test]
fn release_respects_folder_boundaries_and_waits_for_unknown_shell_order() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::held::release_respects_folder_boundaries_and_waits_for_unknown_shell_order",
    ) else {
        return;
    };
    for wrap in [false, true] {
        for forward in [false, true] {
            let (mut app, context, paths) = fixture(&root);
            let (index, key, identity, target) = if forward {
                (95, "Right", Key::ArrowRight, if wrap { 6 } else { 99 })
            } else {
                (4, "Left", Key::ArrowLeft, if wrap { 93 } else { 0 })
            };
            start_at(&mut app, &context, &paths[index]);
            app.folder_navigation_loop = wrap;
            app.process_shortcut(key.parse().expect("press"));
            repeat(&mut app, key, 10);
            let snapshot = app.folder_snapshot.take();
            app.release_image_repeats(&identity, false);
            assert_eq!(
                app.image_sequence.steps.len(),
                10,
                "cannot invent missing Shell order"
            );
            app.folder_snapshot = snapshot;
            present(&mut app, &context);
            assert_eq!(app.path.as_ref(), Some(&paths[target]));
            present(&mut app, &context);
            assert!(app.image_sequence.awaiting.is_none() && app.image_sequence.steps.is_empty());
        }
    }
}

#[test]
fn synthetic_release_focus_loss_and_modal_input_do_not_finish_cancelled_holds() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::sequence_tests::held::synthetic_release_focus_loss_and_modal_input_do_not_finish_cancelled_holds",
    ) else {
        return;
    };
    for cancel in 0..4 {
        let (mut app, context, paths) = fixture(&root);
        start_at(&mut app, &context, &paths[60]);
        app.process_shortcut("Right".parse().expect("press"));
        repeat(&mut app, "Right", 8);
        app.process_shortcut("Left".parse().expect("single press"));
        match cancel {
            0 => app.release_image_repeats(&Key::ArrowRight, true),
            1 => app.cancel_image_repeats(),
            2 => {
                app.pending_guard = Some(GuardedAction::Exit);
                app.release_image_repeats(&Key::ArrowRight, false);
                app.pending_guard = None;
            }
            _ => {
                app.filmstrip_open = true;
                app.release_image_repeats(&Key::ArrowRight, false);
                app.filmstrip_open = false;
            }
        }
        assert_eq!(app.path.as_ref(), Some(&paths[61]));
        assert_eq!(app.image_sequence.steps.len(), 1, "keep the ordinary press");
        present(&mut app, &context);
        assert_eq!(app.path.as_ref(), Some(&paths[60]));
        present(&mut app, &context);
        assert!(app.image_sequence.steps.is_empty());
    }
}

// The native harness owns these generated two-pixel BMPs and a hidden GPU window.
// Input is scripted; this is an original/Present control, not physical key timing.
pub(super) fn native_release_trial<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    paths: &[PathBuf],
) {
    fn settle<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.image_loading || app.image_sequence.awaiting.is_some() {
            assert!(Instant::now() < deadline, "held original/Present timeout");
            app.render_frame();
            assert!(app.image_error.is_none() && app.playback_error.is_none());
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let sources: Vec<_> = paths
        .iter()
        .map(|path| std::fs::read(path).expect("source"))
        .collect();
    settle(app);
    for start in [50, 80, 95] {
        app.request_guarded(GuardedAction::Navigate(paths[start].clone()));
        settle(app);
        let prior = app.image.as_ref().expect("prior original").decoded.clone();
        app.process_shortcut("Right".parse().expect("press"));
        for _ in 0..12 {
            app.repeat_media_shortcut("Right".parse().expect("repeat"));
        }
        app.release_image_repeats(&Key::ArrowRight, false);
        let destination = (start + 13) % paths.len();
        assert_eq!(app.path.as_ref(), Some(&paths[destination]));
        assert!(app.image_sequence.steps.is_empty());
        assert!(
            app.image_handoff.is_some(),
            "no blank/preview substitution during replacement"
        );
        assert_eq!(
            prior.frames[0].rgba,
            [56, start as u8, 12, 255, 56, start as u8, 12, 255]
        );
        settle(app);
        let original = app.image.as_ref().expect("final original");
        assert_eq!(original.dimensions(), (2, 1));
        assert_eq!(
            original.decoded.frames[0].rgba,
            [
                56,
                destination as u8,
                12,
                255,
                56,
                destination as u8,
                12,
                255
            ]
        );
        assert!(app.image_handoff.is_none());
        for _ in 0..4 {
            app.render_frame();
            assert_eq!(app.path.as_ref(), Some(&paths[destination]));
            assert!(app.image_sequence.awaiting.is_none());
        }
    }
    // A ready original retains its presentation even when more repeats are queued.
    app.request_guarded(GuardedAction::Navigate(paths[50].clone()));
    settle(app);
    app.process_shortcut("Right".parse().expect("press"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.image_loading {
        assert!(Instant::now() < deadline, "ready-original timeout");
        app.finish_image_load();
        std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..5 {
        app.repeat_media_shortcut("Right".parse().expect("repeat"));
    }
    for path in &paths[51..=56] {
        assert_eq!(app.path.as_ref(), Some(path));
        let generation = app.media_generation;
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.image_sequence.awaiting == Some(generation) {
            assert!(Instant::now() < deadline, "ready presentation timeout");
            app.render_frame();
            assert!(app.image_error.is_none());
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    app.release_image_repeats(&Key::ArrowRight, false);
    assert!(app.image_sequence.steps.is_empty());
    for (path, original) in paths.iter().zip(sources) {
        assert_eq!(std::fs::read(path).expect("unchanged source"), original);
    }
    eprintln!(
        "PASS held navigation: three middle/late release destinations, exact full originals, no replay, six ordered ready presentations; generated BMPs, real workers, hidden GPU/Present, scripted input"
    );
}
