//! Last closed window's normal placement, published once after host shutdown.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const HEADER: &str = "towavue window placement v1";

/// Native workspace coordinates, paired exclusively with Get/SetWindowPlacement.
/// No HWND, native pointer or minimized/fullscreen state leaves the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedWindowPlacement {
    pub(crate) bounds: [i32; 4],
    pub(crate) dpi: u32,
    pub(crate) maximized: bool,
}

impl SavedWindowPlacement {
    pub fn maximized(self) -> bool {
        self.maximized
    }

    pub(crate) fn valid(self) -> bool {
        let [left, top, right, bottom] = self.bounds;
        (48..=960).contains(&self.dpi)
            && [right.checked_sub(left), bottom.checked_sub(top)]
                .into_iter()
                .all(|extent| extent.is_some_and(|value| (1..=100_000).contains(&value)))
    }
}

pub struct WindowPlacementPreferences {
    path: PathBuf,
    initial: Option<SavedWindowPlacement>,
    pending: Option<SavedWindowPlacement>,
}

impl WindowPlacementPreferences {
    /// Read once. An error disables persistence rather than overwriting a record
    /// with fallback geometry. Tests supply their own isolated path.
    pub fn open(path: PathBuf) -> io::Result<Self> {
        Ok(Self {
            initial: read(&path)?,
            path,
            pending: None,
        })
    }

    pub fn initial(&self) -> Option<SavedWindowPlacement> {
        self.initial
    }

    /// Called only after the app's close/save guard accepts a visible window.
    /// No disk work occurs during input, resize, playback or individual closure.
    pub fn remember(&mut self, placement: SavedWindowPlacement) {
        if placement.valid() {
            self.pending = Some(placement);
        }
    }
}

impl Drop for WindowPlacementPreferences {
    fn drop(&mut self) {
        if let Some(placement) = self.pending
            && let Err(error) = write(&self.path, placement)
        {
            crate::diagnostic!("Could not save window placement: {error}");
        }
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid window placement preference",
    )
}

fn read(path: &Path) -> io::Result<Option<SavedWindowPlacement>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut text = String::new();
    file.take(257).read_to_string(&mut text)?;
    let mut lines = text.lines();
    if text.len() > 256 || lines.next() != Some(HEADER) {
        return Err(invalid());
    }
    let mut number = || {
        lines
            .next()
            .ok_or_else(invalid)?
            .parse::<i32>()
            .map_err(|_| invalid())
    };
    let bounds = [number()?, number()?, number()?, number()?];
    let dpi = u32::try_from(number()?).map_err(|_| invalid())?;
    let maximized = match lines.next() {
        Some("normal") => false,
        Some("maximized") => true,
        _ => return Err(invalid()),
    };
    let placement = SavedWindowPlacement {
        bounds,
        dpi,
        maximized,
    };
    if lines.next().is_some() || !placement.valid() {
        return Err(invalid());
    }
    Ok(Some(placement))
}

fn write(path: &Path, placement: SavedWindowPlacement) -> io::Result<()> {
    let parent = path.parent().ok_or_else(invalid)?;
    fs::create_dir_all(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    // Shutdown never waits for another process to release its publication lock.
    lock.try_lock().map_err(io::Error::from)?;
    read(path)?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{next}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        let [left, top, right, bottom] = placement.bounds;
        let mode = if placement.maximized {
            "maximized"
        } else {
            "normal"
        };
        writeln!(
            file,
            "{HEADER}\n{left}\n{top}\n{right}\n{bottom}\n{}\n{mode}",
            placement.dpi
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
