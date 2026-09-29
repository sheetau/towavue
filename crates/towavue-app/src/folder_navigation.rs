use crate::{AppEvent, Application, FolderIntent};
use towavue_core::FolderNavigation;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn navigate_folder(&mut self, direction: FolderNavigation) {
        if self.modal_input_blocked() || !self.can_replace_active_tab() {
            return;
        }
        let Some(folder) = self.path.as_deref().and_then(std::path::Path::parent) else {
            return;
        };
        let generation = self
            .folder_order
            .request_navigation(folder.to_owned(), direction);
        self.pending_folder = Some((
            generation,
            FolderIntent::Related(self.tabs.active_id(), self.media_generation),
        ));
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests;
