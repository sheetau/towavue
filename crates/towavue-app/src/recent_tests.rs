mod command_history;
mod removal;

use crate::*;
use menu::{OpenTarget, RecentAction};
use towavue_core::KeySequence;
use towavue_runtime_windows::{RecentFiles, RecentKind};

#[test]
fn hierarchy_picker_opens_a_real_nested_result_and_clears_work_on_close() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::hierarchy_picker_opens_a_real_nested_result_and_clears_work_on_close",
    ) else {
        return;
    };
    let first = root.join("first.bmp");
    let nested = root.join("nested");
    std::fs::create_dir(&nested).expect("nested directory");
    let target = nested.join("unique-result.bmp");
    for path in [&first, &target] {
        tab_transfer::tests::bitmap(path);
    }
    let mut app = Application::new(None, |_| {}).expect("app");
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    tab_transfer::tests::install(&mut app, first, tab_transfer::tests::decoded(false));
    app.dispatch(CommandId::GoToFile);
    let frame = |app: &mut Application<_>, events| {
        let mut actions = Vec::new();
        let _ = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |_| {
                app.draw_command_palette(&context, 0.0, &mut actions);
            },
        );
        for action in actions {
            app.handle_ui_action(action);
        }
    };
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    frame(&mut app, vec![egui::Event::Text("unique-result".into())]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.file_search.result().is_none() {
        assert!(Instant::now() < deadline, "background hierarchy result");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        app.file_search.result().expect("result").paths.as_slice(),
        std::slice::from_ref(&target)
    );
    frame(&mut app, vec![]);
    frame(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(app.path.as_ref(), Some(&target));
    assert!(!app.palette_open);
    assert!(
        app.file_search.result().is_none(),
        "opening retires the search"
    );
}

#[test]
fn picker_shortcuts_work_from_text_focus_without_stealing_composition_or_custom_bindings() {
    let Some(_) = tests::isolated_test_root(
        "recent_tests::picker_shortcuts_work_from_text_focus_without_stealing_composition_or_custom_bindings",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    let mut query = String::new();
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        ui.text_edit_singleline(&mut query).request_focus();
    });
    assert!(context.text_edit_focused());
    let stroke = |text: &str| text.parse::<KeySequence>().expect("key").strokes()[0].clone();
    for palette in [false, true] {
        app.palette_open = palette;
        for keys in ["Ctrl+P", "Ctrl+Shift+P", "Ctrl+F"] {
            assert!(app.owns_focused_shortcut(&stroke(keys)));
            app.native_ime_composing = true;
            assert!(!app.owns_focused_shortcut(&stroke(keys)));
            app.native_ime_composing = false;
            app.pending_guard = Some(GuardedAction::CloseTab(
                app.tabs.gallery().expect("Gallery"),
            ));
            assert!(!app.owns_focused_shortcut(&stroke(keys)));
            app.pending_guard = None;
        }
    }
    for kind in [
        None,
        Some(MediaKind::Image),
        Some(MediaKind::Video),
        Some(MediaKind::Audio),
    ] {
        app.media_kind = kind;
        app.palette_open = false;
        assert!(app.owns_focused_shortcut(&stroke("Ctrl+F")));
        app.process_shortcut(stroke("Ctrl+F"));
        assert!(
            app.palette_open,
            "folder picker opens from text focus for {kind:?}"
        );
    }
    app.media_kind = None;
    app.palette_open = false;
    app.shortcuts.set(
        CommandId::GoToFile,
        "Ctrl+Q".parse().expect("custom shortcut"),
    );
    assert!(!app.owns_focused_shortcut(&stroke("Ctrl+P")));
    assert!(app.owns_focused_shortcut(&stroke("Ctrl+Q")));
    assert!(!app.owns_focused_shortcut(&stroke("Q")));
    for command in [
        CommandId::GoToFile,
        CommandId::OpenRecentFolder,
        CommandId::ToggleCommandPalette,
    ] {
        app.shortcuts
            .set(command, "Ctrl+K P".parse().expect("picker chord"));
        assert!(app.owns_focused_shortcut(&stroke("Ctrl+K")));
        app.process_shortcut(stroke("Ctrl+K"));
        assert!(app.prefix_started.is_some());
        assert!(
            app.owns_focused_shortcut(&stroke("P")),
            "unmodified suffix belongs to the chord"
        );
        app.native_ime_composing = true;
        assert!(!app.owns_focused_shortcut(&stroke("P")));
        app.native_ime_composing = false;
        app.process_shortcut(stroke("P"));
        assert!(app.palette_open);
        assert!(app.entered_shortcut.is_empty());
        app.process_shortcut(stroke("Ctrl+K"));
        assert!(app.owns_focused_shortcut(&stroke("Escape")));
        app.process_shortcut(stroke("Escape"));
        assert!(app.entered_shortcut.is_empty());
        app.shortcuts.remove(command);
    }
}

