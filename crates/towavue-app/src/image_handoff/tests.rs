use super::*;

mod gpu;
mod release;

type App = Application<fn(AppEvent)>;

fn decoded(width: u32, height: u32, color: [u8; 4]) -> Arc<DecodedImage> {
    Arc::new(DecodedImage {
        format: "test",
        frames: vec![towavue_runtime_windows::DecodedImageFrame {
            width,
            height,
            rgba: color.repeat((width * height) as usize),
            delay: Duration::ZERO,
        }],
    })
}

fn fixture(root: &Path) -> (App, egui::Context, TabId) {
    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
    let context = fonts::test_context();
    let path = root.join("old.png");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
    app.path = Some(path.clone());
    app.displayed_tab = Some(tab);
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    app.ui_context = Some(context.clone());
    app.image = Some(
        ImagePresentation::from_decoded(&context, &path, decoded(160, 90, [20, 40, 60, 255]))
            .expect("image"),
    );
    (app, context, tab)
}

fn frame(app: &mut App, context: &egui::Context) -> egui::FullOutput {
    frame_with_events(app, context, Vec::new())
}

fn frame_with_events(
    app: &mut App,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run_ui(
        egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        },
        |ui| app.draw_ui(ui, &mut Vec::new()),
    )
}

fn navigate_pending(app: &mut App, path: PathBuf) {
    app.navigate_to_unchecked(path);
    // Hold completion deterministically: discard real worker work, retaining the app's ticket.
    // Tests deliver controlled chunks below, including intentionally stale generations.
    app.image_loader.request(Vec::new());
    assert!(app.image_loading && app.image.is_none());
}

