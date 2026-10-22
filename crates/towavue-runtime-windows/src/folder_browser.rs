//! On-demand directory menus using the media browser's cancellable Shell STA.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
    requested: Option<PathBuf>,
    result: Option<Arc<FolderListing>>,
}

/// Lazily created by the app. The shared Shell provider owns generation checks,
/// COM apartment lifetime and nonblocking retirement if a Shell extension stalls.
pub struct FolderBrowser {
    worker: crate::FolderOrderProvider,
    state: Mutex<State>,
}

impl FolderBrowser {
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> std::io::Result<Self> {
        Ok(Self {
            worker: crate::FolderOrderProvider::with_notify(notify)
                .map_err(std::io::Error::other)?,
            state: Mutex::default(),
        })
    }

    pub fn request(&mut self, path: Option<PathBuf>) {
        let mut state = self.state.lock().expect("folder browser state");
        if state.requested == path {
            return;
        }
        state.requested = path.clone();
        state.result = None;
        self.worker.request_browser(path);
    }

    pub fn result(&self) -> Option<Arc<FolderListing>> {
        let mut state = self.state.lock().expect("folder browser state");
        if let Some(result) = self.worker.take_browser()
            && Some(&result.path) == state.requested.as_ref()
        {
            state.result = Some(Arc::new(result));
        }
        state.result.clone()
    }
}

fn directory(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    // Explicit address entry remains available; nested menus do not traverse
    // reparse points or cycles. Filtering never changes the remaining Shell order.
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && metadata.file_attributes() & 0x400 == 0)
}

pub(crate) fn enumerate(
    path: PathBuf,
    current: &impl Fn() -> bool,
    mut directories: impl FnMut(&Path) -> Option<Vec<PathBuf>>,
) -> Option<FolderListing> {
    if !current() {
        return None;
    }
    let mut listing = FolderListing {
        path,
        folders: Vec::new(),
        truncated: false,
        unavailable: false,
    };
    let Some(paths) = directories(&listing.path) else {
        listing.unavailable = true;
        return current().then_some(listing);
    };
    listing.truncated = paths.len() > FOLDER_BROWSER_LIMIT;
    for path in paths.into_iter().take(FOLDER_BROWSER_LIMIT) {
        if !current() {
            return None;
        }
        if !directory(&path) {
            continue;
        }
        let mut has_children = false;
        if let Ok(children) = fs::read_dir(&path) {
            for child in children {
                if !current() {
                    return None;
                }
                if child.is_ok_and(|entry| directory(&entry.path())) {
                    has_children = true;
                    break;
                }
            }
        }
        listing.folders.push(BrowserFolder { path, has_children });
    }
    current().then_some(listing)
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
        let (tx, rx) = std::sync::mpsc::channel();
        let mut worker = FolderBrowser::new(move || {
            let _ = tx.send(());
        })
        .expect("worker");
        worker.request(Some(root.clone()));
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("completion");
        let listing = worker.result().expect("result");
        assert_eq!(listing.path, root);
        assert_eq!(listing.folders.len(), 2);
        assert_eq!(
            listing.folders[0].path.file_name().expect("name"),
            "folder2"
        );
        assert!(!listing.folders[0].has_children);
        assert!(listing.folders[1].has_children);

        worker.request(None);
        assert!(worker.result().is_none());
        fs::remove_dir_all(root).expect("remove owned fixture");
    }
}
