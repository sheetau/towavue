use super::*;

fn button(pressed: bool, pos: egui::Pos2, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    }
}

fn start_drag<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>) -> egui::Pos2 {
    frame_input(app, egui::Modifiers::ALT, vec![]);
    // egui adjusts the input rect on the first pass after a test scale change.
    // Read the settled logical viewport before choosing the owned press position.
    frame_input(app, egui::Modifiers::ALT, vec![]);
    let start = app.video_rect.expect("video rect").center();
    frame_input(
        app,
        egui::Modifiers::ALT,
        vec![egui::Event::PointerMoved(start)],
    );
    frame_input(
        app,
        egui::Modifiers::ALT,
        vec![button(true, start, egui::Modifiers::ALT)],
    );
    assert!(
        app.video_rotation_drag.is_some(),
        "owned Alt press {start:?} starts rotation in {:?}",
        app.video_rect,
    );
    start
}

pub(super) fn preview_cancel<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>) {
    let history = app.edits.clone();
    let view = app.image_view;
    let generation = app.generation;
    let start = start_drag(app);
    let end = start + egui::vec2(60.0, 40.0);
    frame_input(
        app,
        egui::Modifiers::ALT,
        vec![egui::Event::PointerMoved(end)],
    );
    assert_eq!(
        app.video_rotation_drag
            .as_ref()
            .expect("drag")
            .preview
            .value()
            .expect("rotation")
            .tenths(),
        300
    );
    assert!(app.video_raster_operations.is_some());
    assert_eq!(app.edits, history);
    assert_eq!(app.image_view, view);
    if let Some(state) = &mut app.ui_state {
        state
            .egui_input_mut()
            .events
            .push(button(false, end, egui::Modifiers::ALT));
        let modifiers = app.modifiers;
        app.modifiers = ModifiersState::empty();
        app.rotation_modifiers_changed();
        assert!(
            app.video_rotation_drag.is_some(),
            "queued Alt release precedes modifier loss"
        );
        let queued = app
            .ui_state
            .as_mut()
            .expect("input state")
            .egui_input_mut()
            .events
            .pop();
        assert!(matches!(
            queued,
            Some(egui::Event::PointerButton { pressed: false, .. })
        ));
        app.modifiers = modifiers;
    }
    frame_input(app, egui::Modifiers::NONE, vec![]);
    assert!(app.video_rotation_drag.is_none());
    frame_input(
        app,
        egui::Modifiers::NONE,
        vec![button(false, end, egui::Modifiers::NONE)],
    );
    assert_eq!(app.edits, history);
    assert_eq!(app.image_view, view);
    assert_eq!(app.generation, generation);
    eprintln!(
        "PASS held video rotation: GPU preview and Alt-first cancellation preserve history/view/generation"
    );
}

