use super::*;
use crate::rotation::tests::application;

fn button(pressed: bool, pos: egui::Pos2, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    }
}

fn frame<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    modifiers: egui::Modifiers,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let context = app.ui_context.clone().expect("context");
    context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(500.0, 400.0),
            )),
            modifiers,
            events,
            ..Default::default()
        },
        |ui| app.draw_image(ui),
    )
}

#[test]
fn held_alt_rotation_previews_without_editing_then_commits_once_and_undo_restores_source() {
    for zoom in [ZoomMode::Fit, ZoomMode::Custom(40.0)] {
        let (mut app, events) = application();
        let source = app.image.as_ref().expect("image").decoded.clone();
        let tab = app.tabs.active().expect("tab").id;
        app.push_visual_edit(EditOperation::RotateClockwise);
        app.image_view.selection = Some(UnitRect {
            min: UnitPoint { x: 0.25, y: 0.25 },
            max: UnitPoint { x: 0.75, y: 0.75 },
        });
        app.image_view.zoom = zoom;
        let view = app.image_view;
        let history = app.edits.clone();
        let start = egui::pos2(200.0, 200.0);
        let end = egui::pos2(260.0, 260.0);
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![egui::Event::PointerMoved(start)],
        );
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![button(true, start, egui::Modifiers::ALT)],
        );
        let output = frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![egui::Event::PointerMoved(end)],
        );
        assert_eq!(app.rotation_drag.as_ref().expect("drag").tenths, 300);
        assert_eq!(
            app.rotation_drag
                .as_ref()
                .expect("drag")
                .preview
                .transform
                .size,
            (6.0, 8.0)
        );
        assert!(output.shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.vertices.len() == 16)
        ));
        assert_eq!(app.edits, history);
        assert_eq!(app.image_view, view);
        assert!(
            !app.image_edit_pending && app.rotation_dialog.is_none() && app.view_drag.is_none()
        );
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![button(false, end, egui::Modifiers::ALT)],
        );
        assert!(app.rotation_drag.is_none() && app.image_edit_pending);
        let rotation = ImageRotation::new(300, (6, 8)).expect("rotation");
        assert_eq!(
            app.edits[&tab].operations(),
            &[
                EditOperation::RotateClockwise,
                EditOperation::RotateImage(rotation)
            ]
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.image_edit_pending {
            app.handle_app_event(
                events
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .expect("worker"),
            );
        }
        assert!(app.image_error.is_none() && app.image_materialized);
        assert_eq!(
            app.image.as_ref().expect("rotated").dimensions(),
            rotation.size()
        );
        let committed = app.edits.clone();
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![button(false, end, egui::Modifiers::ALT)],
        );
        assert_eq!(
            app.edits, committed,
            "duplicate release is not another edit"
        );
        app.dispatch(CommandId::Undo);
        assert_eq!(app.edits[&tab].operations(), history[&tab].operations());
        assert!(Arc::ptr_eq(
            &source,
            &app.image.as_ref().expect("original").decoded
        ));
    }
}

#[test]
fn rotation_drag_cancels_on_modifier_escape_focus_pointer_and_context_interruptions() {
    for case in 0..9 {
        let (mut app, _) = application();
        let start = egui::pos2(200.0, 200.0);
        let end = egui::pos2(260.0, 220.0);
        app.image_view.selection = Some(UnitRect::FULL);
        let view = app.image_view;
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![egui::Event::PointerMoved(start)],
        );
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![button(true, start, egui::Modifiers::ALT)],
        );
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![egui::Event::PointerMoved(end)],
        );
        assert!(app.rotation_drag.is_some());
        let mut modifiers = egui::Modifiers::ALT;
        let mut input = vec![];
        match case {
            0 => modifiers = egui::Modifiers::NONE,
            1 => input.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }),
            2 => input.push(egui::Event::WindowFocused(false)),
            3 => input.push(egui::Event::PointerGone),
            4 => {
                app.cancel_view_drag();
            }
            5 => {
                app.media_generation += 1;
                input.push(button(false, end, modifiers));
            }
            6 => {
                app.palette_open = true;
            }
            7 => {
                app.modifiers = ModifiersState::empty();
                app.rotation_modifiers_changed();
            }
            8 => modifiers.ctrl = true,
            _ => unreachable!(),
        }
        let output = frame(&mut app, modifiers, input);
        assert!(
            output.shapes.iter().any(
                |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.vertices.len() == 16)
            ),
            "original remains visible on cancellation {case}"
        );
        assert!(app.rotation_drag.is_none(), "case {case}");
        assert_eq!(app.image_view, view, "case {case}");
        assert!(
            app.edits.is_empty() && !app.image_edit_pending,
            "case {case}"
        );
        app.palette_open = false;
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![button(false, end, egui::Modifiers::ALT)],
        );
        assert!(
            app.edits.is_empty() && app.view_drag.is_none(),
            "late release {case}"
        );
    }
}

