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
        towavue_core::localization::formatted::no_folder_media(
            app.language(),
            &root.display().to_string()
        ),
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
    assert_eq!(result.target.as_deref(), target.parent());
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
        towavue_core::localization::formatted::no_folder_media(
            app.language(),
            &root.join("b").display().to_string()
        ),
    );
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
