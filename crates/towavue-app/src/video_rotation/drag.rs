use super::*;
use crate::rotation::RotationResponse;

pub(crate) struct VideoRotationDrag {
    pub(super) preview: VideoRotationDialog,
    origin: egui::Pos2,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(crate) fn update_video_rotation_drag(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        video_rect: egui::Rect,
    ) -> RotationResponse {
        let input = rotation::rotation_input(ui, response);
        if self.video_rotation_drag.is_none()
            && (!input.alt_press
                || !self.visual_selection_enabled()
                || self.media_kind != Some(MediaKind::Video))
        {
            return RotationResponse::Inactive;
        }
        if input.interrupted
            || !(input.held || input.released_with_alt)
            || ui.ctx().dragged_id().is_some_and(|id| id != response.id)
            || !self.view_drag_allowed(ui.ctx())
            || self
                .video_rotation_drag
                .as_ref()
                .is_some_and(|drag| !self.video_rotation_is_current(&drag.preview))
        {
            if self.video_rotation_drag.is_some() {
                self.cancel_view_drag();
            }
            return RotationResponse::Cancelled;
        }
        if self.video_rotation_drag.is_none() {
            let Some(origin) = input
                .origin
                .filter(|position| video_rect.contains(*position))
            else {
                return RotationResponse::Cancelled;
            };
            if self.view_drag.is_some() {
                return RotationResponse::Inactive;
            }
            let preview = match self.capture_video_rotation() {
                Ok(preview) => preview,
                Err(error) => {
                    self.set_status(error);
                    return RotationResponse::Cancelled;
                }
            };
            self.video_rotation_drag = Some(VideoRotationDrag { preview, origin });
        }
        let drag = self
            .video_rotation_drag
            .as_mut()
            .expect("video rotation drag");
        if let Some(pointer) = input
            .release
            .or_else(|| ui.input(|input| input.pointer.hover_pos()))
        {
            let tenths = ((pointer.x - drag.origin.x) * 5.0)
                .round()
                .clamp(-1800.0, 1800.0) as i16;
            drag.preview.angle = format!("{:.1}", f32::from(tenths) / 10.0);
        }
        let value = drag.preview.value();
        response
            .clone()
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        ui.put(
            egui::Rect::from_min_size(ui.max_rect().min + egui::vec2(8.0, 8.0), egui::vec2((ui.max_rect().width() - 16.0).max(1.0), 48.0)),
            egui::Label::new(match &value {
                Ok(value) => format!("Rotation: {:.1} degrees · Release mouse to apply; release Alt or press Escape to cancel", f32::from(value.tenths()) / 10.0),
                Err(error) => format!("{error} · Preview unchanged; release cancels"),
            }).wrap(),
        );
        if input.release.is_some() {
            let drag = self
                .video_rotation_drag
                .take()
                .expect("released video rotation");
            if input.released_with_alt {
                if let Err(error) = &value {
                    self.set_status(format!("Rotation cancelled: {error}"));
                }
                self.commit_video_rotation(drag.preview, value.ok());
            } else {
                self.request_redraw();
            }
        }
        RotationResponse::Preview
    }
}
