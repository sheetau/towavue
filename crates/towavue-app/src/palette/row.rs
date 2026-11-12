use super::*;

pub(super) fn category(ui: &mut egui::Ui, label: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 16.0), egui::Sense::hover());
    ui.painter().text(
        rect.left_center() + egui::vec2(crate::chrome::PADDING as f32, 0.0),
        egui::Align2::LEFT_CENTER,
        label.to_uppercase(),
        egui::FontId::proportional(crate::chrome::TEXT_SM),
        crate::chrome::FAINT,
    );
}

/// Key names have caps; chord separators remain ordinary text. The width is
/// measured once and reserved in the row so long names cannot overlap actions.
pub(super) struct Keycaps(Vec<(std::sync::Arc<egui::Galley>, bool)>, f32);

impl Keycaps {
    pub fn new(ui: &egui::Ui, label: &str, selected: bool) -> Self {
        let color = if selected {
            crate::chrome::FOREGROUND
        } else {
            crate::chrome::MUTED
        };
        let mut parts = Vec::new();
        let mut start = 0;
        for (index, c) in label.char_indices() {
            if (c == '+' || c == ' ') && index > start {
                parts.push((&label[start..index], true));
                parts.push((&label[index..index + c.len_utf8()], false));
                start = index + c.len_utf8();
            }
        }
        if start < label.len() {
            parts.push((&label[start..], true));
        }
        let parts: Vec<_> = parts
            .into_iter()
            .map(|(text, cap)| {
                (
                    ui.painter().layout_no_wrap(
                        text.into(),
                        egui::FontId::proportional(crate::chrome::TEXT_MD),
                        color,
                    ),
                    cap,
                )
            })
            .collect();
        let width = parts
            .iter()
            .map(|(galley, cap)| galley.size().x + if *cap { 8.0 } else { 6.0 })
            .sum();
        Self(parts, width)
    }

    pub fn width(&self) -> f32 {
        self.1
    }

    pub fn paint(&self, ui: &egui::Ui, rect: egui::Rect, selected: bool) {
        let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
        let mut x = rect.right() - self.1;
        for (text, cap) in &self.0 {
            let padding = if *cap { 4.0 } else { 3.0 };
            let width = text.size().x + padding * 2.0;
            if *cap {
                painter.rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(x + width * 0.5, rect.center().y),
                        egui::vec2(width, text.size().y + 2.0),
                    ),
                    crate::chrome::RADIUS_XS,
                    if selected {
                        crate::chrome::KEY_ACTIVE
                    } else {
                        crate::chrome::SURFACE
                    },
                );
            }
            painter.galley(
                egui::pos2(x + padding, rect.center().y - text.size().y * 0.5),
                text.clone(),
                crate::chrome::MUTED,
            );
            x += width;
        }
    }
}

pub(super) struct RowActions {
    pub body: egui::Rect,
    pub remove: Option<egui::Rect>,
    pub configure: Option<egui::Rect>,
}

impl RowActions {
    pub fn new(row: egui::Rect, remove: bool, configure: bool) -> Self {
        let mut right = row.right();
        let mut slot = |visible: bool| {
            visible.then(|| {
                let rect = egui::Rect::from_center_size(
                    egui::pos2(right - crate::chrome::ICON_SIZE * 0.5, row.center().y),
                    egui::Vec2::splat(crate::chrome::ICON_SIZE),
                );
                right = rect.left() - crate::chrome::PADDING as f32;
                rect
            })
        };
        let remove = slot(remove);
        let configure = slot(configure);
        Self {
            body: egui::Rect::from_min_max(row.min, egui::pos2(right, row.bottom())),
            remove,
            configure,
        }
    }
}

pub(super) fn label_gap(ui: &egui::Ui) -> egui::Atom<'static> {
    egui::Atom::default().atom_size(egui::vec2(ui.spacing().icon_spacing, 0.0))
}

pub(super) fn row_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    selected: bool,
    mut atoms: egui::Atoms<'_>,
) -> egui::Response {
    // Keep the shared left inset, with no symmetric right padding or
    // empty trailing atoms extending the gap before independent row actions.
    atoms
        .push_left(egui::Atom::default().atom_size(egui::vec2(crate::chrome::PADDING as f32, 0.0)));
    crate::chrome::flat_buttons(ui);
    let foreground = ui.visuals().widgets.inactive.fg_stroke.color;
    let hovered = ui.visuals().widgets.hovered.fg_stroke.color;
    let active = ui.visuals().widgets.active.fg_stroke.color;
    let open = ui.visuals().widgets.open.fg_stroke.color;
    if !selected {
        // Only the Enter target uses selected text; pointer feedback keeps its fill.
        ui.visuals_mut().widgets.hovered.fg_stroke.color = foreground;
        ui.visuals_mut().widgets.active.fg_stroke.color = foreground;
        ui.visuals_mut().widgets.open.fg_stroke.color = foreground;
    }
    let response = ui.put(
        rect,
        egui::Button::selectable(selected, atoms)
            .gap(0.0)
            .frame(false)
            .truncate()
            .min_size(rect.size()),
    );
    ui.visuals_mut().widgets.inactive.fg_stroke.color = foreground;
    ui.visuals_mut().widgets.hovered.fg_stroke.color = hovered;
    ui.visuals_mut().widgets.active.fg_stroke.color = active;
    ui.visuals_mut().widgets.open.fg_stroke.color = open;
    response
}

