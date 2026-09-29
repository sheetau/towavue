use super::*;

#[test]
fn application_ui_scale_ignores_zoom_input_and_keeps_native_dpi_and_media_gestures() {
    let ctrl = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: ctrl,
    };
    let frame = |context: &egui::Context, density, events| {
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))),
            focused: true,
            modifiers: ctrl,
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .expect("viewport")
            .native_pixels_per_point = Some(density);
        context.run_ui(input, |ui| {
            let response = ui.interact(
                Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 200.0)),
                "media zoom target".into(),
                egui::Sense::hover(),
            );
            crate::wheel_input::begin_frame(context);
            let zoom = crate::wheel_input::video_zoom_events(context, &response);
            context.data_mut(|data| data.insert_temp(egui::Id::new("zoom results"), zoom));
        })
    };
    // Prove that the same key events would change egui's default GUI scale.
    let control = egui::Context::default();
    frame(&control, 1.0, vec![key(egui::Key::Plus)]);
    frame(&control, 1.0, vec![]);
    assert!(control.zoom_factor() > 1.0);
    for density in [1.0, 1.25, 2.0] {
        let context = egui::Context::default();
        configure_input(&context);
        frame(
            &context,
            density,
            vec![egui::Event::PointerMoved(egui::pos2(100.0, 100.0))],
        );
        for input_key in [
            egui::Key::Plus,
            egui::Key::Equals,
            egui::Key::Minus,
            egui::Key::Num0,
        ] {
            frame(&context, density, vec![key(input_key)]);
            let output = frame(&context, density, vec![]);
            assert_eq!(context.zoom_factor(), 1.0);
            assert_eq!(output.pixels_per_point, density);
        }
        for event in [
            egui::Event::Zoom(1.25),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, 1.0),
                modifiers: ctrl,
                phase: egui::TouchPhase::Move,
            },
        ] {
            frame(&context, density, vec![event]);
            let zoom = context
                .data(|data| data.get_temp::<Vec<(Pos2, f32)>>(egui::Id::new("zoom results")))
                .expect("zoom results");
            assert_eq!(zoom.len(), 1, "media still receives its zoom input");
            assert!(zoom[0].1 > 1.0);
            let output = frame(&context, density, vec![]);
            assert_eq!(context.zoom_factor(), 1.0);
            assert_eq!(output.pixels_per_point, density);
        }
    }
}

#[test]
fn menu_separators_keep_border_color_without_restoring_flat_button_strokes() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.global_style_mut(style);
        context.set_pixels_per_point(density);
        let output = context.run_ui(Default::default(), |ui| {
            flat_buttons(ui);
            let before = ui.visuals().widgets.clone();
            ui.label("First group");
            separator(ui);
            ui.label("Second group");
            assert_eq!(ui.visuals().widgets, before);
        });
        assert!(output.shapes.iter().any(|shape| matches!(shape.shape,
            egui::Shape::LineSegment { points, stroke }
                if points[0].y == points[1].y && points[0].x < points[1].x
                    && stroke.color == BORDER && stroke.width == 1.0)));
    }
}

#[test]
fn text_inputs_share_height_and_corners_without_collapsing_multiline_editors() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.global_style_mut(style);
        let mut value = String::from("Value");
        let mut multiline = String::from("First\nSecond");
        let mut baseline = multiline.clone();
        let mut readonly = String::from("Ctrl+K");
        let mut rects = [Rect::NOTHING; 5];
        let mut frame = || {
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 600.0))),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .expect("root viewport")
                .native_pixels_per_point = Some(density);
            context.run_ui(input, |ui| {
                let single = crate::resize::text_input(ui, "Single", &mut value, "");
                rects[0] = single.rect;
                single.request_focus();
                rects[1] = crate::resize::unframed_text_input(ui, "Search", &mut value).rect;
                rects[2] = text_edit(
                    ui,
                    egui::TextEdit::singleline(&mut readonly).interactive(false),
                    false,
                )
                .rect;
                rects[3] =
                    crate::resize::multiline_text_input(ui, "Multiline", &mut multiline).rect;
                rects[4] = ui
                    .add(
                        egui::TextEdit::multiline(&mut baseline)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                            .char_limit(4097),
                    )
                    .rect;
            })
        };
        frame();
        let output = frame();
        assert_eq!(output.pixels_per_point, density);
        for rect in &rects[..3] {
            assert!((rect.height() - 26.0).abs() <= 1.0 / density, "{rect:?}");
        }
        assert_eq!(rects[3].height(), rects[4].height());
        assert!(rects[3].height() > 26.0);
        for rect in [rects[0], rects[2], rects[3]] {
            let border = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(shape)
                        if shape.rect.min.distance(rect.min) <= 1.0 / density
                            && shape.rect.max.distance(rect.max) <= 1.0 / density =>
                    {
                        Some(shape)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("input frame at {density}x in {rect:?}"));
            assert_eq!(border.corner_radius, egui::CornerRadius::same(3));
        }
    }
}

#[test]
fn flat_buttons_and_combo_keep_corners_through_hover_press_and_open() {
    for density in [1.0, 1.25, 2.0] {
        for target in [0, 1] {
            let context = crate::fonts::test_context();
            context.global_style_mut(style);
            let mut rects = [Rect::NOTHING; 2];
            let mut open = false;
            let mut frame = |events| {
                let mut input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 240.0))),
                    events,
                    ..Default::default()
                };
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .expect("viewport")
                    .native_pixels_per_point = Some(density);
                let output = context.run_ui(input, |ui| {
                    ui.horizontal(|ui| {
                        flat_buttons(ui);
                        rects[0] = ui.button("Apply").rect;
                        let combo = combo_box(
                            ui,
                            egui::ComboBox::from_id_salt("filter").selected_text("Bicubic"),
                            |ui| {
                                let _ = ui.selectable_label(true, "Bicubic");
                                let _ = ui.selectable_label(false, "Lanczos");
                            },
                        );
                        rects[1] = combo.response.rect;
                        open = combo.inner.is_some();
                    });
                });
                for rect in rects {
                    let shape = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Rect(shape)
                                if shape.rect.min.distance(rect.min) <= 1.0 / density
                                    && shape.rect.max.distance(rect.max) <= 1.0 / density =>
                            {
                                Some(shape)
                            }
                            _ => None,
                        })
                        .expect("painted control background");
                    assert_eq!(shape.corner_radius, egui::CornerRadius::same(3));
                    assert_eq!(shape.stroke, Stroke::NONE);
                }
                (rects, open)
            };
            frame(vec![]);
            let initial = frame(vec![]).0;
            let position = initial[target].center();
            frame(vec![egui::Event::PointerMoved(position)]);
            frame(vec![]);
            frame(vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }]);
            frame(vec![]);
            frame(vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            let (final_rects, open) = frame(vec![]);
            assert_eq!(initial, final_rects, "interaction must not resize controls");
            assert_eq!(open, target == 1, "combo opens after activation");
            frame(vec![egui::Event::PointerMoved(egui::pos2(390.0, 230.0))]);
            frame(vec![]);
        }
    }
}
