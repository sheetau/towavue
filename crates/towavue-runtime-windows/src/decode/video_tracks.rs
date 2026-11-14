use super::*;
use towavue_core::{VideoTrack, VideoTrackCatalog};

pub(crate) fn timed(stream: &ffmpeg::Stream<'_>) -> bool {
    stream.parameters().medium() == Type::Video
        && !stream
            .disposition()
            .contains(ffmpeg::format::stream::Disposition::ATTACHED_PIC)
}

pub(crate) fn selected_stream(
    input: &format::context::Input,
    track: Option<VideoTrackId>,
) -> Result<Option<ffmpeg::Stream<'_>>, DecodeError> {
    match track {
        None => Ok(playback_video_stream(input)),
        Some(id) => input
            .stream(id.index())
            .filter(timed)
            .map(Some)
            .ok_or(DecodeError::VideoTrackUnavailable),
    }
}

pub(crate) fn catalog(input: &format::context::Input) -> VideoTrackCatalog {
    VideoTrackCatalog {
        tracks: input
            .streams()
            .filter(timed)
            .map(|stream| VideoTrack {
                id: VideoTrackId::from_index(stream.index()),
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
        preferred: playback_video_stream(input)
            .map(|stream| VideoTrackId::from_index(stream.index())),
    }
}

pub(super) fn selected_config(
    input: &format::context::Input,
    track: Option<VideoTrackId>,
) -> Result<StreamConfig, DecodeError> {
    match track {
        Some(_) => selected_stream(input, track)?.map(stream_config),
        None => best_stream_config(input, Type::Video),
    }
    .ok_or(DecodeError::NoMediaStream)
}
