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
    Empty,
    Gallery(TabId),
    Media(TabId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabSet {
    tabs: Vec<Tab>,
    gallery: Option<(TabId, usize)>,
    active: ActiveTab,
    next_id: u64,
}

impl Default for TabSet {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            gallery: Some((TabId(0), 0)),
            active: ActiveTab::Gallery(TabId(0)),
            next_id: 1,
        }
    }
}

impl TabSet {
    /// Gallery participates in tab ordering without acquiring media-only state.
    pub fn gallery(&self) -> Option<TabId> {
        self.gallery.map(|(id, _)| id)
    }

    pub fn open_gallery(&mut self) -> TabId {
        let id = self.gallery().unwrap_or_else(|| {
            let id = TabId(self.next_id);
            self.next_id = self.next_id.wrapping_add(1);
            self.gallery = Some((id, self.tabs.len()));
            id
        });
        self.active = ActiveTab::Gallery(id);
        id
    }

    pub fn active_id(&self) -> Option<TabId> {
        match self.active {
            ActiveTab::Empty => None,
            ActiveTab::Gallery(id) | ActiveTab::Media(id) => Some(id),
        }
    }

    pub fn len(&self) -> usize {
        self.tabs.len() + usize::from(self.gallery.is_some())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn tab_ids(&self) -> impl Iterator<Item = TabId> + '_ {
        (0..self.len()).map(|index| match self.gallery {
            Some((id, position)) if index == position => id,
            Some((_, position)) if index > position => self.tabs[index - 1].id,
            _ => self.tabs[index].id,
        })
    }

    pub fn can_close(&self, id: TabId) -> bool {
        if self.gallery() == Some(id) {
            self.len() > 1
        } else {
            self.tabs.iter().any(|tab| tab.id == id)
        }
    }

    pub fn close_gallery(&mut self, id: TabId) -> bool {
        if self.gallery() != Some(id) || !self.can_close(id) {
            return false;
        }
        self.take_gallery(id)
    }

    pub fn take_gallery(&mut self, id: TabId) -> bool {
        if self.gallery() != Some(id) {
            return false;
        }
        let (_, index) = self.gallery.take().expect("checked Gallery");
        if self.active == ActiveTab::Gallery(id) {
            self.active = self
                .tabs
                .get(index.min(self.tabs.len().saturating_sub(1)))
                .map_or(ActiveTab::Empty, |tab| ActiveTab::Media(tab.id));
        }
        true
    }

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

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
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
        if self.gallery() == Some(id) {
            self.active = ActiveTab::Gallery(id);
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
        let removed = self.take(id)?;
        if self.is_empty() {
            self.open_gallery();
        }
        Some(removed)
    }

    /// Transfer removes a media tab without manufacturing a replacement Gallery.
    pub fn take(&mut self, id: TabId) -> Option<Tab> {
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        let position = self
            .tab_ids()
            .position(|item| item == id)
            .expect("media tab");
        let removed = self.tabs.remove(index);
        if let Some((_, gallery_index)) = &mut self.gallery
            && index < *gallery_index
        {
            *gallery_index -= 1;
        }
        if self.active == ActiveTab::Media(id) {
            let next = self
                .tab_ids()
                .nth(position.min(self.len().saturating_sub(1)));
            self.active = ActiveTab::Empty;
            if let Some(next) = next {
                self.activate(next);
            }
        }
        Some(removed)
    }

    /// Move a tab to a gap in the current ordering; identity and selection stay unchanged.
    pub fn reorder(&mut self, id: TabId, gap: usize) -> bool {
        let Some(from) = self.tab_ids().position(|item| item == id) else {
            return false;
        };
        if gap > self.len() {
            return false;
        }
        let to = gap - usize::from(from < gap);
        if from == to {
            return false;
        }
        if let Some((gallery, position)) = &mut self.gallery {
            if *gallery == id {
                *position = to;
                return true;
            }
            let media_from = from - usize::from(from > *position);
            *position -= usize::from(from < *position);
            let media_to = to - usize::from(to > *position);
            *position += usize::from(to <= *position);
            let tab = self.tabs.remove(media_from);
            self.tabs.insert(media_to, tab);
        } else {
            let tab = self.tabs.remove(from);
            self.tabs.insert(to, tab);
        }
        true
    }

