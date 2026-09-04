use std::path::{Path, PathBuf};

use crate::MediaKind;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TabId(u64);

impl TabId {
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TabTarget {
    Media { path: PathBuf, kind: MediaKind },
    AudioFolder { folder: PathBuf, current: PathBuf },
}

impl TabTarget {
    pub fn current_path(&self) -> &Path {
        match self {
            Self::Media { path, .. } => path,
            Self::AudioFolder { current, .. } => current,
        }
    }

    pub fn media_kind(&self) -> MediaKind {
        match self {
            Self::Media { kind, .. } => *kind,
            Self::AudioFolder { .. } => MediaKind::Audio,
        }
    }

    pub fn set_current_path(&mut self, path: PathBuf, kind: MediaKind) {
        if kind == MediaKind::Audio {
            *self = Self::AudioFolder {
                folder: path.parent().unwrap_or_else(|| Path::new("")).to_owned(),
                current: path,
            };
        } else {
            *self = Self::Media { path, kind };
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tab {
    pub id: TabId,
    pub target: TabTarget,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TabSet {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next_id: u64,
}

impl TabSet {
    pub fn open_external(&mut self, path: PathBuf, kind: MediaKind) -> TabId {
        if kind == MediaKind::Audio {
            let folder = path.parent().unwrap_or_else(|| Path::new("")).to_owned();
            if let Some(tab) = self.tabs.iter_mut().find(|tab| {
                matches!(&tab.target, TabTarget::AudioFolder { folder: open, .. } if open == &folder)
            }) {
                tab.target = TabTarget::AudioFolder {
                    folder,
                    current: path,
                };
                self.active = Some(tab.id);
                return tab.id;
            }
        }

        self.open_new(path, kind)
    }

    pub fn open_new(&mut self, path: PathBuf, kind: MediaKind) -> TabId {
        let id = TabId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        let target = if kind == MediaKind::Audio {
            TabTarget::AudioFolder {
                folder: path.parent().unwrap_or_else(|| Path::new("")).to_owned(),
                current: path,
            }
        } else {
            TabTarget::Media { path, kind }
        };
        self.tabs.push(Tab { id, target });
        self.active = Some(id);
        id
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active(&self) -> Option<&Tab> {
        let active = self.active?;
        self.tabs.iter().find(|tab| tab.id == active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Tab> {
        let active = self.active?;
        self.tabs.iter_mut().find(|tab| tab.id == active)
    }

    pub fn activate(&mut self, id: TabId) -> bool {
        if self.tabs.iter().any(|tab| tab.id == id) {
            self.active = Some(id);
            true
        } else {
            false
        }
    }

    pub fn close(&mut self, id: TabId) -> Option<Tab> {
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        let removed = self.tabs.remove(index);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(index.min(self.tabs.len().saturating_sub(1)))
                .map(|tab| tab.id);
        }
        Some(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_audio_in_the_same_folder_reuses_its_playlist_tab() {
        let mut tabs = TabSet::default();
        let first = tabs.open_external(PathBuf::from("album/one.flac"), MediaKind::Audio);
        let second = tabs.open_external(PathBuf::from("album/two.flac"), MediaKind::Audio);

        assert_eq!(first, second);
        assert_eq!(tabs.tabs().len(), 1);
        assert_eq!(
            tabs.active().map(|tab| tab.target.current_path()),
            Some(Path::new("album/two.flac"))
        );
    }

    #[test]
    fn visual_media_open_in_distinct_tabs() {
        let mut tabs = TabSet::default();

        tabs.open_external(PathBuf::from("one.mp4"), MediaKind::Video);
        tabs.open_external(PathBuf::from("two.mp4"), MediaKind::Video);

        assert_eq!(tabs.tabs().len(), 2);
    }

    #[test]
    fn explicit_new_audio_tab_does_not_reuse_a_folder_playlist() {
        let mut tabs = TabSet::default();

        tabs.open_external(PathBuf::from("album/one.flac"), MediaKind::Audio);
        tabs.open_new(PathBuf::from("album/two.flac"), MediaKind::Audio);

        assert_eq!(tabs.tabs().len(), 2);
    }

    #[test]
    fn navigating_a_visual_tab_to_audio_establishes_a_folder_playlist() {
        let mut tabs = TabSet::default();
        tabs.open_external(PathBuf::from("album/cover.jpg"), MediaKind::Image);

        tabs.active_mut()
            .expect("active tab")
            .target
            .set_current_path(PathBuf::from("album/song.flac"), MediaKind::Audio);

        assert_eq!(
            tabs.active().map(|tab| &tab.target),
            Some(&TabTarget::AudioFolder {
                folder: PathBuf::from("album"),
                current: PathBuf::from("album/song.flac"),
            })
        );
    }
}
