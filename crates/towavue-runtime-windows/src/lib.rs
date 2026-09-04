//! Windows-specific media, graphics, audio, and Shell integration boundary.

mod audio;
mod decode;
mod playback;
mod renderer;

pub use audio::{AudioOutput, AudioOutputError, AudioOutputEvent};
pub use decode::{AudioChunk, AudioFormat, DecodeError, DecodeOutput, DecodeSummary, VideoFrame};
pub use playback::{DecodePath, PlaybackError, PlaybackEvent, PlaybackMetrics, PlaybackSession};
pub use renderer::{AdapterLuid, FrameRenderer, GraphicsDevice, RenderError};

use std::path::Path;

/// Decodes a complete media file through the M1 software path.
pub fn decode_file(
    path: &Path,
    emit: impl FnMut(DecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    decode::decode_file(path, emit)
}
