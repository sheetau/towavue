use crate::*;
use towavue_core::VideoRotation;
#[cfg(test)]
use towavue_runtime_windows::VideoOrientation;
use towavue_runtime_windows::video_edit_geometry;

mod drag;
pub(super) use drag::VideoRotationDrag;

pub(super) struct VideoRotationDialog {
    token: u64,
    snapshot: video_edit::VideoEditSnapshot,
    angle: String,
    first_frame: bool,
}

impl VideoRotationDialog {
    fn value(&self) -> Result<VideoRotation, String> {
        let angle = self
            .angle
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|angle| angle.is_finite() && (-180.0..=180.0).contains(angle))
            .ok_or("Use a finite angle from -180 to 180 degrees")?;
        let value = VideoRotation::new(
            (angle * 10.0).round() as i16,
            (self.snapshot.geometry.0, self.snapshot.geometry.1),
            self.snapshot.geometry.2,
        )
        .ok_or("The rotated canvas exceeds the image size limit")?;
        if value.tenths() != 0 {
            self.snapshot.validate(EditOperation::RotateVideo(value))?;
        }
        Ok(value)
    }

    fn show(&mut self, context: &egui::Context) -> Option<Option<VideoRotation>> {
        let previous_angle = self.angle.clone();
        let mut action = None;
        let id = egui::Id::new("free-rotate-video");
        let modal = egui::Modal::new(id)
            .area(
                egui::Modal::default_area(id)
                    .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0)),
            )
            .backdrop_color(Color32::TRANSPARENT)
            .show(context, |ui| {
                ui.set_width((context.content_rect().width() - 48.0).clamp(1.0, 340.0));
                egui::ScrollArea::vertical()
                    .max_height((context.content_rect().height() - 48.0).max(1.0))
                    .show(ui, |ui| {
                        chrome::modal_heading(ui, "Free rotate video");
                        ui.label("Preview on the video. Apply adds one undoable edit.");
                        ui.label("Angle in degrees (clockwise, 0.1 degree steps)");
                        let response = resize::text_input(
                            ui,
                            "Video rotation angle in degrees",
                            &mut self.angle,
                        );
                        if self.first_frame {
                            response.request_focus();
                            self.first_frame = false;
                        }
                        let mut degrees = self
                            .angle
                            .trim()
                            .parse::<f64>()
                            .ok()
                            .filter(|value| value.is_finite())
                            .map(|value| (value.clamp(-180.0, 180.0) * 10.0).round() / 10.0)
                            .unwrap_or(0.0);
                        if ui
                            .add(
                                egui::Slider::new(&mut degrees, -180.0..=180.0)
                                    .step_by(0.1)
                                    .show_value(false)
                                    .text("Video rotation angle"),
                            )
                            .changed()
                        {
                            self.angle = format!("{degrees:.1}");
                        }
                        let value = self.value();
                        match &value {
                            Ok(value) if value.tenths() == 0 => {
                                ui.label("0 degrees — no edit or pixel-aspect change");
                            }
                            Ok(value) => {
                                ui.label(format!(
                                    "{:.1} degrees — {} x {} square pixels",
                                    f64::from(value.tenths()) / 10.0,
                                    value.size().0,
                                    value.size().1
                                ));
                            }
                            Err(error) => {
                                ui.label(error);
                            }
                        }
                        ui.label("Black canvas; resampled on the GPU. Export encoding may differ.");
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(value.is_ok(), egui::Button::new("Apply rotation"))
                                .clicked()
                            {
                                action = Some(value.ok());
                            }
                            if ui.button("Cancel").clicked() {
                                action = Some(None);
                            }
                        });
                    });
            });
        if modal.is_top_modal
            && !modal.any_popup_open
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            action = Some(None);
        }
        if self.angle != previous_angle {
            // The media rect and raster plan were captured before this modal pass.
            context.request_repaint();
        }
        action
    }
}

