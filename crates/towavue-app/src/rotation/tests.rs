use super::*;
use std::sync::mpsc;

#[test]
fn video_resize_rejects_wrong_media_or_unavailable_source() {
    let (mut app, _) = application();
    app.image_view.selection = Some(UnitRect::FULL);
    app.image_view.zoom = ZoomMode::Custom(2.0);
    app.image_view.pan = (13.0, -7.0);
    let view = app.image_view;
    let edits = app.edits.clone();
    let resize = towavue_core::VideoResize::new(
        (96, 48),
        towavue_core::ResampleFilter::Lanczos,
        (64, 48),
        2.0,
    )
    .expect("resize");
    for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
        app.media_kind = Some(kind);
        app.push_visual_edit(EditOperation::ResizeVideo(resize));
        assert_eq!(app.edits, edits);
        assert_eq!(app.image_view, view);
    }
    assert_eq!(
        towavue_runtime_windows::video_edit_geometry(
            (64, 48),
            2.0,
            towavue_runtime_windows::VideoOrientation::default(),
            &[EditOperation::ResizeVideo(resize)],
            16384
        )
        .expect("GPU resize geometry"),
        (96, 48, 1.0)
    );
    let transformed = ImageTransform::new(
        (64, 48),
        &[
            EditOperation::ResizeVideo(resize),
            EditOperation::RotateClockwise,
        ],
    );
    assert_eq!(transformed.size, (48.0, 96.0));
    assert_eq!(transformed.pixel_aspect(2.0), 1.0);
    assert_eq!(
        transformed.uv,
        towavue_runtime_windows::VideoOrientation::default().source_uv()
    );
}

#[test]
fn video_rotation_rejects_missing_video_frame_without_changing_visual_state() {
    let (mut app, _) = application();
    app.media_kind = Some(MediaKind::Video);
    app.image_view.selection = Some(UnitRect::FULL);
    let view = app.image_view;
    app.push_visual_edit(EditOperation::RotateVideo(
        towavue_core::VideoRotation::new(317, (64, 48), 2.0).expect("rotation"),
    ));
    assert_eq!(app.image_view, view);
    assert!(app.edits.is_empty() && !app.image_edit_pending && app.rotation_dialog.is_none());
}

pub(super) fn application() -> (
    Application<impl Fn(AppEvent) + Send + Sync>,
    mpsc::Receiver<AppEvent>,
) {
    let (sender, events) = mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = sender.send(event);
    })
    .expect("app");
    let context = fonts::test_context();
    context.enable_accesskit();
    app.ui_context = Some(context.clone());
    app.shortcuts = shortcuts::defaults();
    let path = PathBuf::from("rotation-dialog.png");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
    app.path = Some(path.clone());
    app.media_kind = Some(MediaKind::Image);
    app.displayed_tab = Some(tab);
    let source = Arc::new(DecodedImage {
        format: "test",
        frames: vec![towavue_runtime_windows::DecodedImageFrame {
            width: 8,
            height: 6,
            rgba: (0..48_u8)
                .flat_map(|n| [n * 5, 240 - n * 5, 110, 255])
                .collect(),
            delay: Duration::ZERO,
        }],
    });
    app.image = Some(ImagePresentation::from_decoded(&context, &path, source).expect("image"));
    (app, events)
}