#[test]
fn quick_open_dispatches_from_gallery_and_preserves_replacement_guards() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::quick_open_dispatches_from_gallery_and_preserves_replacement_guards",
    ) else {
        return;
    };
    let first = root.join("first.bmp");
    let second = root.join("second.bmp");
    for path in [&first, &second] {
        tab_transfer::tests::bitmap(path);
    }
    let mut app = Application::new(None, |_| {}).expect("app");
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    let tab =
        tab_transfer::tests::install(&mut app, first.clone(), tab_transfer::tests::decoded(false));
    app.push_visual_edit(EditOperation::RotateClockwise);
    let edits = app.edits[&tab].clone();
    let gallery = app.tabs.gallery().expect("Gallery");
    app.activate_tab(gallery);
    app.recent_paths = vec![first.clone()];
    let frame = |app: &mut Application<_>, events| {
        let mut actions = Vec::new();
        let _ = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |_| {
                if app.palette_open {
                    app.draw_command_palette(&context, 0.0, &mut actions);
                }
            },
        );
        for action in actions {
            app.handle_ui_action(action);
        }
    };
    let enter = |modifiers| {
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }]
    };
    app.process_shortcut("Ctrl+P".parse::<KeySequence>().expect("binding").strokes()[0].clone());
    assert!(app.palette_open);
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    frame(&mut app, enter(egui::Modifiers::NONE));
    assert_eq!(app.tabs.active_id(), Some(tab));
    assert!(!app.palette_open);
    assert_eq!(app.edits[&tab], edits);
    app.recent_paths = vec![second.clone()];
    app.dispatch(CommandId::GoToFile);
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    frame(&mut app, enter(egui::Modifiers::ALT));
    assert!(!app.palette_open);
    assert!(matches!(&app.pending_guard, Some(GuardedAction::View(path)) if path == &second));
    app.dispatch(CommandId::OpenRecentFolder);
    assert!(!app.palette_open, "pending guard blocks other pickers");
    app.resolve_guard(GuardDecision::Cancel);
    assert_eq!(app.path.as_ref(), Some(&first));
    assert_eq!(app.edits[&tab], edits);
    app.activate_tab(gallery);
    app.recent_folders = vec![root.clone()];
    app.process_shortcut(
        "Ctrl+F"
            .parse::<KeySequence>()
            .expect("folder binding")
            .strokes()[0]
            .clone(),
    );
    assert!(app.palette_open);
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    frame(&mut app, enter(egui::Modifiers::CTRL));
    assert_eq!(app.pending_window_launches, [root]);
    assert_eq!(app.tabs.active_id(), Some(gallery));
    assert_eq!(app.edits[&tab], edits);
    let gallery_position = app
        .tabs
        .tab_ids()
        .position(|id| id == gallery)
        .expect("Gallery slot");
    app.recent_paths = vec![second.clone()];
    app.dispatch(CommandId::GoToFile);
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    frame(&mut app, enter(egui::Modifiers::ALT));
    let replacement = app.tabs.active_id().expect("replacement");
    assert_ne!(replacement, tab);
    assert!(app.tabs.gallery().is_none());
    assert_eq!(
        app.tabs.tab_ids().position(|id| id == replacement),
        Some(gallery_position)
    );
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.path.as_ref(), Some(&second));
    assert_eq!(app.edits[&tab], edits);
    assert!(app.pending_guard.is_none());
}

