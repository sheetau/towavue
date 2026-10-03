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

// A stable virtual crop avoids accumulating newly revealed margins as the next
// resize baseline. User zoom/pan, media geometry, or mode changes replace it.
#[derive(Clone)]
pub(super) struct Snapshot {
    generation: u64,
    extent: egui::Vec2,
    pixel_size: (u32, u32),
    reference: egui::Vec2,
    scale: f32,
    center_offset: egui::Vec2,
    applied: ImageViewState,
    mode: Mode,
}

impl Snapshot {
    fn capture(
        generation: u64,
        extent: egui::Vec2,
        viewport: egui::Vec2,
        density: f32,
        scale: f32,
        view: ImageViewState,
        mode: Mode,
    ) -> Self {
        let logical_scale = scale / density;
        let visible = (viewport / logical_scale).min(extent);
        // With margins on both axes, expand the virtual crop until its fitted
        // scale equals the current zoom. Fit and zoom-out stay stable too.
        let reference = visible * ((viewport / visible).min_elem() / logical_scale);
        Self {
            generation,
            extent,
            pixel_size: (extent.x.ceil() as u32, extent.y.ceil() as u32),
            reference,
            scale,
            center_offset: egui::vec2(view.pan.0, view.pan.1) / logical_scale,
            applied: view,
            mode,
        }
    }

