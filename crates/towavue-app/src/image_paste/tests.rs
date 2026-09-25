use super::*;
use std::sync::mpsc;

pub(crate) fn fixture() -> PastedImage {
    let (sent, result) = mpsc::channel();
    let job = ImagePasteJob::from_rgba(3, 2, [40, 80, 160, 255].repeat(6), move |value| {
        let _ = sent.send(value);
    })
    .expect("owned pixels");
    let pasted = result
        .recv_timeout(Duration::from_secs(10))
        .expect("prepared")
        .expect("pixels");
    drop(job);
    pasted
}

pub(crate) fn inject<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: &mpsc::Receiver<AppEvent>,
) -> TabId {
    app.image_paste.serial = app.image_paste.serial.wrapping_add(1);
    let serial = app.image_paste.serial;
    let notify = Arc::clone(&app.notify);
    let rgba = vec![
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 20, 40, 60, 128, 80, 100, 120, 0, 140, 160,
        180, 255,
    ];
    app.image_paste.pending = Some(
        ImagePasteJob::from_rgba(3, 2, rgba, move |result| {
            notify(AppEvent::ImagePasted(serial, result));
        })
        .expect("owned paste worker"),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.image_paste.pending.is_some() {
        app.handle_app_event(
            events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("paste completion"),
        );
    }
    assert!(app.path.is_none());
    app.tabs.active().expect("pasted tab").id
}

#[test]
fn untitled_paste_edits_and_save_as_preserve_original_history_and_pathless_state() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_paste::tests::untitled_paste_edits_and_save_as_preserve_original_history_and_pathless_state",
    ) else {
        return;
    };
    let (sent, events) = mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = sent.send(event);
    })
    .expect("app");
    app.ui_context = Some(fonts::test_context());
    let id = inject(&mut app, &events);
    let original = app.document_input(id).expect("private input");
    assert!(original.retained_source().is_some());
    assert!(
        app.tabs
            .active()
            .expect("tab")
            .target
            .current_path()
            .is_none()
    );
    assert!(!app.edits[&id].is_dirty());
    assert!(app.title().starts_with("image.png"));
    assert!(!app.title().contains(" *"));
    assert!(app.folder_snapshot.is_none() && !app.filmstrip_open && app.source_versions.is_empty());
    assert!(app.image_copy_request().is_some());
    for command in [
        CommandId::CopyFilePath,
        CommandId::RevealFile,
        CommandId::DeleteFile,
        CommandId::RenameFile,
        CommandId::MoveFile,
        CommandId::ToggleFilmstrip,
        CommandId::ToggleReadingMode,
    ] {
        assert!(
            !towavue_core::command_definitions()
                .iter()
                .find(|d| d.id == command)
                .expect("command")
                .is_enabled(app.command_context()),
            "{command:?}"
        );
    }
    for density in [1.0, 1.25, 2.0] {
        let context = fonts::test_context();
        context.global_style_mut(chrome::style);
        context.set_pixels_per_point(density);
        context.enable_accesskit();
        for selected in [false, true, false] {
            app.image_view.selection = selected.then_some(UnitRect::FULL);
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    app.draw_status_bar(ui, &mut actions, &mut Vec::new());
                },
            );
            let expected = if selected {
                "image.png · Selection(XY:0,0 · 3×2px)"
            } else {
                "image.png"
            };
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == expected)));
            assert!(!app.edits[&id].is_dirty());
            let tree = output.platform_output.accesskit_update.expect("tree");
            let (_, button) = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.label()
                        .is_some_and(|label| label.starts_with("Reading mode"))
                })
                .expect("reading control");
            assert!(button.is_disabled(), "pathless image at density {density}");
            assert!(actions.is_empty());
        }
    }
    assert!(!app.reading_control_enabled());
    app.dispatch(CommandId::ToggleReadingMode);
    app.begin_reading_drag(egui::Pos2::ZERO, egui::vec2(48.0, 0.0));
    assert!(!app.reading_mode && app.reading_drag.is_none() && app.reading_cursor.is_none());
    app.dispatch(CommandId::FlipHorizontal);
    app.dispatch(CommandId::RotateClockwise);
    let applied = app.edits[&id].operations().to_vec();
    app.dispatch(CommandId::Undo);
    app.dispatch(CommandId::Undo);
    assert!(
        !app.edits[&id].is_dirty(),
        "returning to the pasted baseline clears the edit warning"
    );
    app.dispatch(CommandId::Redo);
    app.dispatch(CommandId::Redo);
    assert_eq!(app.edits[&id].operations(), applied);
    let target = root.join("pasted.png");
    assert!(app.start_test_save_as(target.clone(), None));
    crate::source_save::tests::finish(&mut app, &events);
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    assert_eq!(app.path, Some(target.clone()));
    assert!(
        app.reading_control_enabled(),
        "saved image can enter reading"
    );
    assert_eq!(app.tabs.active().expect("tab").id, id);
    assert!(!app.edits[&id].is_dirty());
    assert_eq!(
        app.document_input(id).expect("retained input").path(),
        original.path()
    );
    app.dispatch(CommandId::Undo);
    app.dispatch(CommandId::Undo);
    assert!(app.edits[&id].is_dirty());
    assert_eq!(app.path, Some(target.clone()));
    assert!(app.save_source(None));
    crate::source_save::tests::finish(&mut app, &events);
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    let restored = towavue_runtime_windows::decode_image(&target).expect("saved original");
    assert_eq!(
        restored.frames[0].rgba,
        app.image.as_ref().expect("view").decoded.frames[0].rgba
    );
    assert!(!app.edits[&id].is_dirty());
}

