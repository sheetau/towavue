//! Subtitle reads own their demuxer, decoder and AVSubtitle buffers on one worker.
//! Only bounded, owned text/palette data crosses into the application.

use crate::{Cancellation, MediaInput};
use ffmpeg::Rescale;
use ffmpeg_next as ffmpeg;
use std::ffi::CStr;
use towavue_core::localization::{Language, Text};
use towavue_core::{MediaTime, SubtitleCue, SubtitleTimeline, SubtitleTrack, SubtitleTrackId};

mod bitmap;
mod text;
pub use bitmap::SubtitleBitmap;

#[cfg(test)]
mod tests;

const MAX_CUES: usize = 100_000;
const MAX_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug)]
pub enum SubtitleContent {
    Text(String),
    Bitmap(Vec<SubtitleBitmap>),
}

pub type SubtitleDocument = SubtitleTimeline<SubtitleContent>;

#[derive(Debug, thiserror::Error)]
pub enum SubtitleError {
    #[error("subtitle reading cancelled")]
    Cancelled,
    #[error("{}", .0.in_language(Language::English))]
    Message(Text),
    #[error("could not decode subtitles: {0}")]
    Native(#[from] ffmpeg::Error),
}

impl SubtitleError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Message(text) => text.in_language(language).into(),
            Self::Cancelled => Text::SubtitleReadCancelled.in_language(language).into(),
            Self::Native(error) => format!(
                "{}: {error}",
                Text::SubtitleReadFailed.in_language(language)
            ),
        }
    }
}

pub(crate) fn catalog(input: &ffmpeg::format::context::Input) -> Vec<SubtitleTrack> {
    input
        .streams()
        .filter(|stream| stream.parameters().medium() == ffmpeg::media::Type::Subtitle)
        .map(|stream| SubtitleTrack {
            id: SubtitleTrackId::from_index(stream.index()),
            title: stream.metadata().get("title").map(str::to_owned),
            language: stream.metadata().get("language").map(str::to_owned),
        })
        .collect()
}

/// The caller must keep this on a worker, retaining MediaInput until it returns.
/// None selects a standalone subtitle file, whose authored origin is always zero.
pub fn read_subtitles(
    source: &MediaInput,
    track: Option<SubtitleTrackId>,
    cancellation: &Cancellation,
) -> Result<SubtitleDocument, SubtitleError> {
    let check = || {
        if cancellation.is_cancelled() {
            Err(SubtitleError::Cancelled)
        } else {
            Ok(())
        }
    };
    check()?;
    ffmpeg::init()?;
    let interrupt = cancellation.clone();
    let result = (|| {
        let mut input =
            ffmpeg::format::input_with_interrupt(source.path(), move || interrupt.is_cancelled())?;
        check()?;
        let selected = track
            .or_else(|| catalog(&input).first().map(|track| track.id))
            .ok_or(SubtitleError::Message(Text::SubtitleTrackUnavailable))?;
        let stream = input
            .streams()
            .find(|stream| {
                stream.index() == selected.index()
                    && stream.parameters().medium() == ffmpeg::media::Type::Subtitle
            })
            .ok_or(SubtitleError::Message(Text::SubtitleTrackUnavailable))?;
        let base = stream.time_base();
        let mut context =
            ffmpeg::codec::context::Context::from_parameters(stream.parameters())?.decoder();
        context.set_packet_time_base(base);
        let mut decoder = context.subtitle()?;
        let origin = if track.is_some() {
            crate::decode::input_origin(&input)
        } else {
            0
        };
        crate::decode::discard_other_streams(&mut input, selected.index());
        let mut builder = Builder::default();
        loop {
            check()?;
            let mut packet = ffmpeg::Packet::empty();
            match packet.read(&mut input) {
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) => break,
                Err(error) => return Err(error.into()),
            }
            if packet.stream() != selected.index() {
                continue;
            }
            let mut decoded = Decoded::new();
            if decoder.decode(&packet, &mut decoded.0)? {
                builder.append(
                    &decoded.0,
                    packet
                        .pts()
                        .map(|pts| pts.rescale(base, ffmpeg::rescale::TIME_BASE)),
                    (packet.duration() > 0)
                        .then(|| packet.duration().rescale(base, ffmpeg::rescale::TIME_BASE)),
                    origin,
                    bitmap_canvas(&decoder),
                    &check,
                )?;
            }
        }
        // Delayed subtitle decoders may retain their final event until EOF.
        for index in 0..32 {
            check()?;
            let mut decoded = Decoded::new();
            if !decoder.decode(&ffmpeg::Packet::empty(), &mut decoded.0)? {
                break;
            }
            builder.append(
                &decoded.0,
                None,
                None,
                origin,
                bitmap_canvas(&decoder),
                &check,
            )?;
            if index == 31 {
                return Err(SubtitleError::Message(Text::SubtitleTooLarge));
            }
        }
        builder.finish()
    })();
    check()?;
    result
}

fn bitmap_canvas(decoder: &ffmpeg::decoder::Subtitle) -> Option<(u32, u32)> {
    // Only read dimensions from the worker-owned decoder after decode returns.
    // Subtitle formats such as PGS can update these at presentation boundaries.
    let context = unsafe { &*decoder.as_ptr() };
    (context.width > 0 && context.height > 0)
        .then_some((context.width as u32, context.height as u32))
}

