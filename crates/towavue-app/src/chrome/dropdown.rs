use egui::{Response, Ui};

/// Paint a single row background while retaining each option's actual toggle state.
pub struct Dropdown {
    background: egui::layers::ShapeIdx,
    rows: Vec<(Response, bool)>,
}

impl Dropdown {
    pub fn new(ui: &Ui) -> Self {
        Self {
            background: ui.painter().add(egui::Shape::Noop),
            rows: Vec::new(),
        }
    }

    pub fn row(&mut self, ui: &mut Ui, enabled: bool, selected: bool, label: &str) -> Response {
        let response = ui.add_enabled(
            enabled,
            egui::Button::selectable(selected, label).frame(false),
        );
        self.rows.push((response.clone(), selected));
        response
    }

    pub fn value<T: PartialEq>(
        &mut self,
        ui: &mut Ui,
        current: &mut T,
        value: T,
        label: &str,
    ) -> Response {
        let mut response = self.row(ui, true, *current == value, label);
        if response.clicked() && *current != value {
            *current = value;
            response.mark_changed();
        }
        response
    }

    pub fn finish(self, ui: &Ui) {
        let hovered = self.rows.iter().find(|(response, _)| {
            response.enabled()
                && response.contains_pointer()
                && ui
                    .ctx()
                    .pointer_hover_pos()
                    .is_some_and(|point| ui.ctx().layer_id_at(point) == Some(response.layer_id))
        });
        let focused = self
            .rows
            .iter()
            .find(|(response, _)| response.enabled() && response.has_focus());
        let selected = self.rows.iter().find(|(_, selected)| *selected);
        if let Some((response, _)) = hovered.or(focused).or(selected) {
            ui.painter().set(
                self.background,
                egui::Shape::rect_filled(
                    response.rect,
                    ui.visuals().widgets.hovered.corner_radius,
                    ui.visuals().selection.bg_fill,
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_and_held_pointer_choose_one_background_without_changing_selection() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.global_style_mut(crate::chrome::style);
            context.set_pixels_per_point(density);
            let draw = |events| {
                let mut bounds = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        crate::chrome::menu(ui);
                        let mut rows = Dropdown::new(ui);
                        for (index, label) in
                            ["Selected", "Other", "Disabled"].into_iter().enumerate()
                        {
                            bounds.push(rows.row(ui, index != 2, index == 0, label).rect);
                        }
                        rows.finish(ui);
                    },
                );
                let highlights: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) if rect.fill == crate::chrome::HOVER => {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(highlights.len(), 1);
                (bounds, highlights[0])
            };
            draw(vec![]);
            let (bounds, selected) = draw(vec![]);
            assert_eq!(selected, bounds[0]);
            let point = bounds[1].center();
            assert_eq!(draw(vec![egui::Event::PointerMoved(point)]).1, bounds[1]);
            assert_eq!(
                draw(vec![egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }])
                .1,
                bounds[1]
            );
            draw(vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            context.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
            assert_eq!(draw(vec![egui::Event::PointerGone]).1, bounds[0]);
            assert_eq!(
                draw(vec![egui::Event::PointerMoved(bounds[2].center())]).1,
                bounds[0]
            );
        }
    }
}
