use super::*;

pub(super) struct IdleGraphics {
    windows: Vec<(WindowKey, u64, u64)>,
    deadline: Instant,
    attempted: bool,
}

impl WindowHost {
    pub(super) fn trim_idle_graphics(&mut self, now: Instant) -> ControlFlow {
        let eligible = !self.windows.is_empty()
            && self.pending_launches.is_empty()
            && self.windows.values().all(|app| {
                app.tabs.tabs().is_empty()
                    && app.session.is_none()
                    && app.retained_images.is_empty()
                    && app.retained_playback.is_empty()
                    && app.initial_path.is_none()
                    && app.pending_window_open.is_none()
                    && app.pending_folder.is_none()
                    && app.active_export.is_none()
                    && !app.modal_input_blocked()
                    && !app.palette_open
                    && !app.grid_open
                    && !app.filmstrip_open
                    && !app
                        .ui_context
                        .as_ref()
                        .is_some_and(egui::Popup::is_any_open)
                    && app.graphics_recovery_request.is_none()
                    && app.queued_recovery.is_none()
                    && app.renderer.is_some()
                    && app.idle_graphics_frame == Some((app.media_generation, app.graphics_epoch))
            });
        if !eligible {
            self.idle_graphics = None;
            return ControlFlow::Wait;
        }
        let same = self.idle_graphics.as_ref().is_some_and(|idle| {
            idle.windows.len() == self.windows.len()
                && idle.windows.iter().zip(&self.windows).all(
                    |(&(key, instance, epoch), (&current_key, app))| {
                        key == current_key
                            && instance == app.media_generation
                            && epoch == app.graphics_epoch
                    },
                )
        });
        if !same {
            self.idle_graphics = Some(IdleGraphics {
                windows: self
                    .windows
                    .iter()
                    .map(|(&key, app)| (key, app.media_generation, app.graphics_epoch))
                    .collect(),
                deadline: now + Duration::from_secs(1),
                attempted: false,
            });
        }
        let idle = self.idle_graphics.as_mut().expect("eligible idle windows");
        if idle.attempted {
            return ControlFlow::Wait;
        }
        if now < idle.deadline {
            return ControlFlow::WaitUntil(idle.deadline);
        }
        idle.attempted = true;
        // All hosted surfaces share one device. One trim covers it without replacing any surface.
        let renderer = self
            .windows
            .values_mut()
            .next()
            .expect("idle window")
            .renderer
            .as_mut()
            .expect("idle renderer");
        if let Err(error) = renderer.trim_idle_resources() {
            eprintln!("towavue: idle graphics cache trim was unavailable: {error}");
        }
        ControlFlow::Wait
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod performance_tests;
