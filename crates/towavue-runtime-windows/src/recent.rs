use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

const LIMIT: usize = 40;
const MAX_BYTES: u64 = 1024 * 1024;
const HEADER: &str = "towavue recent files v2";
const LEGACY_HEADER: &str = "towavue recent files v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentEntry {
    pub path: PathBuf,
    /// Last open in UTC milliseconds since Unix epoch; legacy entries stay unknown.
    pub opened_at: Option<u64>,
}

impl RecentEntry {
    /// Convert at history delivery, not on each UI frame. No filesystem access.
    pub fn opened_month(&self) -> Option<(u16, u16)> {
        let ticks = self
            .opened_at?
            .checked_mul(10_000)?
            .checked_add(116_444_736_000_000_000)?;
        crate::file_details::local_month(ticks)
    }
}

pub struct RecentUpdate {
    pub entries: Vec<RecentEntry>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Mailbox {
    pending: Vec<RecentEntry>,
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
                        entries: paths.clone(),
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
                        pending.retain(|old| old.path != item.path);
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
        let entry = RecentEntry {
            path,
            opened_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|time| u64::try_from(time.as_millis()).ok()),
        };
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("recent mailbox");
        mailbox.pending.retain(|old| old.path != entry.path);
        mailbox.pending.push(entry);
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

fn remember(paths: &mut Vec<RecentEntry>, entry: RecentEntry) {
    if paths
        .iter()
        .any(|old| old.path == entry.path && old.opened_at > entry.opened_at)
    {
        return;
    }
    paths.retain(|old| old.path != entry.path);
    paths.insert(0, entry);
    // A delayed worker must not put older opens ahead of newer persisted records.
    paths.sort_by_key(|entry| std::cmp::Reverse(entry.opened_at));
    paths.truncate(LIMIT);
}

fn read(path: &Path) -> std::io::Result<Vec<RecentEntry>> {
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
    let header = lines.next();
    if !matches!(header, Some(HEADER | LEGACY_HEADER)) {
        return Err(std::io::Error::other(
            "Unrecognized recent files format; existing file was retained.",
        ));
    }
    let mut paths = Vec::new();
    for line in lines {
        let (opened_at, value) = if header == Some(LEGACY_HEADER) {
            (None, line)
        } else {
            let (stamp, value) = line.split_once('\t').ok_or_else(|| {
                std::io::Error::other("Invalid recent timestamp entry; existing file was retained.")
            })?;
            let stamp = if stamp == "-" {
                None
            } else {
                Some(
                    stamp
                        .parse::<u64>()
                        .ok()
                        .filter(|value| *value <= 253_402_300_799_999)
                        .ok_or_else(|| {
                            std::io::Error::other(
                                "Invalid recent timestamp; existing file was retained.",
                            )
                        })?,
                )
            };
            (stamp, value)
        };
        let path = PathBuf::from(value);
        if !path.is_absolute() || line.contains('\0') || paths.len() == LIMIT {
            return Err(std::io::Error::other(
                "Invalid recent files entry; existing file was retained.",
            ));
        }
        if !paths.iter().any(|entry: &RecentEntry| entry.path == path) {
            paths.push(RecentEntry { path, opened_at });
        }
    }
    Ok(paths)
}

fn update(path: &Path, pending: &[RecentEntry]) -> std::io::Result<Vec<RecentEntry>> {
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
        if item
            .opened_at
            .is_some_and(|stamp| stamp > 253_402_300_799_999)
        {
            return Err(std::io::Error::other(
                "Recent timestamp is out of range; existing file was retained.",
            ));
        }
        let value = item
            .path
            .to_str()
            .filter(|value| !value.contains(['\n', '\r', '\0']))
            .filter(|_| item.path.is_absolute())
            .ok_or_else(|| {
                std::io::Error::other("Recent path cannot be represented in the history file.")
            })?;
        if let Some(stamp) = item.opened_at {
            text.push_str(&stamp.to_string());
        } else {
            text.push('-');
        }
        text.push('\t');
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
    fn legacy_history_stays_unchanged_until_an_open_records_a_real_timestamp() {
        let path = fixture("migration");
        let source = path.parent().expect("directory").join("legacy.png");
        let original = format!("{LEGACY_HEADER}\n{}\n", source.display());
        fs::write(&path, &original).expect("legacy fixture");
        let loaded = update(&path, &[]).expect("load without migration");
        assert_eq!(
            loaded,
            [RecentEntry {
                path: source.clone(),
                opened_at: None
            }]
        );
        assert_eq!(fs::read(&path).expect("original"), original.as_bytes());
        let fresh = RecentEntry {
            path: path.parent().expect("directory").join("fresh.png"),
            opened_at: Some(12_345),
        };
        let migrated = update(&path, std::slice::from_ref(&fresh)).expect("record and migrate");
        assert_eq!(migrated, [fresh, loaded[0].clone()]);
        assert_eq!(read(&path).expect("reload"), migrated);
        assert!(
            fs::read_to_string(&path)
                .expect("v2 file")
                .starts_with(HEADER)
        );
        assert!(
            !source.exists(),
            "history must not stat or create the referenced media"
        );
        clean(&path);
    }

    #[test]
    fn delayed_batches_preserve_newer_timestamps_and_chronological_order() {
        let path = fixture("delayed");
        let a = path.parent().expect("directory").join("a.png");
        let b = path.parent().expect("directory").join("b.png");
        let recent = RecentEntry {
            path: a.clone(),
            opened_at: Some(300),
        };
        update(&path, std::slice::from_ref(&recent)).expect("newer worker");
        let older = [
            RecentEntry {
                path: b,
                opened_at: Some(200),
            },
            RecentEntry {
                path: a,
                opened_at: Some(100),
            },
        ];
        assert_eq!(
            update(&path, &older).expect("delayed worker"),
            [recent, older[0].clone()]
        );
        clean(&path);
    }

    #[test]
    fn invalid_timestamp_records_preserve_the_existing_file() {
        let path = fixture("timestamps");
        let source = path.parent().expect("directory").join("source.png");
        let fresh = RecentEntry {
            path: source.clone(),
            opened_at: Some(100),
        };
        for stamp in [
            "bad",
            "-1",
            "1.5",
            "18446744073709551616",
            "253402300800000",
        ] {
            let original = format!("{HEADER}\n{stamp}\t{}\n", source.display());
            fs::write(&path, &original).expect("invalid fixture");
            assert!(update(&path, std::slice::from_ref(&fresh)).is_err());
            assert_eq!(fs::read(&path).expect("preserved"), original.as_bytes());
        }
        let original = format!("{LEGACY_HEADER}\n{}\n", source.display());
        fs::write(&path, &original).expect("valid legacy fixture");
        let invalid = RecentEntry {
            path: source,
            opened_at: Some(u64::MAX),
        };
        assert!(update(&path, &[invalid]).is_err());
        assert_eq!(fs::read(&path).expect("preserved"), original.as_bytes());
        clean(&path);
    }

    #[test]
    fn known_and_unknown_open_times_have_safe_local_months() {
        let mut entry = RecentEntry {
            path: "unused.png".into(),
            opened_at: None,
        };
        assert_eq!(entry.opened_month(), None);
        entry.opened_at = Some(15 * 86_400_000);
        assert_eq!(entry.opened_month(), Some((1970, 1)));
        entry.opened_at = Some(u64::MAX);
        assert_eq!(entry.opened_month(), None);
    }

    #[test]
    fn recent_order_is_bounded_and_unicode_round_trips_without_statting_sources() {
        let path = fixture("order");
        let entries: Vec<_> = (0..45)
            .map(|index| {
                let path = path
                    .parent()
                    .expect("fixture parent")
                    .join(format!("日本語 & media {index}.png"));
                RecentEntry {
                    path,
                    opened_at: Some(index),
                }
            })
            .collect();
        let result = update(&path, &entries).expect("store");
        assert_eq!(result.len(), LIMIT);
        assert_eq!(result[0], entries[44]);
        assert_eq!(result[39], entries[5]);
        let mut revisited = entries[10].clone();
        revisited.opened_at = Some(100);
        let result = update(&path, &[revisited.clone()]).expect("revisit");
        assert_eq!(result[0], revisited);
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
                && update.entries.iter().any(|entry| entry.path == item)
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
        assert!(result.iter().all(|entry| entry.opened_at.is_some()));
        for prefix in ["first", "second"] {
            let matching: Vec<_> = result
                .iter()
                .filter(|item| {
                    item.path
                        .file_name()
                        .expect("fixture filename")
                        .to_string_lossy()
                        .starts_with(prefix)
                })
                .collect();
            assert!(
                matching[0]
                    .path
                    .file_stem()
                    .expect("fixture stem")
                    .to_string_lossy()
                    .ends_with('9')
            );
        }
        clean(&path);
    }
}