#[test]
fn recent_targets_preserve_tabs_and_guard_file_and_folder_replacement() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::recent_targets_preserve_tabs_and_guard_file_and_folder_replacement",
    ) else {
        return;
    };
    let first = root.join("first.bmp");
    let second = root.join("second.bmp");
    let folder = root.join("folder.mp4");
    std::fs::create_dir(&folder).expect("owned folder");
    let third = folder.join("third.bmp");
    for path in [&first, &second, &third] {
        tab_transfer::tests::bitmap(path);
    }
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(fonts::test_context());
    let tab =
        tab_transfer::tests::install(&mut app, first.clone(), tab_transfer::tests::decoded(false));
    app.push_visual_edit(EditOperation::RotateClockwise);
    let edits = app.edits[&tab].clone();
    let generation = app.media_generation;
    app.handle_recent_action(RecentAction::Open(
        first.clone(),
        RecentKind::File,
        OpenTarget::Tab,
    ));
    assert_eq!(
        app.media_generation, generation,
        "already-active tab is not reloaded"
    );
    assert_eq!(app.edits[&tab], edits);
    app.activate_tab(app.tabs.gallery().expect("Gallery"));
    app.handle_recent_action(RecentAction::Open(
        first.clone(),
        RecentKind::File,
        OpenTarget::Tab,
    ));
    assert_eq!(app.tabs.active_id(), Some(tab));
    assert_eq!(app.edits[&tab], edits);
    app.handle_recent_action(RecentAction::Open(
        first.clone(),
        RecentKind::File,
        OpenTarget::Window,
    ));
    assert_eq!(app.pending_window_launches, std::slice::from_ref(&first));
    assert_eq!(app.edits[&tab], edits);
    app.pending_window_launches.clear();
    let moved = root.join("temporarily-moved.bmp");
    std::fs::rename(&first, &moved).expect("move owned source");
    app.activate_tab(app.tabs.gallery().expect("Gallery"));
    app.handle_recent_action(RecentAction::Open(
        first.clone(),
        RecentKind::File,
        OpenTarget::Tab,
    ));
    assert_eq!(
        app.tabs.active_id(),
        Some(tab),
        "existing tab does not require its source to exist"
    );
    assert_eq!(app.edits[&tab], edits);
    std::fs::rename(moved, &first).expect("restore owned source");
    app.handle_recent_action(RecentAction::Open(
        second.clone(),
        RecentKind::File,
        OpenTarget::Replace,
    ));
    assert!(matches!(&app.pending_guard, Some(GuardedAction::View(path)) if path == &second));
    app.resolve_guard(GuardDecision::Cancel);
    assert_eq!(app.path.as_ref(), Some(&first));
    assert_eq!(app.edits[&tab], edits);
    app.handle_recent_action(RecentAction::Open(
        second.clone(),
        RecentKind::File,
        OpenTarget::Replace,
    ));
    app.resolve_guard(GuardDecision::Discard);
    assert_eq!(app.tabs.active_id(), Some(tab));
    assert_eq!(app.path.as_ref(), Some(&second));
    app.edits
        .get_mut(&tab)
        .expect("history")
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    assert!(app.edits[&tab].is_dirty());
    let edits = app.edits[&tab].clone();
    app.handle_recent_action(RecentAction::Open(
        folder.clone(),
        RecentKind::Folder,
        OpenTarget::Replace,
    ));
    app.media_generation = app.media_generation.wrapping_add(1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.pending_folder.is_some() {
        app.finish_folder_load();
        assert!(Instant::now() < deadline, "stale folder delivery");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        app.pending_guard.is_none(),
        "stale replacement cannot prompt for another media generation"
    );
    assert_eq!(app.path.as_ref(), Some(&second));
    assert_eq!(app.edits[&tab], edits);
    for discard in [false, true] {
        app.handle_recent_action(RecentAction::Open(
            folder.clone(),
            RecentKind::Folder,
            OpenTarget::Replace,
        ));
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.pending_folder.is_some() {
            app.finish_folder_load();
            assert!(Instant::now() < deadline, "folder delivery");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            matches!(&app.pending_guard, Some(GuardedAction::NavigateFromFolder(path, origin))
            if path == &third && origin == &folder)
        );
        app.resolve_guard(if discard {
            GuardDecision::Discard
        } else {
            GuardDecision::Cancel
        });
        if !discard {
            assert_eq!(app.path.as_ref(), Some(&second));
            assert_eq!(app.edits[&tab], edits);
        }
    }
    assert_eq!(app.path.as_ref(), Some(&third));
    assert_eq!(app.tabs.active_id(), Some(tab));
    app.activate_tab(app.tabs.gallery().expect("Gallery"));
    app.handle_recent_action(RecentAction::Open(
        first.clone(),
        RecentKind::File,
        OpenTarget::Replace,
    ));
    assert_ne!(
        app.tabs.active_id(),
        Some(tab),
        "replacing Gallery does not overwrite another media tab"
    );
    assert_eq!(app.path.as_ref(), Some(&first));
    assert!(
        app.tabs.gallery().is_none(),
        "Gallery becomes the media tab"
    );
}

