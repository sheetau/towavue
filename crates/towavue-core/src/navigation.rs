use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::MediaKind;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ShellIdentity(Vec<u8>);

impl ShellIdentity {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PropertyKey {
    pub format_id: u128,
    pub property_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SortColumn {
    pub property: PropertyKey,
    pub direction: SortDirection,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FolderSnapshotSource {
    LiveExplorerView,
    PersistedShellView,
    NaturalNameFallback,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderMediaItem {
    pub identity: ShellIdentity,
    pub path: PathBuf,
    pub kind: MediaKind,
}

#[derive(Clone, Debug)]
pub struct FolderSnapshot {
    pub folder_identity: ShellIdentity,
    pub folder_path: PathBuf,
    pub items: Vec<FolderMediaItem>,
    pub sort_columns: Vec<SortColumn>,
    pub source: FolderSnapshotSource,
    pub generation: u64,
    pub captured_at: SystemTime,
}

impl FolderSnapshot {
    pub fn item_index(&self, path: &Path) -> Option<usize> {
        self.items.iter().position(|item| item.path == path)
    }

    pub fn items_of_kind(&self, kind: MediaKind) -> impl Iterator<Item = &FolderMediaItem> {
        self.items.iter().filter(move |item| item.kind == kind)
    }

    pub fn match_item(
        &self,
        identity: Option<&ShellIdentity>,
        path: &Path,
    ) -> Option<&FolderMediaItem> {
        identity
            .and_then(|identity| self.items.iter().find(|item| &item.identity == identity))
            .or_else(|| self.items.iter().find(|item| item.path == path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_filters_preserve_shell_view_order() {
        let snapshot = FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![0]),
            folder_path: PathBuf::from("media"),
            items: vec![
                item("second.mp4", MediaKind::Video),
                item("first.jpg", MediaKind::Image),
                item("third.mkv", MediaKind::Video),
            ],
            sort_columns: Vec::new(),
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: SystemTime::UNIX_EPOCH,
        };

        let videos = snapshot
            .items_of_kind(MediaKind::Video)
            .map(|item| item.path.as_path())
            .collect::<Vec<_>>();

        assert_eq!(videos, [Path::new("second.mp4"), Path::new("third.mkv")]);
    }

    #[test]
    fn shell_identity_remaps_an_item_before_its_old_path() {
        let mut snapshot = FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![0]),
            folder_path: PathBuf::from("media"),
            items: vec![item("renamed.jpg", MediaKind::Image)],
            sort_columns: Vec::new(),
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 2,
            captured_at: SystemTime::UNIX_EPOCH,
        };
        snapshot.items[0].identity = ShellIdentity::new(vec![42]);

        assert_eq!(
            snapshot
                .match_item(Some(&ShellIdentity::new(vec![42])), Path::new("old.jpg"))
                .map(|item| item.path.as_path()),
            Some(Path::new("renamed.jpg"))
        );
    }

    fn item(path: &str, kind: MediaKind) -> FolderMediaItem {
        FolderMediaItem {
            identity: ShellIdentity::new(path.as_bytes().to_vec()),
            path: PathBuf::from(path),
            kind,
        }
    }
}