struct Decoded(ffmpeg::Subtitle);
impl Decoded {
    fn new() -> Self {
        let mut subtitle = ffmpeg::Subtitle::new();
        subtitle.set_pts(None);
        Self(subtitle)
    }
}
impl Drop for Decoded {
    fn drop(&mut self) {
        // ffmpeg-next's Subtitle does not implement Drop. This owns every native
        // rectangle allocation, including partially decoded/error results.
        unsafe {
            ffmpeg::ffi::avsubtitle_free(self.0.as_mut_ptr());
        }
    }
}

#[derive(Default)]
struct Builder {
    cues: Vec<SubtitleCue<SubtitleContent>>,
    pending_bitmap: Option<(i64, Option<i64>, Vec<SubtitleBitmap>)>,
    bytes: usize,
}

impl Builder {
    fn push(
        &mut self,
        start: i64,
        end: i64,
        content: SubtitleContent,
    ) -> Result<(), SubtitleError> {
        if self.cues.len() >= MAX_CUES {
            return Err(SubtitleError::Message(Text::SubtitleTooLarge));
        }
        if let Some(cue) = SubtitleCue::new(
            MediaTime::from_nanoseconds(start),
            MediaTime::from_nanoseconds(end),
            content,
        ) {
            self.cues.push(cue);
        }
        Ok(())
    }
    fn close_bitmap(&mut self, end: i64) -> Result<(), SubtitleError> {
        if let Some((start, declared_end, images)) = self.pending_bitmap.take() {
            self.push(
                start,
                declared_end.map_or(end, |limit| limit.min(end)),
                SubtitleContent::Bitmap(images),
            )?;
        }
        Ok(())
    }
    fn append(
        &mut self,
        subtitle: &ffmpeg::Subtitle,
        packet_pts: Option<i64>,
        packet_duration: Option<i64>,
        origin: i64,
        canvas: Option<(u32, u32)>,
        check: &impl Fn() -> Result<(), SubtitleError>,
    ) -> Result<(), SubtitleError> {
        let pts = subtitle
            .pts()
            .or(packet_pts)
            .ok_or(SubtitleError::Message(Text::SubtitleInvalidData))?;
        let timestamp = pts.saturating_sub(origin).saturating_mul(1000);
        let start = timestamp.saturating_add(i64::from(subtitle.start()) * 1_000_000);
        let end = if subtitle.end() > subtitle.start() && subtitle.end() != u32::MAX {
            Some(timestamp.saturating_add(i64::from(subtitle.end()) * 1_000_000))
        } else {
            packet_duration.map(|duration| timestamp.saturating_add(duration.saturating_mul(1000)))
        };
        let mut texts = Vec::new();
        let mut images = Vec::new();
        for rect in subtitle.rects() {
            check()?;
            match rect {
                ffmpeg::subtitle::Rect::Text(_) | ffmpeg::subtitle::Rect::Ass(_) => {
                    let ass = matches!(rect, ffmpeg::subtitle::Rect::Ass(_));
                    // Rectangle pointers remain owned by Decoded. Do not use the
                    // wrapper's unchecked UTF-8 accessor on untrusted subtitles.
                    let value = unsafe {
                        let raw = &*rect.as_ptr();
                        let pointer = if ass { raw.ass } else { raw.text };
                        if pointer.is_null() {
                            return Err(SubtitleError::Message(Text::SubtitleInvalidData));
                        }
                        CStr::from_ptr(pointer).to_string_lossy()
                    };
                    if value.len() > MAX_BYTES.saturating_sub(self.bytes) {
                        return Err(SubtitleError::Message(Text::SubtitleTooLarge));
                    }
                    let text = text::plain(&value, ass);
                    self.bytes += text.len();
                    if !text.is_empty() {
                        texts.push(text);
                    }
                }
                ffmpeg::subtitle::Rect::Bitmap(bitmap) => {
                    let image = bitmap::copy(&bitmap, canvas, check)?;
                    self.bytes = self.bytes.saturating_add(image.storage_size());
                    images.push(image);
                }
                ffmpeg::subtitle::Rect::None(_) => {}
            }
            if self.bytes > MAX_BYTES {
                return Err(SubtitleError::Message(Text::SubtitleTooLarge));
            }
        }
        if !texts.is_empty() {
            let end = end.ok_or(SubtitleError::Message(Text::SubtitleInvalidData))?;
            self.push(start, end, SubtitleContent::Text(texts.join("\n")))?;
        }
        if !images.is_empty() {
            self.close_bitmap(start)?;
            self.pending_bitmap = Some((start, end, images));
        } else if subtitle.rects().len() == 0 {
            self.close_bitmap(start)?;
        }
        Ok(())
    }
    fn finish(mut self) -> Result<SubtitleDocument, SubtitleError> {
        // PGS ends a presentation only at the next presentation/clear event.
        // An unclosed final cue remains valid through the video's end; container
        // duration estimates must not truncate it or reject standalone SUP files.
        self.close_bitmap(i64::MAX)?;
        Ok(SubtitleTimeline::new(self.cues))
    }
}
