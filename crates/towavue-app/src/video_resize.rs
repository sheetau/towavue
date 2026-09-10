use crate::*;
use towavue_core::VideoResize;

pub(super) struct VideoResizeDialog {
    token: u64,
    snapshot: video_edit::VideoEditSnapshot,
    inputs: resize::ResizeDialog,
}

impl VideoResizeDialog {
    fn value(&self) -> Result<VideoResize, String> {
        let dimensions = self
            .inputs
            .value()
            .ok_or("Use even dimensions from 16 to 16384 pixels, up to 128 Mi pixels")?;
        let geometry = self.snapshot.geometry;
        let value = VideoResize::new(
            dimensions.size(),
            dimensions.filter,
            (geometry.0, geometry.1),
            geometry.2,
        )
        .ok_or("Use even dimensions from 16 to 16384 pixels")?;
        self.snapshot.validate(EditOperation::ResizeVideo(value))?;
        Ok(value)
    }

    fn show(&mut self, context: &egui::Context) -> Option<Option<VideoResize>> {
        let mut action = None;
        let id = egui::Id::new("resize-video");
        let modal = egui::Modal::new(id)
            .area(egui::Modal::default_area(id).anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0)))
            .backdrop_color(Color32::TRANSPARENT)
            .show(context, |ui| {
                ui.set_width((context.content_rect().width() - 48.0).clamp(1.0, 360.0));
                egui::ScrollArea::vertical()
                    .max_height((context.content_rect().height() - 48.0).max(1.0))
                    .show(ui, |ui| {
                        chrome::modal_heading(ui, "Resize / resample video");
                        ui.label("Preview on the video. Apply adds one undoable edit.");
                        self.inputs.controls(ui);
                        let value = self.value();
                        match &value {
                            Ok(value) => {
                                ui.label(format!("{} x {} square pixels — ratio {:.4}:1{}",
                                    value.size().0, value.size().1,
                                    f64::from(value.size().0) / f64::from(value.size().1),
                                    if value.is_identity() { " — no edit" } else { "" }));
                            }
                            Err(error) => { ui.label(error); }
                        }
                        ui.label("Even dimensions; linked edge rounds to 2 pixels. Export encoding may differ.");
                        ui.horizontal(|ui| {
                            if ui.add_enabled(value.is_ok(), egui::Button::new("Apply resize")).clicked() {
                                action = Some(value.ok());
                            }
                            if ui.button("Cancel").clicked() { action = Some(None); }
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
        action
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn open_video_resize(&mut self) {
        if !self.visual_selection_enabled() || self.media_kind != Some(MediaKind::Video) {
            return;
        }
        let snapshot = match self.capture_video_edit() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.set_status(error);
                return;
            }
        };
        self.guard_return_focus = self
            .ui_context
            .as_ref()
            .and_then(|context| context.memory(egui::Memory::focused))
            .map(|focus| (snapshot.tab, focus));
        let inputs = resize::ResizeDialog::for_video(
            (snapshot.geometry.0, snapshot.geometry.1),
            snapshot.geometry.2,
        );
        self.rotation_generation = self.rotation_generation.wrapping_add(1);
        if let Some(context) = &self.ui_context {
            egui::Popup::close_all(context);
        }
        self.video_resize_dialog = Some(VideoResizeDialog {
            token: self.rotation_generation,
            snapshot,
            inputs,
        });
        self.request_redraw();
    }

    pub(super) fn cancel_stale_video_resize(&mut self) {
        if self
            .video_resize_dialog
            .as_ref()
            .is_some_and(|dialog| !self.video_edit_is_current(&dialog.snapshot))
        {
            self.video_resize_dialog = None;
            self.set_status("Resize cancelled because the video changed".into());
        }
    }

    pub(super) fn show_video_resize(
        &mut self,
        context: &egui::Context,
        actions: &mut Vec<UiAction>,
    ) {
        if let Some(dialog) = &mut self.video_resize_dialog
            && let Some(action) = dialog.show(context)
        {
            actions.push(UiAction::FinishVideoResize(dialog.token, action));
        }
    }

    pub(super) fn finish_video_resize(&mut self, token: u64, value: Option<VideoResize>) {
        if self
            .video_resize_dialog
            .as_ref()
            .is_none_or(|dialog| dialog.token != token)
        {
            return;
        }
        let dialog = self
            .video_resize_dialog
            .take()
            .expect("matching resize dialog");
        if let Some(value) = value {
            if self.video_edit_is_current(&dialog.snapshot)
                && value.source_size() == (dialog.snapshot.geometry.0, dialog.snapshot.geometry.1)
                && value.source_pixel_aspect() == dialog.snapshot.geometry.2
            {
                self.push_visual_edit(EditOperation::ResizeVideo(value));
            } else {
                self.set_status("Resize cancelled because the video changed".into());
            }
        }
        self.request_redraw();
    }

    pub(super) fn video_resize_preview(&self) -> Option<VideoResize> {
        let dialog = self.video_resize_dialog.as_ref()?;
        self.video_edit_is_current(&dialog.snapshot)
            .then(|| dialog.value().ok())
            .flatten()
            .filter(|value| !value.is_identity())
    }
}

#[cfg(test)]
pub(crate) mod tests;
