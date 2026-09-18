use super::*;

#[test]
fn fitted_image_scale_does_not_overflow_after_dpi_conversion() {
    for density in [1.0, 1.25, 1.5, 2.0] {
        for edge in 1..=16_384 {
            for size in [(113, edge), (edge, 113)] {
                let viewport = egui::vec2(640.0, 480.0);
                let scale = ImageViewState::default().logical_scale(size, viewport.into(), density);
                let displayed = egui::vec2(size.0 as f32, size.1 as f32) * scale;
                assert!(
                    displayed.x <= viewport.x && displayed.y <= viewport.y,
                    "logical Fit overflow: {size:?} at {density}x gives {displayed:?}"
                );
            }
        }
    }
}

#[test]
fn inset_scrollbar_gutters_do_not_capture_background_drags() {
    let context = fonts::test_context();
    context.global_style_mut(chrome::style);
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
    let background = egui::Id::new("scrollbar-gutter-background");
    let mut view = ImageViewState::default();
    let mut time = 0.0;
    for density in [1.0, 1.25, 2.0] {
        let mut frame = |events| {
            time += 0.1;
            let mut input = egui::RawInput {
                screen_rect: Some(viewport),
                time: Some(time),
                events,
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .expect("viewport")
                .native_pixels_per_point = Some(density);
            let _ = context.run_ui(input, |ui| {
                ui.interact(viewport, background, egui::Sense::click_and_drag());
                super::bars(ui, viewport, egui::vec2(1000.0, 800.0), &mut view, true);
            });
            assert_eq!(view.pan, (0.0, 0.0), "gutters must not scroll the image");
        };
        for point in [
            egui::pos2(399.0, 150.0),
            egui::pos2(200.0, 299.0),
            egui::pos2(389.5, 1.0),
            egui::pos2(389.5, 299.0),
            egui::pos2(1.0, 289.5),
            egui::pos2(399.0, 289.5),
            egui::pos2(389.5, 289.5),
        ] {
            let button = |pressed| egui::Event::PointerButton {
                pos: point,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            };
            frame(vec![egui::Event::PointerMoved(point)]);
            frame(vec![]);
            frame(vec![button(true)]);
            frame(vec![egui::Event::PointerMoved(
                point + egui::vec2(30.0, 30.0),
            )]);
            assert_eq!(context.dragged_id(), Some(background));
            frame(vec![button(false)]);
        }
    }
}

#[test]
fn selection_zoom_keeps_the_full_image_and_clears_selection_in_the_input_frame() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_scroll::tests::selection_zoom_keeps_the_full_image_and_clears_selection_in_the_input_frame",
    ) else {
        return;
    };
    let context = fonts::test_context();
    context.enable_accesskit();
    let mut app = Application::new(None, |_| {}).expect("app");
    let path = root.join("selection.png");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
    app.path = Some(path.clone());
    app.media_kind = Some(MediaKind::Image);
    app.fullscreen = true;
    app.ui_context = Some(context.clone());
    app.image = Some(
        ImagePresentation::from_decoded(
            &context,
            &path,
            DecodedImage {
                animation_plays: 0,
                format: "test",
                frames: vec![towavue_runtime_windows::DecodedImageFrame {
                    width: 600,
                    height: 400,
                    rgba: vec![255; 600 * 400 * 4],
                    delay: Duration::ZERO,
                }],
            }
            .into(),
        )
        .expect("texture"),
    );
    let texture = app.image.as_ref().expect("image").texture.id();
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
    let mut time = 0.0;
    let mut frame = |app: &mut Application<_>, density, events| {
        time += 0.05;
        let mut input = egui::RawInput {
            screen_rect: Some(viewport),
            time: Some(time),
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .expect("viewport")
            .native_pixels_per_point = Some(density);
        context.run_ui(input, |ui| app.draw_ui(ui, &mut Vec::new()))
    };
    let mesh = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.clone()),
                _ => None,
            })
            .expect("image mesh")
    };
    for (operation, pixels) in [
        (EditOperation::FlipHorizontal, egui::vec2(600.0, 400.0)),
        (EditOperation::RotateClockwise, egui::vec2(400.0, 600.0)),
        (
            EditOperation::Crop(PixelCrop {
                x: 40,
                y: 60,
                width: 320,
                height: 480,
            }),
            egui::vec2(320.0, 480.0),
        ),
    ] {
        app.edits
            .entry(tab)
            .or_default()
            .push(operation, MediaKind::Image);
        let history = app.edits.clone();
        for density in [1.0, 1.25, 2.0] {
            for held in [false, true] {
                app.image_view.fit();
                let selected = UnitRect {
                    min: UnitPoint { x: 0.1, y: 0.2 },
                    max: UnitPoint { x: 0.5, y: 0.6 },
                };
                app.image_view.selection = Some(selected);
                frame(
                    &mut app,
                    density,
                    vec![egui::Event::PointerMoved(egui::pos2(200.0, 150.0))],
                );
                assert!(
                    !app.fullscreen_controls_visible,
                    "selection does not reveal status"
                );
                frame(
                    &mut app,
                    density,
                    vec![egui::Event::PointerMoved(egui::pos2(200.0, 285.0))],
                );
                for _ in 0..3 {
                    frame(&mut app, density, vec![]);
                }
                let output = frame(&mut app, density, vec![]);
                let before = mesh(&output);
                let crop = PixelCrop::from_selection(
                    selected,
                    (pixels.x as u32, pixels.y as u32),
                    MediaKind::Image,
                )
                .expect("edited selection");
                let expected = format!(
                    "Selection: x={} y={} · {}×{} px",
                    crop.x, crop.y, crop.width, crop.height
                );
                assert!(
                    output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("tree")
                        .nodes
                        .iter()
                        .any(|(_, node)| node.value().is_some_and(|text| text.contains(&expected))),
                    "status follows edited dimensions: {expected}"
                );
                let start = selection_rect(before.calc_bounds(), selected).center();
                let button = |pressed| egui::Event::PointerButton {
                    pos: start,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                };
                frame(&mut app, density, vec![egui::Event::PointerMoved(start)]);
                let hover = frame(&mut app, density, vec![]);
                assert_eq!(hover.platform_output.cursor_icon, egui::CursorIcon::ZoomIn);
                let output = if held {
                    let pressed = frame(&mut app, density, vec![button(true)]);
                    assert_eq!(
                        pressed.platform_output.cursor_icon,
                        egui::CursorIcon::ZoomIn
                    );
                    assert!(matches!(app.image_view.zoom, ZoomMode::Custom(_)));
                    assert!(app.image_view.selection.is_none());
                    let released = frame(&mut app, density, vec![button(false)]);
                    assert_eq!(mesh(&pressed).vertices, mesh(&released).vertices);
                    pressed
                } else {
                    frame(&mut app, density, vec![button(true), button(false)])
                };
                let zoomed = mesh(&output);
                let scale = (viewport.size() / (pixels * 0.4)).min_elem();
                let displayed = pixels * scale;
                let limit = (displayed - viewport.size()).max(egui::Vec2::ZERO) * 0.5;
                let expected_pan = (pixels * egui::vec2(0.2, 0.1) * scale).clamp(-limit, limit);
                assert!((zoomed.calc_bounds().size() - displayed).length() < 0.001);
                assert!(
                    (zoomed.calc_bounds().center() - viewport.center() - expected_pan).length()
                        < 0.001
                );
                assert!(
                    matches!(app.image_view.zoom, ZoomMode::Custom(z) if (z - scale * density).abs() < 0.001)
                );
                assert_eq!(
                    zoomed.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(),
                    before.vertices.iter().map(|v| v.uv).collect::<Vec<_>>()
                );
                assert!(app.image_view.selection.is_none());
                assert_eq!(app.edits, history);
                assert!(
                    output
                        .textures_delta
                        .set
                        .iter()
                        .all(|(id, _)| *id == egui::TextureId::Managed(0)),
                    "only the font atlas may change for new status glyphs: {:?}",
                    output
                        .textures_delta
                        .set
                        .iter()
                        .map(|(id, _)| id)
                        .collect::<Vec<_>>()
                );
                assert!(
                    output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("accessibility")
                        .nodes
                        .iter()
                        .any(|(_, node)| node.role() == egui::accesskit::Role::ScrollBar)
                );
                let view = app.image_view;
                app.dispatch(CommandId::ZoomSelection);
                assert_eq!(
                    app.image_view, view,
                    "command and click share a non-toggle view"
                );
                let stable = mesh(&frame(&mut app, density, vec![]));
                assert_eq!(stable.calc_bounds(), zoomed.calc_bounds());
                app.dispatch(CommandId::ClearSelection);
                let cleared = mesh(&frame(&mut app, density, vec![]));
                assert_eq!(cleared.calc_bounds(), zoomed.calc_bounds());
                assert!(app.image_view.selection.is_none());
                app.dispatch(CommandId::FitToWindow);
                let fitted = mesh(&frame(&mut app, density, vec![]));
                assert_eq!(fitted.calc_bounds(), before.calc_bounds());
                assert_eq!(app.edits, history);
            }
        }
    }
    app.image_view.selection = Some(
        PixelCrop {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }
        .unit_rect((600, 400)),
    );
    app.zoom_visual_selection((600, 400), viewport.size(), 2.0, 1.0);
    assert_eq!(app.image_view.zoom, ZoomMode::Custom(64.0));
    assert_eq!(app.image_view.pan, (9400.0, 6250.0));
    let limited = app.image_view;
    app.zoom_visual_selection((600, 400), egui::Vec2::ZERO, 2.0, 1.0);
    assert_eq!(app.image_view, limited);
    app.image_view.selection = None;
    let no_selection = app.image_view;
    app.zoom_visual_selection((600, 400), viewport.size(), 2.0, 1.0);
    assert_eq!(app.image_view, no_selection);
}

