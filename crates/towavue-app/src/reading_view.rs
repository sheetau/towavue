use crate::*;

#[cfg(test)]
mod tests;

pub(super) struct ReadingHandoff {
    // Retain shared originals as well as handles so every held page can recover its texture.
    pub images: Vec<ImagePresentation>,
    pages: Vec<(Option<egui::TextureId>, egui::Vec2)>,
    settings: ReadingSettings,
    extent: egui::Vec2,
}

impl ReadingHandoff {
    pub fn draw(&self, ui: &mut egui::Ui, mut view: ImageViewState) {
        let viewport = ui.max_rect();
        let scale = scale(
            view,
            self.extent,
            viewport.size(),
            ui.ctx().pixels_per_point(),
        );
        let displayed = self.extent * scale;
        image_scroll::clamp(&mut view, displayed, viewport.size());
        let spread =
            egui::Rect::from_center_size(viewport.center() + egui::Vec2::from(view.pan), displayed);
        let sizes: Vec<_> = self.pages.iter().map(|page| page.1).collect();
        let rects = reading_page_rects(spread, &sizes, self.settings.axis, self.settings.reversed);
        let painter = ui.painter_at(viewport);
        for ((texture, _), rect) in self.pages.iter().zip(rects) {
            let Some(texture) = texture else { continue };
            painter.image(
                *texture,
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        image_scroll::held_bars(ui, viewport, displayed, view);
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn capture_reading_handoff(&self) -> ReadingHandoff {
        let pages = self.reading_page_views();
        let extent = self.reading_extent(&pages);
        ReadingHandoff {
            images: self
                .reading_pages
                .iter()
                .filter_map(|page| page.as_ref().ok())
                .cloned()
                .collect(),
            pages,
            settings: self.reading_settings,
            extent,
        }
    }

    fn reading_page_views(&self) -> Vec<(Option<egui::TextureId>, egui::Vec2)> {
        let first = self
            .image
            .as_ref()
            .map(|image| Ok((&image.texture, image.texture.size_vec2())))
            .or_else(|| self.image_error.as_ref().map(Err));
        let mut pages: Vec<_> = self
            .reading_pages
            .iter()
            .map(|page| {
                Some(
                    page.as_ref()
                        .map(|image| (&image.texture, image.texture.size_vec2())),
                )
            })
            .collect();
        let ordered = self
            .folder_snapshot
            .as_ref()
            .zip(self.path.as_ref())
            .map(|(snapshot, path)| {
                snapshot.reading_items(
                    path,
                    ReadingSettings {
                        reversed: false,
                        ..self.reading_settings
                    },
                )
            })
            .unwrap_or_default();
        if first.is_some() || self.image_loading {
            let index = ordered
                .iter()
                .position(|item| Some(&item.path) == self.path.as_ref())
                .unwrap_or(0);
            let count = ordered.len().max(1);
            if self.image_loading {
                pages.resize(count.saturating_sub(1), None);
            }
            pages.insert(index.min(pages.len()), first);
        }
        for (index, page) in pages.iter_mut().enumerate() {
            let path = ordered
                .get(index)
                .map(|item| &item.path)
                .or_else(|| self.path.as_ref().filter(|_| index == 0));
            if page.is_none()
                && let Some(preview) = path.and_then(|path| self.image_previews.get(path))
            {
                *page = Some(Ok((
                    &preview.texture,
                    egui::vec2(preview.source_size.0 as f32, preview.source_size.1 as f32),
                )));
            }
        }
        let pending_size = self
            .image
            .as_ref()
            .map_or(egui::Vec2::splat(1.0), |image| image.texture.size_vec2());
        pages
            .into_iter()
            .map(|page| match page {
                Some(Ok((texture, size))) => (Some(texture.id()), size),
                Some(Err(_)) => (None, egui::Vec2::splat(1.0)),
                None => (None, pending_size),
            })
            .collect()
    }

    fn reading_extent(&self, pages: &[(Option<egui::TextureId>, egui::Vec2)]) -> egui::Vec2 {
        // Joined pages share the current source page's cross-axis pixel extent.
        // Actual size therefore remains literal for that page even with mixed sizes.
        let reference = self
            .image
            .as_ref()
            .map(|image| image.texture.size_vec2())
            .unwrap_or_else(|| pages.first().map_or(egui::Vec2::splat(1.0), |page| page.1));
        match self.reading_settings.axis {
            ReadingAxis::Horizontal => egui::vec2(
                pages.iter().map(|page| page.1.x / page.1.y).sum::<f32>() * reference.y,
                reference.y,
            ),
            ReadingAxis::Vertical => egui::vec2(
                reference.x,
                pages.iter().map(|page| page.1.y / page.1.x).sum::<f32>() * reference.x,
            ),
        }
    }

    pub(super) fn zoom_reading(&mut self, factor: f32) {
        let Some(context) = &self.ui_context else {
            return;
        };
        let pages = self.reading_page_views();
        if pages.is_empty() || self.image_viewport.min_elem() <= 0.0 {
            return;
        }
        let extent = self.reading_extent(&pages);
        zoom(
            &mut self.image_view,
            factor,
            extent,
            self.image_viewport,
            context.pixels_per_point(),
        );
        self.request_redraw();
    }

    pub(super) fn draw_reading_pages(&mut self, ui: &mut egui::Ui) {
        let viewport = ui.max_rect();
        self.image_viewport = viewport.size();
        let pages = self.reading_page_views();
        if pages.is_empty() || viewport.size().min_elem() <= 0.0 {
            return;
        }
        let extent = self.reading_extent(&pages);
        let density = ui.ctx().pixels_per_point();
        let mut displayed = extent * scale(self.image_view, extent, viewport.size(), density);
        image_scroll::clamp(&mut self.image_view, displayed, viewport.size());
        let mut response = ui.interact(
            viewport,
            ui.id().with("reading-surface"),
            egui::Sense::click_and_drag(),
        );
        if self.view_input_allowed(ui.ctx()) && self.view_drag.is_none() {
            for (pointer, event) in wheel_input::image_events(ui.ctx(), &response) {
                match event {
                    wheel_input::ViewWheel::Zoom(factor) => {
                        let surface = image_scroll::surface(
                            viewport,
                            displayed,
                            ui.spacing().scroll.bar_width,
                        );
                        if !surface.contains(pointer) {
                            continue;
                        }
                        let center = viewport.center() + egui::Vec2::from(self.image_view.pan);
                        let before = scale(self.image_view, extent, viewport.size(), density);
                        zoom(
                            &mut self.image_view,
                            factor,
                            extent,
                            viewport.size(),
                            density,
                        );
                        let ratio =
                            scale(self.image_view, extent, viewport.size(), density) / before;
                        self.image_view.pan = (egui::Vec2::from(self.image_view.pan)
                            + (pointer - center) * (1.0 - ratio))
                            .into();
                        displayed *= ratio;
                    }
                    wheel_input::ViewWheel::Pan(delta) => {
                        self.image_view.pan =
                            (egui::Vec2::from(self.image_view.pan) + delta).into();
                    }
                }
                image_scroll::clamp(&mut self.image_view, displayed, viewport.size());
            }
        }
        response.interact_rect =
            image_scroll::surface(viewport, displayed, ui.spacing().scroll.bar_width);
        if displayed.x > viewport.width() || displayed.y > viewport.height() {
            self.update_pan(&response, ui.input(|input| input.pointer.hover_pos()));
            if matches!(self.view_drag, Some(ViewDrag::Pan { .. })) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        } else if matches!(self.view_drag, Some(ViewDrag::Pan { .. })) {
            self.cancel_view_drag();
        }
        image_scroll::clamp(&mut self.image_view, displayed, viewport.size());
        let painter = ui.painter_at(viewport);
        let shapes: Vec<_> = pages
            .iter()
            .map(|_| painter.add(egui::Shape::Noop))
            .collect();
        let enabled = self.view_drag_allowed(ui.ctx()) && self.view_drag.is_none();
        image_scroll::bars(ui, viewport, displayed, &mut self.image_view, enabled);
        let spread = egui::Rect::from_center_size(
            viewport.center() + egui::Vec2::from(self.image_view.pan),
            displayed,
        );
        let sizes: Vec<_> = pages.iter().map(|page| page.1).collect();
        let rects = reading_page_rects(
            spread,
            &sizes,
            self.reading_settings.axis,
            self.reading_settings.reversed,
        );
        for (((texture, _), rect), shape) in pages.into_iter().zip(rects).zip(shapes) {
            if let Some(texture) = texture {
                painter.set(
                    shape,
                    egui::Shape::image(
                        texture,
                        rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    ),
                );
            }
        }
    }
}

// Keep fractional joined extents exact for Fit/Cover and mixed-page seams;
// reuse image zoom limits for Custom rather than rounding the layout itself.
fn scale(view: ImageViewState, extent: egui::Vec2, viewport: egui::Vec2, density: f32) -> f32 {
    match view.zoom {
        ZoomMode::Fit => (viewport / extent).min_elem(),
        ZoomMode::Cover => (viewport / extent).max_elem(),
        _ => {
            view.scale(
                (extent.x.ceil() as u32, extent.y.ceil() as u32),
                (viewport * density).into(),
            ) / density
        }
    }
}

fn zoom(
    view: &mut ImageViewState,
    factor: f32,
    extent: egui::Vec2,
    viewport: egui::Vec2,
    density: f32,
) {
    view.zoom = ZoomMode::Custom(scale(*view, extent, viewport, density) * density);
    view.zoom_by(
        factor,
        (extent.x.ceil() as u32, extent.y.ceil() as u32),
        (viewport * density).into(),
    );
}
