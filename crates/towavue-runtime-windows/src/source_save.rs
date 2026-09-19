//! Two-phase source saving. Preparation never changes the target. Publication
//! requires the caller to quiesce every reader and revalidate its tab/edit owner.
mod pasted;

use crate::{
    ExportError, ExportOptions, ExportOutcome, ExportOutput, ExportRequest, FileOperationError,
    FileOperationSource,
};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use thiserror::Error;
use windows::Win32::Storage::FileSystem::{FILE_SHARE_READ, REPLACE_FILE_FLAGS, ReplaceFileW};
use windows::core::PCWSTR;

#[derive(Debug, Error)]
pub enum SourceSaveError {
    #[error("source save failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Source(#[from] FileOperationError),
    #[error(transparent)]
    Export(#[from] ExportError),
    #[error("source save requires the original target path and full-media output")]
    InvalidRequest,
    #[error("source replacement needs recovery: {message}; preserved files: {directory}")]
    RecoveryRequired { message: String, directory: PathBuf },
}

pub enum SourceSaveEvent {
    Progress(Duration),
    AnalyzingAudio(Duration),
    Prepared(Result<PreparedSourceSave, SourceSaveError>),
}

/// Cancellable encoding only. A delivered candidate is still unpublished; a caller
/// that has cancelled or lost ownership must drop it instead of committing it.
pub struct SourceSaveJob {
    cancelled: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SourceSaveJob {
    pub fn start(
        expected: FileOperationSource,
        request: ExportRequest,
        options: ExportOptions,
        notify: impl Fn(SourceSaveEvent) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        Self::spawn(expected, None, request, options, notify)
    }

    pub fn start_recreating(
        expected: FileOperationSource,
        original: RetainedSource,
        request: ExportRequest,
        options: ExportOptions,
        notify: impl Fn(SourceSaveEvent) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        Self::spawn(expected, Some(original), request, options, notify)
    }

    fn spawn(
        expected: FileOperationSource,
        original: Option<RetainedSource>,
        request: ExportRequest,
        options: ExportOptions,
        notify: impl Fn(SourceSaveEvent) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let thread = std::thread::Builder::new()
            .name("towavue-save-prepare".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    prepare(
                        expected,
                        original,
                        request,
                        options,
                        &worker_cancelled,
                        &|time| notify(SourceSaveEvent::Progress(time)),
                        &|time| notify(SourceSaveEvent::AnalyzingAudio(time)),
                    )
                }))
                .unwrap_or_else(|_| {
                    Err(SourceSaveError::Io(io::Error::other(
                        "source-save preparation worker stopped unexpectedly",
                    )))
                });
                notify(SourceSaveEvent::Prepared(result));
            })?;
        Ok(Self {
            cancelled,
            thread: Some(thread),
        })
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
impl Drop for SourceSaveJob {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Files {
    directory: PathBuf,
    prepared: PathBuf,
    original: PathBuf,
    preserve: AtomicBool,
}

impl Files {
    fn new(target: &Path) -> io::Result<Arc<Self>> {
        let parent = target
            .parent()
            .ok_or_else(|| io::Error::other("source folder unavailable"))?;
        Self::under(target, parent, "save")
    }