#[test]
fn rotation_commits_the_first_release_without_combining_later_gestures() {
    let start = egui::pos2(200.0, 200.0);
    let end = egui::pos2(240.0, 200.0);
    for already_held in [false, true] {
        for later_alt in [false, true] {
            let (mut app, _) = application();
            frame(
                &mut app,
                egui::Modifiers::ALT,
                vec![egui::Event::PointerMoved(start)],
            );
            let mut events = vec![button(true, start, egui::Modifiers::ALT)];
            if already_held {
                frame(&mut app, egui::Modifiers::ALT, std::mem::take(&mut events));
            }
            let later_modifiers = if later_alt {
                egui::Modifiers::ALT
            } else {
                egui::Modifiers::NONE
            };
            events.extend([
                egui::Event::PointerMoved(end),
                button(false, end, egui::Modifiers::ALT),
                button(true, egui::pos2(280.0, 200.0), later_modifiers),
                button(false, egui::pos2(320.0, 200.0), later_modifiers),
            ]);
            frame(&mut app, egui::Modifiers::NONE, events);
            let tab = app.tabs.active().expect("tab").id;
            assert_eq!(
                app.edits.get(&tab).map(|edit| edit.operations()),
                Some(
                    [EditOperation::RotateImage(
                        ImageRotation::new(200, (8, 6)).expect("rotation")
                    )]
                    .as_slice()
                ),
                "already_held={already_held}, later_alt={later_alt}"
            );
            assert!(app.rotation_drag.is_none());
        }
    }
}

#[test]
fn rotation_does_not_borrow_alt_from_a_later_gesture_at_the_same_coordinates() {
    let start = egui::pos2(200.0, 200.0);
    let end = egui::pos2(240.0, 200.0);
    for first_alt in [false, true] {
        for final_modifiers in [egui::Modifiers::NONE, egui::Modifiers::ALT] {
            let (mut app, _) = application();
            frame(
                &mut app,
                egui::Modifiers::NONE,
                vec![egui::Event::PointerMoved(start)],
            );
            frame(
                &mut app,
                final_modifiers,
                vec![
                    button(
                        true,
                        start,
                        if first_alt {
                            egui::Modifiers::ALT
                        } else {
                            egui::Modifiers::NONE
                        },
                    ),
                    button(false, end, egui::Modifiers::NONE),
                    button(true, start, egui::Modifiers::ALT),
                    button(false, end, egui::Modifiers::ALT),
                ],
            );
            assert!(
                app.edits.is_empty() && app.rotation_drag.is_none(),
                "first_alt={first_alt}"
            );
        }
    }
}

