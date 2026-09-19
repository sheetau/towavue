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
    let record_key = "Alt+K".parse().expect("key");
    assert!(app.keyboard_search_owns_shortcut(&record_key));
    app.native_ime_composing = true;
    assert!(!app.keyboard_search_owns_shortcut(&record_key));
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
    app.keyboard_settings
        .capture("Ctrl+K".parse().expect("key"));
    app.keyboard_settings.capture("F2".parse().expect("key"));
    let size = egui::vec2(780.0, 540.0);
    crate::audio_export::tests::frame(&mut app, size, vec![]);
    app.process_shortcut("Enter".parse().expect("key"));
    crate::audio_export::tests::frame(&mut app, size, vec![]);
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

fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn search_controls_require_focus_and_recording_enter_submits_without_changing_the_sequence() {
    for density in [1.0, 1.25, 2.0] {
        let context = fonts::test_context();
        context.global_style_mut(chrome::style);
        context.set_pixels_per_point(density);
        let mut settings = KeyboardSettings::default();
        let bindings = shortcuts::defaults();
        let frame = |settings: &mut KeyboardSettings, mut events: Vec<egui::Event>| {
            // Deliver complete presses: egui infers repeat when a previous key has no release.
            let releases: Vec<_> = events
                .iter()
                .filter_map(|event| {
                    if let egui::Event::Key { key, modifiers, .. } = event {
                        Some(egui::Event::Key {
                            key: *key,
                            physical_key: None,
                            pressed: false,
                            repeat: false,
                            modifiers: *modifiers,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            events.extend(releases);
            let mut change = None;
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 440.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| change = settings.show(ui, &bindings, true),
            );
            change
        };
        frame(&mut settings, vec![]);
        frame(
            &mut settings,
            vec![key_event(egui::Key::K, egui::Modifiers::ALT)],
        );
        assert!(
            !settings.record_search,
            "Alt+K outside the search cannot start recording"
        );
        settings.request_search_focus();
        frame(&mut settings, vec![]);
        frame(
            &mut settings,
            vec![key_event(egui::Key::K, egui::Modifiers::ALT)],
        );
        assert!(settings.record_search);
        frame(
            &mut settings,
            vec![key_event(egui::Key::W, egui::Modifiers::CTRL)],
        );
        assert_eq!(settings.query, "\"Ctrl+W\"");
        frame(
            &mut settings,
            vec![key_event(egui::Key::P, egui::Modifiers::ALT)],
        );
        assert!(settings.precedence);
        assert_eq!(settings.query, "\"Ctrl+W\"");
        frame(
            &mut settings,
            vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(settings.query.is_empty() && !settings.capturing());
        settings.begin_edit(CommandId::OpenFile, Some(0), &bindings);
        frame(
            &mut settings,
            vec![key_event(egui::Key::K, egui::Modifiers::CTRL)],
        );
        frame(
            &mut settings,
            vec![key_event(egui::Key::F2, egui::Modifiers::NONE)],
        );
        let change = frame(
            &mut settings,
            vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)],
        )
        .expect("Enter submits");
        assert_eq!(change.replacement[0].to_string(), "Ctrl+K F2");
        assert_eq!(change.expected, bindings.all(CommandId::OpenFile));
        assert!(
            frame(&mut settings, vec![]).is_none(),
            "no repeated submission"
        );
        frame(
            &mut settings,
            vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(settings.edit.is_none());
        settings.begin_edit(CommandId::OpenFile, Some(0), &bindings);
        assert!(frame(&mut settings, vec![egui::Event::WindowFocused(false)]).is_none());
        assert!(
            settings.edit.is_none() && !settings.capturing(),
            "focus loss cancels the recorder transaction"
        );
    }
}

#[test]
fn settings_list_is_dense_inset_nonselectable_and_keeps_colors_while_blocked() {
    for density in [1.0, 1.25, 2.0] {
        for width in [320.0, 960.0] {
            let context = fonts::test_context();
            context.global_style_mut(chrome::style);
            context.enable_accesskit();
            context.set_pixels_per_point(density);
            let mut settings = KeyboardSettings::default();
            let bindings = shortcuts::defaults();
            let frame = |settings: &mut KeyboardSettings, enabled, events| {
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 440.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        settings.show(ui, &bindings, enabled);
                    },
                )
            };
            frame(&mut settings, true, vec![]);
            let output = frame(&mut settings, true, vec![]);
            let nodes = &output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("tree")
                .nodes;
            let rect = |label: &str| {
                let bounds = nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .expect(label)
                    .1
                    .bounds()
                    .expect("bounds");
                egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                )
            };
            let record = rect("Record keys");
            let sort = rect("Sort by precedence");
            let clear = rect("Clear keybindings search input");
            assert_eq!(record.size(), egui::Vec2::splat(20.0));
            assert_eq!(sort.left() - record.right(), 2.0);
            assert_eq!(clear.left() - sort.right(), 2.0);
            assert!(clear.right() <= width - 10.0);
            let mut rows: Vec<_> = nodes
                .iter()
                .filter(|(_, node)| {
                    node.role() == egui::accesskit::Role::Button
                        && node.label().is_some_and(|label| label.contains(": "))
                })
                .map(|(_, node)| node.bounds().expect("row bounds"))
                .collect();
            rows.sort_by(|a, b| a.y0.total_cmp(&b.y0));
            assert!(rows.len() > 4);
            assert!(
                (rows[1].y0 - rows[0].y0 - 24.0).abs() < 0.1,
                "row bounds at {density}: {rows:?}"
            );
            assert!(rows[0].x0 >= 8.0);
            let point = egui::pos2(rows[0].x0 as f32 + 70.0, rows[0].y0 as f32 + 12.0);
            let hovered = frame(&mut settings, true, vec![egui::Event::PointerMoved(point)]);
            assert_ne!(hovered.platform_output.cursor_icon, egui::CursorIcon::Text);
            let colors = |output: &egui::FullOutput| {
                output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == "Command" => Some(
                            text.galley
                                .job
                                .sections
                                .iter()
                                .map(|section| section.format.color)
                                .collect::<Vec<_>>(),
                        ),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            let border = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::LineSegment { points, .. }
                        if points[0].y == points[1].y
                            && points[0].distance(points[1]) > width * 0.8 =>
                    {
                        Some(points[0].y)
                    }
                    _ => None,
                })
                .expect("header border");
            let first = settings.rows(&bindings)[0].command.title;
            let clip = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == first => Some(shape.clip_rect),
                    _ => None,
                })
                .expect("first row text");
            assert!((clip.top() - border - 1.0).abs() <= 1.0 / density);
            assert!(
                clip.bottom() <= 432.0,
                "bottom inset remains outside the scroll clip"
            );
            let blocked = frame(&mut settings, false, vec![]);
            assert!(!colors(&output).is_empty());
            assert_eq!(
                colors(&output),
                colors(&blocked),
                "popup guards must not dim the list"
            );
            let secondary = |pressed| egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            // Re-enable and register hit regions after the explicit disabled-paint probe.
            frame(&mut settings, true, vec![]);
            frame(&mut settings, true, vec![secondary(true)]);
            frame(&mut settings, true, vec![secondary(false)]);
            assert!(egui::Popup::is_any_open(&context));
            let popup = frame(&mut settings, false, vec![]);
            assert_eq!(colors(&output), colors(&popup));
            let tree = popup
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("menu tree");
            let (add, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Add keybinding"))
                .expect("context menu action");
            assert!(!node.is_disabled());
            frame(
                &mut settings,
                false,
                vec![egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: *add,
                        action: egui::accesskit::Action::Click,
                        data: None,
                    },
                )],
            );
            assert!(
                settings
                    .edit
                    .as_ref()
                    .is_some_and(|edit| edit.slot.is_none())
            );
        }
    }
}

