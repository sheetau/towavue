use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

mod commands;
pub use commands::LIMIT as COMMAND_HISTORY_LIMIT;

const FILE_LIMIT: usize = 10_000;
const FOLDER_LIMIT: usize = 40;
const MAX_BYTES: u64 = 16 * 1024 * 1024;
const HEADER: &str = "towavue recent files v3";
const TIMESTAMP_HEADER: &str = "towavue recent files v2";
const LEGACY_HEADER: &str = "towavue recent files v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentKind {
    File,
    Folder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentEntry {
    pub path: PathBuf,
    pub kind: RecentKind,
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
    /// None while a newer command edit is pending or its store is unavailable.
    pub commands: Option<Vec<towavue_core::CommandId>>,
    pub entries: Vec<RecentEntry>,
    /// Gallery-only omissions. Persisted history and resume positions are untouched.
    pub missing_files: Vec<PathBuf>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Mailbox {
    pending: Vec<RecentEntry>,
    commands: Vec<(towavue_core::CommandId, bool)>,
    clear: bool,
    refresh_gallery: bool,
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
                let command_path = path.with_file_name("command-history.txt");
                let mut command_history = Vec::new();
                let mut command_pending = Vec::new();
                let mut load_commands = true;
                let mut load_files = true;
                let mut paths = Vec::new();
                let mut pending = Vec::new();
                let mut clear = false;
                let mut refresh_gallery = true;
                let mut missing_files = Vec::new();
                loop {
                    // A successfully opened source must not retain an older missing mark.
                    let opened: HashSet<_> = pending
                        .iter()
                        .map(|entry: &RecentEntry| &entry.path)
                        .collect();
                    missing_files.retain(|path| !clear && !opened.contains(path));
                    let mut error = if load_files || clear || !pending.is_empty() {
                        match update(&path, &pending, clear) {
                            Ok(loaded) => {
                                paths = loaded;
                                load_files = false;
                                pending.clear();
                                clear = false;
                                None
                            }
                            Err(error) => {
                                if clear {
                                    paths.clear();
                                }
                                remember(&mut paths, &pending);
                                Some(format!("Recent files unavailable: {error}"))
                            }
                        }
                    } else {
                        None
                    };
                    let mut commands_available = true;
                    if load_commands || !command_pending.is_empty() {
                        match commands::update(&command_path, &command_pending) {
                            Ok(loaded) => {
                                command_history = loaded;
                                command_pending.clear();
                                load_commands = false;
                            }
                            Err(command_error) => {
                                commands_available = false;
                                let message =
                                    format!("Command history unavailable: {command_error}");
                                error = Some(error.map_or(message.clone(), |error| {
                                    format!("{error}; {message}")
                                }));
                            }
                        }
                    }
                    if refresh_gallery {
                        if let Some(missing) = missing_paths(&paths, || {
                            let mailbox = mutex.lock().expect("recent mailbox");
                            mailbox.closed
                                || mailbox.clear
                                || !mailbox.pending.is_empty()
                                || !mailbox.commands.is_empty()
                        }) {
                            missing_files = missing;
                            refresh_gallery = false;
                        }
                    } else {
                        let retained: HashSet<_> = paths.iter().map(|entry| &entry.path).collect();
                        missing_files.retain(|path| retained.contains(path));
                    }
                    {
                        let mut mailbox = mutex.lock().expect("recent mailbox");
                        if !mailbox.clear {
                            mailbox.completed = Some(RecentUpdate {
                                commands: (commands_available && mailbox.commands.is_empty())
                                    .then(|| command_history.clone()),
                                entries: paths.clone(),
                                missing_files: missing_files.clone(),
                                error,
                            });
                        }
                    }
                    notify();
                    let mut mailbox = ready
                        .wait_while(mutex.lock().expect("recent mailbox"), |mailbox| {
                            !mailbox.closed
                                && mailbox.pending.is_empty()
                                && !mailbox.clear
                                && mailbox.commands.is_empty()
                                && !mailbox.refresh_gallery
                        })
                        .expect("recent mailbox");
                    if mailbox.pending.is_empty()
                        && mailbox.commands.is_empty()
                        && !mailbox.clear
                        && mailbox.closed
                    {
                        break;
                    }
                    for change in std::mem::take(&mut mailbox.commands) {
                        command_pending
                            .retain(|old: &(towavue_core::CommandId, bool)| old.0 != change.0);
                        command_pending.push(change);
                    }
                    let refresh = std::mem::take(&mut mailbox.refresh_gallery);
                    load_files |= refresh;
                    refresh_gallery |= refresh;
                    if std::mem::take(&mut mailbox.clear) {
                        pending.clear();
                        clear = true;
                    }
                    for item in std::mem::take(&mut mailbox.pending) {
                        pending.retain(|old| old.path != item.path);
                        pending.push(item);
                    }
                    trim_pending(&mut pending);
                }
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    /// Only palette activation enters MRU; menu/shortcut dispatch is not recorded.
    pub fn record_command(&self, command: towavue_core::CommandId) {
        self.queue_command(command, true);
    }

    pub fn remove_command(&self, command: towavue_core::CommandId) {
        self.queue_command(command, false);
    }

    fn queue_command(&self, command: towavue_core::CommandId, remember: bool) {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("recent mailbox");
        // One final operation per finite registered command, in last-action order.
        mailbox.commands.retain(|old| old.0 != command);
        mailbox.commands.push((command, remember));
        if let Some(completed) = &mut mailbox.completed {
            completed.commands = None;
        }
        ready.notify_one();
    }

    pub fn record(&self, path: PathBuf) {
        self.record_kind(path, RecentKind::File);
    }

    pub fn record_folder(&self, path: PathBuf) {
        self.record_kind(path, RecentKind::Folder);
    }

    /// Recheck Gallery on activation, never on the UI thread or every redraw.
    /// Only confirmed absence hides a card; IO errors retain it and a later check
    /// can restore a reconnected or recreated source without losing its history.
    pub fn refresh_gallery(&self) {
        let (mutex, ready) = &*self.shared;
        mutex.lock().expect("recent mailbox").refresh_gallery = true;
        ready.notify_one();
    }

    /// Clear both history groups at the next serialized write, not the source media.
    /// Opens queued after this request are retained; earlier pending opens are discarded.
    pub fn clear(&self) {
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("recent mailbox");
        mailbox.pending.clear();
        mailbox.completed = None;
        mailbox.clear = true;
        ready.notify_one();
    }

    fn record_kind(&self, path: PathBuf, kind: RecentKind) {
        let entry = RecentEntry {
            path,
            kind,
            opened_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|time| u64::try_from(time.as_millis()).ok()),
        };
        let (mutex, ready) = &*self.shared;
        let mut mailbox = mutex.lock().expect("recent mailbox");
        mailbox.pending.retain(|old| old.path != entry.path);
        mailbox.pending.push(entry);
        trim_pending(&mut mailbox.pending);
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

fn trim_pending(pending: &mut Vec<RecentEntry>) {
    // Pending opens are oldest first; keep each group's newest bounded set.
    pending.reverse();
    trim_entries(pending);
    pending.reverse();
}

fn trim_entries(entries: &mut Vec<RecentEntry>) {
    let mut files = 0;
    let mut folders = 0;
    entries.retain(|entry| {
        let count = match entry.kind {
            RecentKind::File => &mut files,
            RecentKind::Folder => &mut folders,
        };
        *count += 1;
        *count <= entry_limit(entry.kind)
    });
}

fn entry_limit(kind: RecentKind) -> usize {
    match kind {
        RecentKind::File => FILE_LIMIT,
        RecentKind::Folder => FOLDER_LIMIT,
    }
}

fn remember(paths: &mut Vec<RecentEntry>, pending: &[RecentEntry]) {
    let previous = std::mem::take(paths);
    // Later queued opens win ties; existing unknown-date order remains stable.
    paths.extend(pending.iter().rev().cloned());
    paths.extend(previous);
    paths.sort_by_key(|entry| std::cmp::Reverse(entry.opened_at));
    let mut seen = HashSet::with_capacity(paths.len());
    paths.retain(|entry| seen.insert(entry.path.clone()));
    trim_entries(paths);
}

fn missing_paths(
    paths: &[RecentEntry],
    mut interrupted: impl FnMut() -> bool,
) -> Option<Vec<PathBuf>> {
    let mut missing = Vec::new();
    for entry in paths.iter().filter(|entry| entry.kind == RecentKind::File) {
        // Do not make an open, clear or shutdown wait for the entire archive.
        // An individual synchronous filesystem query can still block on Windows.
        if interrupted() {
            return None;
        }
        if matches!(entry.path.try_exists(), Ok(false)) {
            missing.push(entry.path.clone());
        }
    }
    Some(missing)
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
    if !matches!(header, Some(HEADER | TIMESTAMP_HEADER | LEGACY_HEADER)) {
        return Err(std::io::Error::other(
            "Unrecognized recent files format; existing file was retained.",
        ));
    }
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut files = 0;
    let mut folders = 0;
    for line in lines {
        let (kind, line) = if header == Some(HEADER) {
            match line.split_once('\t') {
                Some(("F", rest)) => (RecentKind::File, rest),
                Some(("D", rest)) => (RecentKind::Folder, rest),
                _ => {
                    return Err(std::io::Error::other(
                        "Invalid recent entry kind; existing file was retained.",
                    ));
                }
            }
        } else {
            (RecentKind::File, line)
        };
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
        let count = match kind {
            RecentKind::File => &mut files,
            RecentKind::Folder => &mut folders,
        };
        if !path.is_absolute() || line.contains('\0') || *count == entry_limit(kind) {
            return Err(std::io::Error::other(
                "Invalid recent files entry; existing file was retained.",
            ));
        }
        if seen.insert(path.clone()) {
            *count += 1;
            paths.push(RecentEntry {
                path,
                kind,
                opened_at,
            });
        }
    }
    Ok(paths)
}

fn update(path: &Path, pending: &[RecentEntry], clear: bool) -> std::io::Result<Vec<RecentEntry>> {
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
    let mut paths = if clear { Vec::new() } else { read(path)? };
    if pending.is_empty() && !clear {
        return Ok(paths);
    }
    remember(&mut paths, pending);
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
        text.push_str(match item.kind {
            RecentKind::File => "F\t",
            RecentKind::Folder => "D\t",
        });
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

    fn update(path: &Path, pending: &[RecentEntry]) -> std::io::Result<Vec<RecentEntry>> {
        super::update(path, pending, false)
    }

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
        let command_lock = path.with_file_name("command-history.lock");
        if command_lock.exists() {
            fs::remove_file(command_lock).expect("remove owned command lock");
        }
        fs::remove_dir(path.parent().expect("directory")).expect("no temporary files left");
    }

    #[test]
    fn gallery_refresh_hides_only_absent_files_without_rewriting_history() {
        let path = fixture("gallery-presence");
        let root = path.parent().expect("parent");
        let present = root.join("present.png");
        let missing = root.join("missing.mp4");
        let folder = root.join("missing-folder");
        let unreadable = root.join("invalid?.png");
        assert!(
            unreadable.try_exists().is_err(),
            "invalid Windows path control"
        );
        fs::write(&present, b"original source").expect("owned source");
        let entries: Vec<_> = [&present, &missing, &folder, &unreadable]
            .into_iter()
            .enumerate()
            .map(|(index, path)| RecentEntry {
                path: path.clone(),
                kind: if path == &folder {
                    RecentKind::Folder
                } else {
                    RecentKind::File
                },
                opened_at: Some(index as u64),
            })
            .collect();
        update(&path, &entries).expect("seed history");
        let original = fs::read(&path).expect("history bytes");
        let (sender, receiver) = std::sync::mpsc::channel();
        let recent = RecentFiles::new(path.clone(), move || {
            let _ = sender.send(());
        })
        .expect("worker");
        let delivered = || {
            receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("worker notification");
            let update = recent.take_completed().expect("delivered snapshot");
            assert!(update.error.is_none());
            update
        };
        let initial = delivered();
        assert_eq!(initial.missing_files, std::slice::from_ref(&missing));
        assert_eq!(initial.entries.len(), 4);
        fs::remove_file(&present).expect("remove owned source");
        fs::write(&missing, b"restored source").expect("restore owned source");
        recent.refresh_gallery();
        let refreshed = delivered();
        assert_eq!(refreshed.missing_files, std::slice::from_ref(&present));
        assert_eq!(refreshed.entries, initial.entries);
        assert_eq!(fs::read(&path).expect("history unchanged"), original);

        // Successful opens clear their stale marks without restatting all history.
        fs::write(&present, b"original source").expect("restore source");
        recent.record(present.clone());
        assert!(delivered().missing_files.is_empty());
        fs::remove_file(&present).expect("remove owned source");
        recent.refresh_gallery();
        assert_eq!(delivered().missing_files, [present]);
        recent.clear();
        let cleared = delivered();
        assert!(cleared.entries.is_empty() && cleared.missing_files.is_empty());
        drop(recent);
        assert_eq!(
            fs::read(&missing).expect("source retained"),
            b"restored source"
        );
        fs::remove_file(missing).expect("remove owned source");
        clean(&path);
    }

    #[test]
    fn timestamp_history_migrates_without_inferring_folder_kind_from_paths() {
        let path = fixture("typed-migration");
        let source = path.parent().expect("parent").join("old.png");
        let original = format!("{TIMESTAMP_HEADER}\n12345\t{}\n", source.display());
        fs::write(&path, &original).expect("v2 fixture");
        let loaded = update(&path, &[]).expect("load v2");
        assert_eq!(loaded[0].kind, RecentKind::File);
        assert_eq!(loaded[0].opened_at, Some(12_345));
        assert_eq!(fs::read(&path).expect("unchanged"), original.as_bytes());
        let folder = RecentEntry {
            path: path.parent().expect("parent").join("folder.mp4"),
            kind: RecentKind::Folder,
            opened_at: Some(20_000),
        };
        let migrated = update(&path, std::slice::from_ref(&folder)).expect("record folder");
        assert_eq!(migrated, [folder, loaded[0].clone()]);
        assert_eq!(read(&path).expect("v3 round trip"), migrated);
        clean(&path);
    }

    #[test]
    fn file_and_folder_limits_are_independent_in_storage_and_pending_batches() {
        let path = fixture("groups");
        let mut pending = Vec::new();
        for kind in [RecentKind::File, RecentKind::Folder] {
            for index in 0..entry_limit(kind) + 15 {
                pending.push(RecentEntry {
                    path: path
                        .parent()
                        .expect("parent")
                        .join(format!("{kind:?}-{index}")),
                    kind,
                    opened_at: Some(index as u64),
                });
            }
        }
        trim_pending(&mut pending);
        assert_eq!(pending.len(), FILE_LIMIT + FOLDER_LIMIT);
        let stored = update(&path, &pending).expect("store both groups");
        assert_eq!(stored.len(), FILE_LIMIT + FOLDER_LIMIT);
        for kind in [RecentKind::File, RecentKind::Folder] {
            let group: Vec<_> = stored.iter().filter(|entry| entry.kind == kind).collect();
            let limit = entry_limit(kind);
            assert_eq!(group.len(), limit);
            assert_eq!(group[0].opened_at, Some(limit as u64 + 14));
            assert_eq!(group[limit - 1].opened_at, Some(15));
        }
        assert_eq!(read(&path).expect("reload"), stored);
        clean(&path);
    }

    #[test]
    fn queued_clear_discards_earlier_opens_but_flushes_later_groups_without_touching_sources() {
        let path = fixture("clear-queue");
        let source = path.parent().expect("parent").join("source.png");
        fs::write(&source, b"untouched source").expect("owned source");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .expect("owned lock");
        lock.lock().expect("hold initial worker read");
        let recent = RecentFiles::new(path.clone(), || {}).expect("worker");
        recent.record(source.clone());
        recent.record_folder(path.parent().expect("parent").join("old-folder"));
        recent.clear();
        let file = path.parent().expect("parent").join("new.png");
        let folder = path.parent().expect("parent").join("new-folder");
        recent.record(file.clone());
        recent.record_folder(folder.clone());
        drop(lock);
        drop(recent);
        let stored = read(&path).expect("flushed");
        assert_eq!(stored.len(), 2);
        assert!(
            stored
                .iter()
                .any(|entry| entry.path == file && entry.kind == RecentKind::File)
        );
        assert!(
            stored
                .iter()
                .any(|entry| entry.path == folder && entry.kind == RecentKind::Folder)
        );
        assert_eq!(
            fs::read(&source).expect("source survives"),
            b"untouched source"
        );
        fs::remove_file(source).expect("remove owned source");
        clean(&path);
    }

    #[test]
    fn explicit_clear_can_replace_corrupt_history_and_an_idle_worker_does_not_restore_it() {
        let path = fixture("clear-workers");
        fs::write(&path, b"invalid history").expect("corrupt fixture");
        assert!(
            super::update(&path, &[], true)
                .expect("explicit clear")
                .is_empty()
        );
        let old = RecentEntry {
            path: path.parent().expect("parent").join("old.png"),
            kind: RecentKind::File,
            opened_at: Some(1),
        };
        update(&path, &[old]).expect("old record");
        let (sent, events) = std::sync::mpsc::channel();
        let idle = RecentFiles::new(path.clone(), move || {
            let _ = sent.send(());
        })
        .expect("idle worker");
        events
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("old history loaded");
        let clearing = RecentFiles::new(path.clone(), || {}).expect("clearing worker");
        clearing.clear();
        drop(clearing);
        assert!(read(&path).expect("cleared").is_empty());
        let new = path.parent().expect("parent").join("new.png");
        idle.record(new.clone());
        drop(idle);
        let stored = read(&path).expect("new record only");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].path, new);
        clean(&path);
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
                kind: RecentKind::File,
                path: source.clone(),
                opened_at: None
            }]
        );
        assert_eq!(fs::read(&path).expect("original"), original.as_bytes());
        let fresh = RecentEntry {
            kind: RecentKind::File,
            path: path.parent().expect("directory").join("fresh.png"),
            opened_at: Some(12_345),
        };
        let migrated = update(&path, std::slice::from_ref(&fresh)).expect("record and migrate");
        assert_eq!(migrated, [fresh, loaded[0].clone()]);
        assert_eq!(read(&path).expect("reload"), migrated);
        assert!(
            fs::read_to_string(&path)
                .expect("v3 file")
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
            kind: RecentKind::File,
            path: a.clone(),
            opened_at: Some(300),
        };
        update(&path, std::slice::from_ref(&recent)).expect("newer worker");
        let older = [
            RecentEntry {
                kind: RecentKind::File,
                path: b,
                opened_at: Some(200),
            },
            RecentEntry {
                kind: RecentKind::File,
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
    fn batched_revisits_keep_newest_kind_timestamp_and_stable_unknown_order() {
        let entry = |name, kind, opened_at| RecentEntry {
            path: PathBuf::from(name),
            kind,
            opened_at,
        };
        let mut stored = vec![
            entry("a", RecentKind::File, Some(30)),
            entry("b", RecentKind::Folder, Some(20)),
            entry("c", RecentKind::File, None),
            entry("d", RecentKind::File, None),
        ];
        let pending = vec![
            entry("a", RecentKind::Folder, Some(10)),
            entry("e", RecentKind::File, Some(30)),
            entry("b", RecentKind::File, Some(30)),
            entry("c", RecentKind::Folder, None),
            entry("e", RecentKind::Folder, Some(30)),
        ];
        remember(&mut stored, &pending);
        assert_eq!(
            stored,
            [
                pending[4].clone(),
                pending[2].clone(),
                entry("a", RecentKind::File, Some(30)),
                pending[3].clone(),
                entry("d", RecentKind::File, None),
            ]
        );
    }

    #[test]
    fn large_archive_round_trip_presence_and_interrupt_costs() {
        let path = fixture("large-archive");
        // Daily opens spanning decades, with ordinary UTF-8 paths longer than 100 bytes.
        let entries: Vec<_> = (0..FILE_LIMIT)
            .map(|index| RecentEntry {
                path: path.parent().expect("parent").join(format!(
                    "long archive 日本語 folder name for history scale testing/{index:05}.png"
                )),
                kind: RecentKind::File,
                opened_at: Some(946_684_800_000 + index as u64 * 86_400_000),
            })
            .collect();
        let started = std::time::Instant::now();
        let stored = update(&path, &entries).expect("seed full archive");
        let write_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(stored.len(), FILE_LIMIT);
        assert!(fs::metadata(&path).expect("history").len() > 1024 * 1024);
        let started = std::time::Instant::now();
        assert_eq!(read(&path).expect("reload archive"), stored);
        let read_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = std::time::Instant::now();
        assert_eq!(
            stored.iter().filter_map(RecentEntry::opened_month).count(),
            FILE_LIMIT
        );
        let months_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = std::time::Instant::now();
        assert_eq!(
            missing_paths(&stored, || false)
                .expect("presence scan")
                .len(),
            FILE_LIMIT
        );
        let presence_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut queries = 0;
        assert!(
            missing_paths(&stored, || {
                queries += 1;
                queries == 17
            })
            .is_none()
        );
        assert_eq!(
            queries, 17,
            "pending work stops a scan before walking the archive"
        );
        let mut revisit = entries[0].clone();
        revisit.opened_at = Some(entries.last().expect("last").opened_at.expect("date") + 1);
        let started = std::time::Instant::now();
        let revised = update(&path, std::slice::from_ref(&revisit)).expect("single revisit");
        let revisit_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(revised.len(), FILE_LIMIT);
        assert_eq!(revised[0], revisit);
        assert!(revised.last().expect("old date").opened_at < Some(978_307_200_000));
        assert!(
            super::update(&path, &[], true)
                .expect("clear archive")
                .is_empty()
        );
        assert!(read(&path).expect("cleared persisted file").is_empty());
        eprintln!(
            "RECENT_ARCHIVE entries={FILE_LIMIT} write_ms={write_ms:.3} read_ms={read_ms:.3} months_ms={months_ms:.3} presence_ms={presence_ms:.3} revisit_ms={revisit_ms:.3}; generated absent local paths, warm filesystem, no cold/network-device claim"
        );
        clean(&path);
    }

    #[test]
    fn invalid_timestamp_records_preserve_the_existing_file() {
        let path = fixture("timestamps");
        let source = path.parent().expect("directory").join("source.png");
        let fresh = RecentEntry {
            kind: RecentKind::File,
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
            let original = format!("{HEADER}\nF\t{stamp}\t{}\n", source.display());
            fs::write(&path, &original).expect("invalid fixture");
            assert!(update(&path, std::slice::from_ref(&fresh)).is_err());
            assert_eq!(fs::read(&path).expect("preserved"), original.as_bytes());
        }
        let original = format!("{LEGACY_HEADER}\n{}\n", source.display());
        fs::write(&path, &original).expect("valid legacy fixture");
        let invalid = RecentEntry {
            kind: RecentKind::File,
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
            kind: RecentKind::File,
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
        let entries: Vec<_> = (0..FILE_LIMIT + 5)
            .map(|index| {
                let path = path
                    .parent()
                    .expect("fixture parent")
                    .join(format!("日本語 & media {index}.png"));
                RecentEntry {
                    kind: RecentKind::File,
                    path,
                    opened_at: Some(index as u64),
                }
            })
            .collect();
        let result = update(&path, &entries).expect("store");
        assert_eq!(result.len(), FILE_LIMIT);
        assert_eq!(result[0], entries[FILE_LIMIT + 4]);
        assert_eq!(result[FILE_LIMIT - 1], entries[5]);
        let mut revisited = entries[10].clone();
        revisited.opened_at = Some(FILE_LIMIT as u64 + 100);
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
