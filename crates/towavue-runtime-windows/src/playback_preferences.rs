//! One host-owned, bounded writer for the last user-adjusted listening volume.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HEADER: &str = "towavue playback volume v1";
const QUIET: Duration = Duration::from_millis(200);

#[derive(Clone, Copy)]
struct Record {
    observed: u128,
    level: f32,
    unmuted: f32,
}

#[derive(Default)]
struct Mailbox {
    pending: Option<Record>,
    changed: Option<Instant>,
    observed: u128,
    closed: bool,
}

/// Open at host startup, before seeding tabs. Only this initial bounded read is
/// synchronous; input callbacks enqueue snapshots without filesystem work.
/// Final owner drop drains the latest write before joining the worker.
pub struct PlaybackVolumePreferences {
    initial: (f32, f32),
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl PlaybackVolumePreferences {
    pub fn open(path: PathBuf, failed: impl Fn(String) + Send + 'static) -> io::Result<Self> {
        let record = read(&path)?;
        let initial = record.map_or((0.5, 0.5), |record| (record.level, record.unmuted));
        let shared = Arc::new((
            Mutex::new(Mailbox {
                observed: record.map_or(0, |record| record.observed),
                ..Default::default()
            }),
            Condvar::new(),
        ));
        let state = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("towavue-volume-preferences".into())
            .spawn(move || {
                loop {
                    let (lock, ready) = &*state;
                    let mut mailbox = ready
                        .wait_while(lock.lock().expect("volume mailbox"), |mailbox| {
                            mailbox.pending.is_none() && !mailbox.closed
                        })
                        .expect("volume mailbox");
                    while !mailbox.closed {
                        let Some(delay) = mailbox
                            .changed
                            .and_then(|time| QUIET.checked_sub(time.elapsed()))
                        else {
                            break;
                        };
                        mailbox = ready
                            .wait_timeout(mailbox, delay)
                            .expect("volume mailbox")
                            .0;
                    }
                    let Some(record) = mailbox.pending.take() else {
                        if mailbox.closed {
                            break;
                        }
                        continue;
                    };
                    drop(mailbox);
                    if let Err(error) = write(&path, record) {
                        failed(error.to_string());
                    }
                }
            })?;
        Ok(Self {
            initial,
            shared,
            worker: Some(worker),
        })
    }

    /// Current listening level and the positive level restored after unmuting.
    pub fn initial(&self) -> (f32, f32) {
        self.initial
    }

    pub fn remember(&self, level: f32, unmuted: f32) -> io::Result<()> {
        validate(level, unmuted)?;
        let mut mailbox = self.shared.0.lock().expect("volume mailbox");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        mailbox.observed = now.max(mailbox.observed.saturating_add(1));
        mailbox.pending = Some(Record {
            observed: mailbox.observed,
            level,
            unmuted,
        });
        mailbox.changed = Some(Instant::now());
        self.shared.1.notify_one();
        Ok(())
    }
}

impl Drop for PlaybackVolumePreferences {
    fn drop(&mut self) {
        self.shared.0.lock().expect("volume mailbox").closed = true;
        self.shared.1.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid playback volume preference",
    )
}

fn validate(level: f32, unmuted: f32) -> io::Result<()> {
    if !level.is_finite()
        || !unmuted.is_finite()
        || !(0.0..=towavue_core::MAX_VOLUME).contains(&level)
        || unmuted <= 0.0
        || unmuted > towavue_core::MAX_VOLUME
        || (level != 0.0 && level != unmuted)
    {
        return Err(invalid());
    }
    Ok(())
}

fn read(path: &Path) -> io::Result<Option<Record>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut text = String::new();
    file.take(257).read_to_string(&mut text)?;
    let parts: Vec<_> = text.lines().collect();
    if text.len() > 256 || parts.len() != 4 || parts[0] != HEADER {
        return Err(invalid());
    }
    let record = Record {
        observed: parts[1].parse().map_err(|_| invalid())?,
        level: parts[2].parse().map_err(|_| invalid())?,
        unmuted: parts[3].parse().map_err(|_| invalid())?,
    };
    validate(record.level, record.unmuted)?;
    Ok(Some(record))
}

fn write(path: &Path, record: Record) -> io::Result<()> {
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
    if read(path)?.is_some_and(|old| old.observed > record.observed) {
        return Ok(());
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{next}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        write!(
            file,
            "{HEADER}\n{}\n{}\n{}\n",
            record.observed, record.level, record.unmuted
        )?;
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
