//! Windows-specific media, graphics, audio, and Shell integration boundary.

mod audio;
mod cancellation;
mod caption;
mod decode;
mod dialog;
mod export;
mod fonts;
mod image;
mod image_clipboard;
mod image_edits;
mod image_loader;
mod input;
mod latest_task;
mod launch;
mod media_tools;
mod orientation;
mod pinned_cursor;
mod playback;
mod preview;
mod preview_loader;
mod recent;
mod renderer;
mod selection_outline;
mod shell;

pub use recent::{RecentFiles, RecentUpdate};
mod tempo;
mod watch;
mod waveform;
mod window_point;

pub use launch::{LaunchRequest, LaunchRole, LaunchServer};
pub use window_point::{monitor_work_area, unobscured_window_point};

pub use audio::{AudioOutput, AudioOutputError, AudioOutputEvent};
pub use cancellation::Cancellation;
pub use caption::{CaptionAction, CaptionButton, NativeCaption};
pub use decode::{
    AudioChunk, AudioFormat, DecodeError, DecodeOutput, DecodeSummary, VideoFrame,
    adjacent_video_frame,
};
pub use dialog::{
    DialogError, FileDialogKind, PromptButtons, PromptResponse, cursor_position_in_window,
    pick_path, show_prompt,
};
pub use export::{
    AudioChannels, AudioExportOptions, ExportError, ExportEvent, ExportJob, ExportOptions,
    ExportOutcome, ExportOutput, ExportRequest, ImageMetadataFormat, MetadataExportOptions,
    MetadataField, MetadataSourceValue, export_media, export_media_with_options,
    export_media_with_output, read_export_metadata,
};
pub use fonts::japanese_ui_font;
pub use image::{DecodedImage, DecodedImageFrame, ImageDecodeError, decode_image};
pub use image_clipboard::{ImageCopyJob, ImageCopyRequest};
pub use image_edits::render_image_edits;
pub use image_loader::{ImageLoader, LoadedImagePreview, LoadedImages};
pub use input::configure_mouse_input;
pub use latest_task::LatestTask;
pub use orientation::VideoOrientation;
pub use pinned_cursor::PinnedCursor;
pub use playback::{DecodePath, PlaybackError, PlaybackEvent, PlaybackMetrics, PlaybackSession};
pub use preview::{
    CachedImagePreview, MediaPreview, PreviewCache, PreviewError, PreviewImage, VideoPreviewSheet,
    VideoSheetLayout,
};
pub use preview_loader::{PreviewLoader, VISIBLE_PREVIEW_LIMIT};
pub use renderer::{AdapterLuid, FrameRenderer, GraphicsDevice, RenderError, video_edit_geometry};
pub use selection_outline::{paint_selection_outline, paint_time_selection};
pub use shell::{
    FolderOrderError, FolderOrderProvider, canonical_shell_path, reveal_file, reveal_license_guide,
};
pub use watch::{FolderWatchError, FolderWatcher};

use std::path::Path;

/// Decodes a complete media file through the M1 software path.
pub fn decode_file(
    path: &Path,
    emit: impl FnMut(DecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    decode::decode_file(path, emit)
}
