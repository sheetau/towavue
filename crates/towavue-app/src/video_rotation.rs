use crate::localization::{Language, Text, language};
use crate::*;
use towavue_core::VideoRotation;
use towavue_core::localization::formatted;
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
    #[cfg(test)]
    fn value(&self) -> Result<VideoRotation, String> {
        self.value_in(Language::English)
    }

    fn value_in(&self, display_language: Language) -> Result<VideoRotation, String> {
        let angle = self
            .angle
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|angle| angle.is_finite() && (-180.0..=180.0).contains(angle))
            .ok_or(Text::FiniteRotationAngle.in_language(display_language))?;
        let value = VideoRotation::new(
            (angle * 10.0).round() as i16,
            (self.snapshot.geometry.0, self.snapshot.geometry.1),
            self.snapshot.geometry.2,
        )
        .ok_or(Text::RotatedCanvasLimit.in_language(display_language))?;
        if value.tenths() != 0 {
            self.snapshot
                .validate(EditOperation::RotateVideo(value))
                .map_err(|error| error.message(display_language))?;
        }
        Ok(value)
    }

    fn show(&mut self, context: &egui::Context) -> Option<Option<VideoRotation>> {
        let display_language = language(context);
        let previous_angle = self.angle.clone();
        let mut action = None;
        let id = egui::Id::new("free-rotate-video");
        let modal = chrome::modal(context, id, true).show(context, |ui| {
            let value = chrome::modal_body(
                ui,
                Text::CommandFreeRotateVideo.in_language(display_language),
                &[
                    Text::ApplyRotation.in_language(display_language),
                    Text::Cancel.in_language(display_language),
                ],
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().text_edit_width = 96.0;
                        let response = resize::text_input(
                            ui,
                            Text::VideoRotationAngleInput.in_language(display_language),
                            &mut self.angle,
                            "°",
                        )
                        .help_text(Text::RotationAngleHelp.in_language(display_language));
                        if self.first_frame {
                            response.request_focus();
                            self.first_frame = false;
                        }
                        if let Ok(value) = self.value_in(display_language) {
                            let size = if value.tenths() == 0 {
                                self.snapshot.geometry
                            } else {
                                self.snapshot
                                    .geometry_with(EditOperation::RotateVideo(value))
                                    .expect("validated rotation")
                            };
                            ui.label(formatted::pixel_size(display_language, size.0, size.1));
                        }
                    });
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
                                .text(Text::VideoRotationAngle.in_language(display_language)),
                        )
                        .changed()
                    {
                        self.angle = format!("{degrees:.1}");
                    }
                    let value = self.value_in(display_language);
                    if let Err(error) = &value {
                        ui.label(error);
                    }
                    value
                },
            );
            ui.horizontal_wrapped(|ui| {
                crate::chrome::flat_buttons(ui);
                if ui
                    .add_enabled(
                        value.is_ok(),
                        egui::Button::new(Text::ApplyRotation.in_language(display_language)),
                    )
                    .clicked()
                {
                    action = Some(value.ok());
                }
                if ui
                    .button(Text::Cancel.in_language(display_language))
                    .clicked()
                {
                    action = Some(None);
                }
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
            .ok_or(Text::WaitVideoEdit.in_language(self.language()))?;
        let (width, height, aspect) = session
            .video_geometry()
            .ok_or(Text::WaitVideoFrameEdit.in_language(self.language()))?;
        let max_side = self
            .renderer
            .as_ref()
            .ok_or(Text::VideoRendererUnavailable.in_language(self.language()))?
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

    pub(super) fn step_video_rotation(&mut self, clockwise: bool) {
        let mut dialog = match self.capture_video_rotation() {
            Ok(dialog) => dialog,
            Err(error) => {
                self.set_status(error);
                return;
            }
        };
        dialog.angle = if clockwise { "5.0" } else { "-5.0" }.into();
        match dialog.value_in(self.language()) {
            Ok(value) => self.commit_video_rotation(dialog, Some(value)),
            Err(error) => self.set_status(error),
        }
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
            self.set_status(
                Text::RotationVideoChanged
                    .in_language(self.language())
                    .into(),
            );
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
                self.set_status(
                    Text::RotationVideoChanged
                        .in_language(self.language())
                        .into(),
                );
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
            && let Ok(value) = dialog.value_in(self.language())
            && value.tenths() != 0
        {
            operations.push(EditOperation::RotateVideo(value));
            operations = towavue_core::compose_rotations(&operations);
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
