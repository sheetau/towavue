use crate::*;
use towavue_runtime_windows::MediaInput;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
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
