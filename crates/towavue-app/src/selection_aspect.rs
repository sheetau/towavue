use crate::*;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn select_aspect(&mut self, aspect: (u32, u32)) {
        let Some(kind) = self.media_kind else {
            return;
        };
        let geometry = match kind {
            MediaKind::Image
                if !self.reading_mode && !self.image_edit_pending && self.image_error.is_none() =>
            {
                self.image.as_ref().map(|image| {
                    let size = image.dimensions();
                    (size.0, size.1, 1.0)
                })
            }
            MediaKind::Video if self.visual_selection_enabled() => self
                .session
                .as_ref()
                .and_then(PlaybackSession::video_geometry),
            _ => return,
        };
        let Some((width, height, pixel_aspect)) = geometry else {
            self.set_status("Wait for media to load before selecting".into());
            return;
        };
        let transform = self.visual_transform((width, height));
        let size = (transform.size.0 as u32, transform.size.1 as u32);
        let Some(crop) =
            PixelCrop::centered_aspect(size, kind, aspect, transform.pixel_aspect(pixel_aspect))
        else {
            self.set_status("Media dimensions cannot form this selection".into());
            return;
        };
        self.set_time_selection(None);
        self.image_view.selection = Some(crop.unit_rect(size));
        self.image_view.crop_preview = false;
        if let Some(context) = &self.ui_context {
            selection::focus_first(context, self.selection_identity());
        }
        self.set_status(format!(
            "Selection {}:{} ({} x {} pixels)",
            aspect.0, aspect.1, crop.width, crop.height
        ));
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests;
