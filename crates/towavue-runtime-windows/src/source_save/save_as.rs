//! Save as retains the document input separately from a destination's old bytes.
//! Preparation cannot publish. The host validates ownership and drains destination
//! readers before accepting the single non-cancellable publication.
use super::*;
use crate::MediaInput;

/// Capture on the worker that accepts the selected destination, before queued
/// preparation. Never recapture a changed target to authorize replacement.
#[derive(Clone, Debug)]
pub enum SaveAsTarget {
    New(PathBuf),
    Existing(FileOperationSource),
}
impl SaveAsTarget {
    pub fn capture(path: &Path) -> Result<Self, SourceSaveError> {
        let path = std::path::absolute(path)?;
        match fs::symlink_metadata(&path) {
            Ok(_) => Ok(Self::Existing(FileOperationSource::capture(&path)?)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::New(path)),
            Err(error) => Err(error.into()),
        }
    }
    pub fn path(&self) -> &Path {
        match self {
            Self::New(path) => path,
            Self::Existing(source) => source.path(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SaveAsRequest {
    pub input: MediaInput,
    pub source_version: Option<FileOperationSource>,
    pub target: SaveAsTarget,
    pub export: ExportRequest,
    pub options: ExportOptions,
}

pub enum SaveAsEvent {
    Progress(Duration),
    AnalyzingAudio(Duration),
    Prepared(Result<PreparedSaveAs, SourceSaveError>),
}

pub struct SaveAsJob {
    cancelled: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl SaveAsJob {
    pub fn start(
        request: SaveAsRequest,
        notify: impl Fn(SaveAsEvent) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let thread = std::thread::Builder::new()
            .name("towavue-save-as-prepare".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    prepare_save_as(
                        request,
                        &worker_cancelled,
                        &|time| notify(SaveAsEvent::Progress(time)),
                        &|time| notify(SaveAsEvent::AnalyzingAudio(time)),
                    )
                }))
                .unwrap_or_else(|_| {
                    Err(SourceSaveError::Io(io::Error::other(
                        "Save as preparation worker stopped unexpectedly",
                    )))
                });
                notify(SaveAsEvent::Prepared(result));
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
impl Drop for SaveAsJob {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

enum Destination {
    Existing(PreparedSourceSave),
    New {
        path: PathBuf,
        files: Arc<Files>,
        candidate: FileOperationSource,
        outcome: ExportOutcome,
    },
}

/// One unpublished candidate plus its immutable undo input. Dropping cancels it.
/// Never Clone: exactly one host may accept and publish this candidate.
pub struct PreparedSaveAs {
    original: RetainedSource,
    destination: Destination,
}
impl PreparedSaveAs {
    pub fn target(&self) -> &Path {
        match &self.destination {
            Destination::Existing(prepared) => prepared.expected.path(),
            Destination::New { path, .. } => path,
        }
    }
    pub fn expected_target(&self) -> Option<&FileOperationSource> {
        match &self.destination {
            Destination::Existing(prepared) => Some(&prepared.expected),
            Destination::New { .. } => None,
        }
    }
    fn files(&self) -> &Arc<Files> {
        match &self.destination {
            Destination::Existing(prepared) => &prepared.files,
            Destination::New { files, .. } => files,
        }
    }
}

/// The owner adopts current_source's path but continues editing retained_source.
/// Other already-loaded destination documents keep replaced_source's old input.
#[derive(Clone)]
pub struct SavedAsSource {
    original: RetainedSource,
    current: FileOperationSource,
    replaced: Option<SavedSource>,
    pub outcome: ExportOutcome,
}
impl SavedAsSource {
    pub fn retained_source(&self) -> RetainedSource {
        self.original.clone()
    }
    pub fn current_source(&self) -> &FileOperationSource {
        &self.current
    }
    pub fn replaced_source(&self) -> Option<&SavedSource> {
        self.replaced.as_ref()
    }
}

/// Blocking worker-only preparation. An unbacked source must match the version
/// actually loaded by the document, not a new snapshot taken when Save as starts.
/// Existing retained/untitled originals reuse their immutable lease. Cancellation
/// is checked around original copying and throughout the existing encode path.
pub fn prepare_save_as(
    plan: SaveAsRequest,
    cancelled: &AtomicBool,
    progress: &(impl Fn(Duration) + Sync),
    analyzing: &(impl Fn(Duration) + Sync),
) -> Result<PreparedSaveAs, SourceSaveError> {
    check_cancelled(cancelled)?;
    let SaveAsRequest {
        input,
        source_version,
        target,
        export: mut request,
        options,
    } = plan;
    if request.source != input.logical_path() || options.output != ExportOutput::Media {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Save as requires the current document input and full-media output",
        )
        .into());
    }
    request.target = std::path::absolute(&request.target)?;
    if request.target != target.path() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Save as destination changed after selection",
        )
        .into());
    }
    if crate::export::same_path(input.logical_path(), &request.target)
        || crate::export::same_path(input.path(), &request.target)
    {
        return Err(ExportError::SameAsSource.into());
    }
    // Revalidate the selection snapshot before copying, and again during
    // preparation/publication. A late-created or replaced destination wins.
    let expected_target = match target {
        SaveAsTarget::Existing(expected) => {
            expected.verify()?;
            Some(expected)
        }
        SaveAsTarget::New(path) => {
            require_missing(&path)?;
            None
        }
    };
    let original = if let Some(original) = input.retained_source() {
        original.original_source().verify()?;
        original.clone()
    } else {
        let expected = source_version
            .as_ref()
            .filter(|expected| crate::export::same_path(expected.path(), input.path()))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "The loaded source version is unavailable; reopen it before Save as",
                )
            })?;
        RetainedSource::capture(expected)?
    };
    check_cancelled(cancelled)?;
    request.source = original.original_path().to_owned();
    let destination = if let Some(expected) = expected_target {
        Destination::Existing(prepare_source_save(
            expected, request, options, cancelled, progress, analyzing,
        )?)
    } else {
        require_missing(&request.target)?;
        let path = request.target.clone();
        let files = Files::new(&path)?;
        request.target = files.prepared.clone();
        let outcome = crate::export::export_options_cancellable(
            &request, options, cancelled, progress, analyzing,
        )?;
        check_cancelled(cancelled)?;
        require_missing(&path)?;
        let candidate = FileOperationSource::capture(&files.prepared)?;
        Destination::New {
            path,
            files,
            candidate,
            outcome,
        }
    };
    Ok(PreparedSaveAs {
        original,
        destination,
    })
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), SourceSaveError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(ExportError::Cancelled.into())
    } else {
        Ok(())
    }
}

