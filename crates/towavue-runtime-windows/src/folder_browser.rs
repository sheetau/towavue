//! On-demand directory menus. No media probing, Shell views, or preview queues.
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::{Cancellation, LatestTask};

pub const FOLDER_BROWSER_LIMIT: usize = 4096;

#[derive(Debug)]
pub struct BrowserFolder {
    pub path: PathBuf,
    pub has_children: bool,
}

#[derive(Debug)]
pub struct FolderListing {
    pub path: PathBuf,
    pub folders: Vec<BrowserFolder>,
    pub truncated: bool,
    pub unavailable: bool,
}

#[derive(Default)]
struct State {
    revision: u64,
    requested: Option<PathBuf>,
    result: Option<Arc<FolderListing>>,
}

/// Created only when a directory menu is first opened. One latest request and
/// one result slot; cancellation/revision guards also reject A -> B -> A races.
pub struct FolderBrowser {
    worker: LatestTask,
    state: Arc<Mutex<State>>,
    notify: Arc<dyn Fn() + Send + Sync>,
}

impl FolderBrowser {
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> std::io::Result<Self> {
        Ok(Self {
            worker: LatestTask::new("towavue-folder-browser")?,
            state: Arc::default(),
            notify: Arc::new(notify),
        })
    }

    pub fn request(&mut self, path: Option<PathBuf>) {
        let revision = {
            let mut state = self.state.lock().expect("folder browser state");
            if state.requested == path {
                return;
            }
            state.revision = state.revision.wrapping_add(1);
            state.requested = path.clone();
            state.result = None;
            state.revision
        };
        let Some(path) = path else {
            self.worker.clear();
            return;
        };
        let state = Arc::clone(&self.state);
        let notify = Arc::clone(&self.notify);
        self.worker.submit(move |cancel| {
            let Some(result) = enumerate(path, &cancel) else {
                return;
            };
            let mut state = state.lock().expect("folder browser state");
            if state.revision != revision || cancel.is_cancelled() {
                return;
            }
            state.result = Some(Arc::new(result));
            drop(state);
            notify();
        });
    }

    pub fn result(&self) -> Option<Arc<FolderListing>> {
        self.state
            .lock()
            .expect("folder browser state")
            .result
            .clone()
    }
}

impl Drop for FolderBrowser {
    fn drop(&mut self) {
        self.request(None);
    }
}

fn directory(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    // Match the search worker's junction policy; explicit address entry remains
    // available, while nested menus never recurse through cycles/reparse points.
    entry
        .metadata()
        .is_ok_and(|metadata| metadata.is_dir() && metadata.file_attributes() & 0x400 == 0)
}

fn enumerate(path: PathBuf, cancel: &Cancellation) -> Option<FolderListing> {
    if cancel.is_cancelled() {
        return None;
    }
    let mut listing = FolderListing {
        path,
        folders: Vec::new(),
        truncated: false,
        unavailable: false,
    };
    let entries = match fs::read_dir(&listing.path) {
        Ok(entries) => entries,
        Err(_) => {
            listing.unavailable = true;
            return (!cancel.is_cancelled()).then_some(listing);
        }
    };
    for entry in entries {
        if cancel.is_cancelled() {
            return None;
        }
        let Ok(entry) = entry else { continue };
        if !directory(&entry) {
            continue;
        }
        if listing.folders.len() == FOLDER_BROWSER_LIMIT {
            listing.truncated = true;
            break;
        }
        let path = entry.path();
        let mut has_children = false;
        if let Ok(children) = fs::read_dir(&path) {
            for child in children {
                if cancel.is_cancelled() {
                    return None;
                }
                if child.is_ok_and(|entry| directory(&entry)) {
                    has_children = true;
                    break;
                }
            }
        }
        listing.folders.push(BrowserFolder { path, has_children });
    }
    listing
        .folders
        .sort_by(|a, b| crate::shell::natural_path_cmp(&a.path, &b.path));
    (!cancel.is_cancelled()).then_some(listing)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directories_include_empty_parents_and_lazy_children_without_media_probing() {
        let root = std::env::temp_dir().join(format!(
            "towavue-browser-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("folder10/empty/deep")).expect("nested directories");
        fs::create_dir_all(root.join("folder2")).expect("empty directory");
        fs::write(root.join("file.jpg"), b"not decoded").expect("media entry");
        let listing = enumerate(root.clone(), &Cancellation::default()).expect("listing");
        assert_eq!(listing.folders.len(), 2);
        assert_eq!(
            listing.folders[0].path.file_name().expect("name"),
            "folder2"
        );
        assert!(!listing.folders[0].has_children);
        assert!(listing.folders[1].has_children);
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(enumerate(root.clone(), &cancel).is_none());
        let (tx, rx) = std::sync::mpsc::channel();
        let mut worker = FolderBrowser::new(move || {
            let _ = tx.send(());
        })
        .expect("worker");
        worker.request(Some(root.clone()));
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("completion");
        assert_eq!(worker.result().expect("result").path, root);
        worker.request(None);
        assert!(worker.result().is_none());
        fs::remove_dir_all(root).expect("remove owned fixture");
    }
}
