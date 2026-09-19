use crate::*;
use towavue_runtime_windows::MediaInput;

#[derive(Clone)]
pub(super) struct DeletedSource {
    pub path: PathBuf,
    pub before: Arc<FolderSnapshot>,
}

impl DeletedSource {
    // Document navigation and the active tab's filmstrip include this held
    // position. The shared disk listing remains the actual Shell snapshot.
    pub fn navigation_snapshot(&self, current: &FolderSnapshot) -> FolderSnapshot {
        let mut result = current.clone();
        result.items.retain(|item| item.path != self.path);
        let Some(index) = self
            .before
            .items
            .iter()
            .position(|item| item.path == self.path)
        else {
            return result;
        };
        let next = self.before.items[index + 1..]
            .iter()
            .find_map(|old| result.items.iter().position(|item| item.path == old.path));
        let previous = self.before.items[..index].iter().rev().find_map(|old| {
            result
                .items
                .iter()
                .position(|item| item.path == old.path)
                .map(|i| i + 1)
        });
        let slot = next.or(previous).unwrap_or(index.min(result.items.len()));
        result.items.insert(slot, self.before.items[index].clone());
        result
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn current_source_deleted(&self) -> bool {
        self.displayed_tab
            .is_some_and(|id| self.deleted_sources.contains_key(&id))
    }

    pub(super) fn deleted_path_prefix(&self) -> &'static str {
        if self.current_source_deleted() {
            "(deleted) "
        } else {
            ""
        }
    }

    pub(super) fn reading_snapshot(&self) -> Option<&FolderSnapshot> {
        self.displayed_tab
            .and_then(|id| self.deleted_sources.get(&id))
            .map(|deleted| deleted.before.as_ref())
            .or(self.folder_snapshot.as_ref())
    }

    pub(super) fn navigation_snapshot(&self) -> Option<std::borrow::Cow<'_, FolderSnapshot>> {
        let deleted = self
            .displayed_tab
            .and_then(|id| self.deleted_sources.get(&id));
        match (self.folder_snapshot.as_ref(), deleted) {
            (Some(current), Some(deleted)) if current.folder_path == deleted.before.folder_path => {
                Some(std::borrow::Cow::Owned(
                    deleted.navigation_snapshot(current),
                ))
            }
            (_, Some(deleted)) => Some(std::borrow::Cow::Borrowed(&deleted.before)),
            (current, None) => current.map(std::borrow::Cow::Borrowed),
        }
    }

    pub(super) fn media_input_for(&self, id: Option<TabId>, path: &Path) -> MediaInput {
        let original = id
            .filter(|id| {
                self.tabs
                    .tabs()
                    .iter()
                    .any(|tab| tab.id == *id && tab.target.current_path() == path)
            })
            .and_then(|id| self.source_backings.get(&id))
            .cloned();
        original.map_or_else(
            || MediaInput::new(path.to_owned()),
            |original| MediaInput::retained(path.to_owned(), original),
        )
    }
    pub(super) fn media_input(&self, path: &Path) -> MediaInput {
        self.media_input_for(self.displayed_tab, path)
    }
    pub(super) fn media_input_for_instance(&self, path: &Path, instance: u64) -> MediaInput {
        let id = if instance == self.media_generation && self.path.as_deref() == Some(path) {
            self.displayed_tab
        } else {
            self.retained_playback.iter().find_map(|(id, saved)| {
                (saved.instance == instance && saved.path == path).then_some(*id)
            })
        };
        self.media_input_for(id, path)
    }
}

#[cfg(test)]
mod tests;
