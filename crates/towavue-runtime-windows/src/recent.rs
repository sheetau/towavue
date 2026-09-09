use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

const LIMIT: usize = 40;
const MAX_BYTES: u64 = 1024 * 1024;
const HEADER: &str = "towavue recent files v1";

pub struct RecentUpdate {
    pub paths: Vec<PathBuf>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Mailbox {
    pending: Vec<PathBuf>,
    completed: Option<RecentUpdate>,
    closed: bool,
}

pub struct RecentFiles {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl RecentFiles {
    pub fn new(path: PathBuf, notify: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("towavue-recent-files".into())
            .spawn(move || {
                let (mutex, ready) = &*worker_shared;
                let mut paths = Vec::new();
                let mut pending = Vec::new();
                loop {
                    let error = match update(&path, &pending) {
                        Ok(loaded) => {
                            paths = loaded;
                            pending.clear();
                            None
                        }
                        Err(error) => {
                            for item in &pending {
                                remember(&mut paths, item.clone());
                            }
                            Some(format!("Recent files unavailable: {error}"))
                        }
                    };
                    mutex.lock().expect("recent mailbox").completed = Some(RecentUpdate {
                        paths: paths.clone(),
                        error,
                    });
                    notify();
                    let mut mailbox = ready
                        .wait_while(mutex.lock().expect("recent mailbox"), |mailbox| {
                            !mailbox.closed && mailbox.pending.is_empty()
                        })
                        .expect("recent mailbox");
                    if mailbox.pending.is_empty() && mailbox.closed {
                        break;
                    }
                    for item in std::mem::take(&mut mailbox.pending) {
                        pending.retain(|old| old != &item);
                        pending.push(item);
                    }
                    if pending.len() > LIMIT {
                        pending.drain(..pending.len() - LIMIT);
                    }
                }
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub fn record(&self, path: PathBuf) {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("recent mailbox");
        mailbox.pending.retain(|old| old != &path);
        mailbox.pending.push(path);
        if mailbox.pending.len() > LIMIT {
            mailbox.pending.remove(0);
        }
        ready.notify_one();
    }

    pub fn take_completed(&self) -> Option<RecentUpdate> {
        self.shared
            .0
            .lock()
            .expect("recent mailbox")
            .completed
            .take()
    }
}

impl Drop for RecentFiles {
    fn drop(&mut self) {
        let (mutex, ready) = &*self.shared;
        mutex.lock().expect("recent mailbox").closed = true;
        ready.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn remember(paths: &mut Vec<PathBuf>, path: PathBuf) {
    paths.retain(|old| old != &path);
    paths.insert(0, path);
    paths.truncate(LIMIT);
}

fn read(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut text = String::new();
    file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_BYTES {
        return Err(std::io::Error::other(
            "Recent files list exceeds its size limit.",
        ));
    }
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(std::io::Error::other(
            "Unrecognized recent files format; existing file was retained.",
        ));
    }
    let mut paths = Vec::new();
    for line in lines {
        let path = PathBuf::from(line);
        if !path.is_absolute() || line.contains('\0') || paths.len() == LIMIT {
            return Err(std::io::Error::other(
                "Invalid recent files entry; existing file was retained.",
            ));
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn update(path: &Path, pending: &[PathBuf]) -> std::io::Result<Vec<PathBuf>> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Recent files path has no parent."))?;
    fs::create_dir_all(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    lock.lock()?;
    let mut paths = read(path)?;
    if pending.is_empty() {
        return Ok(paths);
    }
    for item in pending {
        remember(&mut paths, item.clone());
    }
    let mut text = format!("{HEADER}\n");
    for item in &paths {
        let value = item
            .to_str()
            .filter(|value| !value.contains(['\n', '\r', '\0']))
            .filter(|_| item.is_absolute())
            .ok_or_else(|| {
                std::io::Error::other("Recent path cannot be represented in the history file.")
            })?;
        text.push_str(value);
        text.push('\n');
    }
    if text.len() as u64 > MAX_BYTES {
        return Err(std::io::Error::other(
            "Recent files list exceeds its size limit.",
        ));
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), nonce));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("towavue-recent-{name}-{}", std::process::id()));
        fs::create_dir_all(&root).expect("owned test directory");
        root.join("recent-files.txt")
    }

    fn clean(path: &Path) {
        if path.exists() {
            fs::remove_file(path).expect("remove owned history");
        }
        fs::remove_file(path.with_extension("lock")).expect("remove owned lock");
        fs::remove_dir(path.parent().expect("directory")).expect("no temporary files left");
    }

    #[test]
    fn recent_order_is_bounded_and_unicode_round_trips_without_statting_sources() {
        let path = fixture("order");
        let entries: Vec<_> = (0..45)
            .map(|index| {
                path.parent()
                    .expect("fixture parent")
                    .join(format!("日本語 & media {index}.png"))
            })
            .collect();
        let result = update(&path, &entries).expect("store");
        assert_eq!(result.len(), LIMIT);
        assert_eq!(result[0], entries[44]);
        assert_eq!(result[39], entries[5]);
        let result = update(&path, &[entries[10].clone()]).expect("revisit");
        assert_eq!(result[0], entries[10]);
        assert_eq!(read(&path).expect("reload"), result);
        clean(&path);
    }

    #[test]
    fn corrupt_history_is_preserved_and_worker_still_publishes_current_paths() {
        let path = fixture("corrupt");
        fs::write(&path, b"not a history file").expect("owned bad file");
        let item = path.parent().expect("fixture parent").join("source.png");
        let (sent, events) = std::sync::mpsc::channel();
        let recent = RecentFiles::new(path.clone(), move || {
            let _ = sent.send(());
        })
        .expect("worker");
        recent.record(item.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            events
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("update");
            if let Some(update) = recent.take_completed()
                && update.paths.contains(&item)
            {
                assert!(update.error.is_some());
                break;
            }
        }
        drop(recent);
        assert_eq!(
            fs::read(&path).expect("preserved bytes"),
            b"not a history file"
        );
        clean(&path);
    }

    #[test]
    fn independent_workers_merge_under_file_lock_and_flush_on_drop() {
        let path = fixture("workers");
        let first = RecentFiles::new(path.clone(), || {}).expect("first worker");
        let second = RecentFiles::new(path.clone(), || {}).expect("second worker");
        for index in 0..10 {
            first.record(
                path.parent()
                    .expect("fixture parent")
                    .join(format!("first-{index}.png")),
            );
            second.record(
                path.parent()
                    .expect("fixture parent")
                    .join(format!("second-{index}.mp4")),
            );
        }
        drop(first);
        drop(second);
        let result = read(&path).expect("merged file");
        assert_eq!(result.len(), 20);
        for prefix in ["first", "second"] {
            let matching: Vec<_> = result
                .iter()
                .filter(|item| {
                    item.file_name()
                        .expect("fixture filename")
                        .to_string_lossy()
                        .starts_with(prefix)
                })
                .collect();
            assert!(
                matching[0]
                    .file_stem()
                    .expect("fixture stem")
                    .to_string_lossy()
                    .ends_with('9')
            );
        }
        clean(&path);
    }
}