#[test]
fn numeric_rotation_dialog_validates_accessible_input_apply_and_escape_without_mutating_the_app() {
    let (mut app, _) = application();
    app.dispatch(CommandId::FreeRotateImage);
    let context = app.ui_context.clone().expect("context");
    let mut dialog = app.rotation_dialog.take().expect("dialog");
    let frame = |dialog: &mut RotationDialog, events| {
        let mut action = None;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                action = dialog.show(ui.ctx());
            },
        );
        (action, output)
    };
    frame(&mut dialog, vec![]);
    let (_, output) = frame(&mut dialog, vec![]);
    let tree = output.platform_output.accesskit_update.expect("tree");
    let input = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Rotation angle in degrees"))
        .map(|(id, _)| *id)
        .expect("angle field");
    let set = |text: &str| {
        egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
            action: egui::accesskit::Action::SetValue,
            target_tree: egui::accesskit::TreeId::ROOT,
            target_node: input,
            data: Some(egui::accesskit::ActionData::Value(text.into())),
        })
    };
    for value in ["", "-", "NaN", "inf", "180.1", "-180.1"] {
        let (action, output) = frame(&mut dialog, vec![set(value)]);
        assert!(action.is_none() && dialog.value().is_none());
        assert!(
            output
                .platform_output
                .accesskit_update
                .expect("tree")
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Apply rotation") && node.is_disabled())
        );
    }
    let (_, output) = frame(&mut dialog, vec![set("31.74")]);
    assert_eq!(dialog.value().expect("rounded").tenths(), 317);
    let tree = output.platform_output.accesskit_update.expect("tree");
    let apply = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Apply rotation"))
        .map(|(id, _)| *id)
        .expect("apply");
    let (action, _) = frame(
        &mut dialog,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: apply,
                data: None,
            },
        )],
    );
    assert_eq!(action.flatten().expect("apply").tenths(), 317);
    let (action, _) = frame(
        &mut dialog,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(action, Some(None));
    assert!(app.edits.is_empty());
    for angle in ["-180", "180", "0", "-0.04"] {
        dialog.angle = angle.into();
        assert!(dialog.value().is_some());
    }
    dialog.transform.size = (10000.0, 10000.0);
    dialog.angle = "45".into();
    assert!(dialog.value().is_none(), "expanded canvas exceeds budget");
}

#[test]
fn rotation_preview_mesh_fits_the_canvas_and_preserves_edited_source_uvs() {
    let transform = ImageTransform::new(
        (8, 6),
        &[
            EditOperation::Crop(PixelCrop {
                x: 1,
                y: 1,
                width: 6,
                height: 4,
            }),
            EditOperation::RotateClockwise,
            EditOperation::FlipHorizontal,
        ],
    );
    let rect = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 200.0));
    let uv = transformed_image_mesh(egui::TextureId::Managed(0), rect, transform)
        .vertices
        .into_iter()
        .map(|v| v.uv)
        .collect::<Vec<_>>();
    for angle in [-1800, -900, -317, -1, 0, 1, 317, 900, 1800] {
        let rotation = ImageRotation::new(angle, (4, 6)).expect("angle");
        let mesh = preview_mesh(rect, egui::TextureId::Managed(0), transform, rotation);
        assert_eq!(mesh.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(), uv);
        assert!(
            mesh.vertices
                .iter()
                .all(|v| rect.expand(0.001).contains(v.pos))
        );
        assert!((mesh.calc_bounds().center() - rect.center()).length() < 0.001);
        let actual = (mesh.vertices[3].pos - mesh.vertices[0].pos).normalized();
        let (sin, cos) = (f32::from(angle) / 10.0).to_radians().sin_cos();
        assert!(
            (actual - egui::vec2(cos, sin)).length() < 0.001,
            "clockwise preview at {angle}"
        );
    }
}

#[test]
fn rotation_dialog_rejects_changed_context_and_unavailable_sources() {
    for case in 0..12 {
        let (mut app, _) = application();
        app.dispatch(CommandId::FreeRotateImage);
        let token = app.rotation_dialog.as_ref().expect("dialog").token;
        let mut value = ImageRotation::new(317, (8, 6)).expect("value");
        match case {
            0 => {
                app.tabs
                    .open_new(PathBuf::from("other.png"), MediaKind::Image);
            }
            1 => app.media_generation += 1,
            2 => app.image_edit_generation += 1,
            3 => app.path = Some(PathBuf::from("other.png")),
            4 => app.image = None,
            5 => app.reading_mode = true,
            6 => app.image_edit_pending = true,
            7 => app.image_error = Some("test failure".into()),
            8 => app.media_kind = Some(MediaKind::Video),
            9 => {
                let tab = app.tabs.active().expect("tab").id;
                app.edits
                    .entry(tab)
                    .or_default()
                    .push(EditOperation::FlipHorizontal, MediaKind::Image);
            }
            10 => value = ImageRotation::new(317, (6, 8)).expect("wrong geometry"),
            11 => app.image = application().0.image.take(),
            _ => unreachable!(),
        }
        let edits = app.edits.clone();
        let view = app.image_view;
        app.handle_ui_action(UiAction::FinishRotation(token, Some(value)));
        assert!(app.rotation_dialog.is_none(), "case {case}");
        assert_eq!(app.edits, edits, "case {case}");
        assert_eq!(app.image_view, view, "case {case}");
    }
    for case in 0..6 {
        let (mut app, _) = application();
        match case {
            0 => app.image = None,
            1 => app.reading_mode = true,
            2 => app.image_edit_pending = true,
            3 => app.image_error = Some("test failure".into()),
            4 => app.media_kind = Some(MediaKind::Video),
            5 => app.media_kind = Some(MediaKind::Audio),
            _ => unreachable!(),
        }
        app.dispatch(CommandId::FreeRotateImage);
        assert!(app.rotation_dialog.is_none(), "case {case}");
        assert!(app.edits.is_empty());
    }
}