    fn restore(&mut self, view: &mut ImageViewState, viewport: egui::Vec2, density: f32) {
        let scale = match self.mode {
            Mode::KeepArea => (viewport / self.reference).min_elem() * density,
            Mode::KeepScale => self.scale,
            Mode::Fit => {
                view.fit();
                return;
            }
        };
        let scale = ImageViewState {
            zoom: ZoomMode::Custom(scale),
            ..*view
        }
        .scale(self.pixel_size, (viewport * density).into());
        view.zoom = ZoomMode::Custom(scale);
        view.pan = (self.center_offset * (scale / density)).into();
        image_scroll::clamp(view, self.extent * (scale / density), viewport);
        self.applied = *view;
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn prepare_view_resize(&mut self) {
        if self.view_resize_mode == Mode::Fit {
            self.pending_view_resize = None;
            self.view_resize_anchor = None;
            self.image_view.fit();
            return;
        }
        if self.pending_view_resize.is_some() || self.image_viewport.min_elem() <= 0.0 {
            return;
        }
        let density = self.image_view_density;
        let geometry = if self.reading_mode {
            self.reading_resize_geometry(density)
                .map(|(extent, scale)| {
                    (
                        extent,
                        scale,
                        (extent.x.ceil() as u32, extent.y.ceil() as u32),
                    )
                })
        } else if let Some(image) = &self.image {
            let transform = self.visual_transform(image.dimensions());
            Some((
                egui::vec2(transform.size.0, transform.size.1),
                self.image_view.logical_scale(
                    (transform.size.0 as u32, transform.size.1 as u32),
                    self.image_viewport.into(),
                    density,
                ) * density,
                (transform.size.0 as u32, transform.size.1 as u32),
            ))
        } else if let Some((width, height, aspect)) = self
            .session
            .as_ref()
            .and_then(PlaybackSession::video_geometry)
        {
            let transform = self.visual_transform((width, height));
            let aspect = transform.pixel_aspect(aspect);
            Some((
                egui::vec2(transform.size.0 * aspect, transform.size.1),
                self.image_view.scale(
                    (transform.size.0 as u32, transform.size.1 as u32),
                    (
                        self.image_viewport.x * density / aspect,
                        self.image_viewport.y * density,
                    ),
                ),
                (transform.size.0 as u32, transform.size.1 as u32),
            ))
        } else {
            None
        };
        let Some((extent, scale, pixel_size)) = geometry else {
            return;
        };
        let reusable = self.view_resize_anchor.as_ref().filter(|anchor| {
            anchor.generation == self.media_generation
                && anchor.mode == self.view_resize_mode
                && anchor.extent == extent
                && anchor.pixel_size == pixel_size
                && anchor.applied.zoom == self.image_view.zoom
                && anchor.applied.pan == self.image_view.pan
        });
        self.pending_view_resize = Some(reusable.cloned().unwrap_or_else(|| {
            let mut snapshot = Snapshot::capture(
                self.media_generation,
                extent,
                self.image_viewport,
                density,
                scale,
                self.image_view,
                self.view_resize_mode,
            );
            snapshot.pixel_size = pixel_size;
            snapshot
        }));
    }

    pub(super) fn apply_view_resize(&mut self, viewport: egui::Vec2, density: f32) {
        if viewport.min_elem() <= 0.0 {
            return;
        }
        if let Some(mut snapshot) = self.pending_view_resize.take()
            && snapshot.generation == self.media_generation
        {
            snapshot.restore(&mut self.image_view, viewport, density);
            self.view_resize_anchor = Some(snapshot);
        }
        self.image_view_density = density;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_crop_keeps_fit_and_zoom_out_stable_under_alternating_axis_resizes() {
        let extent = egui::vec2(1600.0, 900.0);
        let start = egui::vec2(800.0, 600.0);
        for scale in [0.2, 0.5, 2.0] {
            let mut view = ImageViewState {
                zoom: ZoomMode::Custom(scale),
                ..Default::default()
            };
            let mut anchor = Snapshot::capture(0, extent, start, 1.0, scale, view, Mode::KeepArea);
            for _ in 0..100 {
                for viewport in [
                    egui::vec2(800.0, 700.0),
                    egui::vec2(900.0, 700.0),
                    egui::vec2(900.0, 600.0),
                    start,
                ] {
                    anchor.restore(&mut view, viewport, 1.0);
                    let ZoomMode::Custom(actual) = view.zoom else {
                        panic!("custom");
                    };
                    assert!(actual >= scale - 0.00001);
                    if scale <= 0.5 {
                        assert!(
                            (actual - (viewport / extent).min_elem() * scale / 0.5).abs() < 0.00001
                        );
                    }
                }
                assert_eq!(view.zoom, ZoomMode::Custom(scale));
            }
        }
    }

    #[test]
    fn resize_clamps_display_scale_without_losing_the_original_center() {
        let extent = egui::vec2(1600.0, 900.0);
        let viewport = egui::vec2(800.0, 600.0);
        let mut view = ImageViewState {
            zoom: ZoomMode::Custom(2.0),
            pan: (100.0, -40.0),
            ..Default::default()
        };
        let original = view;
        let mut anchor = Snapshot::capture(0, extent, viewport, 1.0, 2.0, view, Mode::KeepArea);
        anchor.restore(&mut view, egui::vec2(1_000_000.0, 1_000_000.0), 1.0);
        assert_eq!(view.zoom, ZoomMode::Custom(64.0));
        assert_eq!(view.pan, (0.0, 0.0));
        anchor.restore(&mut view, viewport, 1.0);
        assert_eq!(view, original);
    }

    #[test]
    fn native_resize_reuses_anchor_until_manual_view_changes_and_rejects_stale_media() {
        let Some(root) = crate::tests::isolated_test_root(
            "view_resize::tests::native_resize_reuses_anchor_until_manual_view_changes_and_rejects_stale_media",
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
        app.image_viewport = egui::vec2(40.0, 30.0);
        app.window_size = Some(winit::dpi::PhysicalSize::new(400, 400));
        app.image_view.zoom = ZoomMode::Custom(10.0);
        app.dispatch(CommandId::ResizeViewKeepArea);
        for _ in 0..12 {
            for (size, viewport) in [
                (
                    winit::dpi::PhysicalSize::new(500, 500),
                    egui::vec2(40.0, 40.0),
                ),
                (
                    winit::dpi::PhysicalSize::new(400, 400),
                    egui::vec2(40.0, 30.0),
                ),
            ] {
                app.resize_window(size);
                app.apply_view_resize(viewport, 1.0);
                app.image_viewport = viewport;
            }
            assert_eq!(app.image_view.zoom, ZoomMode::Custom(10.0));
        }
        app.image_view.zoom = ZoomMode::Custom(5.0);
        app.set_fullscreen(true);
        app.apply_view_resize(egui::vec2(80.0, 60.0), 1.0);
        assert_eq!(app.image_view.zoom, ZoomMode::Custom(10.0));
        app.image_viewport = egui::vec2(80.0, 60.0);
        app.dispatch(CommandId::ResizeViewKeepScale);
        app.set_fullscreen(false);
        app.apply_view_resize(egui::vec2(40.0, 30.0), 2.0);
        assert_eq!(app.image_view.zoom, ZoomMode::Custom(10.0));
        app.resize_window(winit::dpi::PhysicalSize::new(0, 0));
        assert!(app.pending_view_resize.is_none());
        app.set_fullscreen(true);
        app.media_generation += 1;
        app.image_view.fit();
        app.apply_view_resize(egui::vec2(80.0, 60.0), 1.0);
        assert_eq!(app.image_view.zoom, ZoomMode::Fit);
    }

    #[test]
    fn nonsquare_pixel_video_keeps_physical_zoom_and_center_across_dpi() {
        let viewport = egui::vec2(400.0, 300.0);
        let mut view = ImageViewState {
            zoom: ZoomMode::Custom(3.0),
            pan: (60.0, -30.0),
            selection: Some(UnitRect::FULL),
        };
        let mut anchor = Snapshot::capture(
            0,
            egui::vec2(3840.0, 1080.0),
            viewport,
            1.25,
            3.0,
            view,
            Mode::KeepScale,
        );
        anchor.restore(&mut view, egui::vec2(640.0, 500.0), 2.0);
        assert_eq!(view.zoom, ZoomMode::Custom(3.0));
        assert!((egui::vec2(view.pan.0, view.pan.1) - egui::vec2(37.5, -18.75)).length() < 0.00001);
        assert_eq!(view.selection, Some(UnitRect::FULL));
    }
}
