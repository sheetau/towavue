use crate::{AppEvent, Application, FolderIntent};
use towavue_core::FolderNavigation;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn finish_folder_navigation(
        &mut self,
        result: towavue_runtime_windows::FolderNavigationResult,
    ) {
        let Some((generation, FolderIntent::Related(tab, media))) = &self.pending_folder else {
            return;
        };
        if *generation != result.generation {
            return;
        }
        let owner = (*tab, *media);
        self.pending_folder = None;
        if owner != (self.tabs.active_id(), self.media_generation) {
            self.refresh_folder_snapshot();
        } else if let Some(folder) = result.target {
            // Reuse Open folder's enumeration, empty result, and replacement guards.
            // Discovery has already stopped at one supported file; it never builds a listing.
            self.open_folder_path(folder);
            if let Some((_, intent)) = &mut self.pending_folder {
                *intent = FolderIntent::OpenReplacing(owner.0, owner.1);
            }
        } else {
            self.finish_empty_folder(&result.searched_folder);
        }
        self.request_redraw();
    }

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
