use super::*;

#[test]
fn image_pan_wheel_and_bars_share_bounded_offsets_without_editing_pixels() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_scroll::tests::image_pan_wheel_and_bars_share_bounded_offsets_without_editing_pixels",
    ) else {
        return;
    };
    let context = fonts::test_context();
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
        assert!(
            app.image_view.selection.is_none(),
            "bar does not start a selection"
        );
        assert_eq!(app.edits, history);
        assert!(output.textures_delta.set.is_empty());
        assert_eq!(app.image.as_ref().expect("image").texture.id(), texture);
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
        assert!((app.image_view.pan.1 + (800.0 / density - 300.0) * 0.5).abs() < 0.01);
        frame(&mut app, vec![], density, egui::vec2(1400.0, 1200.0));
        assert_eq!(
            app.image_view.pan,
            (0.0, 0.0),
            "resize recenters fitting axes"
        );
    }
}
