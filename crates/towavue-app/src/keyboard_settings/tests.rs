use super::*;

mod gpu;

#[test]
fn records_search_chords_and_cancels_without_dispatching() {
    let mut state = KeyboardSettings {
        record_search: true,
        ..Default::default()
    };
    state.capture("Ctrl+K".parse().expect("valid test key"));
    state.capture("Ctrl+S".parse().expect("valid test key"));
    assert_eq!(state.query, "\"Ctrl+K Ctrl+S\"");
    assert_eq!(
        state.rows(&shortcuts::defaults())[0].command.id,
        CommandId::OpenKeyboardSettings
    );
    state.capture("Escape".parse().expect("valid test key"));
    assert!(!state.capturing());
}

#[test]
fn settings_tab_keeps_media_history_records_without_dispatch_and_saves_through_its_modal() {
    let Some(root) = crate::tests::isolated_test_root(
        "keyboard_settings::tests::settings_tab_keeps_media_history_records_without_dispatch_and_saves_through_its_modal",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("keyboard settings fixture");
    let context = fonts::test_context();
    context.enable_accesskit();
    context.global_style_mut(chrome::style);
    app.ui_context = Some(context);
    let media = tab_transfer::tests::install(
        &mut app,
        root.join("source.png"),
        tab_transfer::tests::decoded(false),
    );
    app.edits
        .entry(media)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    let history = app.edits[&media].clone();
    app.process_shortcut("Ctrl+K".parse().expect("valid test key"));
    app.process_shortcut("Ctrl+S".parse().expect("valid test key"));
    assert!(app.keyboard_settings_active());
    let settings = app.tabs.active_id().expect("active tab");
    assert!(app.tabs.active().is_none() && app.path.is_none());
    assert!(app.retained_images.contains_key(&media));
    assert_eq!(app.edits[&media], history);
    app.dispatch(CommandId::OpenKeyboardSettings);
    assert_eq!(app.tabs.active_id(), Some(settings));
    assert!(app.title().contains("Keyboard Shortcuts"));
    for density in [1.0, 1.25, 2.0] {
        app.ui_context
            .as_ref()
            .expect("keyboard settings fixture")
            .set_pixels_per_point(density);
        let frame = |app: &mut Application<_>, events| {
            crate::audio_export::tests::frame(app, egui::vec2(780.0, 540.0), events)
        };
        frame(&mut app, vec![]);
        let output = frame(&mut app, vec![]);
        let tree = output
            .platform_output
            .accesskit_update
            .expect("accessibility tree");
        for name in [
            "Keyboard Shortcuts tab",
            "Record keys",
            "Sort by precedence",
            "Clear keybindings search input",
        ] {
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some(name)),
                "{name}"
            );
        }
        let record = crate::video_rotation::tests::node(&tree, "Record keys");
        frame(
            &mut app,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Click,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: record,
                    data: None,
                },
            )],
        );
        assert!(app.keyboard_capture_active());
        app.process_shortcut("Ctrl+W".parse().expect("valid test key"));
        assert_eq!(
            app.tabs.active_id(),
            Some(settings),
            "recording cannot close the tab"
        );
        assert_eq!(app.keyboard_settings.query, "\"Ctrl+W\"");
        app.process_shortcut("Escape".parse().expect("valid test key"));
        assert!(!app.keyboard_capture_active());
    }
    let entry: KeyStroke = "Ctrl+K".parse().expect("valid test key");
    assert!(app.owns_focused_shortcut(&entry));
    app.native_ime_composing = true;
    assert!(!app.owns_focused_shortcut(&entry));
    app.native_ime_composing = false;
    app.process_shortcut(entry.clone());
    assert!(app.owns_focused_shortcut(&"Ctrl+S".parse().expect("valid test key")));
    app.process_shortcut("Ctrl+S".parse().expect("valid test key"));
    app.keyboard_settings
        .begin_edit(CommandId::OpenFile, Some(0), &app.shortcuts);
    assert!(app.modal_input_blocked());
    assert!(!app.owns_focused_shortcut(&entry));
    app.handle_ui_action(UiAction::ActivateTab(media));
    app.handle_ui_action(UiAction::FinishResize(None));
    assert_eq!(app.tabs.active_id(), Some(settings));
    assert!(app.keyboard_settings.edit.is_some());
    let edit = app
        .keyboard_settings
        .edit
        .as_mut()
        .expect("keyboard settings fixture");
    edit.text = "Ctrl+K F2".into();
    edit.recording = false;
    let size = egui::vec2(780.0, 540.0);
    crate::audio_export::tests::frame(&mut app, size, vec![]);
    let output = crate::audio_export::tests::frame(&mut app, size, vec![]);
    let tree = output
        .platform_output
        .accesskit_update
        .expect("accessibility tree");
    let save = crate::video_rotation::tests::node(&tree, "Save");
    crate::audio_export::tests::frame(
        &mut app,
        size,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: save,
                data: None,
            },
        )],
    );
    assert!(app.keyboard_settings.edit.is_none());
    assert_eq!(
        app.shortcuts,
        shortcuts::load_from(&app.shortcut_path).expect("keyboard settings fixture")
    );
    assert_eq!(
        app.shortcuts
            .get(CommandId::OpenFile)
            .expect("keyboard settings fixture")
            .to_string(),
        "Ctrl+K F2"
    );
    app.keyboard_settings.query = "reading".into();
    app.keyboard_settings.precedence = true;
    app.dispatch(CommandId::CloseTab);
    assert!(app.tabs.keyboard_settings().is_none());
    app.dispatch(CommandId::ReopenClosedTab);
    assert!(app.keyboard_settings_active());
    assert_ne!(app.tabs.active_id(), Some(settings));
    assert_eq!(app.keyboard_settings.query, "reading");
    assert!(app.keyboard_settings.precedence);
    let disk = std::fs::read(&app.shortcut_path).expect("keyboard settings fixture");
    app.handle_ui_action(UiAction::KeybindingChange(
        settings,
        Change {
            command: CommandId::OpenFile,
            expected: app.shortcuts.all(CommandId::OpenFile).to_vec(),
            replacement: vec!["F9".parse().expect("valid test key")],
        },
    ));
    assert_eq!(
        std::fs::read(&app.shortcut_path).expect("keyboard settings fixture"),
        disk,
        "closed editor actions cannot write through a new tab"
    );
    app.activate_tab(media);
    assert_eq!(app.edits[&media], history);
    assert!(app.image.is_some());
    assert!(app.keyboard_settings.edit.is_none());
}

