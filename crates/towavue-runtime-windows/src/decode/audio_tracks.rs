use super::*;
use towavue_core::{AudioTrack, AudioTrackCatalog};

/// Return source-scoped scalar metadata only; no demuxer or codec handle escapes.
pub fn probe_audio_tracks(path: &Path) -> Result<AudioTrackCatalog, DecodeError> {
    ffmpeg::init()?;
    let input = format::input(path)?;
    Ok(catalog(&input))
}

pub(super) fn catalog(input: &format::context::Input) -> AudioTrackCatalog {
    AudioTrackCatalog {
        tracks: input
            .streams()
            .filter(|stream| stream.parameters().medium() == Type::Audio)
            .map(|stream| AudioTrack {
                id: AudioTrackId::from_index(stream.index()),
                title: stream
                    .metadata()
                    .get("title")
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned),
                language: stream
                    .metadata()
                    .get("language")
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned),
            })
            .collect(),
        preferred: input
            .streams()
            .best(Type::Audio)
            .map(|stream| AudioTrackId::from_index(stream.index())),
    }
}

pub(super) fn selected_config(
    input: &format::context::Input,
    track: Option<AudioTrackId>,
) -> Result<Option<StreamConfig>, DecodeError> {
    match track {
        None => Ok(best_stream_config(input, Type::Audio)),
        Some(id) => input
            .stream(id.index())
            .filter(|stream| stream.parameters().medium() == Type::Audio)
            .map(|stream| Some(stream_config(stream)))
            .ok_or(DecodeError::AudioTrackUnavailable),
    }
}

#[cfg(test)]
mod tests;
