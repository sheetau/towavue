use crate::*;

#[cfg(test)]
mod tests;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    /// Match the rendered scrollbar axes, including the joined reading extent.
    /// An overflowing axis retains ownership at its ends; it must never turn a
    /// held pan key into a file/page change merely because its offset is clamped.
    pub(super) fn image_arrow_pan(&self, stroke: &KeyStroke) -> Option<(egui::Vec2, egui::Vec2)> {
        if self.media_kind != Some(MediaKind::Image)
            || stroke.modifiers != Modifiers::default()
            || !self.entered_shortcut.is_empty()
            || self.native_ime_composing
            || self.image_loading
            || self.image_edit_pending
            || self.image_error.is_some()
            || self.image_handoff.is_some()
            || self.displayed_tab.is_none()
            || self.displayed_tab != self.tabs.active_id()
            || self.view_drag.is_some()
            || self.rotation_drag.is_some()
            || self.image_viewport.min_elem() <= 0.0
        {
            return None;
        }
        let direction = match stroke.key {
            Key::ArrowLeft => egui::vec2(1.0, 0.0),
            Key::ArrowRight => egui::vec2(-1.0, 0.0),
            Key::ArrowUp => egui::vec2(0.0, 1.0),
            Key::ArrowDown => egui::vec2(0.0, -1.0),
            _ => return None,
        };
        // Preserve non-navigation custom bindings and prefixes on an arrow key.
        if !matches!(
            self.shortcuts
                .resolve(std::slice::from_ref(stroke), self.command_context()),
            ShortcutMatch::None
                | ShortcutMatch::Command(
                    CommandId::PreviousImage
                        | CommandId::NextImage
                        | CommandId::ReadingLeft
                        | CommandId::ReadingRight
                        | CommandId::PreviousMedia
                        | CommandId::NextMedia
                        | CommandId::PreviousSameKind
                        | CommandId::NextSameKind
                )
        ) {
            return None;
        }
        let context = self.ui_context.as_ref()?;
        if !self.view_input_allowed(context)
            || context.egui_wants_keyboard_input()
            || !context.input(|input| input.focused)
        {
            return None;
        }
        let density = context.pixels_per_point();
        let displayed = if self.reading_mode {
            self.reading_displayed_extent(density)?
        } else {
            let transform = self.visual_transform(self.image.as_ref()?.dimensions());
            let size = (transform.size.0 as u32, transform.size.1 as u32);
            let scale = self
                .image_view
                .logical_scale(size, self.image_viewport.into(), density);
            egui::vec2(transform.size.0, transform.size.1) * scale
        };
        let axis = usize::from(direction.y != 0.0);
        (displayed[axis] > self.image_viewport[axis]).then_some((displayed, direction * 40.0))
    }

    pub(super) fn pan_image_arrow(&mut self, stroke: &KeyStroke) -> bool {
        let Some((displayed, delta)) = self.image_arrow_pan(stroke) else {
            return false;
        };
        let previous = self.image_view.pan;
        self.image_view.pan = (egui::Vec2::from(previous) + delta).into();
        clamp(&mut self.image_view, displayed, self.image_viewport);
        if self.image_view.pan != previous {
            self.qualify_current_history();
        }
        self.request_redraw();
        true
    }
}

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
    let overflow = (displayed - viewport.size()).max(egui::Vec2::ZERO);
    let mut pan = egui::Vec2::from(view.pan);
    let (pressed, released) = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_released(),
        )
    });
    let mut accepted = false;
    for axis in 0..2_usize {
        if overflow[axis] <= 0.0 {
            continue;
        }
        // Separate axis viewports shorten both painting and hit regions. A
        // paint-only scroll_bar_rect would leave an invisible corner target.
        let mut track = bars;
        track.max[axis] -= (ui.spacing().scroll.bar_width + 3.0).min(track.size()[axis] * 0.25);
        let ratio = track.size()[axis] / viewport.size()[axis];
        let mut offset = egui::Vec2::ZERO;
        offset[axis] = (overflow[axis] * 0.5 - pan[axis]) * ratio;
        let mut content = track.size();
        content[axis] = displayed[axis] * ratio;
        let output = ui
            .scope_builder(egui::UiBuilder::new().max_rect(track), |ui| {
                if !enabled {
                    ui.disable();
                }
                ui.style_mut().animation_time = 0.0;
                let style = &mut ui.style_mut().spacing.scroll;
                style.floating = true;
                style.floating_allocated_width = 0.0;
                style.handle_min_length = style.handle_min_length.min(track.size()[axis]);
                egui::ScrollArea::new([axis == 0, axis == 1])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .id_salt(("image-scroll", axis))
                    .auto_shrink([false, false])
                    .content_margin(0)
                    .animated(false)
                    .scroll_offset(offset)
                    .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR)
                    .show_styled(ui, |ui| ui.set_min_size(content))
            })
            .inner;
        // Layout rounds virtual content. Ignore that idle clamp and map actual
        // bar movement through its measured range so both endpoints stay exact.
        let maximum = (output.content_size[axis] - output.inner_rect.size()[axis]).max(0.0);
        if maximum > 0.0 && output.state.offset[axis] != offset[axis].clamp(0.0, maximum) {
            pan[axis] = overflow[axis] * (0.5 - output.state.offset[axis] / maximum);
        }
        // A stationary thumb press still owns input; rounded clamps do not.
        if enabled && (pressed || released) {
            accepted |= ui
                .ctx()
                .read_response(output.id.with(axis))
                .is_some_and(|response| {
                    let owned = response.enabled()
                        && ((pressed && response.is_pointer_button_down_on())
                            || (released
                                && (response.clicked_by(egui::PointerButton::Primary)
                                    || response.drag_stopped_by(egui::PointerButton::Primary))));
                    if owned {
                        response.surrender_focus();
                    }
                    owned
                });
        }
    }
    view.pan = pan.into();
    clamp(view, displayed, viewport.size());
    accepted
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
    bars(ui, viewport, displayed, &mut view, false);
    ui.set_style(original);
}
