use super::{RecentEntry, RecentKind, trim_pending};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct Pending {
    pub opens: Vec<RecentEntry>,
    pub removals: HashSet<(PathBuf, RecentKind)>,
    pub folder_visibility: HashMap<PathBuf, bool>,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.opens.is_empty() && self.removals.is_empty() && self.folder_visibility.is_empty()
    }

    pub fn record(&mut self, entry: RecentEntry) {
        self.removals
            .remove(&(entry.path.clone(), RecentKind::File));
        self.removals
            .remove(&(entry.path.clone(), RecentKind::Folder));
        self.opens.retain(|old| old.path != entry.path);
        let folder = match entry.kind {
            RecentKind::File => entry.path.parent().map(|path| path.to_path_buf()),
            RecentKind::Folder => Some(entry.path.clone()),
        };
        if let Some(folder) = folder {
            self.set_folder(folder, true);
        }
        self.opens.push(entry);
        trim_pending(&mut self.opens);
    }

    pub fn remove(&mut self, path: PathBuf, kind: RecentKind) {
        self.opens
            .retain(|old| old.path != path || old.kind != kind);
        self.removals.insert((path.clone(), kind));
        if kind == RecentKind::Folder {
            self.set_folder(path, false);
        }
    }

    fn set_folder(&mut self, path: PathBuf, visible: bool) {
        self.folder_visibility.insert(path, visible);
    }

    pub fn merge(&mut self, pending: Self) {
        let opens: HashSet<_> = pending
            .opens
            .iter()
            .map(|entry| entry.path.as_path())
            .collect();
        let removals: HashSet<_> = pending
            .removals
            .iter()
            .map(|(path, kind)| (path.as_path(), *kind))
            .collect();
        self.opens.retain(|old| {
            !opens.contains(old.path.as_path())
                && !removals.contains(&(old.path.as_path(), old.kind))
        });
        self.removals
            .retain(|(path, _)| !opens.contains(path.as_path()));
        self.opens.extend(pending.opens);
        self.removals.extend(pending.removals);
        trim_pending(&mut self.opens);
        // Folder effects survive a later removal of the file that caused the visit.
        self.folder_visibility.extend(pending.folder_visibility);
    }
}
