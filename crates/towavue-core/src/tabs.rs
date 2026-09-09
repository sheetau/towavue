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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ActiveTab {
    #[default]
    Welcome,
    Media(TabId),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TabSet {
    tabs: Vec<Tab>,
    active: ActiveTab,
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
                self.active = ActiveTab::Media(tab.id);
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
        self.active = ActiveTab::Media(id);
        id
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active(&self) -> Option<&Tab> {
        let ActiveTab::Media(active) = self.active else {
            return None;
        };
        self.tabs.iter().find(|tab| tab.id == active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Tab> {
        let ActiveTab::Media(active) = self.active else {
            return None;
        };
        self.tabs.iter_mut().find(|tab| tab.id == active)
    }

    pub fn activate(&mut self, id: TabId) -> bool {
        if self.welcome() == Some(id) {
            return true;
        }
        if self.tabs.iter().any(|tab| tab.id == id) {
            self.active = ActiveTab::Media(id);
            true
        } else {
            false
        }
    }

    pub fn close(&mut self, id: TabId) -> Option<Tab> {
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        let removed = self.tabs.remove(index);
        if self.active == ActiveTab::Media(id) {
            self.active = self
                .tabs
                .get(index.min(self.tabs.len().saturating_sub(1)))
                .map_or(ActiveTab::Welcome, |tab| ActiveTab::Media(tab.id));
        }
        Some(removed)
    }

    /// Move a tab to a gap in the current ordering; identity and selection stay unchanged.
    pub fn reorder(&mut self, id: TabId, gap: usize) -> bool {
        let Some(from) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        if gap > self.tabs.len() {
            return false;
        }
        let to = gap - usize::from(from < gap);
        if from == to {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        true
    }

    /// Media accessors exclude this non-media tab, which exists whenever no files are open.
    pub fn welcome(&self) -> Option<TabId> {
        (self.active == ActiveTab::Welcome).then_some(TabId(u64::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_identity_replaces_empty_selection_and_survives_close_cycles() {
        let mut tabs = TabSet::default();
        let welcome = tabs.welcome().expect("initial Welcome tab");
        assert!(tabs.activate(welcome));
        assert!(tabs.close(welcome).is_none());
        for _ in 0..3 {
            let first = tabs.open_new("a.png".into(), MediaKind::Image);
            let second = tabs.open_new("b.mp4".into(), MediaKind::Video);
            assert!(tabs.welcome().is_none());
            assert!(!tabs.activate(welcome));
            tabs.close(first);
            assert_eq!(tabs.active().expect("media remains").id, second);
            tabs.close(second);
            assert_eq!(tabs.welcome(), Some(welcome));
            assert!(tabs.active().is_none());
        }
    }

    #[test]
    fn reordering_preserves_identity_active_tab_and_targets() {
        let mut tabs = TabSet::default();
        let a = tabs.open_new("a.png".into(), MediaKind::Image);
        let b = tabs.open_new("b.mp4".into(), MediaKind::Video);
        let c = tabs.open_new("album/c.flac".into(), MediaKind::Audio);
        let original = tabs.clone();
        assert!(tabs.reorder(a, 3));
        assert_eq!(
            tabs.tabs().iter().map(|tab| tab.id).collect::<Vec<_>>(),
            [b, c, a]
        );
        assert_eq!(tabs.active(), original.active());
        assert!(tabs.reorder(a, 0));
        assert_eq!(tabs, original);
        assert!(!tabs.reorder(a, 0));
        assert!(!tabs.reorder(a, 1));
        assert!(!tabs.reorder(a, 4));
        assert!(!tabs.reorder(TabId(99), 0));
        assert_eq!(tabs, original);
    }

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
