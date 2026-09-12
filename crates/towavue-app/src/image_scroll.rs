use crate::*;

#[cfg(test)]
mod tests;

fn bar_viewport(viewport: egui::Rect) -> egui::Rect {
    viewport.shrink(8.0_f32.min(viewport.size().min_elem().max(0.0) * 0.25))
}

pub fn clamp(view: &mut ImageViewState, displayed: egui::Vec2, viewport: egui::Vec2) {
    let limit = (displayed - viewport).max(egui::Vec2::ZERO) * 0.5;
    view.pan.0 = view.pan.0.clamp(-limit.x, limit.x);
    view.pan.1 = view.pan.1.clamp(-limit.y, limit.y);
}

pub fn surface(viewport: egui::Rect, displayed: egui::Vec2, bar_width: f32) -> egui::Rect {
    let mut rect = viewport;
    let bars = bar_viewport(viewport);
    if displayed.x > viewport.width() {
        rect.max.y = bars.max.y - bar_width;
    }
    if displayed.y > viewport.height() {
        rect.max.x = bars.max.x - bar_width;
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
    if viewport.size().min_elem() <= 0.0 {
        return;
    }
    let bars = bar_viewport(viewport);
    // Scale the virtual content with the inset tracks so thumb fractions and
    // pan limits still describe the full image viewport, not the smaller UI.
    let ratio = bars.size() / viewport.size();
    let overflow = (displayed - viewport.size()).max(egui::Vec2::ZERO);
    let offset = (overflow * 0.5 - egui::vec2(view.pan.0, view.pan.1)) * ratio;
    let output = ui.scope_builder(egui::UiBuilder::new().max_rect(bars), |ui| {
        if !enabled {
            ui.disable();
        }
        ui.style_mut().animation_time = 0.0;
        let style = &mut ui.style_mut().spacing.scroll;
        style.floating = true;
        style.floating_allocated_width = 0.0;
        style.dormant_handle_opacity = 1.0;
        egui::ScrollArea::new([overflow.x > 0.0, overflow.y > 0.0])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .id_salt("image-scroll")
            .auto_shrink([false, false])
            .content_margin(0)
            .animated(false)
            .scroll_offset(offset)
            .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR)
            .show(ui, |ui| {
                ui.set_min_size(displayed.max(viewport.size()) * ratio);
            })
    });
    let output = output.inner;
    let maximum = (output.content_size - output.inner_rect.size()).max(egui::Vec2::ZERO);
    let mut pan = egui::Vec2::from(view.pan);
    for axis in 0..2 {
        // Layout rounds virtual content. Ignore that idle clamp and map actual
        // bar movement through its measured range so both endpoints stay exact.
        if maximum[axis] > 0.0
            && output.state.offset[axis] != offset[axis].clamp(0.0, maximum[axis])
        {
            pan[axis] = overflow[axis] * (0.5 - output.state.offset[axis] / maximum[axis]);
        }
    }
    view.pan = pan.into();
    clamp(view, displayed, viewport.size());
}
