use crate::*;

#[cfg(test)]
mod tests;

pub fn clamp(view: &mut ImageViewState, displayed: egui::Vec2, viewport: egui::Vec2) {
    let limit = (displayed - viewport).max(egui::Vec2::ZERO) * 0.5;
    view.pan.0 = view.pan.0.clamp(-limit.x, limit.x);
    view.pan.1 = view.pan.1.clamp(-limit.y, limit.y);
}

pub fn surface(viewport: egui::Rect, displayed: egui::Vec2, bar_width: f32) -> egui::Rect {
    let mut rect = viewport;
    if displayed.x > viewport.width() {
        rect.max.y -= bar_width;
    }
    if displayed.y > viewport.height() {
        rect.max.x -= bar_width;
    }
    rect
}

pub fn bars(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    displayed: egui::Vec2,
    view: &mut ImageViewState,
    enabled: bool,
) {
    let overflow = (displayed - viewport.size()).max(egui::Vec2::ZERO);
    let offset = overflow * 0.5 - egui::vec2(view.pan.0, view.pan.1);
    let output = ui.scope_builder(egui::UiBuilder::new().max_rect(viewport), |ui| {
        if !enabled {
            ui.disable();
        }
        let style = &mut ui.style_mut().spacing.scroll;
        style.floating = true;
        style.floating_allocated_width = 0.0;
        style.dormant_handle_opacity = 1.0;
        egui::ScrollArea::both()
            .id_salt("image-scroll")
            .auto_shrink([false, false])
            .content_margin(0)
            .animated(false)
            .scroll_offset(offset)
            .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR)
            .show(ui, |ui| {
                ui.allocate_space(displayed.max(viewport.size()));
            })
    });
    let pan = overflow * 0.5 - output.inner.state.offset;
    view.pan = pan.into();
    clamp(view, displayed, viewport.size());
}