#[test]
fn unassigned_commands_offer_add_and_record_a_first_binding() {
    let context = fonts::test_context();
    context.global_style_mut(chrome::style);
    context.enable_accesskit();
    let bindings = shortcuts::defaults();
    let mut settings = KeyboardSettings::default();
    let rows = settings.rows(&bindings);
    assert!(
        command_definitions()
            .iter()
            .all(|command| rows.iter().any(|row| row.command.id == command.id))
    );
    let command = rows
        .iter()
        .find(|row| row.slot.is_none())
        .expect("unassigned command")
        .command;
    settings.query = command.id.as_str().to_owned();
    let frame = |settings: &mut KeyboardSettings, events| {
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 440.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                settings.show(ui, &bindings, true);
            },
        )
    };
    frame(&mut settings, vec![]);
    let output = frame(&mut settings, vec![]);
    let bounds = output
        .platform_output
        .accesskit_update
        .expect("tree")
        .nodes
        .into_iter()
        .find(|(_, node)| node.label() == Some(format!("{}: ", command.title).as_str()))
        .expect("unassigned row")
        .1
        .bounds()
        .expect("row bounds");
    let point = egui::pos2(bounds.x0 as f32 + 14.0, bounds.y0 as f32 + 12.0);
    frame(&mut settings, vec![egui::Event::PointerMoved(point)]);
    let output = frame(&mut settings, vec![]);
    let add = output
        .platform_output
        .accesskit_update
        .expect("tree")
        .nodes
        .into_iter()
        .find(|(_, node)| node.label() == Some("Add keybinding"))
        .expect("add control")
        .0;
    frame(
        &mut settings,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: add,
                action: egui::accesskit::Action::Click,
                data: None,
            },
        )],
    );
    let edit = settings.edit.as_ref().expect("recorder");
    assert_eq!(edit.command, command.id);
    assert!(edit.slot.is_none() && edit.expected.is_empty());
    settings.capture("Alt+F9".parse().expect("key"));
    settings.capture("Enter".parse().expect("key"));
    let mut change = None;
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        change = settings.show(ui, &bindings, true)
    });
    let change = change.expect("first binding");
    assert!(change.expected.is_empty());
    assert_eq!(change.replacement[0].to_string(), "Alt+F9");
}

