use super::*;

#[test]
fn numeric_values_stay_left_aligned_when_entering_text_edit_mode() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(style);
        for editing in [false, true] {
            let mut control = Rect::NOTHING;
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = context.run_ui(Default::default(), |ui| {
                    let response = number_input(ui, &mut 14.0, -180.0..=180.0, 160.0, "°", "Angle");
                    control = response.rect;
                    if editing {
                        response.request_focus();
                    }
                });
            }
            let text = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "14.0" => Some(text),
                    _ => None,
                })
                .expect("numeric text");
            assert!(
                text.pos.x - control.left() < 12.0,
                "left-aligned value at {density}, editing={editing}"
            );
            assert!((control.height() - INPUT_HEIGHT).abs() <= 1.0 / density);
        }
    }
}

#[test]
fn modal_actions_center_visible_text_and_put_the_primary_action_on_the_right() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(style);
        let mut buttons = Vec::new();
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = context.run_ui(Default::default(), |ui| {
                ui.set_width(240.0);
                ui.style_mut().override_font_id = Some(egui::FontId::proportional(TEXT_MD));
                buttons = modal_actions(ui, &[("H", true), ("Cancel", true)]);
            });
        }
        assert!(buttons[1].rect.right() < buttons[0].rect.left());
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "H" => Some(text),
                _ => None,
            })
            .expect("primary action text");
        let ink = text.galley.mesh_bounds.translate(text.pos.to_vec2());
        assert!((ink.center().x - buttons[0].rect.center().x).abs() <= 1.0 / density);
        assert!(
            (ink.center().y - buttons[0].rect.center().y).abs() <= 1.0 / density,
            "visible cap height must be centered at {density}: {ink:?} {:?}",
            buttons[0].rect
        );
    }
}

#[test]
fn figtree_cap_height_is_centered_at_all_shared_sizes_and_densities() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        let _ = context.run_ui(Default::default(), |ui| {
            for size in [TEXT_SM, TEXT_MD, TEXT_LG] {
                let text = ui.painter().layout_no_wrap(
                    "H".into(),
                    egui::FontId::proportional(size),
                    FOREGROUND,
                );
                assert!(
                    (text.mesh_bounds.center().y - text.size().y * 0.5).abs() <= 1.0 / density,
                    "size={size}, density={density}, ink={:?}, line={:?}",
                    text.mesh_bounds,
                    text.size()
                );
            }
        });
    }
}