#[test]
fn precedence_matches_primary_alternative_resolution_and_when_matches_reading_contexts() {
    let bindings = shortcuts::defaults();
    let state = KeyboardSettings {
        precedence: true,
        ..Default::default()
    };
    let rows = state.rows(&bindings);
    for context in [
        CommandContext::default(),
        CommandContext {
            media_kind: Some(MediaKind::Image),
            ..Default::default()
        },
        CommandContext {
            media_kind: Some(MediaKind::Image),
            reading_mode: true,
            ..Default::default()
        },
        CommandContext {
            media_kind: Some(MediaKind::Video),
            timeline_open: true,
            has_time_selection: true,
            ..Default::default()
        },
    ] {
        for row in &rows {
            let Some(slot) = row.slot else { continue };
            let sequence = &bindings.all(row.command.id)[slot];
            let first = rows
                .iter()
                .find(|other| other.command.is_enabled(context) && other.keys == row.keys);
            if let ShortcutMatch::Command(command) = bindings.resolve(sequence.strokes(), context) {
                assert_eq!(
                    first.expect("keyboard settings fixture").command.id,
                    command
                );
            }
        }
    }
    let reading = rows
        .iter()
        .find(|row| row.command.id == CommandId::ReadingLeft)
        .expect("keyboard settings fixture");
    assert!(reading.when.contains("image && reading"));
    let rotate = rows
        .iter()
        .find(|row| row.command.id == CommandId::RotateClockwise)
        .expect("keyboard settings fixture");
    assert!(rotate.when.contains("image && !reading") && rotate.when.contains("video && timeline"));
}
