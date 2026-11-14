//! Scoped text and numeric inputs with shared geometry and accessibility.
use super::{INPUT_HEIGHT, INPUT_RADIUS, SURFACE};
use egui::{Stroke, Ui};

pub fn input_style(ui: &mut Ui) {
    let style = ui.style_mut();
    for visuals in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        visuals.corner_radius = egui::CornerRadius::same(INPUT_RADIUS);
    }
    style.spacing.interact_size.y = INPUT_HEIGHT;
}

pub fn text_edit(ui: &mut Ui, editor: egui::TextEdit<'_>, multiline: bool) -> egui::Response {
    ui.scope(|ui| {
        input_style(ui);
        if multiline {
            ui.add(editor)
        } else {
            ui.add_sized(
                [
                    ui.spacing().text_edit_width.min(ui.available_width()),
                    INPUT_HEIGHT,
                ],
                editor.vertical_align(egui::Align::Center),
            )
        }
    })
    .inner
}

/// Fixed geometry for editable numeric controls, including their drag state.
pub fn number_input(
    ui: &mut Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    width: f32,
    suffix: &str,
    label: &str,
) -> egui::Response {
    let response = ui
        .scope(|ui| {
            input_style(ui);
            ui.spacing_mut().interact_size = egui::vec2(width, INPUT_HEIGHT);
            ui.style_mut().drag_value_text_style = egui::TextStyle::Body;
            ui.spacing_mut().button_padding.y = 2.0;
            ui.visuals_mut().widgets.inactive.weak_bg_fill = ui.visuals().extreme_bg_color;
            ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::NONE;
            ui.visuals_mut().widgets.hovered.weak_bg_fill = SURFACE;
            ui.visuals_mut().widgets.hovered.bg_fill = SURFACE;
            let layout = egui::Layout::left_to_right(egui::Align::Center)
                .with_main_align(egui::Align::Min)
                .with_cross_justify(true)
                .with_main_justify(true);
            ui.allocate_ui_with_layout(egui::vec2(width, INPUT_HEIGHT), layout, |ui| {
                let id = ui.next_auto_id();
                ui.input_mut(|input| {
                    input.consume_accesskit_action_requests(id, |request| {
                        matches!(request.data,
                            Some(egui::accesskit::ActionData::NumericValue(value))
                                if !value.is_finite())
                    });
                });
                ui.add(
                    egui::DragValue::from_get_set(|next| {
                        if let Some(next) = next.filter(|v| v.is_finite()) {
                            *value = next;
                        }
                        *value
                    })
                    .range(range)
                    .speed(0.1)
                    .fixed_decimals(1)
                    .suffix((egui::Atom::grow(), suffix))
                    .custom_parser(|text| {
                        text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
                    }),
                )
            })
            .inner
        })
        .inner;
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_label(label));
    response
}