pub(super) fn exercise<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>) {
    let original_history = app.edits.clone();
    let original_view = app.image_view;
    let context = app.ui_context.clone().expect("context");
    let tab = app.tabs.active().expect("tab").id;
    let generation = app.generation;
    let position = app.current_position();
    app.image_view.selection = Some(UnitRect::FULL);
    let view = app.image_view;
    for scale in [1.0, 2.0] {
        context.set_pixels_per_point(scale);
        preview_cancel(app);
        let start = start_drag(app);
        let end = start + egui::vec2(-40.0, 30.0);
        frame_input(
            app,
            egui::Modifiers::ALT,
            vec![egui::Event::PointerMoved(end)],
        );
        let value = app
            .video_rotation_drag
            .as_ref()
            .expect("drag")
            .preview
            .value()
            .expect("valid canvas");
        assert_eq!(value.tenths(), -200);
        frame_input(
            app,
            egui::Modifiers::NONE,
            vec![
                button(false, end, egui::Modifiers::ALT),
                egui::Event::PointerMoved(end + egui::vec2(100.0, 0.0)),
            ],
        );
        assert!(app.video_rotation_drag.is_none());
        assert_eq!(
            app.edits[&tab].operations().last(),
            Some(&EditOperation::RotateVideo(value))
        );
        let committed = app.edits.clone();
        if scale == 1.0 {
            let source = app.path.clone().expect("test source");
            let target = source.with_file_name("drag-rotation.mp4");
            towavue_runtime_windows::export_media(&towavue_runtime_windows::ExportRequest {
                source,
                target: target.clone(),
                kind: MediaKind::Video,
                operations: app.edits[&tab].operations().to_vec(),
                hardware_encode: false,
            })
            .expect("export drag edit");
            let mut frames = 0;
            towavue_runtime_windows::decode_file(&target, |output| {
                if let towavue_runtime_windows::DecodeOutput::Video(frame) = output {
                    assert_eq!((frame.width, frame.height), value.size());
                    assert_eq!(frame.pixel_aspect, 1.0);
                    frames += 1;
                }
                true
            })
            .expect("reopen drag export");
            assert_eq!(frames, 5);
        }
        frame_input(
            app,
            egui::Modifiers::NONE,
            vec![button(false, end, egui::Modifiers::ALT)],
        );
        assert_eq!(
            app.edits, committed,
            "duplicate release is not another edit"
        );
        app.dispatch(CommandId::Undo);
        assert_eq!(
            app.edits[&tab].operations(),
            original_history[&tab].operations()
        );
        app.image_view = view;
        let undone = app.edits.clone();
        let start = start_drag(app);
        frame_input(
            app,
            egui::Modifiers::ALT,
            vec![button(false, start, egui::Modifiers::ALT)],
        );
        assert_eq!(app.edits, undone, "zero gesture preserves redo branch");
        assert_eq!(app.image_view, view);
        app.edits = original_history.clone();
    }
    context.set_pixels_per_point(1.0);
    for case in 0..12 {
        let start = start_drag(app);
        let end = start + egui::vec2(40.0, 0.0);
        frame_input(
            app,
            egui::Modifiers::ALT,
            vec![egui::Event::PointerMoved(end)],
        );
        let mut modifiers = egui::Modifiers::ALT;
        let events = match case {
            0 => {
                modifiers.ctrl = true;
                vec![]
            }
            1 => vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            2 => vec![egui::Event::WindowFocused(false)],
            3 => vec![egui::Event::PointerGone],
            4 => {
                app.cancel_view_drag();
                vec![]
            }
            5 => {
                app.generation = app.generation.next();
                vec![button(false, end, modifiers)]
            }
            6 => {
                app.palette_open = true;
                vec![]
            }
            7 => {
                app.modifiers = ModifiersState::empty();
                app.rotation_modifiers_changed();
                vec![]
            }
            8 => {
                app.timeline_open = false;
                vec![]
            }
            9 => vec![egui::Event::MouseWheel {
                phase: egui::TouchPhase::Move,
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 1.0),
                modifiers,
            }],
            10 => vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers,
            }],
            11 => {
                context.set_dragged_id("foreign-video-widget".into());
                vec![]
            }
            _ => unreachable!(),
        };
        frame_input(app, modifiers, events);
        assert!(app.video_rotation_drag.is_none(), "cancel {case}");
        assert_eq!(app.edits, original_history, "history {case}");
        assert_eq!(app.image_view, view, "selection {case}");
        app.palette_open = false;
        app.timeline_open = true;
        app.generation = generation;
        if case == 11 {
            context.stop_dragging();
        }
        frame_input(
            app,
            egui::Modifiers::NONE,
            vec![
                egui::Event::WindowFocused(true),
                button(false, end, egui::Modifiers::NONE),
            ],
        );
        assert_eq!(app.edits, original_history, "late release {case}");
        if case == 10 {
            frame_input(
                app,
                egui::Modifiers::NONE,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Secondary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
    }
    for (modifiers, timeline) in [
        (egui::Modifiers::NONE, true),
        (
            egui::Modifiers {
                alt: true,
                ctrl: true,
                ..egui::Modifiers::NONE
            },
            true,
        ),
        (
            egui::Modifiers {
                alt: true,
                command: true,
                ..egui::Modifiers::NONE
            },
            true,
        ),
        (egui::Modifiers::ALT, false),
    ] {
        app.timeline_open = timeline;
        frame_input(app, modifiers, vec![]);
        let start = app.video_rect.expect("original video").center();
        frame_input(app, modifiers, vec![egui::Event::PointerMoved(start)]);
        frame_input(app, modifiers, vec![button(true, start, modifiers)]);
        assert!(
            app.video_rotation_drag.is_none(),
            "non-rotation context {modifiers:?}, timeline {timeline}"
        );
        frame_input(app, modifiers, vec![button(false, start, modifiers)]);
        app.cancel_view_drag();
        assert_eq!(app.edits, original_history);
        app.image_view = view;
    }
    app.timeline_open = true;
    app.image_view = original_view;
    assert_eq!(app.current_position(), position);
    eprintln!(
        "PASS video Alt-drag: logical scales, ordered release, one commit/Undo, zero/redo, twelve cancellation paths, non-rotation contexts and unchanged time"
    );
}
