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

/// Returns whether a scrollbar accepted a pointer press or completed gesture.
pub fn bars(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    displayed: egui::Vec2,
    view: &mut ImageViewState,
    enabled: bool,
) -> bool {
    if viewport.size().min_elem() <= 0.0 {
        return false;
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
            .show_styled(ui, |ui| {
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
    // egui 0.35 identifies each bar by the ScrollArea ID plus its usize axis.
    // Inspect its response, not offset changes: a thumb press can leave the view
    // stationary, and rounded layout clamps are not pointer interactions.
    // Response methods may lock the context again; release the input borrow first.
    let (pressed, released) = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_released(),
        )
    });
    enabled
        && (pressed || released)
        && (0..2_usize).any(|axis| {
            overflow[axis] > 0.0
                && ui
                    .ctx()
                    .read_response(output.id.with(axis))
                    .is_some_and(|response| {
                        let owned = response.enabled()
                            && ((pressed && response.is_pointer_button_down_on())
                                || (released
                                    && (response.clicked_by(egui::PointerButton::Primary)
                                        || response
                                            .drag_stopped_by(egui::PointerButton::Primary))));
                        if owned {
                            response.surrender_focus();
                        }
                        owned
                    })
        })
}

pub fn held_bars(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    displayed: egui::Vec2,
    mut view: ImageViewState,
) {
    // Keep the completed image's idle chrome, but never edit a pending image's
    // view or let the display-only handoff acquire pointer/keyboard ownership.
    // Keep normal IDs and suppress content-hover fading as well as disabled dimming.
    let original = ui.style().clone();
    ui.visuals_mut().disabled_alpha = 1.0;
    ui.style_mut().spacing.scroll.active_handle_opacity = 1.0;
    bars(ui, viewport, displayed, &mut view, false);
    ui.set_style(original);
}