pub(super) fn row_icon(ui: &mut egui::Ui, rect: egui::Rect, glyph: char) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.fg_stroke.color = crate::chrome::MUTED;
        ui.visuals_mut().widgets.hovered.fg_stroke.color = crate::chrome::FOREGROUND;
        crate::chrome::icon_button_at(
            ui,
            rect,
            egui::Button::new(
                egui::RichText::new(glyph.to_string())
                    .font(crate::fonts::icon_font())
                    .atom_size(egui::Vec2::splat(crate::chrome::ICON_SIZE))
                    .atom_align(egui::Align2::CENTER_CENTER),
            )
            .frame(false),
        )
    })
    .inner
}

// ScrollArea paints and registers the floating bar after its children, so its
// full hit region takes priority over row actions beneath it without a gutter.
pub(super) fn overlay_scroll_bar(ui: &mut egui::Ui) {
    crate::scroll_style::control_style(ui.style_mut());
    ui.spacing_mut().scroll.floating = true;
    ui.spacing_mut().scroll.floating_allocated_width = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_caret_uses_only_frame_button_and_input_insets() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            context.global_style_mut(crate::chrome::style);
            let mut palette = CommandPalette::default();
            palette.begin_options(
                CommandId::CycleVolumeStep,
                localization::Language::English,
                (None, 0),
                vec![],
            );
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(800.0, 500.0),
                        )),
                        ..Default::default()
                    },
                    |_| {
                        palette.show(
                            &context,
                            CommandContext {
                                palette_open: true,
                                ..Default::default()
                            },
                            &ShortcutBindings::default(),
                            0.0,
                        );
                    },
                );
            }
            let panel = context
                .memory(|memory| memory.area_rect(egui::Id::new("command-palette")))
                .expect("palette frame");
            let caret = output
                .platform_output
                .ime
                .expect("focused query")
                .cursor_rect;
            assert!(
                (caret.center().x - panel.left() - 27.0).abs() <= 1.5 / density,
                "caret {caret:?}, panel {panel:?}, density {density}"
            );
        }
    }

    #[test]
    fn row_states_keep_selection_hover_text_and_keycaps_distinct() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            context.global_style_mut(crate::chrome::style);
            let rows = [0.0, 26.0].map(|y| {
                egui::Rect::from_min_size(egui::pos2(20.0, 30.0 + y), egui::vec2(240.0, 24.0))
            });
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = context.run_ui(
                    egui::RawInput {
                        events: vec![egui::Event::PointerMoved(rows[1].center())],
                        ..Default::default()
                    },
                    |ui| {
                        ui.visuals_mut().disabled_alpha = 1.0;
                        for (index, rect) in rows.into_iter().enumerate() {
                            let mut row =
                                ui.new_child(egui::UiBuilder::new().id_salt(index).max_rect(rect));
                            let slot = row.painter().add(egui::Shape::Noop);
                            let selected = index == 0;
                            let response = row_button(
                                &mut row,
                                rect,
                                selected,
                                egui::Atoms::new((
                                    ["Selected", "Hovered"][index],
                                    egui::Atom::grow(),
                                )),
                            );
                            row.painter()
                                .set(slot, row_background(&row, rect, &response, selected));
                            Keycaps::new(&row, "Ctrl+K", selected).paint(&row, rect, selected);
                        }
                    },
                );
            }
            for (index, expected) in [crate::chrome::SURFACE, crate::chrome::FLOATING_BACKGROUND]
                .into_iter()
                .enumerate()
            {
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Rect(background) if background.rect == rows[index] && background.fill == expected)));
            }
            for (index, expected) in [crate::chrome::FOREGROUND, crate::chrome::MUTED]
                .into_iter()
                .enumerate()
            {
                let text = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if text.galley.text() == ["Selected", "Hovered"][index] =>
                        {
                            Some(text)
                        }
                        _ => None,
                    })
                    .expect("row label");
                assert_eq!(
                    text.override_text_color.unwrap_or(text.fallback_color),
                    expected
                );
                let cap = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if text.galley.text() == "Ctrl" && rows[index].contains(text.pos) =>
                        {
                            Some(text)
                        }
                        _ => None,
                    })
                    .expect("keycap");
                assert_eq!(
                    cap.galley.job.sections[0].format.color,
                    if index == 0 {
                        crate::chrome::FOREGROUND
                    } else {
                        crate::chrome::MUTED
                    }
                );
            }
        }
    }
}
