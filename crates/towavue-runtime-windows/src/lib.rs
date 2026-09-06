//! Windows-specific media, graphics, audio, and Shell integration boundary.

mod audio;
mod cancellation;
mod decode;
mod dialog;
mod export;
mod fonts;
mod image;
mod image_loader;
mod input;
mod latest_task;
mod orientation;
mod playback;
mod preview;
mod preview_loader;
mod renderer;
mod shell;
mod tempo;
mod watch;
mod waveform;

pub use audio::{AudioOutput, AudioOutputError, AudioOutputEvent};
pub use cancellation::Cancellation;
pub use decode::{AudioChunk, AudioFormat, DecodeError, DecodeOutput, DecodeSummary, VideoFrame};
pub use dialog::{
    DialogError, FileDialogKind, PromptButtons, PromptResponse, cursor_position_in_window,
    pick_path, show_prompt,
};
pub use export::{ExportError, ExportEvent, ExportJob, ExportOutcome, ExportRequest, export_media};
pub use fonts::japanese_ui_font;
pub use image::{DecodedImage, DecodedImageFrame, ImageDecodeError, decode_image};
pub use image_loader::{ImageLoader, LoadedImages};
pub use input::configure_mouse_input;
pub use latest_task::LatestTask;
pub use orientation::VideoOrientation;
pub use playback::{DecodePath, PlaybackError, PlaybackEvent, PlaybackMetrics, PlaybackSession};
pub use preview::{MediaPreview, PreviewCache, PreviewError, PreviewImage};
pub use preview_loader::{PreviewLoader, VISIBLE_PREVIEW_LIMIT};
pub use renderer::{AdapterLuid, FrameRenderer, GraphicsDevice, RenderError};
pub use shell::{FolderOrderError, FolderOrderProvider, canonical_shell_path};
pub use watch::{FolderWatchError, FolderWatcher};

use std::path::Path;

/// Decodes a complete media file through the M1 software path.
pub fn decode_file(
    path: &Path,
    emit: impl FnMut(DecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    decode::decode_file(path, emit)
}
