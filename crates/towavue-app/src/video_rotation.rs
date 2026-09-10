use crate::*;
use towavue_core::VideoRotation;
use towavue_runtime_windows::{VideoOrientation, video_edit_geometry};

pub(super) struct VideoRotationDialog {
    token: u64,
    tab: TabId,
    path: PathBuf,
    media_generation: u64,
    generation: PlaybackGeneration,
    source: (u32, u32, f32),
    orientation: VideoOrientation,
    max_side: usize,
    operations: Vec<EditOperation>,
    geometry: (u32, u32, f32),
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
            (self.geometry.0, self.geometry.1),
            self.geometry.2,
        )
        .ok_or("The rotated canvas exceeds the image size limit")?;
        if value.tenths() != 0 {
            let mut edits = self.operations.clone();
            edits.push(EditOperation::RotateVideo(value));
            video_edit_geometry(
                (self.source.0, self.source.1),
                self.source.2,
                self.orientation,
                &edits,
                self.max_side,
            )
            .map_err(|error| error.to_string())?;
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
    operations.iter().any(
        |operation| matches!(operation, EditOperation::RotateVideo(value) if value.tenths() != 0),
    )
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    fn video_operations(&self) -> &[EditOperation] {
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
            .ok_or("Wait for video to load before rotating")?;
        let (width, height, aspect) = session
            .video_geometry()
            .ok_or("Wait for a video frame before rotating")?;
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
        let geometry = match self.validate_video_operations(self.video_operations()) {
            Ok(geometry) => geometry,
            Err(error) => {
                self.set_status(error);
                return;
            }
        };
        let (Some(tab), Some(path), Some(session), Some(renderer)) = (
            self.tabs.active(),
            self.path.as_ref(),
            self.session.as_ref(),
            self.renderer.as_ref(),
        ) else {
            return;
        };
        self.rotation_generation = self.rotation_generation.wrapping_add(1);
        self.video_rotation_dialog = Some(VideoRotationDialog {
            token: self.rotation_generation,
            tab: tab.id,
            path: path.clone(),
            media_generation: self.media_generation,
            generation: self.generation,
            source: session.video_geometry().expect("validated frame"),
            orientation: session.video_orientation().unwrap_or_default(),
            max_side: renderer.max_texture_side(),
            operations: self.video_operations().to_vec(),
            geometry,
            angle: "0.0".into(),
            first_frame: true,
        });
        self.guard_return_focus = self
            .ui_context
            .as_ref()
            .and_then(|context| context.memory(egui::Memory::focused))
            .map(|focus| (tab.id, focus));
        self.request_redraw();
    }

    fn video_rotation_is_current(&self, dialog: &VideoRotationDialog) -> bool {
        self.media_kind == Some(MediaKind::Video)
            && self.timeline_open
            && !self.fullscreen
            && self.tabs.active().is_some_and(|tab| tab.id == dialog.tab)
            && self.path.as_ref() == Some(&dialog.path)
            && self.media_generation == dialog.media_generation
            && self.generation == dialog.generation
            && self.video_operations() == dialog.operations
            && self
                .session
                .as_ref()
                .and_then(PlaybackSession::video_geometry)
                == Some(dialog.source)
            && self
                .session
                .as_ref()
                .and_then(PlaybackSession::video_orientation)
                == Some(dialog.orientation)
            && self
                .renderer
                .as_ref()
                .is_some_and(|renderer| renderer.max_texture_side() == dialog.max_side)
    }

    pub(super) fn cancel_stale_video_rotation(&mut self) {
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
        if let Some(value) = value {
            if self.video_rotation_is_current(&dialog)
                && value.source_size() == (dialog.geometry.0, dialog.geometry.1)
                && value.source_pixel_aspect() == dialog.geometry.2
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
        if let Some(dialog) = &self.video_rotation_dialog
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
