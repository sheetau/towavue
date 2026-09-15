use crate::*;
use menu::{OpenTarget, RecentAction};
use towavue_core::KeySequence;
use towavue_runtime_windows::{RecentFiles, RecentKind};

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
        for keys in ["Ctrl+P", "Ctrl+Shift+P", "Ctrl+Alt+O"] {
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
    app.shortcuts.set(
        CommandId::GoToFile,
        "Ctrl+Q".parse().expect("custom shortcut"),
    );
    assert!(!app.owns_focused_shortcut(&stroke("Ctrl+P")));
    assert!(app.owns_focused_shortcut(&stroke("Ctrl+Q")));
    assert!(!app.owns_focused_shortcut(&stroke("Q")));
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
    assert!(matches!(&app.pending_guard, Some(GuardedAction::Navigate(path)) if path == &second));
    app.dispatch(CommandId::OpenRecentFolder);
    assert!(!app.palette_open, "pending guard blocks other pickers");
    app.resolve_guard(GuardDecision::Cancel);
    assert_eq!(app.path.as_ref(), Some(&first));
    assert_eq!(app.edits[&tab], edits);
    app.activate_tab(gallery);
    app.recent_folders = vec![root.clone()];
    app.process_shortcut(
        "Ctrl+Alt+O"
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
    assert!(matches!(&app.pending_guard, Some(GuardedAction::Navigate(path)) if path == &second));
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
        "Gallery is not overwritten by a media path"
    );
    assert_eq!(app.path.as_ref(), Some(&first));
    assert!(app.tabs.gallery().is_some());
}

#[test]
fn recent_delivery_separates_gallery_files_from_explicit_folders_and_clear_persists() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::recent_delivery_separates_gallery_files_from_explicit_folders_and_clear_persists",
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
    while app.recent_paths.is_empty() || app.recent_folders.is_empty() {
        app.handle_app_event(AppEvent::RecentFilesReady);
        assert!(Instant::now() < deadline, "recent delivery");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(app.recent_paths, std::slice::from_ref(&file));
    assert_eq!(app.recent_folders, std::slice::from_ref(&folder));
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