    /// Media accessors exclude Gallery, even while it is selected.
    pub fn welcome(&self) -> Option<TabId> {
        match self.active {
            ActiveTab::Gallery(id) => Some(id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_survives_media_opening_and_can_close_except_when_alone() {
        let mut tabs = TabSet::default();
        let gallery = tabs.gallery().expect("initial Gallery tab");
        assert!(tabs.activate(gallery));
        assert!(!tabs.can_close(gallery));
        assert!(!tabs.close_gallery(gallery));
        for _ in 0..3 {
            let first = tabs.open_new("a.png".into(), MediaKind::Image);
            let second = tabs.open_new("b.mp4".into(), MediaKind::Video);
            assert_eq!(tabs.gallery(), Some(gallery));
            assert_eq!(tabs.tab_ids().collect::<Vec<_>>(), [gallery, first, second]);
            assert!(tabs.activate(gallery));
            assert!(tabs.active().is_none());
            assert!(tabs.activate(second));
            tabs.close(first);
            assert_eq!(tabs.active().expect("media remains").id, second);
            tabs.close(second);
            assert_eq!(tabs.active_id(), Some(gallery));
            assert!(tabs.active().is_none());
        }
        let media = tabs.open_new("a.png".into(), MediaKind::Image);
        assert!(tabs.activate(gallery));
        assert!(tabs.close_gallery(gallery));
        assert_eq!(tabs.active_id(), Some(media));
        assert!(tabs.gallery().is_none());
        tabs.close(media);
        let replacement = tabs.gallery().expect("last close restores Gallery");
        assert_ne!(
            replacement, gallery,
            "stale actions cannot target a new tab"
        );
        assert_eq!(tabs.open_gallery(), replacement);
        assert_eq!(tabs.len(), 1);
    }

    #[test]
    fn every_gallery_and_media_reorder_matches_a_single_ordered_list() {
        let mut tabs = TabSet::default();
        let gallery = tabs.gallery().expect("Gallery");
        for path in ["a.png", "b.png", "c.png"] {
            tabs.open_new(path.into(), MediaKind::Image);
        }
        for gallery_gap in 0..=tabs.len() {
            let mut positioned = tabs.clone();
            positioned.reorder(gallery, gallery_gap);
            let original: Vec<_> = positioned.tab_ids().collect();
            for (from, id) in original.iter().copied().enumerate() {
                for gap in 0..=original.len() {
                    let mut changed = positioned.clone();
                    let mut expected = original.clone();
                    expected.remove(from);
                    expected.insert(gap - usize::from(from < gap), id);
                    assert_eq!(changed.reorder(id, gap), expected != original);
                    assert_eq!(changed.tab_ids().collect::<Vec<_>>(), expected);
                    assert_eq!(changed.active_id(), positioned.active_id());
                    assert!(!changed.reorder(id, original.len() + 1));
                }
            }
        }
    }

    #[test]
    fn transferring_the_last_media_leaves_no_replacement_tab() {
        let mut tabs = TabSet::default();
        let gallery = tabs.gallery().expect("Gallery");
        let media = tabs.open_new("a.png".into(), MediaKind::Image);
        tabs.close_gallery(gallery);
        assert_eq!(tabs.take(media).expect("transferred").id, media);
        assert!(tabs.is_empty());
        assert_eq!(tabs.active_id(), None);
        assert_eq!(tabs.gallery(), None);
    }

    #[test]
    fn reordering_preserves_identity_active_tab_and_targets() {
        let mut tabs = TabSet::default();
        let a = tabs.open_new("a.png".into(), MediaKind::Image);
        let b = tabs.open_new("b.mp4".into(), MediaKind::Video);
        let c = tabs.open_new("album/c.flac".into(), MediaKind::Audio);
        tabs.close_gallery(tabs.gallery().expect("Gallery"));
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
