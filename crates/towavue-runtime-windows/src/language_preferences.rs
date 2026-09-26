//! A bounded host-owned writer for an explicit next-launch language choice.
mod error;
pub use error::LanguagePreferenceError;
use towavue_core::localization::Text;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use towavue_core::localization::Language;

const HEADER: &str = "towavue language v1";

pub struct LanguagePreferences {
    initial: Language,
    sender: Option<SyncSender<Language>>,
    worker: Option<JoinHandle<()>>,
}

impl LanguagePreferences {
    /// Read only once at startup. Callers retain English on failure and must not
    /// replace an unreadable or unknown preference with that fallback.
    pub fn open(
        path: PathBuf,
        completed: impl Fn(Result<Language, LanguagePreferenceError>) + Send + 'static,
    ) -> io::Result<Self> {
        let initial = read(&path)?.unwrap_or_default();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("towavue-language-preferences".into())
            .spawn(move || {
                for language in receiver {
                    completed(
                        write(&path, language)
                            .map(|()| language)
                            .map_err(LanguagePreferenceError::from),
                    );
                }
            })?;
        Ok(Self {
            initial,
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    pub fn initial(&self) -> Language {
        self.initial
    }

    /// The host admits one outstanding choice and waits for its completion.
    /// No filesystem work or lock wait occurs on the UI thread.
    pub fn remember(&self, language: Language) -> io::Result<()> {
        self.sender
            .as_ref()
            .expect("live language writer")
            .try_send(language)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    LanguagePreferenceError::Message(Text::LanguagePreferenceBusy),
                )
            })
    }
}

impl Drop for LanguagePreferences {
    fn drop(&mut self) {
        self.sender.take();
        // Drain an accepted write even when the final window closes. Publication
        // never waits for an external lock, so another process cannot stall exit.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        LanguagePreferenceError::Message(Text::LanguagePreferenceInvalid),
    )
}

fn read(path: &Path) -> io::Result<Option<Language>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut text = String::new();
    file.take(129).read_to_string(&mut text)?;
    let mut lines = text.lines();
    if text.len() > 128 || lines.next() != Some(HEADER) {
        return Err(invalid());
    }
    let language = lines
        .next()
        .and_then(Language::from_code)
        .ok_or_else(invalid)?;
    if lines.next().is_some() {
        return Err(invalid());
    }
    Ok(Some(language))
}

fn write(path: &Path, language: Language) -> io::Result<()> {
    let parent = path.parent().ok_or_else(invalid)?;
    fs::create_dir_all(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    lock.try_lock().map_err(io::Error::from)?;
    // Check again under the cross-process publication lock. Preserve foreign,
    // malformed and future-version content even if it changed after startup.
    read(path)?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{next}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        writeln!(file, "{HEADER}\n{}", language.code())?;
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