pub(super) fn uses_raster(operations: &[EditOperation]) -> bool {
    operations.iter().any(|operation| match operation {
        EditOperation::RotateVideo(value) => value.tenths() != 0,
        EditOperation::ResizeVideo(value) => !value.is_identity(),
        _ => false,
    })
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn video_operations(&self) -> &[EditOperation] {
        self.tabs
            .active()
            .and_then(|tab| self.edits.get(&tab.id))
            .map_or(&[], EditHistory::operations)
    }

    pub(super) fn validate_video_operations(
        &self,
        operations: &[EditOperation],
    ) -> Result<(u32, u32, f32), String> {
        let session = self
            .session
            .as_ref()
            .ok_or("Wait for video to load before editing")?;
        let (width, height, aspect) = session
            .video_geometry()
            .ok_or("Wait for a video frame before editing")?;
        let max_side = self
            .renderer
            .as_ref()
            .ok_or("Video renderer is unavailable")?
            .max_texture_side();
        video_edit_geometry(
            (width, height),
            aspect,
            session.video_orientation().unwrap_or_default(),
            operations,
            max_side,
        )
        .map_err(|error| error.to_string())
    }

    pub(super) fn open_video_rotation(&mut self) {
        if !self.visual_selection_enabled() || self.media_kind != Some(MediaKind::Video) {
            return;
        }
        let dialog = match self.capture_video_rotation() {
            Ok(dialog) => dialog,
            Err(error) => {
                self.set_status(error);
                return;
            }
        };
        self.guard_return_focus = self
            .ui_context
            .as_ref()
            .and_then(|context| context.memory(egui::Memory::focused))
            .map(|focus| (dialog.snapshot.tab, focus));
        self.video_rotation_dialog = Some(dialog);
        self.request_redraw();
    }

    fn capture_video_rotation(&mut self) -> Result<VideoRotationDialog, String> {
        let snapshot = self.capture_video_edit()?;
        self.rotation_generation = self.rotation_generation.wrapping_add(1);
        Ok(VideoRotationDialog {
            token: self.rotation_generation,
            snapshot,
            angle: "0.0".into(),
            first_frame: true,
        })
    }

    fn video_rotation_is_current(&self, dialog: &VideoRotationDialog) -> bool {
        self.video_edit_is_current(&dialog.snapshot)
    }

    pub(super) fn cancel_stale_video_rotation(&mut self) {
        if self
            .video_rotation_drag
            .as_ref()
            .is_some_and(|drag| !self.video_rotation_is_current(&drag.preview))
        {
            self.cancel_view_drag();
        }
        if self
            .video_rotation_dialog
            .as_ref()
            .is_some_and(|dialog| !self.video_rotation_is_current(dialog))
        {
            self.video_rotation_dialog = None;
            self.set_status("Rotation cancelled because the video changed".into());
        }
    }

    pub(super) fn show_video_rotation(
        &mut self,
        context: &egui::Context,
        actions: &mut Vec<UiAction>,
    ) {
        if let Some(dialog) = &mut self.video_rotation_dialog
            && let Some(action) = dialog.show(context)
        {
            actions.push(UiAction::FinishVideoRotation(dialog.token, action));
        }
    }

    pub(super) fn finish_video_rotation(&mut self, token: u64, value: Option<VideoRotation>) {
        if self
            .video_rotation_dialog
            .as_ref()
            .is_none_or(|dialog| dialog.token != token)
        {
            return;
        }
        let dialog = self.video_rotation_dialog.take().expect("matching dialog");
        self.commit_video_rotation(dialog, value);
    }

    fn commit_video_rotation(&mut self, dialog: VideoRotationDialog, value: Option<VideoRotation>) {
        if let Some(value) = value {
            if self.video_rotation_is_current(&dialog)
                && value.source_size() == (dialog.snapshot.geometry.0, dialog.snapshot.geometry.1)
                && value.source_pixel_aspect() == dialog.snapshot.geometry.2
            {
                self.push_visual_edit(EditOperation::RotateVideo(value));
            } else {
                self.set_status("Rotation cancelled because the video changed".into());
            }
        }
        self.request_redraw();
    }

    pub(super) fn video_presentation(
        &self,
        size: (u32, u32),
    ) -> (ImageTransform, Option<Vec<EditOperation>>) {
        let mut operations = self.video_operations().to_vec();
        if let Some(value) = self.video_resize_preview() {
            operations.push(EditOperation::ResizeVideo(value));
        }
        if let Some(dialog) = self
            .video_rotation_dialog
            .as_ref()
            .or_else(|| self.video_rotation_drag.as_ref().map(|drag| &drag.preview))
            && self.video_rotation_is_current(dialog)
            && let Ok(value) = dialog.value()
            && value.tenths() != 0
        {
            operations.push(EditOperation::RotateVideo(value));
        }
        let transform = ImageTransform::with_orientation(
            size,
            self.session
                .as_ref()
                .and_then(PlaybackSession::video_orientation)
                .unwrap_or_default(),
            &operations,
        );
        let raster = uses_raster(&operations).then_some(operations);
        (transform, raster)
    }
}

#[cfg(test)]
pub(crate) mod tests;
