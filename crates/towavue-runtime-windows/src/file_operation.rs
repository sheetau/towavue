//! Single-file mutations. Callers retain their tab/source identity until completion;
//! neither successful submission nor a copied destination proves a completed move.
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
use std::path::{Path, PathBuf};
use std::thread;

use thiserror::Error;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, GetFileInformationByHandle, MOVEFILE_COPY_ALLOWED,
    MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows::core::PCWSTR;

#[derive(Clone, Debug)]
pub enum FileOperationAction {
    Rename(OsString),
    MoveToFolder(PathBuf),
    Recycle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileOperationOutcome {
    Moved(PathBuf),
    Recycled,
    /// Windows can report a cross-volume move as successful even when deleting
    /// its source failed. Keep viewing the source and report both surviving files.
    CopiedButSourceRetained(PathBuf),
}

#[derive(Debug, Error)]
pub enum FileOperationError {
    #[error("file operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("Windows file operation failed: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("the source changed; reopen it before changing the file")]
    SourceChanged,
    #[error("the source must be a regular file, not a folder or reparse point")]
    NotRegularFile,
    #[error("enter one valid file name, without a folder or reserved Windows name")]
    InvalidName,
    #[error("the destination already exists; choose a different name or folder")]
    DestinationExists,
    #[error("Windows cancelled or did not complete the file operation")]
    NotCompleted,
    #[error("the file-operation worker stopped unexpectedly")]
    WorkerStopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Stamp {
    volume: u32,
    index: u64,
    length: u64,
    created: u64,
    written: u64,
}

impl Stamp {
    fn read(file: &File) -> Result<Self, FileOperationError> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: File retains its handle while the synchronous query writes the
        // matching native structure. No handle or pointer leaves this module.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }?;
        if !file.metadata()?.is_file()
            || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        {
            return Err(FileOperationError::NotRegularFile);
        }
        let pair = |high: u32, low: u32| (u64::from(high) << 32) | u64::from(low);
        Ok(Self {
            volume: info.dwVolumeSerialNumber,
            index: pair(info.nFileIndexHigh, info.nFileIndexLow),
            length: pair(info.nFileSizeHigh, info.nFileSizeLow),
            created: pair(
                info.ftCreationTime.dwHighDateTime,
                info.ftCreationTime.dwLowDateTime,
            ),
            written: pair(
                info.ftLastWriteTime.dwHighDateTime,
                info.ftLastWriteTime.dwLowDateTime,
            ),
        })
    }
}

/// A detached identity snapshot taken before presenting the operation's dialog.
/// It owns no native resources and can cross threads. Inspection performs IO;
/// use inspect_file_operation_source from the UI rather than blocking on capture.
#[derive(Clone, Debug)]
pub struct FileOperationSource {
    path: PathBuf,
    stamp: Stamp,
}

impl FileOperationSource {
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn capture(path: &Path) -> Result<Self, FileOperationError> {
        let path = std::path::absolute(path)?;
        validate_name(path.file_name().ok_or(FileOperationError::InvalidName)?)?;
        let file = open_source(&path)?;
        let stamp = Stamp::read(&file)?;
        Ok(Self { path, stamp })
    }

    fn verify(&self) -> Result<File, FileOperationError> {
        let file = open_source(&self.path)?;
        if Stamp::read(&file)? != self.stamp {
            return Err(FileOperationError::SourceChanged);
        }
        Ok(file)
    }
}

fn open_source(path: &Path) -> Result<File, FileOperationError> {
    // Do not dereference a selected file symlink. Parent folders still follow
    // normal Windows resolution. Deny competing data writers during the action,
    // but permit Shell/MoveFileEx to rename or recycle the verified file.
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(FileOperationError::NotRegularFile);
    }
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0)
        .open(path)?)
}

pub fn inspect_file_operation_source(
    path: PathBuf,
    notify: impl FnOnce(Result<FileOperationSource, FileOperationError>) + Send + 'static,
) -> io::Result<()> {
    worker(move || FileOperationSource::capture(&path), notify)
}