#[test]
fn recent_delivery_projects_parent_folders_without_persisting_them_and_clear_persists() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::recent_delivery_projects_parent_folders_without_persisting_them_and_clear_persists",
    ) else {
        return;
    };
    let file = root.join("file.bmp");
    let folder = root.join("folder.bmp");
    tab_transfer::tests::bitmap(&file);
    std::fs::create_dir(&folder).expect("owned folder");
    let history = root.join("recent.txt");
    let mut app = Application::new(None, |_| {}).expect("app");
    app.recent_files = Some(RecentFiles::new(history.clone(), || {}).expect("worker"));
    let recent = app.recent_files.as_ref().expect("worker");
    recent.record(file.clone());
    recent.record_folder(folder.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.recent_paths.is_empty() || !app.recent_folders.contains(&folder) {
        app.handle_app_event(AppEvent::RecentFilesReady);
        assert!(Instant::now() < deadline, "recent delivery");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(app.recent_paths, std::slice::from_ref(&file));
    assert_eq!(app.recent_folders, vec![folder.clone(), root.clone()]);
    let persisted = std::fs::read_to_string(&history).expect("stored history");
    assert_eq!(
        persisted
            .lines()
            .filter(|line| line.starts_with("D\t"))
            .count(),
        1,
        "derived parents are not extra folder records"
    );
    assert!(app.recent_months.contains_key(&file));
    assert!(!app.recent_months.contains_key(&folder));
    app.handle_recent_action(RecentAction::Clear);
    assert!(
        app.recent_paths.is_empty()
            && app.recent_folders.is_empty()
            && app.recent_months.is_empty()
    );
    drop(app.recent_files.take());
    app.recent_files = Some(RecentFiles::new(history, || {}).expect("reopened worker"));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(update) = app.recent_files.as_ref().expect("worker").take_completed() {
            assert!(update.entries.is_empty() && update.error.is_none());
            break;
        }
        assert!(Instant::now() < deadline, "cleared delivery");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        file.is_file() && folder.is_dir(),
        "clearing history leaves source media alone"
    );
}

