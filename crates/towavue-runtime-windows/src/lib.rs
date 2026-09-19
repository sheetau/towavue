//! Windows-specific media, graphics, audio, and Shell integration boundary.

mod drag_badge;
pub use drag_badge::DragBadge;

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
mod file_operation;
mod source_save;
pub use source_save::{
    PreparedSourceSave, RetainedSource, SavedSource, SourceSaveError, SourceSaveEvent,
    SourceSaveJob, commit_source_save, prepare_source_recreation, prepare_source_save,
};
mod file_search;
mod fonts;
mod image;
mod image_clipboard;
mod image_color;
mod image_edits;
mod image_loader;
mod input;
mod latest_task;
mod launch;
mod media_input;
mod media_tools;
pub use media_input::MediaInput;
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
mod video_resume;
pub use video_resume::{VideoResume, VideoResumeEvent, VideoResumeHistory, VideoResumeSource};
mod renderer;
mod selection_outline;
mod shell;
mod taskbar;

pub use taskbar::{
    NativeTaskbar, TaskbarAction, TaskbarEvent, TaskbarIcons, TaskbarProgress, TaskbarTransport,
};

pub use recent::{COMMAND_HISTORY_LIMIT, RecentEntry, RecentFiles, RecentKind, RecentUpdate};
mod tempo;
mod watch;
mod waveform;
mod window_activation;
mod window_point;

pub use launch::{LaunchRequest, LaunchRole, LaunchServer};
pub use window_activation::activate_window;
pub use window_point::{monitor_work_area, unobscured_window_point};

pub use audio::{AudioOutput, AudioOutputError, AudioOutputEvent};
pub use cancellation::Cancellation;
pub use caption::{CaptionAction, CaptionButton, NativeCaption};
pub use decode::{
    AudioChunk, AudioFormat, DecodeError, DecodeOutput, DecodeSummary, FrameStepCache, VideoFrame,
    adjacent_video_frame, edited_video_frame_png, source_video_frame_png,
};
pub use dialog::{
    DeleteConfirmation, DialogError, FileDialogKind, PromptButtons, PromptResponse,
    confirm_file_delete, cursor_position_in_window, pick_path, show_prompt,
};
pub use export::{
    AudioChannels, AudioExportOptions, ExportDialogRequest, ExportError, ExportEvent, ExportJob,
    ExportOptions, ExportOutcome, ExportOutput, ExportRequest, ImageMetadataFormat,
    MetadataExportOptions, MetadataField, MetadataSourceValue, VideoExportQuality,
    VideoFrameSnapshot, export_media, export_media_with_options, export_media_with_output,
    export_video_frame, read_export_metadata,
};
pub use file_details::FileDetails;
pub use file_operation::{
    FileOperationAction, FileOperationError, FileOperationOutcome, FileOperationSource,
    FileRecycleReport, inspect_file_operation_source, start_file_operation, start_file_recycling,
    start_file_recycling_retaining_source,
};
pub use file_search::{FILE_SEARCH_LIMIT, FileSearch, FileSearchRequest, FileSearchResult};
pub use fonts::{UiFontFallback, japanese_ui_font, ui_font_fallbacks, ui_symbol_font};
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
    shell_workers_pending,
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