/// Starts exactly one accepted mutation. The caller must keep its operation
/// pending until notify, including during close requests. There is no latest-only
/// replacement or implicit cancellation that could hide a completed mutation.
pub fn start_file_operation(
    source: FileOperationSource,
    action: FileOperationAction,
    notify: impl FnOnce(Result<FileOperationOutcome, FileOperationError>) + Send + 'static,
) -> io::Result<()> {
    worker(move || perform(&source, action), notify)
}

fn worker<T: Send + 'static>(
    run: impl FnOnce() -> Result<T, FileOperationError> + Send + 'static,
    notify: impl FnOnce(Result<T, FileOperationError>) + Send + 'static,
) -> io::Result<()> {
    thread::Builder::new()
        .name("towavue-file-operation".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
                .unwrap_or(Err(FileOperationError::WorkerStopped));
            notify(result);
        })?;
    Ok(())
}

fn perform(
    source: &FileOperationSource,
    action: FileOperationAction,
) -> Result<FileOperationOutcome, FileOperationError> {
    let lease = source.verify()?;
    let target = match action {
        FileOperationAction::Rename(name) => {
            validate_name(&name)?;
            source.path.with_file_name(name)
        }
        FileOperationAction::MoveToFolder(folder) => {
            let folder = std::path::absolute(folder)?;
            if !folder.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "destination folder is unavailable",
                )
                .into());
            }
            folder.join(
                source
                    .path
                    .file_name()
                    .ok_or(FileOperationError::InvalidName)?,
            )
        }
        FileOperationAction::Recycle => {
            recycle::recycle(&source.path)?;
            drop(lease);
            return if source.path.try_exists()? {
                Err(FileOperationError::NotCompleted)
            } else {
                Ok(FileOperationOutcome::Recycled)
            };
        }
    };
    if target == source.path {
        return Ok(FileOperationOutcome::Moved(target));
    }
    // MoveFileEx itself rejects a destination that appears after this check.
    // Never set REPLACE_EXISTING or emulate a move by first removing the target.
    let same_entry = target.try_exists()?
        && std::fs::canonicalize(&target)? == std::fs::canonicalize(&source.path)?;
    if target.try_exists()? && !same_entry {
        return Err(FileOperationError::DestinationExists);
    }
    let source_wide = wide(&source.path)?;
    let target_wide = wide(&target)?;
    // SAFETY: both terminated path buffers and the source lease remain alive
    // through the synchronous operation. This runs off the UI thread.
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(target_wide.as_ptr()),
            MOVEFILE_COPY_ALLOWED | MOVEFILE_WRITE_THROUGH,
        )?;
    }
    drop(lease);
    move_outcome(&source.path, target, same_entry)
}

fn move_outcome(
    source: &Path,
    target: PathBuf,
    same_entry: bool,
) -> Result<FileOperationOutcome, FileOperationError> {
    if !target.is_file() {
        return Err(FileOperationError::NotCompleted);
    }
    if !same_entry && source.try_exists()? {
        Ok(FileOperationOutcome::CopiedButSourceRetained(target))
    } else {
        Ok(FileOperationOutcome::Moved(target))
    }
}

fn validate_name(name: &std::ffi::OsStr) -> Result<(), FileOperationError> {
    let text = name.to_str().ok_or(FileOperationError::InvalidName)?;
    let stem = text
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|number| {
                number.len() == 1 && matches!(number.as_bytes()[0], b'1'..=b'9')
            })
        });
    if text.is_empty()
        || text.ends_with(['.', ' '])
        || text.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c))
        || reserved
    {
        Err(FileOperationError::InvalidName)
    } else {
        Ok(())
    }
}

fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut result: Vec<_> = path.as_os_str().encode_wide().collect();
    if result.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains a null character",
        ));
    }
    result.push(0);
    Ok(result)
}

mod recycle;

#[cfg(test)]
mod tests;