    fn under(target: &Path, parent: &Path, purpose: &str) -> io::Result<Arc<Self>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let directory = parent.join(format!(
                ".towavue-{purpose}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&directory) {
                Ok(()) => {
                    let extension = target.extension().unwrap_or_default();
                    return Ok(Arc::new(Self {
                        prepared: directory.join("prepared").with_extension(extension),
                        original: directory.join("original").with_extension(extension),
                        directory,
                        preserve: AtomicBool::new(false),
                    }));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        if self.preserve.load(Ordering::Relaxed) {
            return;
        }
        let directory = self.directory.clone();
        let prepared = self.prepared.clone();
        let original = self.original.clone();
        // A retained original can outlive the save worker. Final cleanup runs off
        // the UI thread and removes only our two fixed files, never a directory tree.
        let _ = std::thread::Builder::new()
            .name("towavue-save-cleanup".into())
            .spawn(move || {
                let _ = fs::remove_file(prepared);
                let _ = fs::remove_file(original);
                let _ = fs::remove_dir(directory);
            });
    }
}

/// Owned candidate; dropping before commit cancels publication and cleans staging.
/// Not Clone: a candidate can be submitted for replacement at most once.
pub struct PreparedSourceSave {
    expected: FileOperationSource,
    candidate: FileOperationSource,
    files: Arc<Files>,
    outcome: ExportOutcome,
    recreation: Option<RetainedSource>,
}

struct Original {
    // Drop the read-only lease before the final staging-directory owner. Readers
    // may share it; writers and renamers cannot mutate this retained undo source.
    _lease: File,
    source: FileOperationSource,
    _files: Arc<Files>,
}

/// Shared immutable bytes for an open document after Save or file deletion.
/// Capture is blocking and belongs on a worker, before accepting deletion.
#[derive(Clone)]
pub struct RetainedSource(Arc<Original>);

impl RetainedSource {
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "Windows-only: clear FILE_ATTRIBUTE_READONLY on the owned copy for final cleanup; no Unix mode bits"
    )]
    pub fn capture(expected: &FileOperationSource) -> Result<Self, FileOperationError> {
        let _source_lease = expected.verify_for_copy()?;
        let files = Files::under(expected.path(), &std::env::temp_dir(), "retained")?;
        // Windows CopyFile also keeps the source's named streams. The strong
        // source lease prevents a path swap or write during the entire copy.
        fs::copy(expected.path(), &files.original)?;
        let mut permissions = fs::metadata(&files.original)?.permissions();
        if permissions.readonly() {
            permissions.set_readonly(false);
            fs::set_permissions(&files.original, permissions)?;
        }
        let source = FileOperationSource::capture(&files.original)?;
        let lease = source.verify_for_copy()?;
        expected.verify()?;
        Ok(Self(Arc::new(Original {
            _lease: lease,
            source,
            _files: files,
        })))
    }

    pub(crate) fn preserve_for_recovery(&self) -> PathBuf {
        self.0._files.preserve.store(true, Ordering::Relaxed);
        self.0._files.directory.clone()
    }

    pub fn original_path(&self) -> &Path {
        self.0.source.path()
    }

    pub fn original_source(&self) -> &FileOperationSource {
        &self.0.source
    }
}

impl std::fmt::Debug for RetainedSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("RetainedSource")
            .field(&self.original_path())
            .finish()
    }
}

impl From<SavedSource> for RetainedSource {
    fn from(saved: SavedSource) -> Self {
        Self(saved.original)
    }
}

/// Keeps the pre-save source available to non-destructive Undo/other hosted owners.
/// Callers must keep this alive for every decoder/export using original_path().
#[derive(Clone)]
pub struct SavedSource {
    original: Arc<Original>,
    current: FileOperationSource,
    pub outcome: ExportOutcome,
}

impl SavedSource {
    pub fn retained_source(&self) -> RetainedSource {
        RetainedSource(Arc::clone(&self.original))
    }

    pub fn original_path(&self) -> &Path {
        self.original.source.path()
    }
    pub fn original_source(&self) -> &FileOperationSource {
        &self.original.source
    }
    pub fn current_source(&self) -> &FileOperationSource {
        &self.current
    }
}

/// Blocking preparation for an export worker, never the UI event loop. `expected`
/// must identify the target version loaded by the owning document. `request.source`
/// may be its retained original after a previous save; its target must stay logical.
/// The existing export path keeps its source-clobber rejection unchanged.
pub fn prepare_source_save(
    expected: FileOperationSource,
    request: ExportRequest,
    options: ExportOptions,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
    analyzing: &(impl Fn(Duration) + Sync),
) -> Result<PreparedSourceSave, SourceSaveError> {
    prepare(
        expected, None, request, options, cancelled, progress, analyzing,
    )
}