#[test]
fn batched_rotation_uses_release_position_and_zero_click_or_invalid_canvas_do_not_edit() {
    let start = egui::pos2(200.0, 200.0);
    let end = egui::pos2(140.0, 300.0);
    for zero in [false, true] {
        let (mut app, _) = application();
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![egui::Event::PointerMoved(start)],
        );
        let end = if zero { start } else { end };
        frame(
            &mut app,
            egui::Modifiers::NONE,
            vec![
                button(true, start, egui::Modifiers::ALT),
                egui::Event::PointerMoved(end),
                button(false, end, egui::Modifiers::ALT),
                egui::Event::PointerMoved(egui::pos2(450.0, 300.0)),
            ],
        );
        assert!(app.rotation_drag.is_none());
        if zero {
            assert!(app.edits.is_empty());
        } else {
            let tab = app.tabs.active().expect("tab").id;
            assert_eq!(
                app.edits[&tab].operations(),
                &[EditOperation::RotateImage(
                    ImageRotation::new(-300, (8, 6)).expect("value")
                )]
            );
        }
    }
    let (mut app, _) = application();
    frame(
        &mut app,
        egui::Modifiers::ALT,
        vec![egui::Event::PointerMoved(start)],
    );
    frame(
        &mut app,
        egui::Modifiers::ALT,
        vec![button(true, start, egui::Modifiers::ALT)],
    );
    app.rotation_drag
        .as_mut()
        .expect("drag")
        .preview
        .transform
        .size = (10000.0, 10000.0);
    frame(
        &mut app,
        egui::Modifiers::ALT,
        vec![button(
            false,
            egui::pos2(290.0, 200.0),
            egui::Modifiers::ALT,
        )],
    );
    assert!(app.edits.is_empty() && !app.image_edit_pending && app.rotation_drag.is_none());
}

#[test]
fn rotation_requires_an_owned_alt_press_on_the_image_and_keeps_ordinary_selection() {
    let start = egui::pos2(200.0, 200.0);
    let end = egui::pos2(260.0, 220.0);
    for case in 0..7 {
        let (mut app, _) = application();
        let context = app.ui_context.clone().expect("context");
        let mut modifiers = egui::Modifiers::ALT;
        let mut origin = start;
        match case {
            0 => origin = egui::pos2(-10.0, -10.0),
            1 => modifiers.ctrl = true,
            2 => app.palette_open = true,
            3 => app.grid_open = true,
            4 => app.filmstrip_open = true,
            5 => app.image_error = Some("test error".into()),
            6 => {}
            _ => unreachable!(),
        }
        frame(&mut app, modifiers, vec![egui::Event::PointerMoved(origin)]);
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 400.0),
                )),
                modifiers,
                events: vec![
                    button(true, origin, modifiers),
                    egui::Event::PointerMoved(end),
                    button(false, end, modifiers),
                ],
                ..Default::default()
            },
            |ui| {
                if case == 6 {
                    context.set_dragged_id("other-widget".into());
                }
                app.draw_image(ui);
            },
        );
        assert!(
            app.rotation_drag.is_none() && app.edits.is_empty(),
            "case {case}"
        );
    }
    let (mut app, _) = application();
    frame(
        &mut app,
        egui::Modifiers::NONE,
        vec![egui::Event::PointerMoved(start)],
    );
    frame(
        &mut app,
        egui::Modifiers::NONE,
        vec![
            button(true, start, egui::Modifiers::NONE),
            egui::Event::PointerMoved(end),
            button(false, end, egui::Modifiers::NONE),
        ],
    );
    assert!(
        app.rotation_drag.is_none() && app.edits.is_empty() && app.image_view.selection.is_some()
    );
}

#[test]
fn rotation_drag_geometry_is_logical_point_based_and_checkerboard_work_is_viewport_bounded() {
    for pixels_per_point in [1.0, 2.0] {
        let (mut app, _) = application();
        let context = app.ui_context.clone().expect("context");
        context.set_pixels_per_point(pixels_per_point);
        app.image_view.zoom = ZoomMode::Custom(100000.0);
        let view = app.image_view;
        let start = egui::pos2(200.0, 200.0);
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![egui::Event::PointerMoved(start)],
        );
        frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![button(true, start, egui::Modifiers::ALT)],
        );
        let output = frame(
            &mut app,
            egui::Modifiers::ALT,
            vec![egui::Event::PointerMoved(egui::pos2(290.0, 200.0))],
        );
        assert_eq!(app.rotation_drag.as_ref().expect("drag").tenths, 450);
        assert!(
            output.shapes.len() < 2000,
            "checkerboard only covers the viewport"
        );
        assert_eq!(app.image_view, view);
        app.cancel_view_drag();
        assert!(app.edits.is_empty());
    }
}
