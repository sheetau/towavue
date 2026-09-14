use super::*;

mod gpu;
mod release;

type App = Application<fn(AppEvent)>;

fn decoded(width: u32, height: u32, color: [u8; 4]) -> Arc<DecodedImage> {
    Arc::new(DecodedImage {
        animation_plays: 0,
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
    if app.image_handoff.is_some() {
        assert!(
            app.pending_image_previews.is_empty(),
            "no hidden cache-preview requests"
        );
    }
}

#[test]
fn fitted_image_and_handoff_do_not_paint_phantom_scrollbars() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_handoff::tests::fitted_image_and_handoff_do_not_paint_phantom_scrollbars",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 1.5, 2.0] {
        for size in [(113, 92), (113, 94), (92, 113), (94, 113)] {
            let (mut app, context, _) = fixture(&root);
            context.global_style_mut(chrome::style);
            app.image = Some(
                ImagePresentation::from_decoded(
                    &context,
                    &root.join("old.png"),
                    decoded(size.0, size.1, [20, 40, 60, 255]),
                )
                .expect("image"),
            );
            let mut time = 0.0;
            let mut draw = |app: &mut App| {
                time += 0.25;
                let mut input = egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 480.0),
                    )),
                    ..Default::default()
                };
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .expect("viewport")
                    .native_pixels_per_point = Some(density);
                context.run_ui(input, |ui| app.draw_image(ui))
            };
            draw(&mut app);
            let initial = draw(&mut app);
            app.image_handoff = app.take_navigation_handoff(MediaKind::Image);
            assert!(app.image_handoff.is_some());
            app.image = None;
            app.image_loading = true;
            let held = draw(&mut app);
            for output in [&initial, &held] {
                assert!(
                    !output.shapes.iter().any(|shape| matches!(
                        &shape.shape,
                        egui::Shape::Rect(rect) if rect.fill.a() > 0
                            && rect.rect.size().min_elem() <= 5.0
                            && rect.rect.size().max_elem() > 12.0
                    )),
                    "Fit must not paint a bar: {size:?}, {density}x"
                );
            }
            let meshes = |output: &egui::FullOutput| {
                output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) => Some(mesh.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            let original = meshes(&initial);
            assert_eq!(original.len(), 1, "image must be painted");
            assert_eq!(meshes(&held), original, "held image must not move");
        }
    }
}

#[test]
fn loading_handoff_preserves_idle_scrollbars_without_accepting_scroll_input() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_handoff::tests::loading_handoff_preserves_idle_scrollbars_without_accepting_scroll_input",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for reading in [false, true] {
            let (mut app, context, _) = fixture(&root);
            context.set_pixels_per_point(density);
            context.global_style_mut(chrome::style);
            app.reading_mode = reading;
            app.image_view.zoom = ZoomMode::Custom(16.0);
            app.image_view.pan = (20.0, -15.0);
            if reading {
                app.reading_pages = vec![Ok(ImagePresentation::from_decoded(
                    &context,
                    &root.join("second.png"),
                    decoded(90, 160, [60, 40, 20, 255]),
                )
                .expect("second page"))];
            }
            let mut time = 0.0;
            let mut draw = |app: &mut App, events| {
                time += 0.25;
                context.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(640.0, 480.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| app.draw_image(ui),
                )
            };
            let handles = |output: &egui::FullOutput| {
                output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.fill.a() > 0
                                && rect.rect.size().min_elem() <= 5.0
                                && rect.rect.size().max_elem() > 12.0 =>
                        {
                            Some(rect.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            draw(&mut app, vec![]);
            let initial = draw(&mut app, vec![]);
            let before = handles(&initial);
            let meshes = |output: &egui::FullOutput| {
                output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) => Some(mesh.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            let before_images = meshes(&initial);
            assert_eq!(before.len(), 2, "both axes overflow");
            app.image_handoff = app.take_navigation_handoff(MediaKind::Image);
            app.image = None;
            app.reading_pages.clear();
            app.image_loading = true;
            let held_view = app.image_handoff.as_ref().expect("held image").view;
            let point = before[0].rect.center();
            for events in [
                vec![],
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                vec![egui::Event::PointerMoved(point + egui::vec2(40.0, 40.0))],
                vec![egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: egui::vec2(50.0, 50.0),
                    modifiers: egui::Modifiers::NONE,
                }],
            ] {
                let output = draw(&mut app, events);
                assert_eq!(
                    handles(&output),
                    before,
                    "held bars keep geometry and idle color"
                );
                assert_eq!(
                    meshes(&output),
                    before_images,
                    "held image meshes remain unchanged"
                );
                assert_eq!(
                    app.image_handoff.as_ref().expect("held image").view,
                    held_view
                );
                assert_eq!(context.dragged_id(), None, "held bars cannot own a drag");
            }
        }
    }
}

