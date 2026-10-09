//! UI dispatch while the shared graphics device is being created.
use super::*;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn take_startup_image_completion(
        &mut self,
    ) -> Option<towavue_runtime_windows::LoadedImages> {
        if self.renderer.is_some() || self.startup_surface.is_none() {
            return self.image_loader.take_completed();
        }
        let limit = self
            .ui_context
            .as_ref()
            .expect("startup context")
            .input(|input| input.max_texture_side);
        match self.image_loader.take_completed_with_texture_limit(limit) {
            Ok(result) => result,
            Err(_) => {
                // Keep the completed batch while the device worker is still busy.
                // Its host notification requests a redraw and retries consumption;
                // never block the UI or discard the original for a provisional cap.
                // On failure retain the original batch for Retry, navigation or
                // closure; do not turn a provisional limit into an image error.
                if let Err(error) = self.finish_startup_graphics(false) {
                    towavue_runtime_windows::diagnostic!("Startup graphics failed: {error}");
                }
                self.renderer.as_ref()?;
                self.image_loader.take_completed()
            }
        }
    }

    pub(super) fn finish_startup_graphics(&mut self, wait: bool) -> Result<(), RenderError> {
        if self.startup_failed || self.startup_surface.is_none() || self.renderer.is_some() {
            return Ok(());
        }
        if !wait
            && self.window.as_ref().is_some_and(|window| {
                let size = window.inner_size();
                size.width == 0 || size.height == 0
            })
        {
            return Ok(());
        }
        let Some(job) = &self.startup_device else {
            return Ok(());
        };
        let Some(result) = job.get(wait) else {
            return Ok(());
        };
        let result = result.and_then(|device| {
            self.startup_surface
                .as_mut()
                .expect("startup surface")
                .prepare_handoff(
                    self.native_caption.as_ref().expect("startup caption"),
                    self.ui_context.as_ref().expect("startup context"),
                    device,
                )
        });
        match result {
            Ok(renderer) => {
                let limit = renderer.max_texture_side();
                self.ui_context
                    .as_ref()
                    .expect("startup context")
                    .input_mut(|input| input.max_texture_side = limit);
                self.ui_state
                    .as_mut()
                    .expect("startup input")
                    .set_max_texture_side(limit);
                self.renderer = Some(renderer);
                self.startup_device = None;
                self.request_redraw();
                Ok(())
            }
            Err(error) => {
                // Preserve the last complete CPU frame. Do not endlessly retry a
                // failed device on paint; the existing explicit recovery can retry.
                self.startup_failed = true;
                let message = error.message(self.language());
                self.queued_recovery = Some(FallbackPrompt::Recovery {
                    position: self.current_position(),
                    state: self.state,
                    error: message.clone(),
                });
                self.set_status(message);
                Err(error)
            }
        }
    }

    pub(super) fn prepare_startup_or_gpu_media(
        &mut self,
        context: &egui::Context,
        surface_size: Option<winit::dpi::PhysicalSize<u32>>,
    ) -> Result<bool, RenderError> {
        if self.renderer.is_none()
            && let Some(startup) = &mut self.startup_surface
        {
            if let Some(size) = surface_size {
                startup.resize([size.width as usize, size.height as usize])?;
            }
            return Ok(false);
        }
        let renderer = self.renderer.as_mut().expect("renderer exists");
        if let Some(size) = surface_size {
            renderer.resize_surface(size.width, size.height)?;
        }
        renderer.clear([0.0, 0.0, 0.0, 1.0])?;
        let drawn = if let (Some(session), Some(rect)) = (&mut self.session, self.video_rect) {
            if let Some(operations) = &self.video_raster_operations {
                session.draw_current_edited(
                    renderer,
                    rect * context.pixels_per_point(),
                    self.video_uv,
                    operations,
                )?
            } else {
                session.draw_current(renderer, rect * context.pixels_per_point(), self.video_uv)?
            }
        } else {
            false
        };
        Ok(drawn)
    }

    pub(super) fn submit_startup_or_gpu_ui(
        &mut self,
        context: &egui::Context,
        output: egui::FullOutput,
    ) -> Result<egui::PlatformOutput, RenderError> {
        if let Some(renderer) = &mut self.renderer {
            renderer.render_ui(context, output)
        } else {
            self.startup_surface
                .as_mut()
                .expect("startup surface")
                .draw(context, output)
        }
    }

    pub(super) fn present_startup_or_gpu(&mut self) -> Result<(), RenderError> {
        if let Some(renderer) = &self.renderer {
            renderer.present_surface()?;
            if let Some(startup) = &self.startup_surface {
                startup.publish(renderer)?;
                self.startup_surface = None;
            }
            Ok(())
        } else {
            self.startup_surface
                .as_ref()
                .expect("startup surface")
                .present()
        }
    }
}
