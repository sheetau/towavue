use crate::*;

#[cfg(test)]
mod tests;

pub(super) struct ImageHandoff {
    pub path: PathBuf,
    pub image: ImagePresentation,
    pub view: ImageViewState,
    pub reading: Option<reading_view::ReadingHandoff>,
    transform: ImageTransform,
    pub file_details: Option<towavue_runtime_windows::FileDetails>,
}

impl ImageHandoff {
    pub fn selection_crop(&self) -> Option<PixelCrop> {
        PixelCrop::from_selection(
            self.view.selection?,
            (self.transform.size.0 as u32, self.transform.size.1 as u32),
            MediaKind::Image,
        )
    }

    pub fn draw(&self, ui: &mut egui::Ui) {
        if let Some(reading) = &self.reading {
            reading.draw(ui, self.view);
            return;
        }
        ImageEditView {
            transform: self.transform,
            view: self.view,
            rotation_tenths: 0,
            resized_size: None,
        }
        .draw(ui, self.image.texture.id());
    }
}

/// Geometry only: pending edits keep drawing the existing presentation's texture.
#[derive(Clone, Copy)]
pub(super) struct ImageEditView {
    transform: ImageTransform,
    view: ImageViewState,
    pub(super) rotation_tenths: i16,
    resized_size: Option<(u32, u32)>,
}

impl ImageEditView {
    pub fn resized(mut self, size: (u32, u32)) -> Self {
        // Keep source dimensions for the cropped texture's half-pixel sampling bands.
        self.resized_size = Some(size);
        self.view.fit();
        self.view.selection = None;
        self.rotation_tenths = 0;
        self
    }

    pub fn draw(self, ui: &mut egui::Ui, texture: egui::TextureId) {
        let viewport = ui.max_rect();
        let density = ui.ctx().pixels_per_point();
        let mut view = self.view;
        let size = self
            .resized_size
            .unwrap_or((self.transform.size.0 as u32, self.transform.size.1 as u32));
        let scale = view.logical_scale(size, viewport.size().into(), density);
        let displayed = egui::vec2(size.0 as f32, size.1 as f32) * scale;
        image_scroll::clamp(&mut view, displayed, viewport.size());
        let rect = egui::Rect::from_center_size(
            viewport.center() + egui::vec2(view.pan.0, view.pan.1),
            displayed,
        );
        let painter = ui.painter_at(viewport);
        if self.rotation_tenths != 0 {
            let mesh = rotation::rotated_mesh(texture, rect, self.transform, self.rotation_tenths);
            rotation::paint_checkerboard(&painter, mesh.calc_bounds());
            painter.add(mesh);
            return;
        }
        painter.add(transformed_image_mesh(texture, rect, self.transform));
        image_scroll::held_bars(ui, viewport, displayed, view);
        if let Some(selection) = view.selection {
            paint_selection(&painter, rect, selection);
        }
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn capture_image_edit_view(&self) -> Option<ImageEditView> {
        if self.media_kind != Some(MediaKind::Image) || self.reading_mode {
            return None;
        }
        let image = self.image.as_ref()?;
        Some(image.held_edit_view.unwrap_or_else(|| {
            let mut view = self.image_view;
            // Applying a visual edit clears selection, even while pixels are held.
            view.selection = None;
            ImageEditView {
                transform: self.visual_transform(image.dimensions()),
                view,
                rotation_tenths: 0,
                resized_size: None,
            }
        }))
    }

    pub(super) fn take_navigation_handoff(&mut self, kind: MediaKind) -> Option<ImageHandoff> {
        if kind != MediaKind::Image
            || self.media_kind != Some(MediaKind::Image)
            || self.image_edit_pending
            || self.displayed_tab.is_none()
            || self.displayed_tab != self.tabs.active().map(|tab| tab.id)
        {
            return None;
        }
        if self.reading_mode && self.image_loading {
            return self.image_handoff.take();
        }
        if self.image_error.is_some() {
            return None;
        }
        if let (Some(image), Some(path)) = (&self.image, &self.path) {
            Some(ImageHandoff {
                path: path.clone(),
                image: image.clone(),
                view: self.image_view,
                reading: self.reading_mode.then(|| self.capture_reading_handoff()),
                transform: self.visual_transform(image.dimensions()),
                file_details: self
                    .status_file_details
                    .get(self.status_file_source())
                    .cloned(),
            })
        } else {
            self.image_handoff.take()
        }
    }

    pub(super) fn displayed_image_path(&self) -> Option<&PathBuf> {
        self.image_handoff
            .as_ref()
            .map(|held| &held.path)
            .or(self.path.as_ref())
    }
}