#[test]
fn filmstrip_window_click_preserves_dirty_origin_and_rejects_blocked_or_stale_targets() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::filmstrip_window_click_preserves_dirty_origin_and_rejects_blocked_or_stale_targets",
    ) else {
        return;
    };
    let first = root.join("first.bmp");
    let second = root.join("second.bmp");
    for path in [&first, &second] {
        tab_transfer::tests::bitmap(path);
    }
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(fonts::test_context());
    let tab =
        tab_transfer::tests::install(&mut app, first.clone(), tab_transfer::tests::decoded(false));
    app.push_visual_edit(EditOperation::RotateClockwise);
    let edits = app.edits[&tab].clone();
    app.folder_snapshot = Some(towavue_core::FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(Vec::new()),
        folder_path: root.clone(),
        items: [&first, &second]
            .into_iter()
            .map(|path| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(Vec::new()),
                path: path.clone(),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: Vec::new(),
        source: towavue_core::FolderSnapshotSource::PersistedShellView,
        generation: 1,
        captured_at: std::time::SystemTime::now(),
    });
    for (strip, palette, grid, valid) in [
        (false, false, false, true),
        (true, true, false, true),
        (true, false, true, true),
        (true, false, false, false),
        (true, false, false, true),
    ] {
        app.filmstrip_open = strip;
        app.palette_open = palette;
        app.grid_open = grid;
        app.handle_ui_action(UiAction::OpenFilmstripWindow(if valid {
            second.clone()
        } else {
            root.join("stale.bmp")
        }));
        let allowed = strip && !palette && !grid && valid;
        assert_eq!(app.pending_window_launches.len(), usize::from(allowed));
        if allowed {
            assert_eq!(app.pending_window_launches, std::slice::from_ref(&second));
        }
        assert_eq!(app.tabs.active_id(), Some(tab));
        assert_eq!(app.path.as_ref(), Some(&first));
        assert_eq!(app.edits[&tab], edits);
        assert_eq!(app.filmstrip_open, strip);
        assert!(app.pending_guard.is_none());
        app.pending_window_launches.clear();
    }
}

#[test]
fn gallery_replacement_keeps_its_slot_and_other_dirty_tabs_and_rejects_invalid_paths() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::gallery_replacement_keeps_its_slot_and_other_dirty_tabs_and_rejects_invalid_paths",
    ) else {
        return;
    };
    let source = root.join("first.bmp");
    tab_transfer::tests::bitmap(&source);
    let unsupported = root.join("unsupported.txt");
    std::fs::write(&unsupported, b"not media").expect("fixture");
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(fonts::test_context());
    let original = tab_transfer::tests::install(
        &mut app,
        source.clone(),
        tab_transfer::tests::decoded(false),
    );
    app.push_visual_edit(EditOperation::RotateClockwise);
    let history = app.edits[&original].clone();
    app.dispatch(CommandId::OpenKeyboardSettings);
    let settings = app.tabs.keyboard_settings().expect("settings");
    let gallery = app.tabs.gallery().expect("Gallery");
    assert!(app.tabs.reorder(gallery, 2));
    app.activate_tab(gallery);
    app.gallery_search = "first".into();
    app.gallery_filter = Some(MediaKind::Image);
    let before: Vec<_> = app.tabs.tab_ids().collect();
    assert_eq!(before, [original, gallery, settings]);
    for target in [unsupported, root.join("missing.bmp")] {
        app.handle_recent_action(RecentAction::Open(
            target,
            RecentKind::File,
            OpenTarget::Replace,
        ));
        assert_eq!(app.tabs.tab_ids().collect::<Vec<_>>(), before);
        assert_eq!(app.tabs.active_id(), Some(gallery));
        assert_eq!(app.gallery_search, "first");
        assert_eq!(app.gallery_filter, Some(MediaKind::Image));
        assert_eq!(app.edits[&original], history);
    }
    // Replacement is explicit: a matching open source must not redirect to its dirty owner.
    app.handle_recent_action(RecentAction::Open(
        source.clone(),
        RecentKind::File,
        OpenTarget::Replace,
    ));
    let replacement = app.tabs.active_id().expect("media tab");
    assert_ne!(replacement, original);
    assert_ne!(replacement, gallery);
    assert_eq!(
        app.tabs.tab_ids().collect::<Vec<_>>(),
        [original, replacement, settings]
    );
    assert!(app.tabs.gallery().is_none());
    assert_eq!(app.path.as_ref(), Some(&source));
    assert_eq!(app.displayed_tab, Some(replacement));
    assert_eq!(app.edits[&original], history);
    assert!(!app.edits[&replacement].is_dirty());
    assert!(app.retained_images.contains_key(&original));
    assert!(
        app.closed_tabs.is_empty(),
        "conversion is not a user tab close"
    );
    assert!(app.gallery_search.is_empty() && app.gallery_filter.is_none());
    assert!(!app.exit_requested);
    app.close_tab_unchecked(replacement);
    assert_eq!(app.edits[&original], history);
    app.dispatch(CommandId::OpenGallery);
    assert!(app.tabs.gallery().is_some());
}

