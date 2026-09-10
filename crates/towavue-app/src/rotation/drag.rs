use super::*;

pub(crate) struct RotationDrag {
    preview: RotationDialog,
    origin: egui::Pos2,
    rect: egui::Rect,
    tenths: i16,
}

pub(crate) enum RotationResponse {
    Inactive,
    Preview,
    Cancelled,
}

fn modifiers_allow_rotation(modifiers: egui::Modifiers) -> bool {
    modifiers.alt && !modifiers.ctrl && !modifiers.command && !modifiers.mac_cmd
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(crate) fn cancel_stale_rotation_drag(&mut self) {
        if self
            .rotation_drag
            .as_ref()
            .is_some_and(|drag| !self.rotation_is_current(&drag.preview))
        {
            self.cancel_view_drag();
        }
    }
    pub(crate) fn draw_rotation_drag(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        image_rect: egui::Rect,
    ) -> RotationResponse {
        let (origin, release) = view_drag_button_positions(response, egui::PointerButton::Primary);
        let (alt_press, held, released_with_alt, interrupted) = ui.input(|input| {
            let alt_press = input.events.iter().any(|event| matches!(event,
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers }
                if Some(*pos) == origin && modifiers_allow_rotation(*modifiers)));
            let released_with_alt = input.events.iter().any(|event| matches!(event,
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers }
                if Some(*pos) == release && modifiers_allow_rotation(*modifiers)));
            let interrupted = !input.focused || input.events.iter().any(|event| matches!(event,
                egui::Event::PointerGone | egui::Event::WindowFocused(false)
                | egui::Event::Key { key: egui::Key::Escape, pressed: true, .. }
                | egui::Event::MouseWheel { .. }
                | egui::Event::PointerButton { button: egui::PointerButton::Secondary, pressed: true, .. }));
            (alt_press, modifiers_allow_rotation(input.modifiers), released_with_alt, interrupted)
        });
        if self.rotation_drag.is_none() && !alt_press {
            return RotationResponse::Inactive;
        }
        let was_active = self.rotation_drag.is_some();
        if interrupted
            || ui.ctx().dragged_id().is_some_and(|id| id != response.id)
            || !(held || released_with_alt)
            || !self.view_drag_allowed(ui.ctx())
        {
            if was_active {
                self.cancel_view_drag();
            }
            return RotationResponse::Cancelled;
        }
        if self.rotation_drag.is_none() {
            let Some(origin) = origin.filter(|pos| image_rect.contains(*pos)) else {
                return RotationResponse::Cancelled;
            };
            if self.view_drag.is_some() {
                return RotationResponse::Inactive;
            }
            let Some(preview) = self.capture_rotation() else {
                return RotationResponse::Cancelled;
            };
            let size = (
                preview.transform.size.0 as u32,
                preview.transform.size.1 as u32,
            );
            let pixels_per_point = ui.ctx().pixels_per_point();
            let viewport = ui.max_rect();
            let scale = self
                .image_view
                .scale(size, (viewport.size() * pixels_per_point).into())
                / pixels_per_point;
            let rect = egui::Rect::from_center_size(
                viewport.center() + egui::vec2(self.image_view.pan.0, self.image_view.pan.1),
                egui::vec2(preview.transform.size.0, preview.transform.size.1) * scale,
            );
            self.rotation_drag = Some(RotationDrag {
                preview,
                origin,
                rect,
                tenths: 0,
            });
        }
        let drag = self.rotation_drag.as_mut().expect("rotation drag");
        if let Some(pointer) = release.or_else(|| ui.input(|input| input.pointer.hover_pos())) {
            drag.tenths = ((pointer.x - drag.origin.x) * 5.0)
                .round()
                .clamp(-1800.0, 1800.0) as i16;
        }
        let value = ImageRotation::new(
            drag.tenths,
            (
                drag.preview.transform.size.0 as u32,
                drag.preview.transform.size.1 as u32,
            ),
        );
        let painter = ui.painter_at(ui.max_rect());
        let mesh = rotated_mesh(
            drag.preview.texture.id(),
            drag.rect,
            drag.preview.transform,
            drag.tenths,
        );
        paint_checkerboard(&painter, mesh.calc_bounds());
        painter.add(mesh);
        response
            .clone()
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        ui.put(
            egui::Rect::from_min_size(
                ui.max_rect().min + egui::vec2(8.0, 8.0),
                egui::vec2((ui.max_rect().width() - 16.0).max(1.0), 48.0),
            ),
            egui::Label::new(format!(
                "Rotation: {:.1} degrees · {}",
                f32::from(drag.tenths) / 10.0,
                if value.is_some() {
                    "Release mouse to apply; release Alt or press Escape to cancel"
                } else {
                    "Canvas exceeds image limits; release cancels"
                }
            ))
            .wrap(),
        );
        if release.is_some() {
            let drag = self.rotation_drag.take().expect("released rotation");
            if released_with_alt {
                if value.is_none() {
                    self.set_status(
                        "Rotation cancelled because the canvas exceeds image limits".into(),
                    );
                }
                self.commit_rotation(drag.preview, value);
            } else {
                self.request_redraw();
            }
        }
        RotationResponse::Preview
    }

    pub(crate) fn rotation_modifiers_changed(&mut self) {
        if self.rotation_drag.is_none()
            || (self.modifiers.alt_key()
                && !self.modifiers.control_key()
                && !self.modifiers.super_key())
        {
            return;
        }
        // A release already queued with Alt ends the gesture before this modifier change.
        let released = self.ui_state.as_mut().is_some_and(|state| state.egui_input_mut().events.iter().any(|event| matches!(event,
            egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, modifiers, .. }
            if modifiers_allow_rotation(*modifiers))));
        if !released {
            self.cancel_view_drag();
        }
    }
}

#[cfg(test)]
mod tests;
