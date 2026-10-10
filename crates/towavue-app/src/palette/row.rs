use super::*;

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
                    egui::pos2(right - 10.0, row.center().y),
                    egui::Vec2::splat(20.0),
                );
                right = rect.left() - 2.0;
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
    // Keep the text's six-point left inset, with no symmetric right padding or
    // empty trailing atoms extending the gap before independent row actions.
    atoms.push_left(egui::Atom::default().atom_size(egui::vec2(6.0, 0.0)));
    crate::chrome::flat_buttons(ui);
    ui.put(
        rect,
        egui::Button::selectable(selected, atoms)
            .gap(0.0)
            .frame(false)
            .truncate()
            .min_size(rect.size()),
    )
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
    ui.spacing_mut().scroll.floating = true;
    ui.spacing_mut().scroll.floating_allocated_width = 0.0;
}