#[test]
fn gallery_folder_replacement_checks_its_origin_and_keeps_the_slot() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::gallery_folder_replacement_checks_its_origin_and_keeps_the_slot",
    ) else {
        return;
    };
    let path = root.join("only.bmp");
    tab_transfer::tests::bitmap(&path);
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(fonts::test_context());
    let wait = |app: &mut Application<_>| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.pending_folder.is_some() {
            app.finish_folder_load();
            assert!(Instant::now() < deadline, "folder completion");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    let gallery = app.tabs.gallery().expect("Gallery");
    app.handle_recent_action(RecentAction::Open(
        root.clone(),
        RecentKind::Folder,
        OpenTarget::Replace,
    ));
    app.dispatch(CommandId::OpenKeyboardSettings);
    let settings = app.tabs.keyboard_settings().expect("settings");
    wait(&mut app);
    assert_eq!(app.tabs.active_id(), Some(settings));
    assert_eq!(app.tabs.gallery(), Some(gallery));
    assert!(
        app.tabs.tabs().is_empty(),
        "late folder result cannot replace a utility tab"
    );
    app.activate_tab(gallery);
    app.handle_recent_action(RecentAction::Open(
        root,
        RecentKind::Folder,
        OpenTarget::Replace,
    ));
    wait(&mut app);
    let media = app.tabs.active_id().expect("media");
    assert_eq!(app.tabs.tab_ids().collect::<Vec<_>>(), [media, settings]);
    assert!(app.tabs.gallery().is_none());
    assert_eq!(app.path.as_ref(), Some(&path));
    assert!(!app.exit_requested);
}

#[test]
fn gallery_replacement_does_not_reuse_an_unopened_audio_playlist() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::gallery_replacement_does_not_reuse_an_unopened_audio_playlist",
    ) else {
        return;
    };
    let path = root.join("silence.wav");
    let mut wav = Vec::from(&b"RIFF"[..]);
    wav.extend_from_slice(&164_u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    wav.extend_from_slice(&8_000_u32.to_le_bytes());
    wav.extend_from_slice(&16_000_u32.to_le_bytes());
    wav.extend_from_slice(b"\x02\0\x10\0data");
    wav.extend_from_slice(&128_u32.to_le_bytes());
    wav.resize(172, 0);
    std::fs::write(&path, wav).expect("silent PCM");
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(fonts::test_context());
    app.recent_paths = vec![path.clone()];
    app.handle_ui_action(UiAction::OpenGalleryBackground(path.clone()));
    let background = app.tabs.tabs()[0].id;
    let target = app.tabs.tabs()[0].target.clone();
    app.handle_ui_action(UiAction::Recent(RecentAction::Open(
        path.clone(),
        RecentKind::File,
        OpenTarget::Replace,
    )));
    let active = app.tabs.active_id().expect("replacement");
    assert_ne!(active, background);
    assert_eq!(app.tabs.tab_ids().collect::<Vec<_>>(), [active, background]);
    assert!(app.tabs.gallery().is_none());
    assert_eq!(
        app.tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == background)
            .expect("background")
            .target,
        target
    );
    assert_eq!(app.path.as_ref(), Some(&path));
    assert_eq!(app.media_kind, Some(MediaKind::Audio));
}
