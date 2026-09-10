use super::*;

#[derive(Clone)]
pub(super) struct Request {
    pub path: PathBuf,
    pub point: egui::Pos2,
    pub anchor: egui::Vec2,
    folder_generation: u64,
    tab: Option<TabId>,
    instance: u64,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn can_open_filmstrip_window(&self, path: &Path, generation: u64) -> bool {
        self.filmstrip_open
            && !self.modal_input_blocked()
            && !self.palette_open
            && !self.grid_open
            && !self
                .ui_context
                .as_ref()
                .is_some_and(egui::Popup::is_any_open)
            && self.folder_snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.generation == generation
                    && snapshot.items.iter().any(|item| item.path == path)
            })
    }

    pub(super) fn request_filmstrip_window(
        &mut self,
        path: PathBuf,
        generation: u64,
        point: egui::Pos2,
        anchor: egui::Vec2,
    ) {
        if !point.is_finite() || !anchor.is_finite() {
            return;
        }
        if !self.hosted_graphics {
            self.open_filmstrip_window(path, generation, spawn_new_window);
            return;
        }
        if !self.can_open_filmstrip_window(&path, generation) || self.pending_window_open.is_some()
        {
            return;
        }
        self.pending_window_open = Some(Request {
            path,
            point,
            anchor,
            folder_generation: generation,
            tab: self.tabs.active().map(|tab| tab.id),
            instance: self.media_generation,
        });
        self.request_redraw();
    }

    pub(super) fn window_open_request_is_current(&self, request: &Request) -> bool {
        !self.exit_requested
            && self.tabs.active().map(|tab| tab.id) == request.tab
            && self.media_generation == request.instance
            && self.can_open_filmstrip_window(&request.path, request.folder_generation)
    }

    pub(super) fn position_window_at_drop(
        &self,
        position: winit::dpi::PhysicalPosition<i32>,
        anchor: egui::Vec2,
    ) -> Result<(), String> {
        let window = self.window.as_ref().expect("started window");
        let area = towavue_runtime_windows::monitor_work_area((position.x, position.y))
            .ok_or("Could not query destination monitor work area")?;
        // Enter the selected monitor while hidden so its actual DPI and size are
        // available before applying the logical grab offset and final edge clamp.
        window.set_outer_position(clamp_window_position(position, window.outer_size(), area));
        let inner = window.inner_position().map_err(|error| error.to_string())?;
        let outer = window.outer_position().map_err(|error| error.to_string())?;
        let density = window.scale_factor() as f32
            * self.ui_context.as_ref().expect("started UI").zoom_factor();
        let requested = winit::dpi::PhysicalPosition::new(
            position.x - (anchor.x * density).round() as i32 - (inner.x - outer.x),
            position.y - (anchor.y * density).round() as i32 - (inner.y - outer.y),
        );
        window.set_outer_position(clamp_window_position(requested, window.outer_size(), area));
        Ok(())
    }
}

pub(super) fn clamp_window_position(
    position: winit::dpi::PhysicalPosition<i32>,
    size: winit::dpi::PhysicalSize<u32>,
    area: (i32, i32, i32, i32),
) -> winit::dpi::PhysicalPosition<i32> {
    // Oversized windows keep their top-left controls visible without changing the
    // existing size/minimum-size policy. Use wide arithmetic for negative desktops.
    winit::dpi::PhysicalPosition::new(
        (position.x as i64).clamp(
            area.0 as i64,
            (area.2 as i64 - size.width as i64).max(area.0 as i64),
        ) as i32,
        (position.y as i64).clamp(
            area.1 as i64,
            (area.3 as i64 - size.height as i64).max(area.1 as i64),
        ) as i32,
    )
}