#[test]
fn untitled_switch_close_and_paste_shortcut_preserve_document_and_text_ownership() {
    let Some(_root) = crate::tests::isolated_test_root(
        "image_paste::tests::untitled_switch_close_and_paste_shortcut_preserve_document_and_text_ownership",
    ) else {
        return;
    };
    let (sent, events) = mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = sent.send(event);
    })
    .expect("app");
    app.ui_context = Some(fonts::test_context());
    let first = inject(&mut app, &events);
    app.dispatch(CommandId::FlipHorizontal);
    app.image_view.zoom = ZoomMode::Actual;
    let first_pixels = Arc::clone(&app.image.as_ref().expect("image").decoded);
    let second = inject(&mut app, &events);
    assert_ne!(first, second);
    assert!(app.retained_images[&first].path.is_none());
    app.activate_tab(first);
    assert!(Arc::ptr_eq(
        &first_pixels,
        &app.image.as_ref().expect("restored").decoded
    ));
    assert_eq!(app.image_view.zoom, ZoomMode::Actual);
    assert_eq!(
        app.edits[&first].operations(),
        [EditOperation::FlipHorizontal]
    );
    let stroke = "Ctrl+V".parse().expect("stroke");
    assert!(app.owns_paste_shortcut(&stroke));
    let context = app.ui_context.clone().expect("context");
    let mut text = String::new();
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.text_edit_singleline(&mut text).request_focus();
        });
    });
    assert!(
        !app.owns_paste_shortcut(&stroke),
        "text editor retains Ctrl+V"
    );
    app.request_guarded(GuardedAction::CloseTab(first));
    assert!(app.pending_guard.is_some());
    assert!(app.tabs.tabs().iter().any(|tab| tab.id == first));
    app.pending_guard = None;
    app.close_tab_unchecked(first);
    assert_eq!(app.displayed_tab, Some(second));
    assert!(app.path.is_none() && app.closed_tabs.is_empty());
}

#[test]
fn untitled_canvas_resampling_and_rotation_work_without_file_identity() {
    let Some(_root) = crate::tests::isolated_test_root(
        "image_paste::tests::untitled_canvas_resampling_and_rotation_work_without_file_identity",
    ) else {
        return;
    };
    let (sent, events) = mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = sent.send(event);
    })
    .expect("app");
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    let id = inject(&mut app, &events);
    let original = Arc::clone(&app.image.as_ref().expect("pasted").decoded);
    for density in [1.0, 1.25, 2.0] {
        context.set_pixels_per_point(density);
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 576.0),
                )),
                ..Default::default()
            },
            |ui| app.draw_ui(ui, &mut Vec::new()),
        );
        let texture = app.image.as_ref().expect("pasted").texture.id();
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture)), "untitled image must render at {density}");
        assert!(app.status_details().iter().any(|field| field == "PNG"));
    }
    app.dispatch(CommandId::FreeRotateImage);
    assert!(app.rotation_dialog.is_some(), "pathless rotation dialog");
    app.rotation_dialog = None;
    for operation in [
        EditOperation::Resize(
            towavue_core::ImageResize::new(6, 4, towavue_core::ResampleFilter::Nearest)
                .expect("resize"),
        ),
        EditOperation::RotateImage(
            towavue_core::ImageRotation::new(170, (3, 2)).expect("rotation"),
        ),
    ] {
        app.push_visual_edit(operation);
        assert!(app.image_edit_pending);
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.image_edit_pending {
            app.handle_app_event(
                events
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .expect("edited image"),
            );
        }
        assert!(app.image_error.is_none(), "{:?}", app.image_error);
        assert!(app.image_materialized && app.image_copy_request().is_some());
        assert!(app.edits[&id].is_dirty());
        app.dispatch(CommandId::Undo);
        assert!(Arc::ptr_eq(
            &original,
            &app.image.as_ref().expect("original").decoded
        ));
        assert!(!app.edits[&id].is_dirty() && app.path.is_none());
    }
}

