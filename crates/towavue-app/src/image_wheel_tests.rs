use crate::*;

#[test]
fn image_wheel_reaches_the_rendered_view_without_requiring_window_focus() {
    let Some(root) = tests::isolated_test_root(
        "image_wheel_tests::image_wheel_reaches_the_rendered_view_without_requiring_window_focus",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for focused in [true, false] {
            let context = fonts::test_context();
            let mut app = Application::new(None, |_| {}).expect("headless app");
            let path = root.join("image.png");
            let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
            app.path = Some(path.clone());
            app.media_kind = Some(MediaKind::Image);
            app.ui_context = Some(context.clone());
            app.fullscreen = true;
            app.image_view.zoom = ZoomMode::Custom(4.0);
            app.image = Some(
                ImagePresentation::from_decoded(
                    &context,
                    &path,
                    DecodedImage {
                        format: "test",
                        frames: vec![towavue_runtime_windows::DecodedImageFrame {
                            width: 400,
                            height: 300,
                            rgba: vec![255; 400 * 300 * 4],
                            delay: Duration::ZERO,
                        }],
                    }
                    .into(),
                )
                .expect("image"),
            );
            let texture = app.image.as_ref().expect("image").texture.id();
            let frame = |app: &mut Application<_>, events, focused| {
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events,
                    focused,
                    ..Default::default()
                };
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .expect("root viewport")
                    .native_pixels_per_point = Some(density);
                context.run_ui(input, |ui| app.draw_ui(ui, &mut Vec::new()))
            };
            let mesh = |output: &egui::FullOutput| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                            Some(mesh.calc_bounds())
                        }
                        _ => None,
                    })
                    .expect("source image mesh")
            };
            let point = egui::pos2(240.0, 170.0);
            let wheel = |modifiers, delta| egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta),
                phase: egui::TouchPhase::Move,
                modifiers,
            };
            for _ in 0..3 {
                frame(&mut app, vec![egui::Event::PointerMoved(point)], focused);
            }
            for (modifiers, expected) in [
                (egui::Modifiers::NONE, (0.0, 30.0)),
                (egui::Modifiers::SHIFT, (30.0, 30.0)),
            ] {
                let output = frame(&mut app, vec![wheel(modifiers, 30.0)], focused);
                assert_eq!(
                    app.image_view.pan, expected,
                    "scroll at {density}x, focused={focused}"
                );
                assert!(
                    (mesh(&output).center() - egui::pos2(200.0 + expected.0, 150.0 + expected.1))
                        .length()
                        < 0.01
                );
            }
            let before = mesh(&frame(&mut app, vec![], focused));
            let output = frame(&mut app, vec![wheel(egui::Modifiers::CTRL, 60.0)], focused);
            let factor = (60.0_f32 / 200.0).exp();
            let after = mesh(&output);
            assert!(
                (after.size() - before.size() * factor).length() < 0.01,
                "same-frame zoom at {density}x, focused={focused}"
            );
            assert!(
                (after.center() - (point + (before.center() - point) * factor)).length() < 0.01
            );
            let view = app.image_view;
            for _ in 0..3 {
                assert_eq!(mesh(&frame(&mut app, vec![], focused)), after);
                assert_eq!(app.image_view, view, "no delayed tail");
            }
            assert!(
                app.edits
                    .get(&tab)
                    .is_none_or(|history| !history.is_dirty())
            );
            assert!(
                context.memory(egui::Memory::focused).is_none(),
                "wheel must not focus a widget"
            );

            // Batching OS events into one redraw must not reorder zoom and pan, or
            // cancel a reversal at a scroll boundary before either event is applied.
            for (name, events) in [
                (
                    "vertical boundary reversal",
                    vec![
                        wheel(egui::Modifiers::NONE, 10000.0),
                        wheel(egui::Modifiers::NONE, -30.0),
                    ],
                ),
                (
                    "horizontal boundary reversal",
                    vec![
                        wheel(egui::Modifiers::SHIFT, -10000.0),
                        wheel(egui::Modifiers::SHIFT, 30.0),
                    ],
                ),
                (
                    "pan before anchored zoom",
                    vec![
                        wheel(egui::Modifiers::NONE, 30.0),
                        wheel(egui::Modifiers::CTRL, 60.0),
                    ],
                ),
                (
                    "interleaved pan and zoom",
                    vec![
                        wheel(egui::Modifiers::SHIFT, -40.0),
                        wheel(egui::Modifiers::CTRL, 60.0),
                        egui::Event::PointerMoved(point + egui::vec2(30.0, -20.0)),
                        wheel(egui::Modifiers::NONE, 50.0),
                        wheel(egui::Modifiers::CTRL, -30.0),
                    ],
                ),
                (
                    "zoom follows changing scrollbar geometry",
                    vec![
                        wheel(egui::Modifiers::CTRL, -400.0),
                        egui::Event::PointerMoved(egui::pos2(399.0, 150.0)),
                        wheel(egui::Modifiers::CTRL, 60.0),
                    ],
                ),
            ] {
                app.image_view = view;
                frame(&mut app, vec![egui::Event::PointerMoved(point)], focused);
                for event in &events {
                    frame(&mut app, vec![event.clone()], focused);
                }
                let expected = app.image_view;
                let expected_mesh = mesh(&frame(&mut app, vec![], focused));
                app.image_view = view;
                frame(&mut app, vec![egui::Event::PointerMoved(point)], focused);
                let output = frame(&mut app, events, focused);
                assert_eq!(app.image_view.zoom, expected.zoom);
                assert_eq!(app.image_view.selection, expected.selection);
                // egui rounds scroll content geometry to its sub-point layout grid.
                assert!(
                    (egui::Vec2::from(app.image_view.pan) - egui::Vec2::from(expected.pan))
                        .length()
                        < 0.02,
                    "{name} at {density}x, focused={focused}"
                );
                let actual_mesh = mesh(&output);
                assert!((actual_mesh.min - expected_mesh.min).length() < 0.02);
                assert!((actual_mesh.max - expected_mesh.max).length() < 0.02);
                let actual = app.image_view;
                frame(&mut app, vec![], focused);
                assert_eq!(app.image_view, actual, "no delayed burst tail");
            }
            app.image_view = view;
            frame(&mut app, vec![egui::Event::PointerMoved(point)], focused);

            // A focus-loss transition cancels the entire frame, including later positioned input.
            frame(
                &mut app,
                vec![
                    egui::Event::WindowFocused(false),
                    egui::Event::PointerMoved(point),
                    wheel(egui::Modifiers::CTRL, 60.0),
                    wheel(egui::Modifiers::NONE, 30.0),
                ],
                false,
            );
            assert_eq!(app.image_view, view);
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(point),
                    wheel(egui::Modifiers::CTRL, -60.0),
                ],
                false,
            );
            assert_ne!(
                app.image_view.zoom, view.zoom,
                "subsequent inactive wheel resumes"
            );

            let view = app.image_view;
            for events in [
                vec![
                    egui::Event::PointerMoved(egui::pos2(500.0, 400.0)),
                    wheel(egui::Modifiers::CTRL, 60.0),
                ],
                vec![egui::Event::PointerGone, wheel(egui::Modifiers::CTRL, 60.0)],
                vec![egui::Event::PointerMoved(point), egui::Event::Zoom(1.5)],
            ] {
                frame(&mut app, events, false);
                assert_eq!(app.image_view, view);
            }
            app.palette_open = true;
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(point),
                    wheel(egui::Modifiers::CTRL, 60.0),
                ],
                false,
            );
            assert_eq!(app.image_view, view, "palette blocks background zoom");
        }
    }
}
