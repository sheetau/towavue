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

// Floating bars still own their full expanded hit rectangle. Reserve that width
// so a row action cannot be painted over a scrollbar click/drag target.
pub(super) fn reserve_scroll_bar(ui: &mut egui::Ui) {
    let density = ui.pixels_per_point();
    let scroll = &mut ui.spacing_mut().scroll;
    // ScrollArea rounds its content rectangle to physical pixels. Keep the
    // separately allocated gutter on that same grid to avoid width feedback.
    let width = scroll.floating_allocated_width.max(scroll.bar_width + 2.0);
    scroll.floating_allocated_width = (width * density).ceil() / density;
}