#[test]
fn pasted_baseline_closes_without_prompt_and_can_be_saved_explicitly() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_paste::tests::pasted_baseline_closes_without_prompt_and_can_be_saved_explicitly",
    ) else {
        return;
    };
    let (sent, events) = mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = sent.send(event);
    })
    .expect("app");
    app.ui_context = Some(fonts::test_context());
    let first = inject(&mut app, &events);
    app.request_guarded(GuardedAction::CloseTab(first));
    assert!(app.pending_guard.is_none());
    assert!(!app.tabs.tabs().iter().any(|tab| tab.id == first));
    let second = inject(&mut app, &events);
    assert!(!app.edits[&second].is_dirty());
    assert!(
        towavue_core::command_definitions()
            .iter()
            .find(|command| command.id == CommandId::Save)
            .expect("Save")
            .is_enabled(app.command_context())
    );
    let target = root.join(DEFAULT_NAME);
    assert!(app.start_test_save_as(target.clone(), None));
    crate::source_save::tests::finish(&mut app, &events);
    assert!(target.is_file() && app.path.as_ref() == Some(&target));
    assert!(!app.edits[&second].is_dirty());
}

#[test]
fn pasted_tabs_preview_retained_pixels_and_never_expose_private_paths() {
    let Some(_root) = crate::tests::isolated_test_root(
        "image_paste::tests::pasted_tabs_preview_retained_pixels_and_never_expose_private_paths",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let (sent, events) = mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = sent.send(event);
        })
        .expect("app");
        let context = fonts::test_context();
        context.global_style_mut(|style| {
            chrome::style(style);
            style.interaction.tooltip_delay = 0.0;
            style.interaction.show_tooltips_only_when_still = false;
        });
        context.set_pixels_per_point(density);
        app.ui_context = Some(context.clone());
        let first = inject(&mut app, &events);
        let texture = app.image.as_ref().expect("pasted pixels").texture.id();
        let private = app
            .document_input(first)
            .expect("input")
            .path()
            .to_string_lossy()
            .into_owned();
        app.tabs.close_gallery(app.tabs.gallery().expect("gallery"));
        let mut time = 0.0;
        for background in [false, true] {
            let active = if background {
                inject(&mut app, &events)
            } else {
                first
            };
            let mut output = egui::FullOutput::default();
            for _ in 0..8 {
                time += 0.1;
                output = context.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(960.0, 576.0),
                        )),
                        events: vec![egui::Event::PointerMoved(egui::pos2(90.0, 16.0))],
                        ..Default::default()
                    },
                    |ui| {
                        let mut actions = Vec::new();
                        app.draw_top_bar(ui, &mut actions);
                        assert!(actions.is_empty());
                    },
                );
            }
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture)),
                "retained hover pixels at {density}"
            );
            let tab = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect)
                        if rect.corner_radius == egui::CornerRadius::same(3)
                            && rect.rect.contains(egui::pos2(90.0, 16.0)) =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("painted tab bounds including its close button");
            let pixels = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                        Some(mesh.calc_bounds())
                    }
                    _ => None,
                })
                .expect("preview pixels");
            assert!(
                (pixels.center().x - tab.center().x).abs() <= 1.0 / density,
                "preview centers on the full tab, not its shortened label: {pixels:?}, {tab:?}"
            );
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text()),
                    _ => None,
                })
                .collect();
            assert!(labels.contains(&DEFAULT_NAME));
            assert!(labels.iter().all(|text| !text.contains(&private)));
            assert_eq!(app.tabs.active_id(), Some(active));
            assert!(app.tab_preview.is_idle());
            assert!(
                app.tabs
                    .tabs()
                    .iter()
                    .find(|tab| tab.id == first)
                    .expect("pasted tab")
                    .target
                    .current_path()
                    .is_none()
            );
        }
        assert!(app.retained_images[&first].path.is_none());
        app.request_guarded(GuardedAction::CloseTab(first));
        assert!(app.pending_guard.is_none());
        assert!(app.retained_tab_preview(first, None).is_none());
    }
}