#[test]
fn image_pan_wheel_and_bars_share_bounded_offsets_without_editing_pixels() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_scroll::tests::image_pan_wheel_and_bars_share_bounded_offsets_without_editing_pixels",
    ) else {
        return;
    };
    let context = fonts::test_context();
    context.global_style_mut(chrome::style);
    context.enable_accesskit();
    let mut app = Application::new(None, |_| {}).expect("app");
    let path = root.join("scroll.png");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    app.tabs
        .close_gallery(app.tabs.gallery().expect("media-only fixture"));
    app.path = Some(path.clone());
    app.media_kind = Some(MediaKind::Image);
    app.fullscreen = true;
    app.ui_context = Some(context.clone());
    app.image = Some(
        ImagePresentation::from_decoded(
            &context,
            &path,
            DecodedImage {
                animation_plays: 0,
                format: "test",
                frames: vec![towavue_runtime_windows::DecodedImageFrame {
                    width: 1000,
                    height: 800,
                    rgba: vec![255; 1000 * 800 * 4],
                    delay: Duration::ZERO,
                }],
            }
            .into(),
        )
        .expect("texture"),
    );
    let texture = app.image.as_ref().expect("image").texture.id();
    let history = app.edits.clone();
    let mut time = 0.0;
    let mut frame = |app: &mut Application<_>, events, density: f32, size| {
        time += 0.1;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(time),
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .expect("viewport")
            .native_pixels_per_point = Some(density);
        context.run_ui(input, |ui| app.draw_ui(ui, &mut Vec::new()))
    };
    let bars = |output: &egui::FullOutput| {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("accessibility")
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == egui::accesskit::Role::ScrollBar)
            .map(|(_, node)| node.bounds().expect("bar bounds"))
            .collect::<Vec<_>>()
    };
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Secondary,
        modifiers: egui::Modifiers::NONE,
    };
    let start = egui::pos2(200.0, 150.0);
    let size = egui::vec2(400.0, 300.0);
    for density in [1.0, 1.25, 2.0] {
        app.image_view.fit();
        for _ in 0..3 {
            frame(&mut app, vec![], density, size);
        }
        let output = frame(&mut app, vec![], density, size);
        assert!(bars(&output).is_empty());
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(start)],
            density,
            size,
        );
        frame(&mut app, vec![button(start, true)], density, size);
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(start + egui::vec2(60.0, 40.0)),
                button(start + egui::vec2(60.0, 40.0), false),
            ],
            density,
            size,
        );
        assert_eq!(app.image_view.pan, (0.0, 0.0), "Fit stays centered");
        assert!(app.view_drag.is_none());
        app.image_view.actual_size();
        for _ in 0..3 {
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(start)],
                density,
                size,
            );
        }
        let output = frame(&mut app, vec![], density, size);
        assert_eq!(bars(&output).len(), 2);
        let regions = bars(&output);
        let horizontal = regions
            .iter()
            .find(|bar| bar.width() > bar.height())
            .expect("horizontal");
        let vertical = regions
            .iter()
            .find(|bar| bar.width() < bar.height())
            .expect("vertical");
        assert!(
            horizontal.x1 < vertical.x0 && vertical.y1 < horizontal.y0,
            "tracks and hit regions leave a separated corner: {regions:?}"
        );
        for bar in bars(&output) {
            assert!(bar.x0 >= 8.0 && bar.y0 >= 8.0, "inset start: {bar:?}");
            assert!(
                bar.x1 <= f64::from(size.x - 8.0) && bar.y1 <= f64::from(size.y - 8.0),
                "inset end: {bar:?}"
            );
        }
        assert!(
            bars(&output)
                .iter()
                .all(|bar| (bar.width().min(bar.height()) - 5.0).abs() < 0.01),
            "scrollbar hit width follows the shared narrow style"
        );
        let limit = (egui::vec2(1000.0, 800.0) / density - size) * 0.5;
        frame(&mut app, vec![button(start, true)], density, size);
        let output = frame(
            &mut app,
            vec![egui::Event::PointerMoved(
                start + egui::vec2(2000.0, 2000.0),
            )],
            density,
            size,
        );
        assert_eq!(
            output.platform_output.cursor_icon,
            egui::CursorIcon::Grabbing
        );
        assert!((egui::Vec2::from(app.image_view.pan) - limit).length() < 0.01);
        app.cancel_view_drag();
        frame(&mut app, vec![button(start, false)], density, size);
        assert_eq!(app.image_view.pan, (0.0, 0.0));
        let wheel = |shift| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -30.0),
            phase: egui::TouchPhase::Move,
            modifiers: if shift {
                egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::NONE
            },
        };
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(start), wheel(false)],
            density,
            size,
        );
        assert_eq!(app.image_view.pan, (0.0, -30.0));
        frame(&mut app, vec![wheel(true)], density, size);
        assert_eq!(app.image_view.pan, (-30.0, -30.0));
        let output = frame(&mut app, vec![], density, size);
        assert_eq!(app.image_view.pan, (-30.0, -30.0));
        let bar = bars(&output)
            .into_iter()
            .find(|b| b.width() < b.height())
            .expect("vertical bar");
        let point = egui::pos2(((bar.x0 + bar.x1) / 2.0) as f32, (bar.y1 - 4.0) as f32);
        let primary = |pressed| egui::Event::PointerButton {
            pos: point,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        let bar_selection = PixelCrop {
            x: 400,
            y: 320,
            width: 200,
            height: 160,
        }
        .unit_rect((1000, 800));
        app.image_view.selection = Some(bar_selection);
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(point)],
            density,
            size,
        );
        frame(&mut app, vec![primary(true)], density, size);
        let output = frame(&mut app, vec![primary(false)], density, size);
        assert!(
            app.image_view.pan.1 < -30.0,
            "scrollbar moves the same pan: density={density}, bar={bar:?}, point={point:?}, pan={:?}, drag={}",
            app.image_view.pan,
            app.view_drag.is_some()
        );
        assert_eq!(
            app.image_view.selection,
            Some(bar_selection),
            "bar does not clear a selection"
        );
        assert_eq!(app.edits, history);
        assert!(output.textures_delta.set.is_empty());
        assert_eq!(app.image.as_ref().expect("image").texture.id(), texture);
        assert!(!app.fullscreen_controls_visible, "bars own the bottom edge");
        for bar in bars(&output) {
            let axis = usize::from(bar.width() < bar.height());
            let center = egui::pos2(
                ((bar.x0 + bar.x1) * 0.5) as f32,
                ((bar.y0 + bar.y1) * 0.5) as f32,
            );
            let press = |pos, pressed| egui::Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            };
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(center)],
                density,
                size,
            );
            frame(&mut app, vec![press(center, true)], density, size);
            let mut end = center;
            for direction in [-1.0, 1.0] {
                end[axis] = size[axis] * (0.5 + direction);
                frame(
                    &mut app,
                    vec![egui::Event::PointerMoved(end)],
                    density,
                    size,
                );
                assert!(
                    (egui::Vec2::from(app.image_view.pan)[axis] + direction * limit[axis]).abs()
                        < 0.02,
                    "inset bars still reach the full image limits"
                );
                assert!(app.view_drag.is_none(), "bar drag is not image selection");
                assert!(!app.fullscreen_controls_visible);
                assert_eq!(app.image_view.selection, Some(bar_selection));
            }
            frame(&mut app, vec![press(end, false)], density, size);
        }
        app.image_view.pan = (0.0, 0.0);
        let selected = PixelCrop {
            x: 400,
            y: 320,
            width: 200,
            height: 160,
        };
        app.image_view.selection = Some(selected.unit_rect((1000, 800)));
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(start)],
            density,
            size,
        );
        let end = start + egui::vec2(20.0, -10.0);
        frame(&mut app, vec![button(start, true)], density, size);
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(end), button(end, false)],
            density,
            size,
        );
        let moved = PixelCrop::from_selection(
            app.image_view.selection.expect("moved selection"),
            (1000, 800),
            MediaKind::Image,
        )
        .expect("pixel selection");
        assert_eq!(
            (moved.width, moved.height),
            (selected.width, selected.height)
        );
        assert_eq!(moved.x, selected.x + (20.0 * density).round() as u32);
        assert_eq!(
            moved.y,
            (selected.y as f32 + (-10.0 * density).round()) as u32
        );
        assert_eq!(
            app.image_view.pan,
            (0.0, 0.0),
            "selection drag must not pan the image"
        );
        assert_eq!(app.edits, history);
        let outside = egui::pos2(40.0, 40.0);
        let click = |pressed| egui::Event::PointerButton {
            pos: outside,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(outside)],
            density,
            size,
        );
        let output = frame(&mut app, vec![click(true), click(false)], density, size);
        assert!(app.image_view.selection.is_none());
        assert_eq!(app.image_view.pan, (0.0, 0.0));
        assert_eq!(app.edits, history);
        assert!(output.textures_delta.set.is_empty());
        app.image_view.pan = (10000.0, -10000.0);
        for _ in 0..3 {
            frame(&mut app, vec![], density, egui::vec2(1400.0, 300.0));
        }
        let output = frame(&mut app, vec![], density, egui::vec2(1400.0, 300.0));
        assert_eq!(
            bars(&output).len(),
            1,
            "only the overflowing axis has a bar"
        );
        assert_eq!(app.image_view.pan.0, 0.0);
        assert!(
            (app.image_view.pan.1 + (800.0 / density - 300.0) * 0.5).abs() < 0.01,
            "resize limit: density={density}, pan={:?}, viewport={:?}",
            app.image_view.pan,
            app.image_viewport
        );
        frame(&mut app, vec![], density, egui::vec2(1400.0, 1200.0));
        assert_eq!(
            app.image_view.pan,
            (0.0, 0.0),
            "resize recenters fitting axes"
        );
    }
    app.fullscreen = false;
    for density in [1.0, 1.25, 2.0] {
        for axis in 0..2 {
            for batched in [false, true] {
                app.image_view.actual_size();
                app.image_view.pan = (0.0, 0.0);
                app.image_view.selection = Some(
                    PixelCrop {
                        x: 400,
                        y: 320,
                        width: 200,
                        height: 160,
                    }
                    .unit_rect((1000, 800)),
                );
                frame(&mut app, vec![egui::Event::PointerGone], density, size);
                let focus = egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: app.selection_identity().with(0_usize).accesskit_id(),
                    data: None,
                });
                frame(&mut app, vec![focus], density, size);
                let output = frame(&mut app, vec![], density, size);
                assert!(selection::has_focus(&context));
                let bar = bars(&output)
                    .into_iter()
                    .find(|bar| (axis == 0) == (bar.width() > bar.height()))
                    .expect("overflow bar");
                let point = egui::pos2(
                    ((bar.x0 + bar.x1) * 0.5) as f32,
                    ((bar.y0 + bar.y1) * 0.5) as f32,
                );
                frame(
                    &mut app,
                    vec![egui::Event::PointerMoved(point)],
                    density,
                    size,
                );
                assert!(
                    selection::has_focus(&context),
                    "bar hover preserves explicit numeric focus"
                );
                let pointer = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                };
                let before = app.image_view.selection;
                let mut events = vec![pointer(point, true)];
                if batched {
                    events.push(pointer(point, false));
                }
                frame(&mut app, events, density, size);
                assert!(
                    !selection::has_focus(&context),
                    "bar press must release numeric focus even before offset changes"
                );
                if !batched {
                    let mut end = point;
                    end[axis] += 20.0;
                    frame(
                        &mut app,
                        vec![egui::Event::PointerMoved(end)],
                        density,
                        size,
                    );
                    frame(&mut app, vec![pointer(end, false)], density, size);
                    assert_ne!(
                        egui::Vec2::from(app.image_view.pan)[axis],
                        0.0,
                        "held bar drag still moves the view"
                    );
                } else {
                    assert!(
                        egui::Vec2::from(app.image_view.pan)[axis].abs() < 0.02,
                        "stationary thumb click need not move the view to release focus"
                    );
                }
                assert!(
                    tab_focus::take(&context, tab).is_none(),
                    "bar use must forget the numeric tab-return role"
                );
                frame(
                    &mut app,
                    vec![egui::Event::Key {
                        key: egui::Key::ArrowRight,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    density,
                    size,
                );
                assert_eq!(
                    app.image_view.selection, before,
                    "media arrow must not adjust selection"
                );
                assert_eq!(app.edits, history);
            }
        }
    }
}

