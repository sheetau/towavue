use egui::{Response, Ui};

/// Clip at viewport edges, leaving the rounded row's antialiasing fringe intact.
/// The interaction rectangle also ends at the row itself; those edges are not
/// clipping boundaries and must not cut off the last physical pixel.
pub(super) fn background_clip(ui: &Ui, row: &Response) -> egui::Rect {
    let mut clip = ui.clip_rect();
    for axis in 0..2 {
        if row.interact_rect.min[axis] > row.rect.min[axis] {
            clip.min[axis] = clip.min[axis].max(row.interact_rect.min[axis]);
        }
        if row.interact_rect.max[axis] < row.rect.max[axis] {
            clip.max[axis] = clip.max[axis].min(row.interact_rect.max[axis]);
        }
    }
    clip
}

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

pub(crate) fn menu_checkbox(
    ui: &mut Ui,
    checked: &mut bool,
    label: impl Into<egui::WidgetText>,
) -> Response {
    let label = label.into();
    let text = label.text().to_owned();
    let height = ui.spacing().interact_size.y;
    let padding = ui.spacing().button_padding.x;
    let galley = label.into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (ui.available_width() - height - 2.0 * padding).max(0.0),
        egui::TextStyle::Button,
    );
    let (rect, mut response) = ui.allocate_at_least(
        egui::vec2(height + galley.size().x + 2.0 * padding, height),
        egui::Sense::click(),
    );
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            response.enabled(),
            *checked,
            &text,
        )
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let checkbox = egui::Rect::from_center_size(
            egui::pos2(rect.left() + height * 0.5, rect.center().y),
            egui::Vec2::splat(ui.spacing().icon_width),
        );
        ui.painter().rect(
            checkbox,
            visuals.corner_radius,
            super::BORDER,
            egui::Stroke::NONE,
            egui::StrokeKind::Inside,
        );
        let icon = egui::Rect::from_center_size(
            checkbox.center(),
            egui::Vec2::splat(ui.spacing().icon_width_inner),
        );
        if *checked {
            ui.painter().add(egui::Shape::line(
                vec![
                    egui::pos2(icon.left(), icon.center().y),
                    egui::pos2(icon.center().x, icon.bottom()),
                    egui::pos2(icon.right(), icon.top()),
                ],
                visuals.fg_stroke,
            ));
        }
        ui.painter().galley(
            egui::pos2(
                rect.left() + height + padding,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            visuals.fg_stroke.color,
        );
    }
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data_mut(|data| data.insert_temp(response.id.with("menu-checkbox"), pass));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Resolve pointer, keyboard and open-submenu feedback to one row per popup.
/// Row widgets still own text, checkmarks and accessibility state.
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
                    disabled_foreground(&mut clipped.shape, &rows);
                });
            }
        });
        ui.ctx().graphics_mut(|graphics| {
            graphics.entry(ui.layer_id()).mutate_shape(self.0, |shape| {
                shape.clip_rect = shape.clip_rect.intersect(ui.clip_rect());
            });
        });
        let pass = ui.ctx().cumulative_pass_nr();
        if let Some(row) = selected
            && !ui.data(|data| data.get_temp::<u64>(row.id.with("menu-checkbox")) == Some(pass))
        {
            ui.painter()
                .with_clip_rect(ui.clip_rect())
                .set(self.0, egui::Shape::rect_filled(row.rect, 3, super::HOVER));
            ui.ctx().graphics_mut(|graphics| {
                graphics.entry(ui.layer_id()).mutate_shape(self.0, |shape| {
                    shape.clip_rect = shape.clip_rect.intersect(background_clip(ui, row))
                })
            });
        }
    }
}

fn disabled_foreground(shape: &mut egui::Shape, rows: &[Response]) {
    if let egui::Shape::Vec(shapes) = shape {
        for shape in shapes {
            disabled_foreground(shape, rows);
        }
        return;
    }
    if !rows
        .iter()
        .any(|row| !row.enabled() && row.rect.contains(shape.visual_bounding_rect().center()))
    {
        return;
    }
    // egui disables input/opacity without changing the response's click sense.
    // Use an opaque foreground for disabled rows, preserving their backgrounds.
    match shape {
        egui::Shape::Text(text) => {
            text.fallback_color = super::DISABLED;
            text.override_text_color = Some(super::DISABLED);
        }
        egui::Shape::Mesh(mesh) => {
            for vertex in &mut std::sync::Arc::make_mut(mesh).vertices {
                vertex.color = super::DISABLED.gamma_multiply(vertex.color.a() as f32 / 255.0);
            }
        }
        egui::Shape::Path(path) => {
            path.stroke.color = egui::epaint::ColorMode::Solid(super::DISABLED);
        }
        egui::Shape::LineSegment { stroke, .. } => stroke.color = super::DISABLED,
        _ => {}
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
                        let second = menu_checkbox(ui, &mut checked, "Checked");
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
                assert!(
                    backgrounds.len() <= 1,
                    "at most one row background at {density}"
                );
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Path(path) if path.points.len() == 3)), "checked state still paints");
                (bounds, backgrounds.first().copied())
            };
            draw(vec![]);
            let (bounds, highlighted) = draw(vec![]);
            assert_eq!(highlighted, Some(bounds[0]));
            draw(vec![egui::Event::PointerMoved(bounds[1].center())]);
            assert_eq!(
                draw(vec![egui::Event::PointerMoved(bounds[1].center())]).1,
                None
            );
            assert_eq!(draw(vec![egui::Event::PointerGone]).1, Some(bounds[0]));
        }
    }
}
