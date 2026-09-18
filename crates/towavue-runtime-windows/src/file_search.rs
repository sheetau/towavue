use std::collections::BinaryHeap;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use towavue_core::{MediaKind, file_search_score, search_text};

use crate::{Cancellation, LatestTask};

pub const FILE_SEARCH_LIMIT: usize = 1000;
const DEPTH_LIMIT: usize = 128;
const REPARSE_POINT: u32 = 0x400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSearchRequest {
    pub root: PathBuf,
    pub query: String,
}

#[derive(Debug)]
pub struct FileSearchResult {
    pub request: FileSearchRequest,
    pub paths: Vec<PathBuf>,
    pub matches: u64,
    /// Unreadable entries, reparse points, and directories beyond the depth bound.
    pub skipped: u64,
    pub error: Option<String>,
}

#[derive(Default)]
struct State {
    request: Option<FileSearchRequest>,
    revision: u64,
    result: Option<Arc<FileSearchResult>>,
}

/// One cancellable reader per window, with one bounded result slot. No media
/// bytes, Shell ordering, or native window/GPU resources are touched.
pub struct FileSearch {
    worker: LatestTask,
    state: Arc<Mutex<State>>,
    notify: Arc<dyn Fn() + Send + Sync>,
    #[cfg(test)]
    before_publish: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl FileSearch {
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> std::io::Result<Self> {
        Ok(Self {
            worker: LatestTask::new("towavue-file-search")?,
            state: Arc::new(Mutex::new(State::default())),
            notify: Arc::new(notify),
            #[cfg(test)]
            before_publish: None,
        })
    }

    pub fn update(&mut self, request: Option<FileSearchRequest>) -> bool {
        let revision = {
            let mut state = self.state.lock().expect("file search state");
            if state.request == request {
                return false;
            }
            state.revision = state.revision.wrapping_add(1);
            state.request = request.clone();
            state.result = None;
            state.revision
        };
        let Some(request) = request else {
            self.worker.clear();
            return true;
        };
        let shared = Arc::clone(&self.state);
        let notify = Arc::clone(&self.notify);
        #[cfg(test)]
        let before_publish = self.before_publish.clone();
        self.worker.submit(move |cancellation| {
            let Some(result) = scan(request, &cancellation) else {
                return;
            };
            #[cfg(test)]
            if let Some(hook) = before_publish {
                hook();
            }
            let mut state = shared.lock().expect("file search state");
            // The revision also rejects A -> B -> A completions, and closing
            // invalidates it before cancelling the worker. Notify holds no lock.
            if state.revision != revision || cancellation.is_cancelled() {
                return;
            }
            state.result = Some(Arc::new(result));
            drop(state);
            notify();
        });
        true
    }

    pub fn result(&self) -> Option<Arc<FileSearchResult>> {
        self.state.lock().expect("file search state").result.clone()
    }
}

impl Drop for FileSearch {
    fn drop(&mut self) {
        self.update(None);
    }
}

fn scan(request: FileSearchRequest, cancellation: &Cancellation) -> Option<FileSearchResult> {
    let mut result = FileSearchResult {
        request,
        paths: Vec::new(),
        matches: 0,
        skipped: 0,
        error: None,
    };
    if cancellation.is_cancelled() {
        return None;
    }
    if !result.request.root.is_absolute() || result.request.query.trim().is_empty() {
        result.error = Some("Search requires an absolute folder and a nonempty query".into());
        return Some(result);
    }
    let root = match fs::read_dir(&result.request.root) {
        Ok(root) => root,
        Err(error) => {
            result.error = Some(format!("Cannot search this folder: {error}"));
            return (!cancellation.is_cancelled()).then_some(result);
        }
    };
    // Depth-first enumeration retains at most DEPTH_LIMIT native iterators,
    // not an unbounded queue of discovered directories or matching paths.
    let mut stack = vec![root];
    let mut best = BinaryHeap::new();
    while let Some(entries) = stack.last_mut() {
        if cancellation.is_cancelled() {
            return None;
        }
        let Some(entry) = entries.next() else {
            stack.pop();
            continue;
        };
        let Ok(entry) = entry else {
            result.skipped += 1;
            continue;
        };
        let Ok(metadata) = entry.metadata() else {
            result.skipped += 1;
            continue;
        };
        if metadata.file_attributes() & REPARSE_POINT != 0 {
            result.skipped += 1;
            continue;
        }
        if metadata.is_dir() {
            if stack.len() >= DEPTH_LIMIT {
                result.skipped += 1;
            } else if let Ok(entries) = fs::read_dir(entry.path()) {
                stack.push(entries);
            } else {
                result.skipped += 1;
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let path = entry.path();
        if MediaKind::from_path(&path).is_none() {
            continue;
        }
        let Some(score) = file_search_score(&path, &result.request.query) else {
            continue;
        };
        result.matches += 1;
        let ranked = (score, search_text(&path.to_string_lossy()), path);
        if best.len() < FILE_SEARCH_LIMIT {
            best.push(ranked);
        } else if best.peek().is_some_and(|worst| ranked < *worst) {
            best.pop();
            best.push(ranked);
        }
    }
    if cancellation.is_cancelled() {
        return None;
    }
    result.paths = best
        .into_sorted_vec()
        .into_iter()
        .map(|(_, _, path)| path)
        .collect();
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn late_scans_cannot_publish_after_query_root_round_trip_clear_or_drop() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let root = fixture("late");
        let other = root.join("other");
        fs::create_dir(&other).expect("other root");
        fs::write(root.join("alpha.png"), []).expect("source");
        fs::write(root.join("beta.png"), []).expect("source");
        fs::write(other.join("alpha-other.png"), []).expect("other source");
        for mode in 0..5 {
            let (tx, rx) = mpsc::channel();
            let mut worker = FileSearch::new(move || {
                let _ = tx.send(());
            })
            .expect("worker");
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let release_rx = Mutex::new(release_rx);
            let first = AtomicBool::new(true);
            worker.before_publish = Some(Arc::new(move || {
                if first.swap(false, Ordering::Relaxed) {
                    started_tx.send(()).expect("scan finished");
                    release_rx
                        .lock()
                        .expect("release receiver")
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release scan");
                }
            }));
            let original = FileSearchRequest {
                root: root.clone(),
                query: "alpha".into(),
            };
            worker.update(Some(original.clone()));
            started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("paused before publication");
            let expected = match mode {
                0 => Some(FileSearchRequest {
                    root: root.clone(),
                    query: "beta".into(),
                }),
                1 => Some(FileSearchRequest {
                    root: other.clone(),
                    query: "alpha".into(),
                }),
                2 => {
                    worker.update(Some(FileSearchRequest {
                        root: other.clone(),
                        query: "beta".into(),
                    }));
                    fs::write(root.join("alpha-new.png"), [])
                        .expect("new result after the old scan");
                    Some(original)
                }
                _ => None,
            };
            if mode == 4 {
                drop(worker);
                release_tx.send(()).expect("finish after drop");
                assert_eq!(
                    rx.recv_timeout(Duration::from_secs(5)),
                    Err(mpsc::RecvTimeoutError::Disconnected)
                );
                continue;
            }
            worker.update(expected.clone());
            assert!(worker.result().is_none());
            release_tx.send(()).expect("finish obsolete scan");
            if let Some(expected) = expected {
                rx.recv_timeout(Duration::from_secs(5))
                    .expect("latest result");
                let result = worker.result().expect("latest result");
                assert_eq!(result.request, expected);
                if mode == 2 {
                    assert!(result.paths.contains(&root.join("alpha-new.png")));
                }
            } else {
                let (done_tx, done_rx) = mpsc::channel();
                worker.worker.submit(move |_| {
                    done_tx.send(()).expect("drained obsolete task");
                });
                done_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("worker drained");
                assert!(worker.result().is_none());
            }
            assert!(rx.try_recv().is_err(), "obsolete result never notifies");
        }
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    fn fixture(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("towavue-search-{name}-{}", std::process::id()));
        fs::create_dir(&root).expect("unique owned fixture");
        root
    }

    #[test]
    #[ignore = "requires Windows directory symlink creation permission; uses only owned fixtures"]
    fn descendant_reparse_points_do_not_escape_or_cycle_but_explicit_roots_are_allowed() {
        let root = fixture("reparse");
        let inside = root.join("inside");
        let outside = root.join("outside");
        fs::create_dir(&inside).expect("search root");
        fs::create_dir(&outside).expect("outside owned root");
        fs::write(inside.join("image.png"), []).expect("inside file");
        fs::write(outside.join("image-outside.png"), b"unchanged").expect("outside file");
        let escape = inside.join("escape");
        let cycle = inside.join("cycle");
        std::os::windows::fs::symlink_dir(&outside, &escape).expect("create owned escape symlink");
        std::os::windows::fs::symlink_dir(&inside, &cycle).expect("create owned cycle symlink");
        let result = scan(
            FileSearchRequest {
                root: inside.clone(),
                query: "image".into(),
            },
            &Cancellation::default(),
        )
        .expect("scan");
        assert_eq!(result.paths, [inside.join("image.png")]);
        assert_eq!(result.skipped, 2);
        let explicit = scan(
            FileSearchRequest {
                root: escape.clone(),
                query: "image".into(),
            },
            &Cancellation::default(),
        )
        .expect("explicit linked root");
        assert_eq!(explicit.paths, [escape.join("image-outside.png")]);
        assert_eq!(
            fs::read(outside.join("image-outside.png")).expect("source preserved"),
            b"unchanged"
        );
        fs::remove_dir(escape).expect("remove owned link, not target");
        fs::remove_dir(cycle).expect("remove owned cycle link");
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn hierarchy_search_ranks_all_matches_with_bounded_storage_and_no_media_reads() {
        let root = fixture("bounded");
        let nested = root.join(".hidden");
        fs::create_dir(&nested).expect("nested folder");
        let best = nested.join("image.png");
        fs::write(&best, "not image bytes").expect("metadata-only matching");
        fs::write(nested.join("image.txt"), []).expect("unsupported file");
        for index in 0..FILE_SEARCH_LIMIT + 10 {
            fs::write(root.join(format!("image-{index:04}.jpg")), []).expect("empty media fixture");
        }
        let request = FileSearchRequest {
            root: root.clone(),
            query: "image".into(),
        };
        let result = scan(request, &Cancellation::default()).expect("scan");
        assert_eq!(result.matches, FILE_SEARCH_LIMIT as u64 + 11);
        assert_eq!(result.paths.len(), FILE_SEARCH_LIMIT);
        assert_eq!(result.paths[0], best);
        assert_eq!(result.skipped, 0);
        assert!(result.error.is_none());
        assert_eq!(fs::read(&best).expect("source intact"), b"not image bytes");
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn changed_queries_roots_and_close_reject_obsolete_results() {
        let root = fixture("lifecycle");
        fs::write(root.join("alpha.png"), []).expect("fixture");
        let (tx, rx) = mpsc::channel();
        let mut worker = FileSearch::new(move || {
            let _ = tx.send(());
        })
        .expect("worker");
        let request = FileSearchRequest {
            root: root.clone(),
            query: "alpha".into(),
        };
        assert!(worker.update(Some(request.clone())));
        rx.recv_timeout(Duration::from_secs(5)).expect("completion");
        let first = worker.result().expect("result");
        assert_eq!(first.paths, [root.join("alpha.png")]);
        assert!(
            !worker.update(Some(request.clone())),
            "stable frames never rescan"
        );
        assert!(Arc::ptr_eq(
            &first,
            &worker.result().expect("shared result")
        ));
        let missing = FileSearchRequest {
            root: root.join("missing"),
            query: "alpha".into(),
        };
        worker.update(Some(missing));
        rx.recv_timeout(Duration::from_secs(5))
            .expect("missing folder completion");
        assert!(worker.result().expect("error result").error.is_some());
        worker.update(None);
        assert!(worker.result().is_none());
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert!(scan(request, &cancellation).is_none());
        drop(worker);
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }
}
