use super::*;

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
fn selection_zoom_keeps_the_full_image_and_selection_in_the_input_frame() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_scroll::tests::selection_zoom_keeps_the_full_image_and_selection_in_the_input_frame",
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
                for _ in 0..3 {
                    frame(&mut app, density, vec![]);
                }
                let before = mesh(&frame(&mut app, density, vec![]));
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
                        egui::CursorIcon::Crosshair
                    );
                    assert_eq!(app.image_view.zoom, ZoomMode::Fit);
                    frame(&mut app, density, vec![button(false)])
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
                assert_eq!(app.image_view.selection, Some(selected));
                assert_eq!(app.edits, history);
                assert!(output.textures_delta.set.is_empty());
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
    app.zoom_image_selection((600, 400), viewport.size(), 2.0);
    assert_eq!(app.image_view.zoom, ZoomMode::Custom(64.0));
    assert_eq!(app.image_view.pan, (9400.0, 6250.0));
    let limited = app.image_view;
    app.zoom_image_selection((600, 400), egui::Vec2::ZERO, 2.0);
    assert_eq!(app.image_view, limited);
    app.image_view.selection = None;
    let no_selection = app.image_view;
    app.zoom_image_selection((600, 400), viewport.size(), 2.0);
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
    app.path = Some(path.clone());
    app.media_kind = Some(MediaKind::Image);
    app.fullscreen = true;
    app.ui_context = Some(context.clone());
    app.image = Some(
        ImagePresentation::from_decoded(
            &context,
            &path,
            DecodedImage {
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
}
