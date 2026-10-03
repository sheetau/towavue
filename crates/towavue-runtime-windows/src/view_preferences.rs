//! Bounded, host-owned persistence for explicit viewing and output choices.
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HEADER: &str = "towavue viewing preferences v1";
const QUIET: Duration = Duration::from_millis(200);

/// Stable keys and validated scalar values; no UI or native objects are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViewPreference {
    Resize,
    VolumeStep,
    Nearest,
    Minification,
    FolderLoop,
    ReadingPages,
    ReadingAxis,
    ReadingReversed,
    VideoQuality,
    HardwareEncode,
}

impl ViewPreference {
    pub const ALL: [Self; 10] = [
        Self::Resize,
        Self::VolumeStep,
        Self::Nearest,
        Self::Minification,
        Self::FolderLoop,
        Self::ReadingPages,
        Self::ReadingAxis,
        Self::ReadingReversed,
        Self::VideoQuality,
        Self::HardwareEncode,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::Resize => "resize",
            Self::VolumeStep => "volume-step",
            Self::Nearest => "nearest",
            Self::Minification => "minification",
            Self::FolderLoop => "folder-loop",
            Self::ReadingPages => "reading-pages",
            Self::ReadingAxis => "reading-axis",
            Self::ReadingReversed => "reading-reversed",
            Self::VideoQuality => "video-quality",
            Self::HardwareEncode => "hardware-encode",
        }
    }

    pub fn default_value(self) -> u16 {
        match self {
            Self::VolumeStep => 2,
            Self::Minification | Self::FolderLoop => 1,
            Self::ReadingPages => 202,
            _ => 0,
        }
    }

    fn valid(self, value: u16) -> bool {
        match self {
            Self::Resize | Self::VideoQuality => value <= 2,
            Self::VolumeStep => matches!(value, 2 | 5 | 10),
            Self::ReadingPages => {
                (2..=10).contains(&(value / 100)) && (1..=value / 100).contains(&(value % 100))
            }
            _ => value <= 1,
        }
    }
}

#[derive(Clone, Copy)]
struct Record {
    observed: u128,
    value: u16,
}
type Records = BTreeMap<ViewPreference, Record>;

#[derive(Default)]
struct Mailbox {
    pending: Records,
    current: Records,
    changed: Option<Instant>,
    observed: u128,
    closed: bool,
}

/// Open at host startup, before seeding tabs. Only this initial bounded read is
/// synchronous; input callbacks enqueue snapshots without filesystem work.
/// Final owner drop drains the latest write before joining the worker.
pub struct ViewPreferences {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl ViewPreferences {
    pub fn open(path: PathBuf, failed: impl Fn(io::Error) + Send + 'static) -> io::Result<Self> {
        let record = read(&path)?;
        let shared = Arc::new((
            Mutex::new(Mailbox {
                observed: record
                    .values()
                    .map(|record| record.observed)
                    .max()
                    .unwrap_or(0),
                current: record,
                ..Default::default()
            }),
            Condvar::new(),
        ));
        let state = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("towavue-view-preferences".into())
            .spawn(move || {
                loop {
                    let (lock, ready) = &*state;
                    let mut mailbox = ready
                        .wait_while(lock.lock().expect("view preference mailbox"), |mailbox| {
                            mailbox.pending.is_empty() && !mailbox.closed
                        })
                        .expect("view preference mailbox");
                    while !mailbox.closed {
                        let Some(delay) = mailbox
                            .changed
                            .and_then(|time| QUIET.checked_sub(time.elapsed()))
                        else {
                            break;
                        };
                        mailbox = ready
                            .wait_timeout(mailbox, delay)
                            .expect("view preference mailbox")
                            .0;
                    }
                    if mailbox.pending.is_empty() {
                        if mailbox.closed {
                            break;
                        }
                        continue;
                    }
                    let record = std::mem::take(&mut mailbox.pending);
                    drop(mailbox);
                    if let Err(error) = write(&path, record) {
                        failed(error);
                    }
                }
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub fn value(&self, key: ViewPreference) -> u16 {
        self.shared
            .0
            .lock()
            .expect("view preference mailbox")
            .current
            .get(&key)
            .map_or_else(|| key.default_value(), |record| record.value)
    }

    /// Merge only the changed choice. An older window must not save its other
    /// local settings over choices subsequently adjusted in another window.
    pub fn remember(&self, key: ViewPreference, value: u16) -> io::Result<()> {
        if !key.valid(value) {
            return Err(invalid());
        }
        let mut mailbox = self.shared.0.lock().expect("view preference mailbox");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        mailbox.observed = now.max(mailbox.observed.saturating_add(1));
        let record = Record {
            observed: mailbox.observed,
            value,
        };
        mailbox.current.insert(key, record);
        mailbox.pending.insert(key, record);
        mailbox.changed = Some(Instant::now());
        self.shared.1.notify_one();
        Ok(())
    }
}

impl Drop for ViewPreferences {
    fn drop(&mut self) {
        self.shared
            .0
            .lock()
            .expect("view preference mailbox")
            .closed = true;
        self.shared.1.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        crate::RecoveryDetail::from(towavue_core::localization::Text::ViewPreferenceInvalid),
    )
}

fn read(path: &Path) -> io::Result<Records> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Records::new()),
        Err(error) => return Err(error),
    };
    let mut text = String::new();
    file.take(4097).read_to_string(&mut text)?;
    let mut lines = text.lines();
    if text.len() > 4096 || lines.next() != Some(HEADER) {
        return Err(invalid());
    }
    let mut records = Records::new();
    for line in lines {
        let fields: Vec<_> = line.split(' ').collect();
        if fields.len() != 3 {
            return Err(invalid());
        }
        let key = ViewPreference::ALL
            .into_iter()
            .find(|key| key.key() == fields[0])
            .ok_or_else(invalid)?;
        let record = Record {
            observed: fields[1].parse().map_err(|_| invalid())?,
            value: fields[2].parse().map_err(|_| invalid())?,
        };
        if !key.valid(record.value) || records.insert(key, record).is_some() {
            return Err(invalid());
        }
    }
    Ok(records)
}

fn write(path: &Path, updates: Records) -> io::Result<()> {
    let parent = path.parent().ok_or_else(invalid)?;
    fs::create_dir_all(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    lock.lock()?;
    // Recheck under the cross-process lock. Unknown/external content is never
    // truncated, and delayed older observations cannot replace a newer setting.
    let mut records = read(path)?;
    for (key, record) in updates {
        if records
            .get(&key)
            .is_none_or(|old| old.observed <= record.observed)
        {
            records.insert(key, record);
        }
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{next}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        writeln!(file, "{HEADER}")?;
        for (key, record) in records {
            writeln!(file, "{} {} {}", key.key(), record.observed, record.value)?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests;