#[test]
fn command_query_matches_only_its_id_and_keeps_unassigned_rows_blank() {
    let bindings = shortcuts::defaults();
    let mut settings = KeyboardSettings::default();
    for command in [CommandId::ZoomIn, CommandId::OpenFile] {
        settings.focus_command(command);
        assert_eq!(settings.query, format!("@command:{}", command.as_str()));
        let rows = settings.rows(&bindings);
        assert_eq!(rows.len(), bindings.all(command).len().max(1));
        assert!(rows.iter().all(|row| row.command.id == command));
    }
    settings.query = "@command:open_".into();
    assert!(
        settings.rows(&bindings).is_empty(),
        "command IDs are not prefix searches"
    );
    let unassigned = command_definitions()
        .iter()
        .find(|command| bindings.all(command.id).is_empty())
        .expect("unassigned");
    settings.focus_command(unassigned.id);
    assert_eq!(settings.rows(&bindings).len(), 1);
    let context = fonts::test_context();
    let output = context.run_ui(egui::RawInput::default(), |ui| {
        settings.show(ui, &bindings, true);
    });
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text() == "Unassigned")));
}

#[test]
fn list_navigation_reveals_virtual_rows_without_editing_and_preserves_search_input() {
    for density in [1.0, 1.25, 2.0] {
        let context = fonts::test_context();
        context.enable_accesskit();
        context.global_style_mut(chrome::style);
        context.set_pixels_per_point(density);
        let bindings = shortcuts::defaults();
        let mut settings = KeyboardSettings::default();
        let frame = |settings: &mut KeyboardSettings, events| {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 360.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    assert!(settings.show(ui, &bindings, true).is_none());
                },
            );
            output.platform_output.accesskit_update.expect("tree")
        };
        frame(&mut settings, vec![]);
        let rows = settings.rows(&bindings);
        let tree = frame(&mut settings, vec![]);
        let label = |index: usize| format!("{}: {}", rows[index].command.title, rows[index].keys);
        let first = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label(0).as_str()))
            .expect("first row")
            .0;
        frame(
            &mut settings,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: first,
                    data: None,
                },
            )],
        );
        // 360 minus both margins, search/header/separators leaves 287 points: eleven rows.
        for (key, index) in [
            (egui::Key::PageDown, 11),
            (egui::Key::ArrowDown, 12),
            (egui::Key::PageUp, 1),
            (egui::Key::ArrowUp, 0),
        ] {
            let tree = frame(&mut settings, vec![key_event(key, egui::Modifiers::NONE)]);
            let node = &tree
                .nodes
                .iter()
                .find(|(id, _)| *id == tree.focus)
                .expect("focused row")
                .1;
            assert_eq!(node.label(), Some(label(index).as_str()));
            let bounds = node.bounds().expect("visible bounds");
            assert!(bounds.y0 >= 64.0 && bounds.y1 <= 353.0, "{bounds:?}");
            assert!(settings.edit.is_none());
        }
        settings.request_search_focus();
        frame(&mut settings, vec![]);
        let previous = settings.row_focus;
        frame(
            &mut settings,
            vec![key_event(egui::Key::PageDown, egui::Modifiers::NONE)],
        );
        assert_eq!(settings.row_focus, previous);
        assert!(settings.search_focused(&context));
    }
}

#[test]
fn search_button_help_names_focused_shortcuts_and_empty_clear_is_disabled() {
    let context = fonts::test_context();
    context.global_style_mut(|style| {
        chrome::style(style);
        style.interaction.tooltip_delay = 0.0;
    });
    context.enable_accesskit();
    let bindings = shortcuts::defaults();
    let mut settings = KeyboardSettings::default();
    let mut time = 0.0;
    let mut frame = |settings: &mut KeyboardSettings, events| {
        time += 1.0;
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 440.0),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                settings.show(ui, &bindings, true);
            },
        )
    };
    frame(&mut settings, vec![]);
    let output = frame(&mut settings, vec![]);
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes;
    for (label, key) in [
        ("Record keys", "Alt+K"),
        ("Sort by precedence", "Alt+P"),
        ("Clear keybindings search input", "Escape"),
    ] {
        let node = &nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .expect(label)
            .1;
        assert_eq!(node.is_disabled(), key == "Escape");
        let bounds = node.bounds().expect("button bounds");
        let pointer = egui::pos2(
            (bounds.x0 + bounds.x1) as f32 * 0.5,
            (bounds.y0 + bounds.y1) as f32 * 0.5,
        );
        let mut found = false;
        for _ in 0..4 {
            let output = frame(&mut settings, vec![egui::Event::PointerMoved(pointer)]);
            found |= output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text)
                if text.galley.text().contains(label) && text.galley.text().contains(key)
                    && text.galley.text().contains("search focused"))
            });
        }
        assert!(found, "visible tooltip for {label}");
    }
    settings.query = "Open".into();
    let output = frame(&mut settings, vec![egui::Event::PointerGone]);
    let nodes = &output.platform_output.accesskit_update.expect("tree").nodes;
    let clear = &nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Clear keybindings search input"))
        .expect("clear")
        .1;
    assert!(!clear.is_disabled());
}
