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
