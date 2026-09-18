use super::*;

#[test]
fn picker_removes_derived_folders_and_files_without_closing_dirty_media() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::removal::picker_removes_derived_folders_and_files_without_closing_dirty_media",
    ) else {
        return;
    };
    let folder = root.join("media");
    std::fs::create_dir(&folder).expect("media folder");
    let first = folder.join("first.bmp");
    let second = folder.join("second.bmp");
    for path in [&first, &second] {
        tab_transfer::tests::bitmap(path);
    }
    let source_bytes = std::fs::read(&first).expect("source bytes");
    let recent_path = root.join("recent.txt");
    let mut app = Application::new(None, |_| {}).expect("app");
    let context = fonts::test_context();
    context.enable_accesskit();
    app.ui_context = Some(context.clone());
    let tab =
        tab_transfer::tests::install(&mut app, first.clone(), tab_transfer::tests::decoded(false));
    app.push_visual_edit(EditOperation::RotateClockwise);
    let edits = app.edits[&tab].clone();
    let generation = app.media_generation;
    let tab_count = app.tabs.tabs().len();
    app.recent_files = Some(RecentFiles::new(recent_path.clone(), || {}).expect("worker"));
    let worker = app.recent_files.as_ref().expect("worker");
    worker.record(first.clone());
    worker.record(second.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.recent_paths.len() != 2 || !app.recent_folders.contains(&folder) {
        app.handle_app_event(AppEvent::RecentFilesReady);
        assert!(Instant::now() < deadline, "initial history");
        std::thread::sleep(Duration::from_millis(2));
    }
    let frame = |app: &mut Application<_>, events| {
        let mut actions = Vec::new();
        let output = context.run_ui(
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
        output
    };
    let remove = |app: &mut Application<_>, path: &std::path::Path| {
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame(app, vec![]);
        }
        let label = format!("Remove from Recently Opened: {}", path.display());
        let tree = output.platform_output.accesskit_update.expect("tree");
        let (id, _) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label.as_str()))
            .expect("remove action");
        frame(
            app,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: *id,
                    action: egui::accesskit::Action::Click,
                    data: None,
                },
            )],
        );
        assert!(app.palette_open);
        assert!(app.pending_guard.is_none() && app.pending_dialog.is_none());
        assert_eq!(app.tabs.active_id(), Some(tab));
        assert_eq!(app.tabs.tabs().len(), tab_count);
        assert_eq!(app.media_generation, generation);
        assert_eq!(app.edits[&tab], edits);
    };
    app.dispatch(CommandId::OpenRecentFolder);
    remove(&mut app, &folder);
    assert!(app.recent_folders.is_empty());
    assert_eq!(
        app.recent_paths.len(),
        2,
        "folder removal keeps child file history"
    );
    drop(app.recent_files.take());
    let mut reopened = Application::new(None, |_| {}).expect("reopened consumer");
    reopened.recent_files =
        Some(RecentFiles::new(recent_path.clone(), || {}).expect("reopened history"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while reopened.recent_paths.len() != 2 {
        reopened.handle_app_event(AppEvent::RecentFilesReady);
        assert!(Instant::now() < deadline, "reopened history");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        reopened.recent_folders.is_empty(),
        "derived parent stays removed after restart"
    );
    drop(reopened);
    app.recent_files =
        Some(RecentFiles::new(recent_path.clone(), || {}).expect("continued worker"));
    app.dispatch(CommandId::GoToFile);
    for _ in 0..4 {
        frame(&mut app, vec![]);
    }
    frame(&mut app, vec![egui::Event::Text("first".into())]);
    remove(&mut app, &first);
    assert_eq!(app.recent_paths, std::slice::from_ref(&second));
    assert_eq!(
        std::fs::read(&first).expect("unchanged media"),
        source_bytes
    );
    drop(app.recent_files.take());
    let persisted = std::fs::read_to_string(&recent_path).expect("saved history");
    assert!(!persisted.contains(first.to_str().expect("fixture path")));
    assert!(persisted.contains(second.to_str().expect("fixture path")));
    app.recent_files = Some(RecentFiles::new(recent_path, || {}).expect("revisit worker"));
    app.recent_files
        .as_ref()
        .expect("worker")
        .record(first.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.recent_folders.contains(&folder) || !app.recent_paths.contains(&first) {
        app.handle_app_event(AppEvent::RecentFilesReady);
        assert!(Instant::now() < deadline, "revisited parent is restored");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(app.edits[&tab], edits);
    assert_eq!(
        std::fs::read(&first).expect("source after revisit"),
        source_bytes
    );
}
