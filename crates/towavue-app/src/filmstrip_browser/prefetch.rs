use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use towavue_runtime_windows::FolderListing;

const PREPARE_LIMIT: usize = 4096;

/// A bounded breadth-first queue, serviced by the existing cancellable Shell
/// worker. Explicitly requested branches can preempt background preparation.
#[derive(Default)]
pub(super) struct Prefetch {
    active: Option<(PathBuf, usize)>,
    pending: VecDeque<(PathBuf, usize)>,
    seen: HashSet<PathBuf>,
    known: HashMap<PathBuf, bool>,
    known_order: VecDeque<PathBuf>,
}

impl Prefetch {
    pub fn children(&self, path: &Path) -> Option<bool> {
        self.known.get(path).copied()
    }

    pub fn prioritize(&mut self, path: &Path) {
        if self
            .active
            .as_ref()
            .is_some_and(|(active, _)| active == path)
        {
            return;
        }
        let requested = self
            .pending
            .iter()
            .position(|(pending, _)| pending == path)
            .and_then(|index| self.pending.remove(index))
            .unwrap_or_else(|| (path.to_owned(), 0));
        if let Some(active) = self.active.take() {
            if self.pending.len() == PREPARE_LIMIT {
                self.pending.pop_back();
            }
            self.pending.push_front(active);
        }
        self.active = Some(requested);
        if self.seen.len() < PREPARE_LIMIT {
            self.seen.insert(path.to_owned());
        }
    }

    pub fn complete(&mut self, listing: &FolderListing) -> bool {
        let Some((path, depth)) = &self.active else {
            return false;
        };
        if *path != listing.path {
            return false;
        }
        let depth = *depth;
        self.active = None;
        if !listing.unavailable {
            self.remember(&listing.path, !listing.folders.is_empty());
        } else {
            self.known.remove(&listing.path);
            self.known_order.retain(|path| *path != listing.path);
        }
        // Parent enumeration already proves leaf membership. Do not construct a
        // hidden Shell view merely to enumerate the same empty leaf again.
        if depth < 63 {
            for folder in &listing.folders {
                if let Some(children) = folder.has_children {
                    self.remember(&folder.path, children);
                }
                if folder.has_children != Some(false)
                    && self.seen.len() < PREPARE_LIMIT
                    && self.seen.insert(folder.path.clone())
                {
                    self.pending.push_back((folder.path.clone(), depth + 1));
                }
            }
        }
        true
    }

    fn remember(&mut self, path: &Path, children: bool) {
        self.known.remove(path);
        self.known_order.retain(|saved| saved != path);
        if self.known.len() == PREPARE_LIMIT {
            let oldest = self.known_order.pop_front().expect("known folder order");
            self.known.remove(&oldest);
        }
        self.known.insert(path.to_owned(), children);
        self.known_order.push_back(path.to_owned());
    }

    pub fn request(&mut self) -> Option<&Path> {
        if self.active.is_none() {
            self.active = self.pending.pop_front();
        }
        self.active.as_ref().map(|(path, _)| path.as_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_runtime_windows::BrowserFolder;

    fn listing(path: &str, children: &[&str]) -> FolderListing {
        FolderListing {
            path: path.into(),
            folders: children
                .iter()
                .map(|path| BrowserFolder {
                    path: (*path).into(),
                    has_children: None,
                })
                .collect(),
            unavailable: false,
            truncated: false,
        }
    }

    #[test]
    fn proven_leaves_are_known_without_scheduling_another_shell_view() {
        let mut work = Prefetch::default();
        work.prioritize(Path::new("root"));
        let mut root = listing("root", &["root/leaf", "root/branch"]);
        root.folders[0].has_children = Some(false);
        root.folders[1].has_children = Some(true);
        assert!(work.complete(&root));
        assert_eq!(work.children(Path::new("root/leaf")), Some(false));
        assert_eq!(work.children(Path::new("root/branch")), Some(true));
        assert_eq!(work.request(), Some(Path::new("root/branch")));
        assert!(work.complete(&listing("root/branch", &[])));
        assert_eq!(work.children(Path::new("root/branch")), Some(false));
        assert_eq!(work.request(), None);
    }

    #[test]
    fn preparation_is_parent_first_without_speculative_leaf_arrows_or_stale_delivery() {
        let mut work = Prefetch::default();
        work.prioritize(Path::new("root"));
        assert!(work.complete(&listing("root", &["root/a", "root/b"])));
        assert_eq!(work.children(Path::new("root/a")), None);
        assert_eq!(work.request(), Some(Path::new("root/a")));
        assert!(work.complete(&listing("root/a", &["root/a/deep"])));
        assert_eq!(work.request(), Some(Path::new("root/b")));
        assert!(work.complete(&listing("root/b", &[])));
        assert_eq!(work.children(Path::new("root/b")), Some(false));
        assert_eq!(work.children(Path::new("root/a")), Some(true));
        assert_eq!(work.request(), Some(Path::new("root/a/deep")));
        work.prioritize(Path::new("other"));
        assert!(!work.complete(&listing("root/a/deep", &[])));
        assert_eq!(work.request(), Some(Path::new("other")));
        assert!(work.complete(&listing("other", &[])));
        assert_eq!(work.request(), Some(Path::new("root/a/deep")));
        assert!(work.complete(&listing("root/a/deep", &[])));
        assert_eq!(work.request(), None);
    }

    #[test]
    fn automatic_work_is_bounded_but_explicit_branches_remain_available() {
        let mut work = Prefetch::default();
        work.prioritize(Path::new("root"));
        let mut wide = listing("root", &[]);
        wide.folders = (0..PREPARE_LIMIT + 20)
            .map(|index| BrowserFolder {
                path: format!("root/{index}").into(),
                has_children: None,
            })
            .collect();
        assert!(work.complete(&wide));
        assert_eq!(work.seen.len(), PREPARE_LIMIT);
        assert_eq!(work.pending.len(), PREPARE_LIMIT - 1);
        work.prioritize(Path::new("root/4097"));
        assert_eq!(work.request(), Some(Path::new("root/4097")));
        assert!(work.complete(&listing("root/4097", &["root/4097/child"])));
        assert_eq!(work.children(Path::new("root/4097")), Some(true));
        assert!(work.pending.len() < PREPARE_LIMIT);
        work = Prefetch::default();
        assert!(work.request().is_none());
        assert!(!work.complete(&listing("root", &[])));
    }
}
