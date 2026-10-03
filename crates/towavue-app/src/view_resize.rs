use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Fit,
    KeepArea,
    KeepScale,
}

impl Mode {
    pub(crate) fn command(self) -> CommandId {
        match self {
            Self::Fit => CommandId::ResizeViewFit,
            Self::KeepArea => CommandId::ResizeViewKeepArea,
            Self::KeepScale => CommandId::ResizeViewKeepScale,
        }
    }

    pub(super) fn from_command(command: CommandId) -> Self {
        match command {
            CommandId::ResizeViewKeepArea => Self::KeepArea,
            CommandId::ResizeViewKeepScale => Self::KeepScale,
            _ => Self::Fit,
        }
    }
}

pub(super) struct Snapshot {
    generation: u64,
    viewport: egui::Vec2,
    density: f32,
    scale: f32,
    pan: egui::Vec2,
    mode: Mode,
}

impl Snapshot {
    fn restore(&self, view: &mut ImageViewState, viewport: egui::Vec2, density: f32) {
        // Preserve the center image coordinate. With a changed aspect ratio,
        // KeepArea retains the old visible rectangle and reveals more on one axis.
        let ratio = match self.mode {
            Mode::KeepArea => (viewport / self.viewport).min_elem(),
            Mode::KeepScale => self.density / density,
            Mode::Fit => {
                view.fit();
                return;
            }
        };
        view.zoom = ZoomMode::Custom(self.scale * ratio * density / self.density);
        view.pan = (self.pan * ratio).into();
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn prepare_view_resize(&mut self) {
        if self.view_resize_mode == Mode::Fit {
            self.pending_view_resize = None;
            self.image_view.fit();
            return;
        }
        // Coalesced native resize notifications still refer to the last drawn view.
        if self.pending_view_resize.is_some() || self.image_viewport.min_elem() <= 0.0 {
            return;
        }
        let density = self.image_view_density;
        let scale = if self.reading_mode {
            self.reading_resize_scale(density)
        } else if let Some(image) = &self.image {
            let transform = self.visual_transform(image.dimensions());
            Some(
                self.image_view.logical_scale(
                    (transform.size.0 as u32, transform.size.1 as u32),
                    self.image_viewport.into(),
                    density,
                ) * density,
            )
        } else if let Some((width, height, aspect)) = self
            .session
            .as_ref()
            .and_then(PlaybackSession::video_geometry)
        {
            let transform = self.visual_transform((width, height));
            Some(self.image_view.scale(
                (transform.size.0 as u32, transform.size.1 as u32),
                (
                    self.image_viewport.x * density / transform.pixel_aspect(aspect),
                    self.image_viewport.y * density,
                ),
            ))
        } else {
            None
        };
        self.pending_view_resize = scale.map(|scale| Snapshot {
            generation: self.media_generation,
            viewport: self.image_viewport,
            density,
            scale,
            pan: self.image_view.pan.into(),
            mode: self.view_resize_mode,
        });
    }

    pub(super) fn apply_view_resize(&mut self, viewport: egui::Vec2, density: f32) {
        if viewport.min_elem() <= 0.0 {
            return;
        }
        if let Some(snapshot) = self.pending_view_resize.take()
            && snapshot.generation == self.media_generation
        {
            snapshot.restore(&mut self.image_view, viewport, density);
        }
        self.image_view_density = density;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_modes_preserve_center_scale_and_selection_across_density_changes() {
        for mode in [Mode::KeepArea, Mode::KeepScale] {
            for density in [1.0, 1.25, 2.0] {
                let snapshot = Snapshot {
                    generation: 1,
                    viewport: egui::vec2(400.0, 300.0),
                    density: 1.25,
                    scale: 2.0,
                    pan: egui::vec2(40.0, -20.0),
                    mode,
                };
                let mut view = ImageViewState {
                    selection: Some(UnitRect::FULL),
                    ..Default::default()
                };
                snapshot.restore(&mut view, egui::vec2(800.0, 500.0), density);
                let ZoomMode::Custom(scale) = view.zoom else {
                    panic!("custom zoom");
                };
                let ratio = if mode == Mode::KeepArea {
                    500.0 / 300.0
                } else {
                    1.25 / density
                };
                assert!((scale - 2.0 * ratio * density / 1.25).abs() < 0.0001);
                assert!(
                    (view.pan.0 / (scale / density) - 25.0).abs() < 0.0001,
                    "same center image coordinate"
                );
                assert_eq!(view.selection, Some(UnitRect::FULL));
            }
        }
    }

    #[test]
    fn native_resize_and_fullscreen_coalesce_and_ignore_stale_media() {
        let Some(root) = crate::tests::isolated_test_root(
            "view_resize::tests::native_resize_and_fullscreen_coalesce_and_ignore_stale_media",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(fonts::test_context());
        tab_transfer::tests::install(
            &mut app,
            root.join("resize.bmp"),
            tab_transfer::tests::decoded(false),
        );
        app.image_viewport = egui::vec2(400.0, 300.0);
        app.window_size = Some(winit::dpi::PhysicalSize::new(400, 400));
        app.image_view.zoom = ZoomMode::Custom(4.0);
        app.image_view.pan = (2.0, -1.0);
        app.dispatch(CommandId::ResizeViewKeepArea);
        app.resize_window(winit::dpi::PhysicalSize::new(800, 700));
        app.resize_window(winit::dpi::PhysicalSize::new(1000, 900));
        app.apply_view_resize(egui::vec2(800.0, 600.0), 1.0);
        assert_eq!(app.image_view.zoom, ZoomMode::Custom(8.0));
        assert_eq!(app.image_view.pan, (4.0, -2.0));
        app.image_viewport = egui::vec2(800.0, 600.0);
        app.dispatch(CommandId::ResizeViewKeepScale);
        app.set_fullscreen(true);
        app.apply_view_resize(egui::vec2(1600.0, 900.0), 2.0);
        assert_eq!(app.image_view.zoom, ZoomMode::Custom(8.0));
        assert_eq!(app.image_view.pan, (2.0, -1.0));
        app.resize_window(winit::dpi::PhysicalSize::new(0, 0));
        assert!(app.pending_view_resize.is_none());
        app.set_fullscreen(false);
        app.media_generation += 1;
        app.image_view.fit();
        app.apply_view_resize(egui::vec2(800.0, 600.0), 1.0);
        assert_eq!(app.image_view.zoom, ZoomMode::Fit);
        app.dispatch(CommandId::ResizeViewFit);
        app.image_view.zoom = ZoomMode::Actual;
        app.set_fullscreen(true);
        assert_eq!(app.image_view.zoom, ZoomMode::Fit);
    }
    #[test]
    fn resized_video_preserves_scale_and_center_with_nonsquare_pixels() {
        let old = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        let new = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 500.0));
        for aspect in [0.5, 1.0, 2.0] {
            for mode in [Mode::KeepArea, Mode::KeepScale] {
                let mut view = ImageViewState {
                    zoom: ZoomMode::Custom(3.0),
                    pan: (60.0, -30.0),
                    ..Default::default()
                };
                let size = (1920, 1080);
                let before = video_view::rect(old, size, aspect, 1.25, view);
                let snapshot = Snapshot {
                    generation: 1,
                    viewport: old.size(),
                    density: 1.25,
                    scale: view.scale(size, (old.width() * 1.25 / aspect, old.height() * 1.25)),
                    pan: view.pan.into(),
                    mode,
                };
                snapshot.restore(&mut view, new.size(), 2.0);
                let after = video_view::rect(new, size, aspect, 2.0, view);
                let before_center = (old.center() - before.min) / before.size();
                let after_center = (new.center() - after.min) / after.size();
                assert!((before_center - after_center).length() < 0.00001);
                if mode == Mode::KeepScale {
                    assert!((before.size() * 1.25 - after.size() * 2.0).length() < 0.001);
                } else {
                    assert!((after.width() / before.width() - 1.6).abs() < 0.00001);
                }
            }
        }
    }
}
