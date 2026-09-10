use super::*;

#[cfg(test)]
#[path = "window_drop_tests.rs"]
pub(super) mod tests;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(crate) fn accepts_tab_drop(&self) -> bool {
        !self.fullscreen
            && !self.exit_requested
            && !self.modal_input_blocked()
            && !self.palette_open
            && !self.grid_open
            && !self.filmstrip_open
            && self
                .ui_context
                .as_ref()
                .is_some_and(|context| !egui::Popup::is_any_open(context))
    }

    pub(crate) fn request_tab_drop(&mut self, id: TabId, point: egui::Pos2) {
        if !self.hosted_graphics {
            self.request_guarded(GuardedAction::DetachTab(id));
            return;
        }
        if !self.accepts_tab_drop() || !point.is_finite() {
            return;
        }
        match self.tab_detach_request(id) {
            Ok(request) => self.pending_tab_drop = Some((request, point)),
            Err(error) => self.set_status(format!("Could not move tab: {error}")),
        }
    }

    fn incoming_gap(&self, point: egui::Pos2) -> Option<usize> {
        if !self.accepts_tab_drop() || self.validate_transfer_window().is_err() {
            return None;
        }
        let context = self.ui_context.as_ref()?;
        if let Some(window) = &self.window {
            if (window.scale_factor() as f32 * context.zoom_factor() - context.pixels_per_point())
                .abs()
                > 0.001
            {
                return None;
            }
            let size = window
                .inner_size()
                .to_logical::<f32>(context.pixels_per_point() as f64);
            if (context.viewport_rect().size() - egui::vec2(size.width, size.height)).length_sq()
                > 1.0
            {
                return None;
            }
        }
        tab_drag::incoming_gap(
            context,
            &self
                .tabs
                .tabs()
                .iter()
                .map(|tab| tab.id)
                .collect::<Vec<_>>(),
            point,
        )
    }
}

impl WindowHost {
    fn window_at_drop(
        &self,
        source: WindowKey,
        point: egui::Pos2,
    ) -> Option<(WindowKey, egui::Pos2)> {
        let app = self.windows.get(&source)?;
        let window = app.window.as_ref()?;
        let density = app.ui_context.as_ref()?.pixels_per_point();
        let physical = (
            (point.x * density).round() as i32,
            (point.y * density).round() as i32,
        );
        self.windows.iter().find_map(|(key, target)| {
            if *key == source || target.exit_requested {
                return None;
            }
            let (x, y) = towavue_runtime_windows::unobscured_window_point(
                window.as_ref(),
                target.window.as_ref()?.as_ref(),
                physical,
            )?;
            let density = target.ui_context.as_ref()?.pixels_per_point();
            Some((*key, egui::pos2(x as f32 / density, y as f32 / density)))
        })
    }

    fn merge_tab_drop(
        &mut self,
        source: WindowKey,
        request: &tab_transfer::DetachRequest,
        target: WindowKey,
        point: egui::Pos2,
    ) -> Result<TabId, String> {
        let gap = self
            .windows
            .get(&target)
            .and_then(|app| app.incoming_gap(point))
            .ok_or("drop on an available tab strip")?;
        self.move_tab(source, target, request, gap)
    }

    pub(super) fn update_tab_drops(&mut self, event_loop: &ActiveEventLoop, visible: bool) {
        self.update_tab_drops_with(event_loop, visible, Self::window_at_drop);
    }

    pub(super) fn update_tab_drops_with(
        &mut self,
        event_loop: &ActiveEventLoop,
        visible: bool,
        pick: impl Fn(&Self, WindowKey, egui::Pos2) -> Option<(WindowKey, egui::Pos2)>,
    ) {
        let pending: Vec<_> = self
            .windows
            .iter_mut()
            .filter_map(|(key, app)| app.pending_tab_drop.take().map(|request| (*key, request)))
            .collect();
        for (source, (request, point)) in pending {
            let result = if let Some((target, point)) = pick(self, source, point) {
                self.merge_tab_drop(source, &request, target, point)
                    .map(|_| {
                        if visible && let Some(window) = &self.windows[&target].window {
                            window.focus_window();
                        }
                    })
            } else {
                self.detach_tab(event_loop, source, &request, visible)
                    .map(|_| ())
            };
            if let Err(error) = result
                && let Some(app) = self.windows.get_mut(&source)
            {
                app.set_status(format!("Could not move tab: {error}"));
            }
        }
        let target = self.windows.iter().find_map(|(key, app)| {
            if !app.accepts_tab_drop() {
                return None;
            }
            let (tab, point) = tab_drag::active_pointer(app.ui_context.as_ref()?)?;
            app.tab_detach_request(tab).ok()?;
            let (target, point) = pick(self, *key, point)?;
            self.windows[&target].incoming_gap(point)?;
            Some((target, point))
        });
        for (key, app) in &mut self.windows {
            let pointer = target
                .filter(|(target, _)| target == key)
                .map(|(_, point)| point);
            if app.incoming_tab_pointer != pointer {
                app.incoming_tab_pointer = pointer;
                app.request_redraw();
            }
        }
    }
}
