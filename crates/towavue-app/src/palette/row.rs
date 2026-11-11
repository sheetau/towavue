use super::*;

pub(super) fn category(ui: &mut egui::Ui, label: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 16.0), egui::Sense::hover());
    ui.painter().text(
        rect.left_center() + egui::vec2(crate::chrome::PADDING as f32, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(crate::chrome::TEXT_SM),
        crate::chrome::FAINT,
    );
}

/// Key names have caps; chord separators remain ordinary text. The width is
/// measured once and reserved in the row so long names cannot overlap actions.
pub(super) struct Keycaps(Vec<(std::sync::Arc<egui::Galley>, bool)>, f32);

impl Keycaps {
    pub fn new(ui: &egui::Ui, label: &str, selected: bool, enabled: bool) -> Self {
        let color = if !enabled {
            crate::chrome::DISABLED
        } else if selected {
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
                    egui::pos2(right - crate::chrome::ROW_HEIGHT * 0.5, row.center().y),
                    egui::Vec2::splat(crate::chrome::ROW_HEIGHT),
                );
                right = rect.left() - crate::chrome::ITEM_GAP;
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
    ui.visuals_mut().widgets.hovered.fg_stroke.color = hovered;
    ui.visuals_mut().widgets.active.fg_stroke.color = active;
    ui.visuals_mut().widgets.open.fg_stroke.color = open;
    response
}

pub(super) fn row_icon(ui: &mut egui::Ui, rect: egui::Rect, glyph: char) -> egui::Response {
    crate::chrome::icon_button_at(
        ui,
        rect,
        egui::Button::new(egui::RichText::new(glyph.to_string()).font(crate::fonts::icon_font()))
            .frame(false),
    )
}

// ScrollArea paints and registers the floating bar after its children, so its
// full hit region takes priority over row actions beneath it without a gutter.
pub(super) fn overlay_scroll_bar(ui: &mut egui::Ui) {
    crate::scroll_style::control_style(ui.style_mut());
    ui.spacing_mut().scroll.floating = true;
    ui.spacing_mut().scroll.floating_allocated_width = 0.0;
}
