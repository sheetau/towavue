use super::*;

#[cfg(test)]
#[path = "window_drop_tests.rs"]
pub(super) mod tests;

#[derive(Clone, Copy, Debug, PartialEq)]
struct DragFeedback {
    source: WindowKey,
    target: Option<(WindowKey, egui::Pos2)>,
    cursor: egui::CursorIcon,
}

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

    pub(crate) fn request_tab_drop(&mut self, id: TabId, point: egui::Pos2, anchor: egui::Vec2) {
        if !self.hosted_graphics {
            self.request_guarded(GuardedAction::DetachTab(id));
            return;
        }
        if !self.accepts_tab_drop() || !point.is_finite() || !anchor.is_finite() {
            return;
        }
        match self.tab_detach_request(id) {
            Ok(request) => self.pending_tab_drop = Some((request, point, anchor)),
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
    fn tab_drag_feedback(
        &self,
        pick: impl Fn(&Self, WindowKey, egui::Pos2) -> Option<(WindowKey, egui::Pos2)>,
    ) -> Option<DragFeedback> {
        self.windows.iter().find_map(|(key, app)| {
            if !app.accepts_tab_drop() {
                return None;
            }
            let context = app.ui_context.as_ref()?;
            let (tab, point, local_drop) = tab_drag::active_pointer(
                context,
                (
                    app.tabs.active().map(|tab| tab.id),
                    app.media_generation,
                    app.graphics_epoch,
                ),
            )?;
            let mut feedback = DragFeedback {
                source: *key,
                target: None,
                cursor: egui::CursorIcon::NoDrop,
            };
            if local_drop {
                feedback.cursor = egui::CursorIcon::Move;
            } else if !context.content_rect().contains(point) && app.tab_detach_request(tab).is_ok()
            {
                if let Some((target, point)) = pick(self, *key, point) {
                    if self
                        .windows
                        .get(&target)
                        .and_then(|app| app.incoming_gap(point))
                        .is_some()
                    {
                        feedback.target = Some((target, point));
                        feedback.cursor = egui::CursorIcon::Move;
                    }
                } else {
                    feedback.cursor = egui::CursorIcon::Move;
                }
            }
            Some(feedback)
        })
    }

    fn update_tab_cursor_with(
        &mut self,
        feedback: Option<DragFeedback>,
        mut apply: impl FnMut(&WindowApplication, egui::CursorIcon),
    ) {
        let source = feedback.map(|feedback| feedback.source);
        if self.tab_cursor_owner != source
            && let Some(previous) = self.tab_cursor_owner.and_then(|key| self.windows.get(&key))
        {
            apply(previous, previous.platform_cursor);
        }
        self.tab_cursor_owner = source;
        if let Some(feedback) = feedback
            && let Some(app) = self.windows.get(&feedback.source)
        {
            // Reapply after rendering/native mouse messages, even if the effect did not change.
            apply(app, feedback.cursor);
        }
    }

    pub(super) fn source_client_position(
        &self,
        source: WindowKey,
        point: egui::Pos2,
    ) -> Result<winit::dpi::PhysicalPosition<i32>, String> {
        let app = self.windows.get(&source).ok_or("source window is closed")?;
        let window = app.window.as_ref().ok_or("source window is not ready")?;
        let origin = window.inner_position().map_err(|error| error.to_string())?;
        let density = app
            .ui_context
            .as_ref()
            .ok_or("source UI is not ready")?
            .pixels_per_point();
        Ok(winit::dpi::PhysicalPosition::new(
            origin.x + (point.x * density).round() as i32,
            origin.y + (point.y * density).round() as i32,
        ))
    }

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
        for (source, (request, point, anchor)) in pending {
            let result = if let Some((target, point)) = pick(self, source, point) {
                self.merge_tab_drop(source, &request, target, point)
                    .map(|_| {
                        if visible && let Some(window) = &self.windows[&target].window {
                            window.focus_window();
                        }
                    })
            } else {
                self.source_client_position(source, point)
                    .and_then(|position| {
                        self.detach_tab(event_loop, source, &request, visible, position, anchor)
                    })
                    .map(|_| ())
            };
            if let Err(error) = result
                && let Some(app) = self.windows.get_mut(&source)
            {
                app.set_status(format!("Could not move tab: {error}"));
            }
        }
        let feedback = self.tab_drag_feedback(pick);
        let target = feedback.and_then(|feedback| feedback.target);
        for (key, app) in &mut self.windows {
            let pointer = target
                .filter(|(target, _)| target == key)
                .map(|(_, point)| point);
            if app.incoming_tab_pointer != pointer {
                app.incoming_tab_pointer = pointer;
                app.request_redraw();
            }
        }
        self.update_tab_cursor_with(feedback, |app, cursor| {
            if visible && let Some(window) = &app.window {
                cursor::set_native(window, cursor);
            }
        });
    }
}
