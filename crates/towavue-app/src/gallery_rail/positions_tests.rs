use super::*;

#[test]
fn document_markers_are_independent_of_hover_and_the_viewport_thumb_eases() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        let rail = egui::Rect::from_min_max(egui::pos2(600.0, 40.0), egui::pos2(632.0, 360.0));
        let mut clock = 0.0;
        let mut frame = |selected, snap, events| {
            clock += 1.0 / 60.0;
            let mut offset = 0.0;
            let output = context.run_ui(
                egui::RawInput {
                    time: Some(clock),
                    events,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 400.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    ui.add_space(40.0);
                    let id = ui.make_persistent_id(egui::IdSalt::new("positions"));
                    advance(&context, id);
                    let mut output = egui::ScrollArea::vertical()
                        .id_salt("positions")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .max_height(320.0)
                        .show(ui, |ui| {
                            ui.set_min_height(2400.0);
                        });
                    show_positions(ui, &mut output, rail, 11, Some(3), Some(selected), snap);
                    offset = output.state.offset.y;
                },
            );
            (output, offset)
        };
        frame(3, true, vec![]);
        let (output, _) = frame(3, true, vec![]);
        let lines = |output: &egui::FullOutput, color| {
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(shape)
                        if shape.fill == color && rail.contains_rect(shape.rect) =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(lines(&output, chrome::FOREGROUND).len(), 1);
        assert_eq!(lines(&output, chrome::MUTED).len(), 2);
        let original = lines(&output, chrome::MUTED)[1];
        let (output, _) = frame(7, false, vec![]);
        let gray = lines(&output, chrome::MUTED);
        let white = lines(&output, chrome::FOREGROUND);
        assert_eq!(gray.len(), 2);
        assert_eq!(white.len(), 1);
        assert!(gray[1].top() > original.top());
        assert!(gray[1].top() < rail.top() + rail.height() * 0.7);
        assert!(gray[0].width() < gray[1].width());
        assert_eq!(white[0].top(), rail.top());
        assert!((white[0].height() - 1.0 / density).abs() < 0.0001);
        let point = egui::pos2(616.0, 328.0);
        let (output, _) = frame(7, false, vec![egui::Event::PointerMoved(point)]);
        assert_eq!(
            lines(&output, chrome::MUTED).len(),
            2,
            "hover adds no marker"
        );
        assert_eq!(lines(&output, chrome::MUTED)[0], gray[0]);
        let (output, _) = frame(7, true, vec![]);
        let snapped = lines(&output, chrome::MUTED)[1];
        assert!((snapped.top() - (rail.top() + rail.height() * 0.7)).abs() <= 1.0 / density);
        let button = |pressed| egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(7, false, vec![button(true), button(false)]);
        let (_, first) = frame(7, false, vec![]);
        assert!(
            first > 0.0 && first < 1800.0,
            "rail motion starts between endpoints: {first}"
        );
        let mut settled = first;
        for _ in 0..60 {
            settled = frame(7, false, vec![]).1;
        }
        assert!(settled > first && settled < 2400.0);
    }
}
