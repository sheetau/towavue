use super::*;

#[test]
fn palette_use_and_remove_persist_but_direct_dispatch_does_not() {
    let Some(root) = tests::isolated_test_root(
        "recent_tests::command_history::palette_use_and_remove_persist_but_direct_dispatch_does_not",
    ) else {
        return;
    };
    let recent_path = root.join("recent.txt");
    let mut app = Application::new(None, |_| {}).expect("app");
    let context = fonts::test_context();
    context.enable_accesskit();
    app.ui_context = Some(context.clone());
    app.recent_files = Some(RecentFiles::new(recent_path.clone(), || {}).expect("worker"));
    app.dispatch(CommandId::OpenGallery);
    assert!(
        app.recent_commands.is_empty(),
        "ordinary dispatch is not palette history"
    );
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
    app.dispatch(CommandId::ToggleCommandPalette);
    for _ in 0..4 {
        frame(&mut app, vec![]);
    }
    frame(&mut app, vec![egui::Event::Text("open gallery".into())]);
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
    assert_eq!(app.recent_commands, [CommandId::OpenGallery]);
    assert!(!app.palette_open);
    drop(app.recent_files.take());
    app.recent_commands.clear();
    app.recent_files = Some(RecentFiles::new(recent_path.clone(), || {}).expect("reopen worker"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.recent_commands.is_empty() {
        app.handle_app_event(AppEvent::RecentFilesReady);
        assert!(Instant::now() < deadline, "restored command history");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(app.recent_commands, [CommandId::OpenGallery]);
    app.dispatch(CommandId::ToggleCommandPalette);
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = frame(&mut app, vec![]);
    }
    let tree = output.platform_output.accesskit_update.expect("tree");
    let (id, _) = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Remove Open Gallery from Recently Used"))
        .expect("remove button");
    frame(
        &mut app,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: *id,
                action: egui::accesskit::Action::Click,
                data: None,
            },
        )],
    );
    assert!(app.recent_commands.is_empty());
    assert!(
        app.palette_open,
        "removal neither executes the command nor dismisses the picker"
    );
    drop(app.recent_files.take());
    let persisted =
        std::fs::read_to_string(root.join("command-history.txt")).expect("persisted removal");
    assert_eq!(persisted, "towavue command history v1\n");
}
