use crate::*;
use towavue_core::ImageRotation;

mod drag;
pub(super) use drag::{RotationDrag, RotationResponse};

pub(super) struct RotationDialog {
    pub(super) token: u64,
    tab: TabId,
    media_generation: u64,
    edit_generation: u64,
    path: PathBuf,
    source: Arc<DecodedImage>,
    operations: Vec<EditOperation>,
    texture: TextureHandle,
    transform: ImageTransform,
    angle: String,
    first_frame: bool,
}

impl RotationDialog {
    fn value(&self) -> Option<ImageRotation> {
        let degrees = self.angle.trim().parse::<f64>().ok()?;
        if !degrees.is_finite() || !(-180.0..=180.0).contains(&degrees) {
            return None;
        }
        ImageRotation::new(
            (degrees * 10.0).round() as i16,
            (self.transform.size.0 as u32, self.transform.size.1 as u32),
        )
    }

    pub(super) fn show(&mut self, context: &egui::Context) -> Option<Option<ImageRotation>> {
        let mut action = None;
        let modal = egui::Modal::new("free-rotate-image".into()).show(context, |ui| {
            ui.set_width((context.content_rect().width() - 32.0).clamp(1.0, 420.0));
            egui::ScrollArea::vertical()
                .max_height((context.content_rect().height() - 32.0).max(1.0))
                .show(ui, |ui| {
                    chrome::modal_heading(ui, "Free rotate image");
                    ui.label("Preview only. Apply adds one undoable edit.");
                    ui.label("Tip: hold Alt and drag horizontally on the image.");
                    ui.label("Angle in degrees (clockwise, 0.1 degree steps)");
                    let response =
                        resize::text_input(ui, "Rotation angle in degrees", &mut self.angle);
                    if self.first_frame {
                        response.request_focus();
                        self.first_frame = false;
                    }
                    let mut degrees = self
                        .value()
                        .map_or(0.0, |value| f64::from(value.tenths()) / 10.0);
                    if ui
                        .add(
                            egui::Slider::new(&mut degrees, -180.0..=180.0)
                                .step_by(0.1)
                                .show_value(false)
                                .text("Rotation angle"),
                        )
                        .changed()
                    {
                        self.angle = format!("{degrees:.1}");
                    }
                    let value = self.value();
                    if let Some(value) = value {
                        ui.label(format!(
                            "{:.1} degrees — {} x {} pixels",
                            f64::from(value.tenths()) / 10.0,
                            value.size().0,
                            value.size().1,
                        ));
                        let preview_height =
                            (context.content_rect().height() - 230.0).clamp(60.0, 240.0);
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), preview_height),
                            egui::Sense::hover(),
                        );
                        paint_preview(ui.painter(), rect, self.texture.id(), self.transform, value);
                        ui.label("Placement preview; final pixels are resampled on Apply.");
                    } else {
                        ui.label(concat!(
                            "Use -180 to 180 degrees; the canvas must fit ",
                            "16384 pixels per side and 128 Mi pixels.",
                        ));
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(value.is_some(), egui::Button::new("Apply rotation"))
                            .clicked()
                        {
                            action = Some(value);
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
        action
    }
}

fn preview_mesh(
    rect: egui::Rect,
    texture: egui::TextureId,
    transform: ImageTransform,
    rotation: ImageRotation,
) -> egui::Mesh {
    let (width, height) = rotation.size();
    let scale = (rect.width() / width as f32).min(rect.height() / height as f32);
    let source = egui::Rect::from_center_size(
        rect.center(),
        egui::vec2(transform.size.0, transform.size.1) * scale,
    );
    rotated_mesh(texture, source, transform, rotation.tenths())
}

fn rotated_mesh(
    texture: egui::TextureId,
    source: egui::Rect,
    transform: ImageTransform,
    tenths: i16,
) -> egui::Mesh {
    let mut mesh = transformed_image_mesh(texture, source, transform);
    let angle = egui::emath::Rot2::from_angle((f32::from(tenths) / 10.0).to_radians());
    for vertex in &mut mesh.vertices {
        vertex.pos = source.center() + angle * (vertex.pos - source.center());
    }
    mesh
}

fn paint_preview(
    painter: &egui::Painter,
    rect: egui::Rect,
    texture: egui::TextureId,
    transform: ImageTransform,
    rotation: ImageRotation,
) {
    let painter = painter.with_clip_rect(rect);
    paint_checkerboard(&painter, rect);
    painter.add(preview_mesh(rect, texture, transform, rotation));
}

fn paint_checkerboard(painter: &egui::Painter, rect: egui::Rect) {
    let rect = rect.intersect(painter.clip_rect());
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, Color32::from_gray(32));
    for row in 0..(rect.height() / 12.0).ceil() as usize {
        for column in 0..(rect.width() / 12.0).ceil() as usize {
            if (row + column) % 2 == 0 {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        rect.min + egui::vec2(column as f32 * 12.0, row as f32 * 12.0),
                        egui::vec2(12.0, 12.0),
                    ),
                    0.0,
                    Color32::from_gray(44),
                );
            }
        }
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn open_rotation(&mut self) {
        let Some(dialog) = self.capture_rotation() else {
            self.set_status("Wait for the full image to load before rotating".into());
            return;
        };
        self.guard_return_focus = self
            .ui_context
            .as_ref()
            .and_then(|context| context.memory(|memory| memory.focused()))
            .map(|focus| (dialog.tab, focus));
        self.rotation_dialog = Some(dialog);
        self.request_redraw();
    }

    fn capture_rotation(&mut self) -> Option<RotationDialog> {
        let (Some(tab), Some(path), Some(image)) =
            (self.tabs.active(), self.path.as_ref(), self.image.as_ref())
        else {
            return None;
        };
        if self.media_kind != Some(MediaKind::Image)
            || self.reading_mode
            || self.image_edit_pending
            || self.image_error.is_some()
        {
            return None;
        }
        let transform = self.visual_transform(image.dimensions());
        self.rotation_generation = self.rotation_generation.wrapping_add(1);
        Some(RotationDialog {
            token: self.rotation_generation,
            tab: tab.id,
            media_generation: self.media_generation,
            edit_generation: self.image_edit_generation,
            path: path.clone(),
            source: Arc::clone(&image.decoded),
            operations: self
                .edits
                .get(&tab.id)
                .map_or(&[][..], EditHistory::operations)
                .to_vec(),
            texture: image.texture.clone(),
            transform,
            angle: "0.0".into(),
            first_frame: true,
        })
    }

    pub(super) fn finish_rotation(&mut self, token: u64, value: Option<ImageRotation>) {
        if self
            .rotation_dialog
            .as_ref()
            .is_none_or(|dialog| dialog.token != token)
        {
            return;
        }
        let dialog = self
            .rotation_dialog
            .take()
            .expect("matching rotation dialog");
        self.commit_rotation(dialog, value);
    }

    fn rotation_is_current(&self, dialog: &RotationDialog) -> bool {
        self.tabs.active().is_some_and(|tab| tab.id == dialog.tab)
            && self.media_kind == Some(MediaKind::Image)
            && !self.reading_mode
            && !self.image_edit_pending
            && self.image_error.is_none()
            && self.media_generation == dialog.media_generation
            && self.image_edit_generation == dialog.edit_generation
            && self.path.as_ref() == Some(&dialog.path)
            && self
                .image
                .as_ref()
                .is_some_and(|image| Arc::ptr_eq(&image.decoded, &dialog.source))
            && self
                .edits
                .get(&dialog.tab)
                .map_or(&[][..], EditHistory::operations)
                == dialog.operations
    }

    fn commit_rotation(&mut self, dialog: RotationDialog, value: Option<ImageRotation>) {
        let valid = self.rotation_is_current(&dialog);
        if let Some(value) = value {
            if valid
                && value.source_size()
                    == (
                        dialog.transform.size.0 as u32,
                        dialog.transform.size.1 as u32,
                    )
            {
                if value.tenths() != 0 {
                    self.push_visual_edit(EditOperation::RotateImage(value));
                }
            } else {
                self.set_status("Rotation cancelled because the image changed".into());
            }
        }
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests;