/// Prepare a Save that recreates a deleted document at its original path.
/// The expected snapshot identifies that logical path, not an existing target;
/// encoding must read the retained original and publication must never replace.
pub fn prepare_source_recreation(
    expected: FileOperationSource,
    original: RetainedSource,
    request: ExportRequest,
    options: ExportOptions,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
    analyzing: &(impl Fn(Duration) + Sync),
) -> Result<PreparedSourceSave, SourceSaveError> {
    prepare(
        expected,
        Some(original),
        request,
        options,
        cancelled,
        progress,
        analyzing,
    )
}

fn prepare(
    expected: FileOperationSource,
    recreation: Option<RetainedSource>,
    mut request: ExportRequest,
    options: ExportOptions,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
    analyzing: &(impl Fn(Duration) + Sync),
) -> Result<PreparedSourceSave, SourceSaveError> {
    if request.target != expected.path() || options.output != ExportOutput::Media {
        return Err(SourceSaveError::InvalidRequest);
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled.into());
    }
    let _source_lease = if let Some(original) = &recreation {
        if request.source != original.original_path() {
            return Err(SourceSaveError::InvalidRequest);
        }
        require_missing(expected.path())?;
        original.original_source().verify()?
    } else {
        expected.verify()?
    };
    let files = Files::new(expected.path())?;
    request.target = files.prepared.clone();
    let outcome = if recreation.is_some()
        && request.operations.is_empty()
        && options == ExportOptions::default()
    {
        // Restore an untouched document byte-for-byte, including container data
        // and named streams. A default Save must not add a lossy codec generation.
        fs::copy(&request.source, &request.target)?;
        ExportOutcome {
            used_hardware_encoder: false,
        }
    } else {
        crate::export::export_options_cancellable(
            &request, options, cancelled, progress, analyzing,
        )?
    };
    if recreation.is_some() {
        require_missing(expected.path())?;
    } else {
        expected.verify()?;
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled.into());
    }
    let candidate = FileOperationSource::capture(&files.prepared)?;
    Ok(PreparedSourceSave {
        expected,
        candidate,
        files,
        outcome,
        recreation,
    })
}

fn require_missing(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the deleted document's path is occupied; use Export as or choose another location",
        )),
    }
}

/// One accepted, non-cancellable publication. The callback owns either the retained
/// original or a recovery error; do not discard its result during tab/window closure.
pub fn commit_source_save(
    prepared: PreparedSourceSave,
    notify: impl FnOnce(Result<SavedSource, SourceSaveError>) + Send + 'static,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name("towavue-source-save".into())
        .spawn(move || {
            let files = Arc::clone(
                prepared
                    .recreation
                    .as_ref()
                    .map_or(&prepared.files, |original| &original.0._files),
            );
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| commit(prepared)))
                    .unwrap_or_else(|_| {
                        files.preserve.store(true, Ordering::Relaxed);
                        Err(SourceSaveError::RecoveryRequired {
                            message: "publication worker stopped unexpectedly".into(),
                            directory: files.directory.clone(),
                        })
                    });
            notify(result);
        })?;
    Ok(())
}

fn commit(prepared: PreparedSourceSave) -> Result<SavedSource, SourceSaveError> {
    if prepared.recreation.is_some() {
        return commit_recreation(prepared);
    }
    // Both leases deny writes while identities are checked. ReplaceFile opens the
    // files without sharing; release our leases immediately before its call. This
    // revalidation is not a filesystem compare-and-swap against external renamers.
    let source = prepared.expected.verify()?;
    let candidate = prepared.candidate.verify()?;
    drop(candidate);
    drop(source);
    prepared.files.preserve.store(true, Ordering::Relaxed);
    let result = replace(&prepared.expected, &prepared.files);
    finish_publication(prepared, result)
}

