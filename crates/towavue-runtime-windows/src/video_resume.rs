//! Bounded local resume history. All functions perform blocking filesystem IO
//! and belong on a worker, never in a paint/input callback. Media is read-only.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LEGACY_HEADER: &str = "towavue video resume v1";
const HEADER: &str = "towavue video resume v2";
const LIMIT: usize = 200;
const MAX_BYTES: u64 = 1024 * 1024;

/// Stamp captured while opening the source; keep it with that playback owner.
/// Length/mtime validation is not a content hash or an immutable file snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoResumeSource {
    path: PathBuf,
    length: u64,
    modified: u128,
}

#[derive(Debug)]
pub struct VideoResume {
    pub source: VideoResumeSource,
    pub position: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    source: VideoResumeSource,
    position: u64,
    observed: u128,
}

#[derive(Default)]
struct History {
    cleared: u128,
    entries: Vec<Entry>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl VideoResumeSource {
    fn capture(path: &Path) -> io::Result<Self> {
        if !path.is_absolute()
            || path
                .to_str()
                .is_none_or(|value| value.contains(['\n', '\r', '\0']))
        {
            return Err(invalid(
                "Resume source requires an absolute representable path",
            ));
        }
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(invalid("Resume source is not a file"));
        }
        Ok(Self {
            path: path.to_owned(),
            length: metadata.len(),
            modified: metadata
                .modified()?
                .duration_since(UNIX_EPOCH)
                .map_err(|_| invalid("Resume source timestamp precedes Unix epoch"))?
                .as_nanos(),
        })
    }
}

/// Load only a matching file revision. Zero is retained as a reset/tombstone so
/// a delayed older writer cannot resurrect a previous resume point.
fn load_video_resume(history: &Path, media: &Path) -> io::Result<VideoResume> {
    let source = VideoResumeSource::capture(media)?;
    let position = read(history)?
        .entries
        .iter()
        .find(|entry| entry.source == source)
        .filter(|entry| entry.position != 0)
        .map(|entry| Duration::from_nanos(entry.position));
    Ok(VideoResume { source, position })
}

/// `observed` is captured with the playback position, not when this IO starts.
/// Positions use the original source timeline; the app maps edited playback.
/// Pass zero at natural source EOF. Duration/range policy belongs to the app.
fn remember_video_resume(
    history: &Path,
    source: &VideoResumeSource,
    position: Duration,
    observed: SystemTime,
) -> io::Result<()> {
    let position = u64::try_from(position.as_nanos())
        .ok()
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| invalid("Resume position exceeds the media time range"))?;
    let observed = observed
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("Invalid resume observation time"))?
        .as_nanos();
    let _lock = lock_history(history)?;
    let mut data = read(history)?;
    if data.cleared != 0 && observed <= data.cleared {
        return Ok(());
    }
    // Validate after a possibly contended lock wait, not before it.
    if VideoResumeSource::capture(&source.path)? != *source {
        return Ok(());
    }
    let entries = &mut data.entries;
    if entries
        .iter()
        .any(|entry| entry.source.path == source.path && entry.observed > observed)
    {
        return Ok(());
    }
    entries.retain(|entry| entry.source.path != source.path);
    entries.push(Entry {
        source: source.clone(),
        position,
        observed,
    });
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.observed));
    entries.truncate(LIMIT);
    write(history, &data)
}

fn lock_history(history: &Path) -> io::Result<File> {
    let parent = history
        .parent()
        .ok_or_else(|| invalid("Resume history has no parent"))?;
    fs::create_dir_all(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(history.with_extension("lock"))?;
    lock.lock()?;
    Ok(lock)
}

/// Keep the cutoff even when all entries are gone: an older queued write in
/// another process must not resurrect a cleared path. Later observations survive
/// a delayed clear. Only this explicit operation may replace malformed history.
fn clear_video_resume(history: &Path, observed: SystemTime) -> io::Result<()> {
    let cutoff = observed
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("Invalid resume clear time"))?
        .as_nanos();
    if cutoff == 0 {
        return Err(invalid("Resume clear time must follow Unix epoch"));
    }
    let _lock = lock_history(history)?;
    let mut data = match read(history) {
        Ok(data) => data,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => History::default(),
        Err(error) => return Err(error),
    };
    data.cleared = data.cleared.max(cutoff);
    data.entries.retain(|entry| entry.observed > data.cleared);
    write(history, &data)
}

fn read(path: &Path) -> io::Result<History> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(History::default()),
        Err(error) => return Err(error),
    };
    let mut text = String::new();
    file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_BYTES {
        return Err(invalid("Resume history exceeds its size limit"));
    }
    let mut lines = text.lines();
    let cleared = match lines.next() {
        Some(LEGACY_HEADER) => 0,
        Some(HEADER) => lines
            .next()
            .and_then(|line| line.strip_prefix("cleared\t"))
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| invalid("Invalid resume clear boundary"))?,
        _ => {
            return Err(invalid(
                "Unknown resume history format; existing file retained",
            ));
        }
    };
    let mut entries: Vec<Entry> = Vec::new();
    for line in lines {
        let mut fields = line.splitn(5, '\t');
        let mut number = || {
            fields
                .next()
                .and_then(|value| value.parse::<u128>().ok())
                .ok_or_else(|| invalid("Invalid resume history number"))
        };
        let observed = number()?;
        let length =
            u64::try_from(number()?).map_err(|_| invalid("Invalid resume source length"))?;
        let modified = number()?;
        let position = u64::try_from(number()?)
            .ok()
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or_else(|| invalid("Invalid resume position"))?;
        let path = PathBuf::from(
            fields
                .next()
                .ok_or_else(|| invalid("Missing resume source"))?,
        );
        if !path.is_absolute()
            || line.contains('\0')
            || entries.len() == LIMIT
            || entries.iter().any(|entry| entry.source.path == path)
        {
            return Err(invalid(
                "Invalid resume history entry; existing file retained",
            ));
        }
        entries.push(Entry {
            source: VideoResumeSource {
                path,
                length,
                modified,
            },
            position,
            observed,
        });
    }
    if cleared != 0 && entries.iter().any(|entry| entry.observed <= cleared) {
        return Err(invalid("Resume entry precedes its clear boundary"));
    }
    Ok(History { cleared, entries })
}

fn write(path: &Path, data: &History) -> io::Result<()> {
    let mut text = format!("{HEADER}\ncleared\t{}\n", data.cleared);
    for entry in &data.entries {
        let source = &entry.source;
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            entry.observed,
            source.length,
            source.modified,
            entry.position,
            source
                .path
                .to_str()
                .ok_or_else(|| invalid("Unrepresentable resume path"))?
        ));
    }
    if text.len() as u64 > MAX_BYTES {
        return Err(invalid("Resume history exceeds its size limit"));
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
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
    result
}

#[cfg(test)]
mod tests;

mod worker;
pub use worker::{VideoResumeEvent, VideoResumeHistory};
