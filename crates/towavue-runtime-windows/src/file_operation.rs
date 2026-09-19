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
    RenameToPath(PathBuf),
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
    #[error(
        "file deletion failed and the original source cannot be verified: {message}; retained file directory: {directory}"
    )]
    RecoveryRequired { message: String, directory: PathBuf },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Stamp {
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileOperationSource {
    path: PathBuf,
    stamp: Stamp,
}

impl FileOperationSource {
    pub(crate) fn stamp(&self) -> Stamp {
        self.stamp
    }

    pub(crate) fn from_stamp(path: PathBuf, stamp: Stamp) -> Self {
        Self { path, stamp }
    }

    /// Follow a logical rename without authorizing newly observed bytes. The
    /// original native identity/stamp must still pass verification at this path.
    pub fn with_path(&self, path: PathBuf) -> Self {
        Self {
            path,
            stamp: self.stamp,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Blocking inspection for worker callers. UI code should use the detached
    /// inspect_file_operation_source service instead.
    pub fn capture(path: &Path) -> Result<Self, FileOperationError> {
        let path = std::path::absolute(path)?;
        validate_name(path.file_name().ok_or(FileOperationError::InvalidName)?)?;
        let file = open_source(&path)?;
        let stamp = Stamp::read(&file)?;
        Ok(Self { path, stamp })
    }

    /// Inspect the destination on the mutation worker after a successful move.
    /// Same-volume moves retain file identity; cross-volume copies can have a new
    /// ID/creation time, but must retain the verified content length and write time.
    /// This comparison is not a transaction against noncooperating external writers.
    pub fn after_move(&self, path: &Path) -> Result<Self, FileOperationError> {
        let current = Self::capture(path)?;
        if current.stamp.length != self.stamp.length
            || current.stamp.written != self.stamp.written
            || (current.stamp.volume == self.stamp.volume
                && current.stamp.index != self.stamp.index)
        {
            return Err(FileOperationError::SourceChanged);
        }
        Ok(current)
    }

    /// Replacement can inherit a different creation time; file identity, length
    /// and last-write time still bind the actual source/output bytes.
    pub(crate) fn matches_file_at(&self, path: &Path) -> Result<bool, FileOperationError> {
        let actual = Stamp::read(&open_source(path)?)?;
        Ok(actual.volume == self.stamp.volume
            && actual.index == self.stamp.index
            && actual.length == self.stamp.length
            && actual.written == self.stamp.written)
    }

    /// ReplaceFile can update both creation and write times when merging named
    /// streams. Only use this after native success and strict pre-call verification.
    pub(crate) fn matches_published_file_at(
        &self,
        path: &Path,
    ) -> Result<bool, FileOperationError> {
        let actual = Stamp::read(&open_source(path)?)?;
        Ok(actual.volume == self.stamp.volume
            && actual.index == self.stamp.index
            && actual.length == self.stamp.length)
    }

    pub(crate) fn verify(&self) -> Result<File, FileOperationError> {
        self.verify_with_share(FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0)
    }

    // A snapshot copy must bind both its bytes and its path throughout CopyFile.
    // Unlike rename/recycle verification, deny external renames as well as writes.
    pub(crate) fn verify_for_copy(&self) -> Result<File, FileOperationError> {
        self.verify_with_share(FILE_SHARE_READ.0)
    }

    fn verify_with_share(&self, share: u32) -> Result<File, FileOperationError> {
        let file = open_source_with_share(&self.path, share)?;
        if Stamp::read(&file)? != self.stamp {
            return Err(FileOperationError::SourceChanged);
        }
        Ok(file)
    }
}

fn open_source(path: &Path) -> Result<File, FileOperationError> {
    open_source_with_share(path, FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0)
}

fn open_source_with_share(path: &Path, share: u32) -> Result<File, FileOperationError> {
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
        .share_mode(share)
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

#[derive(Clone, Debug)]
pub struct FileRecycleReport {
    pub before: towavue_core::FolderSnapshot,
    pub after: Option<towavue_core::FolderSnapshot>,
    pub retained_source: Option<crate::RetainedSource>,
}

/// Enumerate on the Shell worker before and after the accepted recycle. A failed
/// post-delete enumeration must not turn a completed deletion into a failed mutation.
pub fn start_file_recycling(
    source: FileOperationSource,
    notify: impl FnOnce(Result<FileRecycleReport, FileOperationError>) + Send + 'static,
) -> io::Result<()> {
    start_recycling(source, false, notify)
}

/// Retain an immutable copy before recycling a file still owned by a document.
/// Failure to retain leaves the source untouched. The report owns the copy until
/// the final document/reader drops it; no recycle-bin lookup is needed afterward.
pub fn start_file_recycling_retaining_source(
    source: FileOperationSource,
    notify: impl FnOnce(Result<FileRecycleReport, FileOperationError>) + Send + 'static,
) -> io::Result<()> {
    start_recycling(source, true, notify)
}

fn start_recycling(
    source: FileOperationSource,
    retain: bool,
    notify: impl FnOnce(Result<FileRecycleReport, FileOperationError>) + Send + 'static,
) -> io::Result<()> {
    worker(
        move || {
            let folder = source
                .path
                .parent()
                .ok_or(FileOperationError::InvalidName)?;
            let mut order = crate::FolderOrderProvider::new()
                .map_err(|error| io::Error::other(error.to_string()))?;
            let before = order
                .snapshot(folder)
                .map_err(|error| io::Error::other(error.to_string()))?;
            let retained_source = retain
                .then(|| crate::RetainedSource::capture(&source))
                .transpose()?;
            // Keep ownership outside the mutation's unwind boundary. A failed
            // or panicking native call may already have removed the source.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                perform(&source, FileOperationAction::Recycle)
            }))
            .unwrap_or(Err(FileOperationError::WorkerStopped));
            finish_retained_recycle(&source, retained_source.as_ref(), result)?;
            let after =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| order.snapshot(folder)))
                    .ok()
                    .and_then(Result::ok);
            Ok(FileRecycleReport {
                before,
                after,
                retained_source,
            })
        },
        notify,
    )
}

pub(crate) fn finish_retained_recycle(
    source: &FileOperationSource,
    retained: Option<&crate::RetainedSource>,
    result: Result<FileOperationOutcome, FileOperationError>,
) -> Result<(), FileOperationError> {
    match result {
        Err(error) if retained.is_some() && source.verify().is_err() => {
            Err(FileOperationError::RecoveryRequired {
                message: error.to_string(),
                directory: retained.expect("retained input").preserve_for_recovery(),
            })
        }
        result => result.map(|_| ()),
    }
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
        FileOperationAction::RenameToPath(target) => {
            let name = target.file_name().ok_or(FileOperationError::InvalidName)?;
            validate_name(name)?;
            if target.parent().map(std::fs::canonicalize).transpose()?
                != source
                    .path
                    .parent()
                    .map(std::fs::canonicalize)
                    .transpose()?
            {
                return Err(FileOperationError::InvalidName);
            }
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