#[test]
fn handoff_is_original_display_only_until_the_latest_source_is_ready() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_handoff::tests::handoff_is_original_display_only_until_the_latest_source_is_ready",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let (mut app, context, tab) = fixture(&root);
        context.set_pixels_per_point(density);
        app.image_view.zoom = ZoomMode::Custom(2.0);
        app.image_view.pan = (10.0, -5.0);
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        app.image_view.selection = Some(
            PixelCrop {
                x: 18,
                y: 32,
                width: 54,
                height: 96,
            }
            .unit_rect((90, 160)),
        );
        let selection_status = app.visual_selection_status();
        assert_eq!(
            selection_status.as_deref(),
            Some("Selection: x=18 y=32 · 54×96 px")
        );
        let old = app.image.as_ref().expect("original").texture.id();
        for _ in 0..3 {
            frame(&mut app, &context);
        }
        assert_eq!(context.pixels_per_point(), density);
        let bounds = |output: &egui::FullOutput| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == old => Some(mesh.calc_bounds()),
                _ => None,
            })
        };
        let before = bounds(&frame(&mut app, &context)).expect("original mesh");
        navigate_pending(&mut app, root.join("next.png"));
        assert_eq!(
            app.visual_selection_status(),
            selection_status,
            "handoff reports displayed pixels, not the pending source"
        );
        let first_generation = app.image_generation;
        let view = app.image_view;
        let history = app.edits.clone();
        app.finish_image_preview(
            root.join("next.png"),
            app.image_preview_generation,
            towavue_runtime_windows::CachedImagePreview {
                source_size: (120, 80),
                image: towavue_runtime_windows::PreviewImage {
                    width: 1,
                    height: 1,
                    rgba: vec![255, 0, 0, 255],
                },
            },
        );
        let preview = app.image_previews[&root.join("next.png")].texture.id();
        let output = frame(&mut app, &context);
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Mesh(mesh) if mesh.texture_id == preview)));
        assert_eq!(
            bounds(&output),
            Some(before),
            "freeze old transform before clearing history"
        );
        assert_eq!(app.displayed_image_path(), Some(&root.join("old.png")));
        assert_eq!(app.path.as_ref(), Some(&root.join("next.png")));
        assert!(
            output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.text().ends_with("old.png · Selection: x=18 y=32 · 54×96 px")))
        );
        assert!(
            !output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.text().contains("Loading images")))
        );
        assert!(
            app.restored_ui_textures(&context)
                .iter()
                .any(|(id, _)| *id == old),
            "graphics recovery must restore the held original"
        );
        for command in [
            CommandId::RotateClockwise,
            CommandId::Undo,
            CommandId::Save,
            CommandId::ExportAs,
            CommandId::MetadataExportOptions,
            CommandId::CopyImage,
            CommandId::CopyFilePath,
            CommandId::RevealFile,
            CommandId::ZoomIn,
            CommandId::ToggleReadingMode,
            CommandId::SelectAll,
        ] {
            assert!(
                !command_definitions()
                    .iter()
                    .find(|d| d.id == command)
                    .expect("command")
                    .is_enabled(app.command_context())
            );
            app.dispatch(command);
        }
        for command in [
            CommandId::NextSameKind,
            CommandId::PreviousSameKind,
            CommandId::JumpImagesForward10,
            CommandId::CloseTab,
            CommandId::NextTab,
        ] {
            assert!(
                command_definitions()
                    .iter()
                    .find(|d| d.id == command)
                    .expect("command")
                    .is_enabled(app.command_context())
            );
        }
        app.push_edit(EditOperation::FlipHorizontal);
        app.undo_edit(false);
        let point = egui::pos2(320.0, 240.0);
        for pressed in [true, false] {
            let output = frame_with_events(
                &mut app,
                &context,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, 60.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::CTRL,
                    },
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            assert_eq!(bounds(&output), Some(before));
        }
        assert!(!app.export_current(false, None));
        assert!(app.image_copy_request().is_none() && app.pending_dialog.is_none());
        assert_eq!(app.edits, history);
        assert_eq!(app.image_view, view);
        navigate_pending(&mut app, root.join("latest.png"));
        assert_eq!(bounds(&frame(&mut app, &context)), Some(before));
        app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
            generation: first_generation,
            first_index: 0,
            total: 1,
            images: vec![(root.join("next.png"), Ok(decoded(10, 10, [255, 0, 0, 255])))],
        });
        assert!(app.image_handoff.is_some() && app.image.is_none());
        app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
            generation: app.image_generation,
            first_index: 0,
            total: 1,
            images: vec![(
                root.join("latest.png"),
                Ok(decoded(120, 80, [0, 255, 0, 255])),
            )],
        });
        assert!(app.image_handoff.is_none() && !app.image_loading);
        assert_eq!(
            app.image.as_ref().expect("new original").dimensions(),
            (120, 80)
        );
        assert!(bounds(&frame(&mut app, &context)).is_none());
        assert!(
            context.tex_manager().read().meta(old).is_none(),
            "release the outgoing texture"
        );
    }
}

#[test]
fn handoff_does_not_survive_failure_departure_or_last_tab_close() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_handoff::tests::handoff_does_not_survive_failure_departure_or_last_tab_close",
    ) else {
        return;
    };
    for ending in 0..4 {
        let (mut app, context, tab) = fixture(&root);
        let old = app.image.as_ref().expect("original").texture.id();
        navigate_pending(&mut app, root.join("next.png"));
        assert!(app.image_handoff.is_some());
        match ending {
            0 => {
                app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
                    generation: app.image_generation,
                    first_index: 0,
                    total: 1,
                    images: vec![(
                        root.join("next.png"),
                        Err(towavue_runtime_windows::ImageDecodeError::UnknownFormat),
                    )],
                });
                assert!(app.image_error.is_some());
            }
            1 => {
                let saved = app.take_image_tab_state();
                assert!(saved.image.is_none() && saved.resume_loading);
                assert_eq!(saved.path, root.join("next.png"));
            }
            2 => app.close_tab_unchecked(tab),
            _ => {
                app.tabs.open_new(root.join("other.png"), MediaKind::Image);
                app.load_path(root.join("other.png"), MediaKind::Image);
                app.image_loader.request(Vec::new());
            }
        }
        assert!(app.image_handoff.is_none());
        assert!(context.tex_manager().read().meta(old).is_none());
    }
}