#[test]
fn arrow_pan_owns_only_overflowing_axes_in_images_and_reading_at_three_densities() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_scroll::tests::arrow_pan_owns_only_overflowing_axes_in_images_and_reading_at_three_densities",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let mut app = Application::new(None, |_| {}).expect("app");
        let context = fonts::test_context();
        app.ui_context = Some(context.clone());
        for reading in [false, true] {
            for (width, height, horizontal, vertical) in [
                (600, 120, true, false),
                (80, 600, false, true),
                (600, 600, true, true),
            ] {
                app.reading_mode = false;
                let decoded = Arc::new(DecodedImage {
                    animation_plays: 0,
                    format: "test",
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width,
                        height,
                        rgba: vec![255; (width * height * 4) as usize],
                        delay: Duration::ZERO,
                    }],
                });
                let path = root.join("arrows.png");
                let tab =
                    crate::tab_transfer::tests::install(&mut app, path.clone(), decoded.clone());
                app.reading_mode = reading;
                app.reading_pages = if reading {
                    vec![Ok(ImagePresentation::from_decoded(
                        &context,
                        &root.join("second.png"),
                        decoded,
                    )
                    .expect("page"))]
                } else {
                    vec![]
                };
                app.image_view = ImageViewState {
                    zoom: ZoomMode::Custom(density),
                    ..Default::default()
                };
                let frame = |app: &mut Application<_>| {
                    let mut input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(240.0, 180.0),
                        )),
                        ..Default::default()
                    };
                    input
                        .viewports
                        .entry(egui::ViewportId::ROOT)
                        .or_default()
                        .native_pixels_per_point = Some(density);
                    context.run_ui(input, |ui| {
                        if reading {
                            app.draw_reading_pages(ui);
                        } else {
                            app.draw_image(ui);
                        }
                    })
                };
                let output = frame(&mut app);
                assert_eq!(output.pixels_per_point, density);
                assert_eq!(app.image_viewport, egui::vec2(240.0, 180.0));
                let generation = app.image_generation;
                for (key, axis, positive, overflow) in [
                    ("Right", 0, false, horizontal),
                    ("Left", 0, true, horizontal),
                    ("Down", 1, false, vertical),
                    ("Up", 1, true, vertical),
                ] {
                    app.image_view.pan = (0.0, 0.0);
                    let stroke = key.parse::<KeyStroke>().expect("key");
                    assert_eq!(
                        app.image_arrow_pan(&stroke).is_some(),
                        overflow,
                        "{key}, {width}x{height}, reading={reading}, density={density}; viewport={:?}, zoom={:?}, loading={}, edit={}, handoff={}, displayed={:?}/{:?}, drag={}/{}, allowed={}, focused={}, keyboard={}, image={}, error={}, keys={:?}",
                        app.image_viewport,
                        app.image_view.zoom,
                        app.image_loading,
                        app.image_edit_pending,
                        app.image_handoff.is_some(),
                        app.displayed_tab,
                        app.tabs.active_id(),
                        app.view_drag.is_some(),
                        app.rotation_drag.is_some(),
                        app.view_input_allowed(&context),
                        context.input(|input| input.focused),
                        context.egui_wants_keyboard_input(),
                        app.image.is_some(),
                        app.image_error.is_some(),
                        app.entered_shortcut
                    );
                    if overflow {
                        assert!(
                            app.owns_focused_shortcut(&stroke),
                            "route before egui arrow focus"
                        );
                        app.process_shortcut(stroke.clone());
                        assert_eq!(
                            egui::Vec2::from(app.image_view.pan)[axis],
                            if positive { 40.0 } else { -40.0 }
                        );
                        for _ in 0..100 {
                            app.repeat_media_shortcut(stroke.clone());
                        }
                        let edge = app.image_view.pan;
                        app.process_shortcut(stroke);
                        assert_eq!(app.image_view.pan, edge, "an edge keeps pan ownership");
                        assert_eq!(app.path.as_ref(), Some(&path));
                        assert_eq!(app.tabs.active_id(), Some(tab));
                        assert_eq!(app.image_generation, generation);
                        assert!(app.image_sequence.steps.is_empty());
                    }
                }
                app.image_view.fit();
                frame(&mut app);
                assert!(
                    app.image_arrow_pan(&"Right".parse().expect("key"))
                        .is_none()
                );
                assert!(app.image_arrow_pan(&"Down".parse().expect("key")).is_none());
                app.image_view.zoom = ZoomMode::Custom(density * 4.0);
                let stroke = "Right".parse::<KeyStroke>().expect("key");
                let original = app.shortcuts.clone();
                app.shortcuts.remove(CommandId::NextImage);
                app.shortcuts.remove(CommandId::ReadingRight);
                app.shortcuts
                    .set(CommandId::ZoomIn, "Right".parse().expect("custom arrow"));
                assert!(!app.pan_image_arrow(&stroke));
                app.shortcuts
                    .set(CommandId::ZoomIn, "Right K".parse().expect("custom prefix"));
                assert!(!app.pan_image_arrow(&stroke));
                app.shortcuts = original;
                app.palette_open = true;
                assert!(!app.pan_image_arrow(&stroke));
                app.palette_open = false;
                app.native_ime_composing = true;
                assert!(!app.pan_image_arrow(&stroke));
                app.native_ime_composing = false;
                app.entered_shortcut.push("Ctrl+K".parse().expect("prefix"));
                assert!(!app.pan_image_arrow(&stroke));
                app.entered_shortcut.clear();
                assert!(!app.pan_image_arrow(&"Ctrl+Right".parse().expect("modified")));
                app.image_error = Some("failed raster fixture".into());
                assert!(!app.pan_image_arrow(&stroke));
                app.image_error = None;
                app.image_loading = true;
                assert!(!app.pan_image_arrow(&stroke));
                app.image_loading = false;
                let input = egui::Id::new("arrow-test-editor");
                let mut text = String::new();
                let _ = context.run_ui(Default::default(), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut text).id(input))
                        .request_focus();
                });
                assert!(!app.pan_image_arrow(&stroke));
                context.memory_mut(|memory| memory.surrender_focus(input));
            }
        }
    }
}
