//! Windows-specific media, graphics, audio, and Shell integration boundary.

mod audio;
mod avif_container;
#[cfg(feature = "presentation-verification")]
mod burst_verification;
#[cfg(feature = "presentation-verification")]
pub use burst_verification::{BurstEvent, burst_enabled, burst_source_id, record_burst};
mod cancellation;
mod caption;
mod decode;
mod dialog;
mod export;
mod file_details;
mod fonts;
mod image;
mod image_clipboard;
mod image_color;
mod image_edits;
mod image_loader;
mod input;
mod latest_task;
mod launch;
mod media_tools;
mod orientation;
mod pinned_cursor;
mod playback;
#[cfg(feature = "presentation-verification")]
mod presentation_verification;
mod preview;
mod preview_loader;
#[cfg(feature = "presentation-verification")]
pub use presentation_verification::{
    towavue_original_ready, towavue_original_submitted, towavue_presentation_stage,
};
mod recent;
mod renderer;
mod selection_outline;
mod shell;

pub use recent::{RecentEntry, RecentFiles, RecentUpdate};
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
pub use file_details::FileDetails;
pub use fonts::japanese_ui_font;
pub use image::{DecodedImage, DecodedImageFrame, ImageDecodeError, decode_image};
pub use image_clipboard::{ImageCopyJob, ImageCopyRequest};
pub use image_color::{premultiplied_color_image, premultiplied_rgba_image};
pub use image_edits::{compare_image_edits, compare_rendered_image_edits, render_image_edits};
#[cfg(feature = "render-verification")]
pub use image_loader::verification::{
    ImageDecodeMetrics, ImageLoadMetrics, ImageLoadTrace, ImageLoadTraceEvent, ImageLoadTraceKind,
    Outcome as ImageDecodeOutcome,
};
pub use image_loader::{ImageLoader, LoadedImagePreview, LoadedImages};
pub use input::{WheelScrollSettings, configure_mouse_input, wheel_scroll_settings};
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
#[cfg(feature = "render-verification")]
pub use renderer::{VerificationMemory, verification_thread_cpu_time};
pub use selection_outline::{paint_selection_outline, paint_time_selection};
#[cfg(feature = "shell-lifecycle-verification")]
pub use shell::verification::ShellLifetimeTrial;
pub use shell::{
    FolderOrderError, FolderOrderProvider, canonical_shell_path, reveal_file, reveal_license_guide,
};
pub use watch::{FolderWatchError, FolderWatcher};
pub use waveform::timeline_waveform;

use std::path::Path;

/// Decodes a complete media file through the M1 software path.
pub fn decode_file(
    path: &Path,
    emit: impl FnMut(DecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    decode::decode_file(path, emit)
}