#[test]
fn compact_rotation_dialog_scrolls_to_apply_and_cancel_controls() {
    let (mut app, _) = application();
    app.dispatch(CommandId::FreeRotateImage);
    let mut dialog = app.rotation_dialog.take().expect("dialog");
    let context = app.ui_context.clone().expect("context");
    let frame = |dialog: &mut RotationDialog, events| {
        let mut action = None;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 300.0),
                )),
                events,
                ..Default::default()
            },
            |ui| action = dialog.show(ui.ctx()),
        );
        (
            action,
            output.platform_output.accesskit_update.expect("tree"),
        )
    };
    frame(&mut dialog, vec![]);
    for _ in 0..12 {
        frame(
            &mut dialog,
            vec![
                egui::Event::PointerMoved(egui::pos2(160.0, 150.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: egui::vec2(0.0, -150.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    let (_, tree) = frame(&mut dialog, vec![]);
    for label in ["Apply rotation", "Cancel"] {
        let (_, node) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .expect("button");
        let bounds = node.bounds().expect("bounds");
        assert!(
            bounds.x0 >= 0.0 && bounds.x1 <= 320.0 && bounds.y0 >= 0.0 && bounds.y1 <= 300.0,
            "{label}: {bounds:?}"
        );
    }
    let cancel = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Cancel"))
        .expect("cancel")
        .0;
    let (action, _) = frame(
        &mut dialog,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: cancel,
                data: None,
            },
        )],
    );
    assert_eq!(action, Some(None));
}

#[test]
fn rotation_modal_routes_keyboard_and_slider_input_and_restores_selection_focus() {
    let (mut app, _) = application();
    let context = app.ui_context.clone().expect("context");
    let frame = |app: &mut Application<_>, events| {
        let mut actions = Vec::new();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.draw_ui(ui, &mut actions),
        );
        for action in actions {
            app.handle_ui_action(action);
        }
        output.platform_output.accesskit_update.expect("tree")
    };
    app.image_view.selection = Some(UnitRect::FULL);
    frame(&mut app, vec![]);
    selection::focus_first(&context, app.selection_identity());
    frame(&mut app, vec![]);
    let focus = context
        .memory(|memory| memory.focused())
        .expect("selection focus");
    assert!(selection::has_focus(&context));
    app.process_shortcut("Ctrl+Shift+R".parse().expect("shortcut"));
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    let tree = frame(
        &mut app,
        vec![
            key(
                egui::Key::A,
                egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..egui::Modifiers::NONE
                },
            ),
            egui::Event::Text("-31.7".into()),
        ],
    );
    assert_eq!(
        app.rotation_dialog
            .as_ref()
            .expect("dialog")
            .value()
            .expect("angle")
            .tenths(),
        -317
    );
    let (slider, node) = tree
        .nodes
        .iter()
        .find(|(_, node)| {
            node.role() == egui::accesskit::Role::Slider && node.label() == Some("Rotation angle")
        })
        .expect("slider");
    let bounds = node.bounds().expect("slider bounds");
    frame(
        &mut app,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: *slider,
                data: Some(egui::accesskit::ActionData::NumericValue(45.0)),
            },
        )],
    );
    assert_eq!(
        app.rotation_dialog
            .as_ref()
            .expect("dialog")
            .value()
            .expect("slider angle")
            .tenths(),
        450
    );
    let pos = egui::pos2(
        (bounds.x0 + (bounds.x1 - bounds.x0) * 0.25) as f32,
        ((bounds.y0 + bounds.y1) / 2.0) as f32,
    );
    for pressed in [true, false] {
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(
        app.rotation_dialog
            .as_ref()
            .expect("dialog")
            .value()
            .expect("pointer angle")
            .tenths()
            < 0
    );
    assert!(app.edits.is_empty());
    frame(
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(app.rotation_dialog.is_none());
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    assert_eq!(context.memory(|memory| memory.focused()), Some(focus));
    assert!(app.edits.is_empty());
    assert_eq!(app.image_view.selection, Some(UnitRect::FULL));
    for overlay in [CommandId::ToggleGridMenu, CommandId::ToggleCommandPalette] {
        app.dispatch(overlay);
        frame(&mut app, vec![]);
        app.dispatch(CommandId::FreeRotateImage);
        assert!(!app.grid_open && !app.palette_open);
        assert_eq!(app.guard_return_focus.map(|(_, id)| id), Some(focus));
        frame(&mut app, vec![]);
        frame(
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        assert!(app.rotation_dialog.is_none());
        assert_eq!(context.memory(|memory| memory.focused()), Some(focus));
        assert!(app.edits.is_empty());
    }
}

#[test]
fn rotation_dialog_commit_is_atomic_cancel_and_identity_preserve_state_and_old_tokens_cannot_apply()
{
    let (mut app, events) = application();
    let tab = app.tabs.active().expect("tab").id;
    app.push_visual_edit(EditOperation::RotateClockwise);
    app.image_view.selection = Some(UnitRect::FULL);
    app.image_view.crop_preview = true;
    app.image_view.zoom = ZoomMode::Custom(2.0);
    app.image_view.pan = (13.0, -7.0);
    let view = app.image_view;
    let history = app.edits.clone();
    for zero in [false, true] {
        app.process_shortcut("Ctrl+Shift+R".parse().expect("shortcut"));
        let dialog = app.rotation_dialog.as_ref().expect("dialog");
        let token = dialog.token;
        let identity = dialog.value().expect("zero");
        assert_eq!(identity.source_size(), (6, 8));
        app.handle_ui_action(UiAction::FinishRotation(token, zero.then_some(identity)));
        assert!(app.rotation_dialog.is_none());
        assert_eq!(app.image_view, view);
        assert_eq!(app.edits, history);
    }
    app.dispatch(CommandId::FreeRotateImage);
    let stale = app.rotation_dialog.as_ref().expect("dialog").token;
    app.handle_ui_action(UiAction::FinishRotation(stale, None));
    app.dispatch(CommandId::FreeRotateImage);
    let token = app.rotation_dialog.as_ref().expect("dialog").token;
    let value = ImageRotation::new(317, (6, 8)).expect("value");
    app.handle_ui_action(UiAction::FinishRotation(stale, Some(value)));
    assert_eq!(
        app.rotation_dialog
            .as_ref()
            .expect("new dialog retained")
            .token,
        token
    );
    app.dispatch(CommandId::CloseTab);
    app.request_guarded(GuardedAction::Exit);
    assert!(app.rotation_dialog.is_some() && app.pending_guard.is_none());
    app.handle_ui_action(UiAction::FinishRotation(token, Some(value)));
    assert!(app.rotation_dialog.is_none() && app.image_edit_pending);
    assert_eq!(
        app.edits[&tab].operations(),
        &[
            EditOperation::RotateClockwise,
            EditOperation::RotateImage(value)
        ]
    );
    assert!(app.image_view.selection.is_none() && !app.image_view.crop_preview);
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.image_edit_pending {
        app.handle_app_event(
            events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("materialized"),
        );
    }
    assert!(app.image_materialized && app.image_error.is_none());
    assert_eq!(
        app.image.as_ref().expect("image").dimensions(),
        value.size()
    );
    let committed = app.edits.clone();
    app.handle_ui_action(UiAction::FinishRotation(token, Some(value)));
    assert_eq!(app.edits, committed);
    app.dispatch(CommandId::Undo);
    assert_eq!(
        app.edits[&tab].operations(),
        &[EditOperation::RotateClockwise]
    );
}
