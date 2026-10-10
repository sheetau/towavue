use egui::{Response, Ui};

pub(super) fn highlighted<'a>(
    ui: &Ui,
    rows: &'a [Response],
    fallback: Option<egui::Id>,
) -> Option<&'a Response> {
    rows.iter()
        .find(|row| {
            row.enabled()
                && row.contains_pointer()
                && ui
                    .ctx()
                    .pointer_hover_pos()
                    .is_some_and(|point| ui.ctx().layer_id_at(point) == Some(row.layer_id))
        })
        .or_else(|| rows.iter().find(|row| row.enabled() && row.has_focus()))
        .or_else(|| {
            rows.iter()
                .find(|row| row.enabled() && Some(row.id) == fallback)
        })
}

/// Resolve pointer, keyboard and open-submenu feedback to one row per popup.
/// Native widget painting still owns text, checkmarks and accessibility state.
pub(crate) struct MenuRows(egui::layers::ShapeIdx);

impl MenuRows {
    pub fn new(ui: &Ui) -> Self {
        Self(ui.painter().add(egui::Shape::Noop))
    }

    pub fn finish(self, ui: &Ui, ids: &[egui::Id]) {
        let rows: Vec<_> = ids
            .iter()
            .filter_map(|id| ui.ctx().read_response(*id))
            .collect();
        let open = egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item);
        let fallback = rows
            .iter()
            .find(|row| Some(egui::containers::menu::SubMenu::id_from_widget_id(row.id)) == open)
            .map(|row| row.id);
        let selected = highlighted(ui, &rows, fallback);
        ui.ctx().graphics_mut(|graphics| {
            let list = graphics.entry(ui.layer_id());
            let end = list.next_idx().0;
            for index in self.0.0 + 1..end {
                list.mutate_shape(egui::layers::ShapeIdx(index), |clipped| {
                    clear_row_backgrounds(&mut clipped.shape, &rows);
                });
            }
        });
        if let Some(row) = selected {
            ui.painter()
                .with_clip_rect(ui.clip_rect().intersect(row.interact_rect))
                .set(self.0, egui::Shape::rect_filled(row.rect, 3, super::HOVER));
        }
    }
}

fn clear_row_backgrounds(shape: &mut egui::Shape, rows: &[Response]) {
    match shape {
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                clear_row_backgrounds(shape, rows);
            }
        }
        egui::Shape::Rect(rect)
            if rect.fill == super::HOVER
                && rows.iter().any(|row| {
                    row.rect.min.distance(rect.rect.min) < 1.0
                        && row.rect.max.distance(rect.rect.max) < 1.0
                }) =>
        {
            *shape = egui::Shape::Noop
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_highlight_replaces_keyboard_background_and_preserves_checkmarks() {
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
                        let rows = MenuRows::new(ui);
                        let first = ui.button("Keyboard");
                        first.request_focus();
                        let mut checked = true;
                        let second = ui.checkbox(&mut checked, "Checked");
                        assert!(checked);
                        let third = ui.add_enabled(false, egui::Button::new("Disabled"));
                        bounds.extend([first.rect, second.rect, third.rect]);
                        rows.finish(ui, &[first.id, second.id, third.id]);
                    },
                );
                let backgrounds: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.fill == crate::chrome::HOVER && bounds.contains(&rect.rect) =>
                        {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(backgrounds.len(), 1, "one menu row background at {density}");
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Path(path) if path.points.len() == 3)), "checked state still paints");
                (bounds, backgrounds[0])
            };
            draw(vec![]);
            let (bounds, highlighted) = draw(vec![]);
            assert_eq!(highlighted, bounds[0]);
            draw(vec![egui::Event::PointerMoved(bounds[1].center())]);
            assert_eq!(
                draw(vec![egui::Event::PointerMoved(bounds[1].center())]).1,
                bounds[1]
            );
            assert_eq!(draw(vec![egui::Event::PointerGone]).1, bounds[0]);
        }
    }
}
