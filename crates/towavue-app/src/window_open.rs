use super::*;

#[derive(Clone)]
pub(super) struct Request {
    pub path: PathBuf,
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

    pub(super) fn request_filmstrip_window(&mut self, path: PathBuf, generation: u64) {
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
}
