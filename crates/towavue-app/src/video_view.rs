use crate::*;

#[cfg(test)]
mod tests;

fn scale_viewport(viewport: egui::Vec2, pixel_aspect: f32, density: f32) -> (f32, f32) {
    (viewport.x * density / pixel_aspect, viewport.y * density)
}

pub(super) fn rect(
    viewport: egui::Rect,
    size: (u32, u32),
    pixel_aspect: f32,
    density: f32,
    view: ImageViewState,
) -> egui::Rect {
    let scale = view.scale(size, scale_viewport(viewport.size(), pixel_aspect, density)) / density;
    egui::Rect::from_center_size(
        viewport.center() + egui::vec2(view.pan.0, view.pan.1),
        egui::vec2(size.0 as f32 * pixel_aspect, size.1 as f32) * scale,
    )
}

pub(super) fn clipped(
    viewport: egui::Rect,
    full: egui::Rect,
    uv: [UnitPoint; 4],
) -> Option<(egui::Rect, [UnitPoint; 4])> {
    let visible = viewport.intersect(full);
    if !visible.is_positive() || !full.is_positive() {
        return None;
    }
    let min = (visible.min - full.min) / full.size();
    let max = (visible.max - full.min) / full.size();
    Some((
        visible,
        [
            bilinear_uv(uv, min.x, min.y),
            bilinear_uv(uv, max.x, min.y),
            bilinear_uv(uv, max.x, max.y),
            bilinear_uv(uv, min.x, max.y),
        ],
    ))
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn zoom_video(&mut self, factor: f32) -> f32 {
        let Some((width, height, aspect)) = self
            .session
            .as_ref()
            .and_then(PlaybackSession::video_geometry)
        else {
            return 1.0;
        };
        let Some(context) = &self.ui_context else {
            return 1.0;
        };
        if self.image_viewport.min_elem() <= 0.0 {
            return 1.0;
        }
        let transform = self.visual_transform((width, height));
        let size = (transform.size.0 as u32, transform.size.1 as u32);
        let viewport = scale_viewport(
            self.image_viewport,
            transform.pixel_aspect(aspect),
            context.pixels_per_point(),
        );
        let before = self.image_view.scale(size, viewport);
        self.image_view.zoom_by(factor, size, viewport);
        self.request_redraw();
        self.image_view.scale(size, viewport) / before
    }

    pub(super) fn update_video_view(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        full: egui::Rect,
    ) {
        if !self.visual_selection_enabled() || !self.view_drag_allowed(ui.ctx()) {
            return;
        }
        let (zoom, pointer, allowed) = ui.input(|input| {
            (
                input.zoom_delta(),
                input.pointer.hover_pos(),
                input.focused
                    && input.modifiers.ctrl
                    && !input.modifiers.alt
                    && !input.modifiers.mac_cmd
                    && !input.pointer.any_down(),
            )
        });
        if zoom != 1.0 && response.hovered() && allowed && self.view_drag.is_none() {
            let ratio = self.zoom_video(zoom);
            if let Some(pointer) = pointer {
                let correction = (pointer - full.center()) * (1.0 - ratio);
                self.image_view.pan.0 += correction.x;
                self.image_view.pan.1 += correction.y;
            }
        }
        self.update_pan(response, pointer);
    }
}
