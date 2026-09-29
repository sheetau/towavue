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
        } else if let Ok(folder) = result.target {
            // Reuse Open folder's enumeration, empty result, and replacement guards.
            // Discovery has already stopped at one supported file; it never builds a listing.
            self.open_folder_path(folder);
            if let Some((_, intent)) = &mut self.pending_folder {
                *intent = FolderIntent::OpenReplacing(owner.0, owner.1, result.used_name_fallback);
            }
        } else {
            use towavue_core::localization::formatted;
            use towavue_runtime_windows::FolderNavigationFailure::*;
            let language = self.language();
            let folder = folder_name(&result.searched_folder);
            let message = match result.target.expect_err("failed navigation") {
                NoParent => formatted::no_parent_folder(language),
                NoChild => formatted::no_child_folder(language, &folder),
                NoSibling => formatted::no_sibling_folder(language),
                NoMedia => formatted::no_folder_media(language, &folder),
                NoChildMedia => formatted::no_child_folder_media(language, &folder),
                NoSiblingMedia => formatted::no_sibling_folder_media(language, &folder),
                Unavailable => formatted::folder_navigation_unavailable(language, &folder),
            };
            self.set_status(message);
            self.refresh_folder_snapshot();
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
        self.clear_status();
        self.request_redraw();
    }
}

pub(super) fn folder_name(folder: &std::path::Path) -> String {
    use std::path::{Component, Prefix};
    if let Some(name) = folder.file_name() {
        return name.to_string_lossy().into_owned();
    }
    // Roots have no file_name. Show just the drive or share, never its ancestry.
    for component in folder.components() {
        if let Component::Prefix(prefix) = component {
            return match prefix.kind() {
                Prefix::UNC(_, share) | Prefix::VerbatimUNC(_, share) => {
                    share.to_string_lossy().into_owned()
                }
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                    format!("{}:", char::from(drive))
                }
                _ => prefix.as_os_str().to_string_lossy().into_owned(),
            };
        }
    }
    folder.as_os_str().to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests;