#[test]
fn reading_handoff_keeps_the_complete_spread_until_all_latest_pages_resolve() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_handoff::tests::reading_handoff_keeps_the_complete_spread_until_all_latest_pages_resolve",
    ) else {
        return;
    };
    for (axis, density) in [ReadingAxis::Horizontal, ReadingAxis::Vertical]
        .into_iter()
        .flat_map(|axis| [1.0, 1.25, 2.0].map(|density| (axis, density)))
    {
        for reversed in [false, true] {
            let (mut app, context, _) = fixture(&root);
            context.set_pixels_per_point(density);
            app.reading_mode = true;
            app.reading_settings.axis = axis;
            app.reading_settings.reversed = reversed;
            app.image_view.zoom = ZoomMode::Custom(3.0);
            app.reading_pages = vec![Ok(ImagePresentation::from_decoded(
                &context,
                &root.join("second.png"),
                decoded(90, 160, [60, 40, 20, 255]),
            )
            .expect("second page"))];
            let ids = [
                app.image.as_ref().expect("first").texture.id(),
                app.reading_pages[0].as_ref().expect("second").texture.id(),
            ];
            let originals = [
                app.image.as_ref().expect("first").decoded.clone(),
                app.reading_pages[0]
                    .as_ref()
                    .expect("second")
                    .decoded
                    .clone(),
            ];
            let bounds = |output: &egui::FullOutput| {
                ids.map(|id| {
                    output.shapes.iter().find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) if mesh.texture_id == id => {
                            Some(mesh.calc_bounds())
                        }
                        _ => None,
                    })
                })
            };
            frame(&mut app, &context);
            let before = bounds(&frame(&mut app, &context));
            assert!(before.iter().all(Option::is_some));
            navigate_pending(&mut app, root.join("next.png"));
            let held = app.image_handoff.as_ref().expect("held spread");
            assert!(Arc::ptr_eq(&held.image.decoded, &originals[0]));
            assert!(Arc::ptr_eq(
                &held.reading.as_ref().expect("reading handoff").images[0].decoded,
                &originals[1]
            ));
            assert_eq!(
                bounds(&frame(&mut app, &context)),
                before,
                "no blank loading frame"
            );
            let old_generation = app.image_generation;
            app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
                generation: old_generation,
                first_index: 0,
                total: 2,
                images: vec![(
                    root.join("next.png"),
                    if reversed {
                        Err(towavue_runtime_windows::ImageDecodeError::UnknownFormat)
                    } else {
                        Ok(decoded(100, 100, [255, 0, 0, 255]))
                    },
                )],
            });
            assert!(app.image_loading);
            assert_eq!(
                bounds(&frame(&mut app, &context)),
                before,
                "partial spread stays hidden"
            );
            assert!(app.image_copy_request().is_none());
            navigate_pending(&mut app, root.join("latest.png"));
            assert_eq!(
                bounds(&frame(&mut app, &context)),
                before,
                "keep displayed spread, not partial intermediate page"
            );
            app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
                generation: old_generation,
                first_index: 1,
                total: 2,
                images: vec![(
                    root.join("stale.png"),
                    Ok(decoded(100, 100, [0, 255, 0, 255])),
                )],
            });
            assert_eq!(bounds(&frame(&mut app, &context)), before);
            let restored = app.restored_ui_textures(&context);
            for (id, original) in ids.into_iter().zip(&originals) {
                let delta = &restored
                    .iter()
                    .find(|(texture, _)| *texture == id)
                    .expect("held page recovery pixels")
                    .1;
                assert_eq!(
                    delta.image.size(),
                    [
                        original.frames[0].width as usize,
                        original.frames[0].height as usize
                    ]
                );
            }
            app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
                generation: app.image_generation,
                first_index: 0,
                total: 2,
                images: vec![(
                    root.join("latest.png"),
                    Ok(decoded(80, 120, [255, 255, 0, 255])),
                )],
            });
            assert_eq!(bounds(&frame(&mut app, &context)), before);
            app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
                generation: app.image_generation,
                first_index: 1,
                total: 2,
                images: vec![(
                    root.join("failed.png"),
                    Err(towavue_runtime_windows::ImageDecodeError::UnknownFormat),
                )],
            });
            assert!(!app.image_loading && app.image_handoff.is_none());
            assert!(
                bounds(&frame(&mut app, &context))
                    .iter()
                    .all(Option::is_none)
            );
            assert!(app.image_copy_request().is_some());
            assert!(
                app.edits
                    .values()
                    .all(|history| history.operations().is_empty())
            );
        }
    }
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
        assert!(
            app.image_previews.is_empty(),
            "a hidden interim preview must not allocate a presentation texture"
        );
        let output = frame(&mut app, &context);
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
                assert!(
                    app.pending_image_previews.contains(&root.join("other.png")),
                    "a fresh tab still requests its initial preview"
                );
            }
        }
        assert!(app.image_handoff.is_none());
        assert!(context.tex_manager().read().meta(old).is_none());
    }
}
