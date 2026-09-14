use super::*;

mod gpu;

#[test]
fn reading_zoom_pan_and_actual_size_preserve_joined_pages_and_read_only_state() {
    let Some(root) = crate::tests::isolated_test_root(
        "reading_view::tests::reading_zoom_pan_and_actual_size_preserve_joined_pages_and_read_only_state",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("application");
    app.media_kind = Some(MediaKind::Image);
    app.reading_mode = true;
    for density in [1.0, 1.25, 2.0] {
        let context = fonts::test_context();
        context.global_style_mut(chrome::style);
        app.ui_context = Some(context.clone());
        let page = |width, height| {
            ImagePresentation::from_decoded(
                &context,
                &root.join("page.png"),
                DecodedImage {
                    animation_plays: 0,
                    format: "PNG",
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width,
                        height,
                        rgba: vec![255; (width * height * 4) as usize],
                        delay: Duration::ZERO,
                    }],
                }
                .into(),
            )
            .expect("page")
        };
        app.image = Some(page(64, 128));
        app.reading_pages = vec![Ok(page(128, 64))];
        let ids = [
            app.image.as_ref().expect("current page").texture.id(),
            app.reading_pages[0]
                .as_ref()
                .expect("second page")
                .texture
                .id(),
        ];
        let original = app.image.as_ref().expect("current page").decoded.clone();
        let generation = app.image_generation;
        let viewport = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(240.0, 180.0));
        let frame = |app: &mut Application<_>, events: Vec<egui::Event>| {
            let mut input = egui::RawInput {
                screen_rect: Some(viewport),
                events,
                ..Default::default()
            };
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .native_pixels_per_point = Some(density);
            let output = context.run_ui(input, |ui| app.draw_reading_pages(ui));
            assert_eq!(output.pixels_per_point, density);
            let rects = ids.map(|id| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) if mesh.texture_id == id => {
                            assert_eq!(shape.clip_rect, viewport);
                            Some(mesh.calc_bounds())
                        }
                        _ => None,
                    })
                    .expect("page mesh")
            });
            (output, rects)
        };
        let wheel = |position, delta, modifiers| {
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, delta),
                    modifiers,
                },
            ]
        };
        let button = |pos, button, pressed| egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for axis in [ReadingAxis::Horizontal, ReadingAxis::Vertical] {
            for reversed in [false, true] {
                app.reading_settings.axis = axis;
                app.reading_settings.reversed = reversed;
                app.image_view = ImageViewState {
                    zoom: ZoomMode::Custom(4.0),
                    ..Default::default()
                };
                frame(&mut app, vec![]);
                let (_, before) = frame(&mut app, vec![]);
                let before = before[0].union(before[1]);
                let pointer = viewport.center() + egui::vec2(30.0, 10.0);
                let (_, after) = frame(&mut app, wheel(pointer, 50.0, egui::Modifiers::CTRL));
                let joined = after[0].union(after[1]);
                let ratio = joined.width() / before.width();
                assert!(ratio > 1.0);
                let limit = (joined.size() - viewport.size()).max(egui::Vec2::ZERO) * 0.5;
                let anchored = pointer + (before.center() - pointer) * ratio;
                assert!(
                    (joined.center()
                        - (viewport.center()
                            + (anchored - viewport.center()).clamp(-limit, limit)))
                    .length()
                        < 0.001
                );
                let [first, second] = if reversed {
                    [after[1], after[0]]
                } else {
                    after
                };
                let seam = match axis {
                    ReadingAxis::Horizontal => first.right() - second.left(),
                    ReadingAxis::Vertical => first.bottom() - second.top(),
                };
                assert!(seam.abs() < 0.001);
                let mut events = wheel(pointer, 100_000.0, egui::Modifiers::NONE);
                events.extend(wheel(pointer, -20.0, egui::Modifiers::NONE));
                frame(&mut app, events);
                assert!(
                    (app.image_view.pan.1 - (limit.y - 20.0).clamp(-limit.y, limit.y)).abs()
                        < 0.001
                );
                let start = viewport.center();
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(start),
                        button(start, egui::PointerButton::Secondary, true),
                    ],
                );
                let (output, _) = frame(
                    &mut app,
                    vec![egui::Event::PointerMoved(start + egui::vec2(40.0, 25.0))],
                );
                assert_eq!(
                    output.platform_output.cursor_icon,
                    egui::CursorIcon::Grabbing
                );
                app.cancel_view_drag();
                frame(
                    &mut app,
                    vec![button(start, egui::PointerButton::Secondary, false)],
                );
                let view = app.image_view;
                app.palette_open = true;
                frame(&mut app, wheel(pointer, 50.0, egui::Modifiers::CTRL));
                assert_eq!(app.image_view, view);
                app.palette_open = false;
                frame(
                    &mut app,
                    vec![button(start, egui::PointerButton::Primary, true)],
                );
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pointer),
                        button(pointer, egui::PointerButton::Primary, false),
                    ],
                );
                assert!(app.image_view.selection.is_none());
                app.dispatch(CommandId::ActualSize);
                let (_, rects) = frame(&mut app, vec![]);
                let cross = match axis {
                    ReadingAxis::Horizontal => rects[0].height() * density / 128.0,
                    ReadingAxis::Vertical => rects[0].width() * density / 64.0,
                };
                assert!((cross - 1.0).abs() < 0.0001);
                app.dispatch(CommandId::CoverWindow);
                let (_, rects) = frame(&mut app, vec![]);
                assert!(
                    rects[0]
                        .union(rects[1])
                        .expand(0.001)
                        .contains_rect(viewport)
                );
                app.dispatch(CommandId::FitToWindow);
                let (_, rects) = frame(&mut app, vec![]);
                assert!(
                    viewport
                        .expand(0.001)
                        .contains_rect(rects[0].union(rects[1]))
                );
                assert_eq!(app.image_view.pan, (0.0, 0.0));
                assert_eq!(app.image_generation, generation);
                assert!(app.edits.is_empty());
                assert!(Arc::ptr_eq(
                    &original,
                    &app.image.as_ref().expect("current page").decoded
                ));
            }
        }
    }
}
