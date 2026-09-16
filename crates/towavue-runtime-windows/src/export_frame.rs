use super::*;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use towavue_core::MediaTime;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    FILE_BASIC_INFO, FILE_ID_INFO, FILE_SHARE_READ, FileBasicInfo, FileIdInfo,
    GetFileInformationByHandleEx,
};

/// Identity of the current picture. Only the playback runtime can construct it;
/// copying it does not retain GPU pixels, a decoder, or a file lock.
#[derive(Clone, Debug)]
pub struct VideoFrameSnapshot {
    pub(crate) source: Arc<FrameSource>,
    pub(crate) time: MediaTime,
}

impl VideoFrameSnapshot {
    pub fn source_path(&self) -> &Path {
        &self.source.path
    }
    pub fn source_time(&self) -> MediaTime {
        self.time
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct FrameSource {
    path: PathBuf,
    stamp: Stamp,
}

#[derive(Debug, Eq, PartialEq)]
struct Stamp {
    volume: u64,
    id: [u8; 16],
    length: u64,
    created: i64,
    written: i64,
    changed: i64,
}

impl Stamp {
    fn read(file: &File) -> io::Result<Self> {
        let mut identity = FILE_ID_INFO::default();
        let mut basic = FILE_BASIC_INFO::default();
        // SAFETY: the borrowed File owns this handle throughout both synchronous
        // queries; each pointer addresses its matching, initialized native type.
        unsafe {
            let handle = HANDLE(file.as_raw_handle());
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&raw mut identity).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
            .map_err(io::Error::other)?;
            GetFileInformationByHandleEx(
                handle,
                FileBasicInfo,
                (&raw mut basic).cast(),
                size_of::<FILE_BASIC_INFO>() as u32,
            )
            .map_err(io::Error::other)?;
        }
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::other("frame source is not a regular file"));
        }
        Ok(Self {
            volume: identity.VolumeSerialNumber,
            id: identity.FileId.Identifier,
            length: metadata.len(),
            created: basic.CreationTime,
            written: basic.LastWriteTime,
            changed: basic.ChangeTime,
        })
    }

    fn same_file(&self, other: &Self) -> bool {
        self.volume == other.volume && self.id == other.id
    }
}

/// A short-lived read lease used during decoder opening or a frame export, never
/// for the whole playback session. Windows rejects competing data writes/deletes.
pub(crate) struct SourceLease {
    file: File,
    pub(crate) source: Arc<FrameSource>,
}

impl SourceLease {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let path = std::path::absolute(path)?;
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(&path)?;
        let stamp = Stamp::read(&file)?;
        Ok(Self {
            file,
            source: Arc::new(FrameSource { path, stamp }),
        })
    }

    pub(crate) fn verify_path(&self) -> io::Result<()> {
        let current = File::open(&self.source.path)?;
        if Stamp::read(&current)? != self.source.stamp
            || Stamp::read(&self.file)? != self.source.stamp
        {
            return Err(io::Error::other(
                "the frame source changed; reopen it before exporting",
            ));
        }
        Ok(())
    }
}

pub fn export_video_frame(
    frame: &VideoFrameSnapshot,
    target: &Path,
    operations: &[EditOperation],
) -> Result<ExportOutcome, ExportError> {
    export_cancellable(frame, target, operations, &AtomicBool::new(false))
}

pub(super) fn export_cancellable(
    frame: &VideoFrameSnapshot,
    target: &Path,
    operations: &[EditOperation],
    cancelled: &AtomicBool,
) -> Result<ExportOutcome, ExportError> {
    check_cancelled(cancelled)?;
    if same_path(frame.source_path(), target) {
        return Err(ExportError::SameAsSource);
    }
    if target
        .extension()
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("png"))
    {
        return Err(ExportError::Failed(
            "Current frame export requires a PNG target".into(),
        ));
    }
    let lease = SourceLease::open(frame.source_path()).map_err(ExportError::Output)?;
    if lease.source.stamp != frame.source.stamp {
        return Err(ExportError::Failed(
            "The frame source changed; reopen it before exporting".into(),
        ));
    }
    match File::open(target) {
        Ok(file)
            if Stamp::read(&file)
                .map_err(ExportError::Output)?
                .same_file(&frame.source.stamp) =>
        {
            return Err(ExportError::SameAsSource);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(ExportError::Output(error)),
    }
    let staging = StagedExport::new(target)?;
    let png = crate::edited_video_frame_png(frame.source_path(), frame.time, operations, &|| {
        cancelled.load(Ordering::Relaxed)
    })
    .map_err(|error| {
        if matches!(error, crate::DecodeError::ConsumerClosed) {
            ExportError::Cancelled
        } else {
            ExportError::Failed(error.to_string())
        }
    })?;
    check_cancelled(cancelled)?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&staging.output)
        .map_err(ExportError::Output)?;
    for chunk in png.chunks(64 * 1024) {
        check_cancelled(cancelled)?;
        output.write_all(chunk).map_err(ExportError::Output)?;
    }
    drop(output);
    lease.verify_path().map_err(ExportError::Output)?;
    staging.publish(target, cancelled, None)?;
    drop(lease);
    Ok(ExportOutcome {
        used_hardware_encoder: false,
    })
}

#[cfg(test)]
#[path = "export_frame/tests.rs"]
mod tests;