/// Accept once after host ownership checks and destination-reader draining.
/// Publication is no longer cancellable and always delivers its one result.
pub fn commit_save_as(
    prepared: PreparedSaveAs,
    notify: impl FnOnce(Result<SavedAsSource, SourceSaveError>) + Send + 'static,
) -> io::Result<()> {
    let worker = std::thread::Builder::new().name("towavue-save-as-publish".into());
    worker.spawn(move || {
        let files = Arc::clone(prepared.files());
        let original = prepared.original.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| commit_as(prepared)))
            .unwrap_or_else(|_| {
                files.preserve.store(true, Ordering::Relaxed);
                Err(SourceSaveError::RecoveryRequired {
                    message: "Save as publication stopped unexpectedly".into(),
                    directory: files.directory.clone(),
                })
            });
        let result = retain_recovery_original(&original, result);
        if let Ok(saved) = &result {
            crate::shell::notify_published_file(saved.current_source().path());
        }
        notify(result);
    })?;
    Ok(())
}

fn retain_recovery_original(
    original: &RetainedSource,
    mut result: Result<SavedAsSource, SourceSaveError>,
) -> Result<SavedAsSource, SourceSaveError> {
    if let Err(SourceSaveError::RecoveryRequired { message, .. }) = &mut result {
        // A destination backup contains another document's bytes. Preserve this
        // document's original too, including native partial failures, not just
        // worker panics. Untitled inputs may have no other persistent copy.
        let directory = original.preserve_for_recovery();
        *message = format!(
            "{message}; document original retained in {}",
            directory.display()
        );
    }
    result
}

fn commit_as(prepared: PreparedSaveAs) -> Result<SavedAsSource, SourceSaveError> {
    let PreparedSaveAs {
        original,
        destination,
    } = prepared;
    let _original_lease = original.original_source().verify()?;
    match destination {
        Destination::Existing(prepared) => {
            let saved = commit(prepared)?;
            Ok(SavedAsSource {
                original,
                current: saved.current_source().clone(),
                outcome: saved.outcome,
                replaced: Some(saved),
            })
        }
        Destination::New {
            path,
            files,
            candidate,
            outcome,
        } => {
            require_missing(&path)?;
            drop(candidate.verify()?);
            restore_missing(&files.prepared, &path)?;
            let current =
                candidate
                    .after_move(&path)
                    .map_err(|error| SourceSaveError::RecoveryRequired {
                        message: error.to_string(),
                        directory: original.preserve_for_recovery(),
                    })?;
            Ok(SavedAsSource {
                original,
                current,
                outcome,
                replaced: None,
            })
        }
    }
}

#[cfg(test)]
mod tests;
