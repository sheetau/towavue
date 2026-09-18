use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{MediaKind, ReadingSettings};

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

    /// Image-only Shell order, optionally reversed for reading without mutating
    /// the snapshot used by ordinary media navigation or the filmstrip.
    pub fn reading_sequence(
        &self,
        reversed: bool,
    ) -> impl DoubleEndedIterator<Item = &FolderMediaItem> + Clone {
        (0..self.items.len())
            .map(move |index| {
                &self.items[if reversed {
                    self.items.len() - 1 - index
                } else {
                    index
                }]
            })
            .filter(|item| item.kind == MediaKind::Image)
    }

    pub fn reading_items(
        &self,
        current_path: &Path,
        settings: ReadingSettings,
    ) -> Vec<&FolderMediaItem> {
        let images = self
            .reading_sequence(settings.folder_reversed)
            .collect::<Vec<_>>();
        let Some(current) = images.iter().position(|item| item.path == current_path) else {
            return Vec::new();
        };
        let range = settings.spread(current, images.len());
        let mut selected = images
            .into_iter()
            .skip(range.start)
            .take(range.len())
            .collect::<Vec<_>>();
        if settings.reversed {
            selected.reverse();
        }
        selected
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

    #[test]
    fn reading_items_follow_shell_order_and_can_reverse_visual_order() {
        let snapshot = FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![0]),
            folder_path: PathBuf::from("media"),
            items: vec![
                item("one.jpg", MediaKind::Image),
                item("clip.mp4", MediaKind::Video),
                item("two.png", MediaKind::Image),
                item("three.webp", MediaKind::Image),
            ],
            sort_columns: Vec::new(),
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: SystemTime::UNIX_EPOCH,
        };

        let paths = snapshot
            .reading_items(
                Path::new("one.jpg"),
                ReadingSettings {
                    reversed: true,
                    ..Default::default()
                },
            )
            .into_iter()
            .map(|item| item.path.as_path())
            .collect::<Vec<_>>();

        assert_eq!(paths, [Path::new("two.png"), Path::new("one.jpg")]);
    }

    fn item(path: &str, kind: MediaKind) -> FolderMediaItem {
        FolderMediaItem {
            identity: ShellIdentity::new(path.as_bytes().to_vec()),
            path: PathBuf::from(path),
            kind,
        }
    }

    #[test]
    fn reading_folder_reversal_partitions_from_the_new_start_independently_of_direction() {
        for total in 0..25 {
            let images: Vec<_> = (0..total)
                .map(|index| item(&format!("{}.png", 30 - index), MediaKind::Image))
                .collect();
            let snapshot = FolderSnapshot {
                folder_identity: ShellIdentity::new(vec![]),
                folder_path: "pages".into(),
                items: images
                    .iter()
                    .cloned()
                    .flat_map(|image| [image, item("sound.wav", MediaKind::Audio)])
                    .collect(),
                sort_columns: vec![],
                source: FolderSnapshotSource::LiveExplorerView,
                generation: 1,
                captured_at: SystemTime::UNIX_EPOCH,
            };
            let original = snapshot.items.clone();
            for folder_reversed in [false, true] {
                let mut expected = images.clone();
                if folder_reversed {
                    expected.reverse();
                }
                assert_eq!(
                    snapshot
                        .reading_sequence(folder_reversed)
                        .cloned()
                        .collect::<Vec<_>>(),
                    expected
                );
                for page_count in 2..=10 {
                    for first_page_count in 1..=page_count {
                        for reversed in [false, true] {
                            let settings = ReadingSettings {
                                page_count,
                                first_page_count,
                                folder_reversed,
                                reversed,
                                ..Default::default()
                            };
                            let mut start = 0;
                            while start < total {
                                let count = if start == 0 {
                                    first_page_count
                                } else {
                                    page_count
                                }
                                .min(total - start);
                                let end = start + count;
                                let mut spread = expected[start..end].iter().collect::<Vec<_>>();
                                if reversed {
                                    spread.reverse();
                                }
                                for entry in &expected[start..end] {
                                    assert_eq!(
                                        snapshot.reading_items(&entry.path, settings),
                                        spread
                                    );
                                }
                                assert_eq!(
                                    settings.adjacent_spread(start, total, true),
                                    (end < total).then_some(end)
                                );
                                if start > 0 {
                                    let previous = settings
                                        .adjacent_spread(start, total, false)
                                        .expect("previous");
                                    assert_eq!(
                                        settings.adjacent_spread(previous, total, true),
                                        Some(start)
                                    );
                                }
                                start = end;
                            }
                        }
                    }
                }
            }
            assert_eq!(
                snapshot.items, original,
                "reading never mutates Shell order"
            );
        }
    }
}
