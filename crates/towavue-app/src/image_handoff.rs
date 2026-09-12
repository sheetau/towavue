use crate::*;

#[cfg(test)]
mod tests;

pub(super) struct ImageHandoff {
    pub path: PathBuf,
    pub image: ImagePresentation,
    pub view: ImageViewState,
    transform: ImageTransform,
    pub bytes: Option<u64>,
}

impl ImageHandoff {
    pub fn selection_crop(&self) -> Option<PixelCrop> {
        PixelCrop::from_selection(
            self.view.selection?,
            (self.transform.size.0 as u32, self.transform.size.1 as u32),
            MediaKind::Image,
        )
    }

    pub fn draw(&self, ui: &egui::Ui) {
        let viewport = ui.max_rect();
        let density = ui.ctx().pixels_per_point();
        let mut view = self.view;
        let size = (self.transform.size.0 as u32, self.transform.size.1 as u32);
        let scale = view.scale(size, (viewport.size() * density).into()) / density;
        let displayed = egui::vec2(self.transform.size.0, self.transform.size.1) * scale;
        image_scroll::clamp(&mut view, displayed, viewport.size());
        let rect = egui::Rect::from_center_size(
            viewport.center() + egui::vec2(view.pan.0, view.pan.1),
            displayed,
        );
        let painter = ui.painter_at(viewport);
        painter.add(transformed_image_mesh(
            self.image.texture.id(),
            rect,
            self.transform,
        ));
        if let Some(selection) = view.selection {
            paint_selection(&painter, rect, selection);
        }
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn take_navigation_handoff(&mut self, kind: MediaKind) -> Option<ImageHandoff> {
        if kind != MediaKind::Image
            || self.media_kind != Some(MediaKind::Image)
            || self.reading_mode
            || self.image_edit_pending
            || self.image_error.is_some()
            || self.displayed_tab.is_none()
            || self.displayed_tab != self.tabs.active().map(|tab| tab.id)
        {
            return None;
        }
        if let (Some(image), Some(path)) = (&self.image, &self.path) {
            Some(ImageHandoff {
                path: path.clone(),
                image: image.clone(),
                view: self.image_view,
                transform: self.visual_transform(image.dimensions()),
                bytes: self.status_file_size.bytes(self.status_file_source()),
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
