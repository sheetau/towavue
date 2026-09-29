use super::folder_name;
use crate::*;

fn settle<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.pending_folder.is_some() {
        app.finish_folder_load();
        assert!(Instant::now() < deadline, "folder navigation delivery");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn related_folder_navigation_wraps_and_preserves_dirty_and_stale_owners() {
    let Some(root) = tests::isolated_test_root(
        "folder_navigation::tests::related_folder_navigation_wraps_and_preserves_dirty_and_stale_owners",
    ) else {
        return;
    };
    let a = root.join("a").join("first.bmp");
    let b = root.join("b").join("second.bmp");
    for path in [&a, &b] {
        std::fs::create_dir(path.parent().expect("fixture parent")).expect("folder");
        tab_transfer::tests::bitmap(path);
    }
    std::fs::create_dir(root.join("empty")).expect("empty sibling");
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(fonts::test_context());
    let tab =
        tab_transfer::tests::install(&mut app, b.clone(), tab_transfer::tests::decoded(false));
    app.push_visual_edit(EditOperation::RotateClockwise);
    let edits = app.edits[&tab].clone();
    app.dispatch(CommandId::NextFolder);
    assert_eq!(app.path.as_ref(), Some(&b), "retain media during lookup");
    let pending = app.pending_folder.as_ref().expect("pending lookup").0;
    app.refresh_folder_snapshot_from_disk();
    assert_eq!(
        app.pending_folder.as_ref().expect("pending lookup").0,
        pending
    );
    settle(&mut app);
    assert!(
        matches!(&app.pending_guard, Some(GuardedAction::NavigateFromFolder(path, _)) if path == &a)
    );
    app.resolve_guard(GuardDecision::Cancel);
    assert_eq!(app.path.as_ref(), Some(&b));
    assert_eq!(app.edits[&tab], edits);

    app.dispatch(CommandId::NextFolder);
    app.media_generation = app.media_generation.wrapping_add(1);
    settle(&mut app);
    assert!(
        app.pending_guard.is_none(),
        "stale generation cannot prompt"
    );
    assert_eq!(app.path.as_ref(), Some(&b));
    assert_eq!(app.edits[&tab], edits);

    app.dispatch(CommandId::NextFolder);
    settle(&mut app);
    app.resolve_guard(GuardDecision::Discard);
    settle(&mut app);
    assert_eq!(app.path.as_ref(), Some(&a));
    assert_eq!(app.tabs.active_id(), Some(tab));
    app.dispatch(CommandId::PreviousFolder);
    settle(&mut app);
    assert_eq!(app.path.as_ref(), Some(&b), "reverse traversal wraps too");
    app.dispatch(CommandId::ParentFolder);
    settle(&mut app);
    assert_eq!(app.path.as_ref(), Some(&b), "empty immediate parent stays");
    assert!(app.pending_guard.is_none());
    assert_eq!(
        app.status_message.as_ref().expect("empty-folder notice").0,
        towavue_core::localization::formatted::no_folder_media(app.language(), &folder_name(&root)),
    );

    app.dispatch(CommandId::NextFolder);
    app.activate_tab(app.tabs.gallery().expect("Gallery"));
    settle(&mut app);
    assert!(app.path.is_none(), "late lookup cannot replace another tab");
    assert!(app.pending_guard.is_none());
}

#[test]
fn related_folder_discovery_reuses_open_folder_and_handles_a_destination_becoming_empty() {
    let Some(root) = tests::isolated_test_root(
        "folder_navigation::tests::related_folder_discovery_reuses_open_folder_and_handles_a_destination_becoming_empty",
    ) else {
        return;
    };
    let source = root.join("a").join("source.bmp");
    let target = root.join("b").join("target.bmp");
    for path in [&source, &target] {
        std::fs::create_dir(path.parent().expect("fixture parent")).expect("folder");
        tab_transfer::tests::bitmap(path);
    }
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(fonts::test_context());
    let tab = tab_transfer::tests::install(
        &mut app,
        source.clone(),
        tab_transfer::tests::decoded(false),
    );
    app.push_visual_edit(EditOperation::RotateClockwise);
    let edits = app.edits[&tab].clone();
    app.dispatch(CommandId::NextFolder);
    let deadline = Instant::now() + Duration::from_secs(30);
    let result = loop {
        if let Some(result) = app.folder_order.take_navigation() {
            break result;
        }
        assert!(Instant::now() < deadline, "path-only delivery");
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(result.target.as_deref().ok(), target.parent());
    assert!(
        app.folder_order.take_completed().is_none(),
        "no duplicate media listing"
    );
    std::fs::remove_file(&target).expect("remove owned media after discovery");
    app.finish_folder_navigation(result);
    assert!(
        matches!(app.pending_folder, Some((_, FolderIntent::OpenReplacing(Some(owner), _))) if owner == tab)
    );
    settle(&mut app);
    assert_eq!(app.path.as_ref(), Some(&source));
    assert_eq!(app.edits[&tab], edits);
    assert!(app.pending_guard.is_none());
    assert_eq!(
        app.status_message
            .as_ref()
            .expect("ordinary empty-folder notice")
            .0,
        towavue_core::localization::formatted::no_folder_media(app.language(), "b"),
    );
}

#[test]
fn folder_notices_use_names_and_distinguish_missing_children_from_empty_children() {
    let Some(root) = tests::isolated_test_root(
        "folder_navigation::tests::folder_notices_use_names_and_distinguish_missing_children_from_empty_children",
    ) else {
        return;
    };
    let folder = root.join("album");
    std::fs::create_dir(&folder).expect("fixture");
    let file = folder.join("first.bmp");
    tab_transfer::tests::bitmap(&file);
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(fonts::test_context());
    tab_transfer::tests::install(&mut app, file.clone(), tab_transfer::tests::decoded(false));
    for language in [
        localization::Language::English,
        localization::Language::Japanese,
    ] {
        // Verify both localized templates through the same failure mapping.
        app.language_settings.display = language;
        localization::set_language(app.ui_context.as_ref().expect("context"), language);
        app.dispatch(CommandId::FirstChildFolder);
        settle(&mut app);
        assert_eq!(app.path.as_ref(), Some(&file));
        assert_eq!(
            app.status_message.as_ref().expect("boundary notice").0,
            towavue_core::localization::formatted::no_child_folder(language, "album")
        );
    }
    let empty = folder.join("empty");
    std::fs::create_dir(&empty).expect("fixture");
    app.dispatch(CommandId::FirstChildFolder);
    settle(&mut app);
    assert_eq!(
        app.status_message.as_ref().expect("empty children").0,
        towavue_core::localization::formatted::no_child_folder_media(app.language(), "album")
    );
    app.open_folder_path(empty);
    settle(&mut app);
    assert_eq!(app.path.as_ref(), Some(&file));
    assert_eq!(
        app.status_message.as_ref().expect("empty folder").0,
        towavue_core::localization::formatted::no_folder_media(app.language(), "empty")
    );
}

#[test]
fn folder_open_and_navigation_start_at_shell_first_after_visiting_a_later_item() {
    let Some(root) = tests::isolated_test_root(
        "folder_navigation::tests::folder_open_and_navigation_start_at_shell_first_after_visiting_a_later_item",
    ) else {
        return;
    };
    let a = root.join("a");
    let b = root.join("b");
    for folder in [&a, &b] {
        std::fs::create_dir(folder).expect("fixture");
        for name in ["page10.bmp", "page2.bmp", "page1.bmp"] {
            tab_transfer::tests::bitmap(&folder.join(name));
        }
    }
    let mut app = Application::new(None, |_| {}).expect("app");
    app.open_folder_path(a.clone());
    settle(&mut app);
    let first = app.folder_snapshot.as_ref().expect("order").items[0]
        .path
        .clone();
    assert_eq!(app.path.as_ref(), Some(&first));
    let later = app.folder_snapshot.as_ref().expect("order").items[2]
        .path
        .clone();
    app.navigate_to_unchecked(later.clone());
    settle(&mut app);
    assert_eq!(app.path.as_ref(), Some(&later));
    app.dispatch(CommandId::NextFolder);
    settle(&mut app);
    assert_eq!(
        app.path.as_ref(),
        Some(&app.folder_snapshot.as_ref().expect("order").items[0].path)
    );
    assert_eq!(
        app.path.as_deref().and_then(Path::parent),
        Some(b.as_path())
    );
    app.dispatch(CommandId::PreviousFolder);
    settle(&mut app);
    assert_eq!(
        app.path.as_ref(),
        Some(&first),
        "no folder-position restoration"
    );
    app.navigate_to_unchecked(later);
    settle(&mut app);
    app.open_folder_path(a);
    settle(&mut app);
    assert_eq!(
        app.path.as_ref(),
        Some(&first),
        "Open folder also starts at the Shell head"
    );
}

#[test]
fn folder_notice_names_handle_drive_and_share_roots() {
    for (path, expected) in [
        (r"C:\private\album\", "album"),
        (r"C:\", "C:"),
        (r"\\server\share\", "share"),
        (r"\\?\UNC\server\share\", "share"),
        (r"\\?\C:\", "C:"),
    ] {
        assert_eq!(folder_name(Path::new(path)), expected);
    }
}

#[test]
fn related_folder_shortcuts_require_file_backed_media() {
    for (command, key) in [
        (CommandId::PreviousFolder, "Ctrl+Alt+Left"),
        (CommandId::NextFolder, "Ctrl+Alt+Right"),
        (CommandId::ParentFolder, "Ctrl+Alt+Up"),
        (CommandId::FirstChildFolder, "Ctrl+Alt+Down"),
    ] {
        let defaults = shortcuts::defaults();
        assert_eq!(
            defaults
                .get(command)
                .expect("default folder shortcut")
                .to_string(),
            key
        );
        let definition = command_definitions()
            .iter()
            .find(|d| d.id == command)
            .expect("registered folder command");
        for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
            let context = CommandContext {
                media_kind: Some(kind),
                ..Default::default()
            };
            assert!(definition.is_enabled(context));
            assert!(!definition.is_enabled(CommandContext {
                source_untitled: true,
                ..context
            }));
        }
        assert!(!definition.is_enabled(CommandContext::default()));
    }
}