fn commit_recreation(prepared: PreparedSourceSave) -> Result<SavedSource, SourceSaveError> {
    let original = prepared.recreation.as_ref().expect("recreation input");
    let _original_lease = original.original_source().verify()?;
    require_missing(prepared.expected.path())?;
    let candidate = prepared.candidate.verify()?;
    drop(candidate);
    // Same-volume rename without replacement closes the destination-creation
    // race. A late file, directory or reparse point wins without being modified.
    restore_missing(&prepared.files.prepared, prepared.expected.path())?;
    // A plain rename must preserve length, write time and same-volume file ID;
    // unlike ReplaceFile, it does not merge timestamps or named streams.
    let current = prepared.candidate.after_move(prepared.expected.path());
    match current {
        Ok(current) => Ok(SavedSource {
            original: Arc::clone(&original.0),
            current,
            outcome: prepared.outcome,
        }),
        Err(error) => {
            // Publication happened but its final identity cannot be certified.
            // Preserve the immutable original even if the caller drops its owner.
            original.0._files.preserve.store(true, Ordering::Relaxed);
            Err(SourceSaveError::RecoveryRequired {
                message: error.to_string(),
                directory: original.0._files.directory.clone(),
            })
        }
    }
}

fn replace(expected: &FileOperationSource, files: &Files) -> io::Result<()> {
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let target = wide(expected.path());
    let candidate = wide(&files.prepared);
    let original = wide(&files.original);
    // SAFETY: this worker owns every terminated UTF-16 buffer through the call.
    // All three paths are siblings on the same volume; no pointer escapes.
    unsafe {
        ReplaceFileW(
            PCWSTR(target.as_ptr()),
            PCWSTR(candidate.as_ptr()),
            PCWSTR(original.as_ptr()),
            REPLACE_FILE_FLAGS(0),
            None,
            None,
        )
    }
    .map_err(io::Error::from)
}

fn finish_publication(
    prepared: PreparedSourceSave,
    result: io::Result<()>,
) -> Result<SavedSource, SourceSaveError> {
    if let Err(error) = result {
        // Some ReplaceFile failures leave the old source at the backup name.
        // Restore only into an absent destination, never over an external file.
        if matches!(prepared.expected.path().try_exists(), Ok(false))
            && prepared
                .expected
                .matches_file_at(&prepared.files.original)
                .unwrap_or(false)
        {
            let _ = restore_missing(&prepared.files.original, prepared.expected.path());
        }
        if prepared
            .expected
            .matches_file_at(prepared.expected.path())
            .unwrap_or(false)
        {
            prepared.files.preserve.store(false, Ordering::Relaxed);
            return Err(SourceSaveError::Io(error));
        }
        return Err(SourceSaveError::RecoveryRequired {
            message: error.to_string(),
            directory: prepared.files.directory.clone(),
        });
    }
    let retained = (|| {
        if !prepared
            .candidate
            .matches_published_file_at(prepared.expected.path())?
            || !prepared
                .expected
                .matches_file_at(&prepared.files.original)?
        {
            return Err(SourceSaveError::Io(io::Error::other(
                "replacement identities changed",
            )));
        }
        let source = FileOperationSource::capture(&prepared.files.original)?;
        let lease = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(source.path())?;
        source.verify()?;
        let current = FileOperationSource::capture(prepared.expected.path())?;
        Ok((source, lease, current))
    })();
    match retained {
        Ok((source, lease, current)) => {
            prepared.files.preserve.store(false, Ordering::Relaxed);
            Ok(SavedSource {
                original: Arc::new(Original {
                    _lease: lease,
                    source,
                    _files: prepared.files,
                }),
                current,
                outcome: prepared.outcome,
            })
        }
        Err(error) => Err(SourceSaveError::RecoveryRequired {
            message: error.to_string(),
            directory: prepared.files.directory.clone(),
        }),
    }
}

fn restore_missing(original: &Path, target: &Path) -> io::Result<()> {
    use windows::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let source = wide(original);
    let destination = wide(target);
    // SAFETY: live terminated buffers; deliberately omit replacement flags so a
    // concurrently-created target is never clobbered by recovery.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(io::Error::from)
}

#[cfg(test)]
mod tests;
