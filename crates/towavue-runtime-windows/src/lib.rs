//! Windows-specific media, graphics, audio, and Shell integration boundary.

mod audio;
mod decode;
mod dialog;
mod export;
mod image;
mod playback;
mod preview;
mod renderer;
mod shell;
mod watch;

pub use audio::{AudioOutput, AudioOutputError, AudioOutputEvent};
pub use decode::{AudioChunk, AudioFormat, DecodeError, DecodeOutput, DecodeSummary, VideoFrame};
pub use dialog::{DialogError, pick_export_file, pick_folder, pick_media_file};
pub use export::{ExportError, ExportOutcome, ExportRequest, export_media};
pub use image::{DecodedImage, DecodedImageFrame, ImageDecodeError, decode_image};
pub use playback::{DecodePath, PlaybackError, PlaybackEvent, PlaybackMetrics, PlaybackSession};
pub use preview::{PreviewCache, PreviewError, PreviewImage};
pub use renderer::{AdapterLuid, FrameRenderer, GraphicsDevice, RenderError};
pub use shell::{FolderOrderError, FolderOrderProvider};
pub use watch::{FolderWatchError, FolderWatcher};

use std::path::Path;

/// Decodes a complete media file through the M1 software path.
pub fn decode_file(
    path: &Path,
    emit: impl FnMut(DecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    decode::decode_file(path, emit)
}
