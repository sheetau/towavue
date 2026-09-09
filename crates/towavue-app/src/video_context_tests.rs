use super::*;
use egui::accesskit;

#[test]
fn disabled_video_selection_does_not_cancel_the_compact_seek_gesture() {
    let mut app = Application::new(None, |_| {}).expect("app");
    app.media_kind = Some(MediaKind::Video);
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
    let bar = egui::Rect::from_min_max(egui::pos2(0.0, 180.0), rect.max);
    let start = egui::pos2(80.0, 190.0);
    let end = egui::pos2(300.0, 190.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    let mut committed = None;
    for events in [
        vec![],
        vec![egui::Event::PointerMoved(start), button(start, true)],
        vec![egui::Event::PointerMoved(end)],
        vec![button(end, false)],
    ] {
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            },
            |ui| {
                let response = ui.interact(
                    bar,
                    "video-context-seek".into(),
                    egui::Sense::click_and_drag(),
                );
                let drag = timeline_input::video_seek_drag(&response);
                if drag.released {
                    committed = drag.position;
                }
                let surface = ui.interact(
                    egui::Rect::from_min_max(rect.min, egui::pos2(400.0, 180.0)),
                    "video-context-overlay".into(),
                    egui::Sense::click_and_drag(),
                );
                app.update_selection(
                    &surface,
                    surface.rect,
                    (400, 180),
                    false,
                    ui.input(|input| input.pointer.hover_pos()),
                );
            },
        );
    }
    assert_eq!(committed, Some(end));
    assert!(app.image_view.selection.is_none());
}

#[test]
fn video_visual_commands_and_selection_follow_the_visible_timeline() {
    let mut app = Application::new(None, |_| {}).expect("app");
    let path = PathBuf::from("video-context.mp4");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Video);
    app.path = Some(path);
    app.media_kind = Some(MediaKind::Video);
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    app.shortcuts = shortcuts::defaults();
    for key in ["R", "L", "H", "V", "Ctrl+Y", "Ctrl+A"] {
        let key = key.parse::<towavue_core::KeySequence>().expect("key");
        assert_eq!(
            app.shortcuts.resolve(key.strokes(), app.command_context()),
            ShortcutMatch::None
        );
    }
    for command in [
        CommandId::RotateClockwise,
        CommandId::RotateCounterclockwise,
        CommandId::FlipHorizontal,
        CommandId::FlipVertical,
        CommandId::ApplyCrop,
        CommandId::SelectAll,
    ] {
        app.handle_ui_action(UiAction::Command(command));
    }
    assert!(!app.edits.contains_key(&tab));
    assert!(app.image_view.selection.is_none());
    app.timeline_open = true;
    app.process_shortcut("R".parse().expect("rotate"));
    assert_eq!(
        app.edits[&tab].operations(),
        &[EditOperation::RotateClockwise]
    );

    let selected = UnitRect {
        min: UnitPoint { x: 0.2, y: 0.2 },
        max: UnitPoint { x: 0.8, y: 0.8 },
    };
    app.image_view.selection = Some(selected);
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
    let identity = app.selection_identity();
    context.memory_mut(|memory| memory.request_focus(identity.with(0_usize)));
    let _ = context.run_ui(Default::default(), |ui| {
        app.selection_controls(ui, rect, (400, 200))
    });
    assert!(selection::has_focus(&context));
    app.view_drag = Some(ViewDrag::Selection {
        mode: SelectionDrag::New(UnitPoint { x: 0.1, y: 0.1 }),
        before: Some(selected),
    });
    app.image_view.selection = Some(UnitRect::FULL);
    app.dispatch(CommandId::ToggleTimeline);
    assert!(!app.timeline_open);
    assert!(app.view_drag.is_none());
    assert_eq!(app.image_view.selection, Some(selected));
    assert!(!selection::has_focus(&context));

    // Stale numeric actions cannot edit a hidden selection or reclaim its focus.
    let output = context.run_ui(
        egui::RawInput {
            events: vec![egui::Event::AccessKitActionRequest(
                accesskit::ActionRequest {
                    action: accesskit::Action::SetValue,
                    target_tree: accesskit::TreeId::ROOT,
                    target_node: identity.with(0_usize).accesskit_id(),
                    data: Some(accesskit::ActionData::NumericValue(120.0)),
                },
            )],
            ..Default::default()
        },
        |ui| app.selection_controls(ui, rect, (400, 200)),
    );
    assert!(output.shapes.is_empty());
    assert_eq!(app.image_view.selection, Some(selected));
    app.dispatch(CommandId::FlipHorizontal);
    assert_eq!(
        app.edits[&tab].operations(),
        &[EditOperation::RotateClockwise]
    );
    app.dispatch(CommandId::Undo);
    assert!(
        app.edits[&tab].operations().is_empty(),
        "recovery is available while viewing"
    );
    app.dispatch(CommandId::Redo);
    assert_eq!(
        app.edits[&tab].operations(),
        &[EditOperation::RotateClockwise]
    );
    app.timeline_open = true;
    app.set_fullscreen(true);
    assert!(
        !app.visual_selection_enabled(),
        "hidden fullscreen timeline is not editing mode"
    );
    app.dispatch(CommandId::FlipVertical);
    assert_eq!(app.edits[&tab].operations().len(), 1);
    app.set_fullscreen(false);
    assert!(app.visual_selection_enabled());
    app.dispatch(CommandId::FlipVertical);
    assert_eq!(app.edits[&tab].operations().len(), 2);
}

#[test]
fn viewing_video_rejects_selection_drags_but_editing_accepts_the_same_events() {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
    for timeline_open in [false, true] {
        let mut app = Application::new(None, |_| {}).expect("app");
        app.media_kind = Some(MediaKind::Video);
        app.timeline_open = timeline_open;
        let context = fonts::test_context();
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        let start = egui::pos2(80.0, 40.0);
        let end = egui::pos2(320.0, 160.0);
        for events in [
            vec![],
            vec![egui::Event::PointerMoved(start), button(start, true)],
            vec![egui::Event::PointerMoved(end), button(end, false)],
        ] {
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.interact(
                        rect,
                        "video-context-surface".into(),
                        egui::Sense::click_and_drag(),
                    );
                    app.update_selection(
                        &response,
                        rect,
                        (400, 200),
                        false,
                        ui.input(|input| input.pointer.hover_pos()),
                    );
                },
            );
        }
        assert_eq!(app.image_view.selection.is_some(), timeline_open);
        assert!(app.view_drag.is_none());
    }
}
