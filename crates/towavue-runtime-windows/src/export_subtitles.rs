//! Retain embedded text independently of preview selection and display delay.
//! Sidecar times use the same source spans and global rate as video export.
use super::*;
use crate::{MediaInput, SubtitleContent, SubtitleDocument, SubtitleError};
use std::collections::BTreeSet;
use towavue_core::{EditTimeline, MediaTime, SubtitleDelay, SubtitleTrackId};

#[cfg(test)]
#[path = "export_subtitles_tests.rs"]
mod tests;

const MAX_CUES: usize = 100_000;
const MAX_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone)]
struct Track {
    path: PathBuf,
    metadata: Vec<(String, String)>,
    disposition: i32,
}

#[derive(Clone)]
pub(super) struct Source {
    id: SubtitleTrackId,
    metadata: Vec<(String, String)>,
    disposition: i32,
}

pub(super) fn catalog(input: &ffmpeg::format::context::Input) -> Vec<Source> {
    input
        .streams()
        .filter(|stream| {
            let parameters = stream.parameters();
            if parameters.medium() != ffmpeg::media::Type::Subtitle {
                return false;
            }
            // FFmpeg owns static descriptors for its lifetime. Inspect scalar
            // properties only; no pointer or codec parameter leaves this scope.
            unsafe {
                ffmpeg::ffi::avcodec_descriptor_get(parameters.id().into())
                    .as_ref()
                    .is_some_and(|descriptor| {
                        descriptor.props & ffmpeg::ffi::AV_CODEC_PROP_TEXT_SUB != 0
                    })
            }
        })
        .map(|stream| Source {
            id: SubtitleTrackId::from_index(stream.index()),
            metadata: stream
                .metadata()
                .iter()
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
            disposition: stream.disposition().bits(),
        })
        .collect()
}

#[derive(Clone)]
pub(super) struct Tracks {
    codec: &'static str,
    items: Vec<Track>,
}

pub(super) struct Prepared {
    pub tracks: Option<Tracks>,
    paths: Vec<PathBuf>,
}

fn codec(target: &Path) -> Option<&'static str> {
    match target.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp4" | "mov" | "3gp" => Some("mov_text"),
        "mkv" => Some("srt"),
        "webm" => Some("webvtt"),
        _ => None,
    }
}

fn subtitle_error(error: SubtitleError) -> ExportError {
    match error {
        SubtitleError::Cancelled => ExportError::Cancelled,
        SubtitleError::Message(key) => ExportError::Message(key),
        SubtitleError::Native(error) => {
            crate::ExportFailure::diagnostic(Text::SubtitleReadFailed, error).into()
        }
    }
}

impl Prepared {
    pub(super) fn prepare(
        request: &ExportRequest,
        streams: &ExportStreams,
        staging: &StagedExport,
        cancelled: &AtomicBool,
    ) -> Result<Self, ExportError> {
        let Some(codec) = codec(&request.target).filter(|_| request.kind == MediaKind::Video)
        else {
            return Ok(Self {
                tracks: None,
                paths: Vec::new(),
            });
        };
        check_cancelled(cancelled)?;
        if streams.subtitle_sources.is_empty() {
            return Ok(Self {
                tracks: None,
                paths: Vec::new(),
            });
        }
        // Export jobs expose a borrowed flag, while native input callbacks own
        // a 'static token. A scoped watcher bridges them without borrowed native
        // pointers. Closing the channel joins it even on early failure/unwind.
        thread::scope(|scope| {
            let cancellation = crate::Cancellation::default();
            let token = cancellation.clone();
            let (done, receiver) = std::sync::mpsc::channel::<()>();
            let watcher = thread::Builder::new()
                .name("subtitle-export-cancel".into())
                .spawn_scoped(scope, move || {
                    while matches!(
                        receiver.recv_timeout(Duration::from_millis(10)),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    ) {
                        if cancelled.load(Ordering::Relaxed) {
                            token.cancel();
                            break;
                        }
                    }
                })
                .map_err(ExportError::Start)?;
            let result = Self::read(request, streams, staging, codec, &cancellation);
            drop(done);
            watcher.join().expect("subtitle cancellation watcher");
            check_cancelled(cancelled)?;
            result
        })
    }

    fn read(
        request: &ExportRequest,
        streams: &ExportStreams,
        staging: &StagedExport,
        codec: &'static str,
        cancellation: &crate::Cancellation,
    ) -> Result<Self, ExportError> {
        let cancelled = cancellation.flag();
        let mut prepared = Self {
            tracks: None,
            paths: Vec::new(),
        };
        let duration = streams.duration.ok_or(ExportError::InvalidTimeline)?;
        let timeline = EditTimeline::from_operations(duration, &request.operations)
            .ok_or(ExportError::InvalidTimeline)?;
        let rate = EditState::from_operations(&request.operations).rate;
        let mut output = Tracks {
            codec,
            items: Vec::new(),
        };
        let mut bytes = 0;
        for source in &streams.subtitle_sources {
            check_cancelled(cancelled)?;
            let document = crate::read_subtitles(
                &MediaInput::new(request.source.clone()),
                Some(source.id),
                cancellation,
            )
            .map_err(subtitle_error)?;
            let content = project(&document, &timeline, rate, codec != "mov_text", cancelled)?;
            if content.is_empty() {
                continue;
            }
            bytes += content.len();
            if bytes > MAX_BYTES {
                return Err(ExportError::Message(Text::SubtitleTooLarge));
            }
            let path = staging
                .directory
                .join(format!("subtitle-{}.srt", output.items.len()));
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .map_err(ExportError::Output)?;
            prepared.paths.push(path.clone());
            file.write_all(content.as_bytes())
                .map_err(ExportError::Output)?;
            output.items.push(Track {
                path,
                metadata: source.metadata.clone(),
                disposition: source.disposition,
            });
        }
        check_cancelled(cancelled)?;
        if !output.items.is_empty() {
            prepared.tracks = Some(output);
        }
        Ok(prepared)
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

impl Tracks {
    pub(super) fn arguments(&self, arguments: &mut Vec<String>) {
        let inputs = arguments.iter().filter(|arg| arg.as_str() == "-i").count();
        let insertion = arguments
            .iter()
            .position(|arg| arg == "-map_metadata")
            .expect("input boundary");
        let mut sources = Vec::new();
        let mut mappings = Vec::new();
        for (index, track) in self.items.iter().enumerate() {
            // These are already plain text, not authored HTML/ASS directives.
            // Keep the SRT demuxer's timing and use its raw text decoder.
            sources.extend([
                "-c:s".into(),
                "text".into(),
                "-i".into(),
                track.path.display().to_string(),
            ]);
            mappings.extend(["-map".into(), format!("{}:s:0", inputs + index)]);
            mappings.extend([format!("-c:s:{index}"), self.codec.into()]);
            mappings.extend([format!("-map_metadata:s:s:{index}"), "-1".into()]);
            for (key, value) in &track.metadata {
                // Container-generated statistics describe the old track, not
                // the newly projected intervals; the output muxer replaces them.
                if key.starts_with('_')
                    || ["DURATION", "NUMBER_OF_FRAMES", "NUMBER_OF_BYTES", "BPS"]
                        .iter()
                        .any(|generated| key.eq_ignore_ascii_case(generated))
                {
                    continue;
                }
                mappings.extend([format!("-metadata:s:s:{index}"), format!("{key}={value}")]);
            }
            mappings.extend([
                format!("-disposition:s:{index}"),
                track.disposition.to_string(),
            ]);
        }
        arguments.splice(insertion..insertion, sources);
        let target = arguments.pop().expect("output target");
        arguments.extend(mappings);
        arguments.push(target);
    }
}

fn project(
    document: &SubtitleDocument,
    timeline: &EditTimeline,
    rate: f32,
    escape_markup: bool,
    cancelled: &AtomicBool,
) -> Result<String, ExportError> {
    if !rate.is_finite() || !(0.25..=4.0).contains(&rate) {
        return Err(ExportError::InvalidTimeline);
    }
    // Every accepted f32 rate is an exact multiple of 2^-25. Round absolute
    // boundaries once to milliseconds; never accumulate rounded cue durations.
    const SCALE: i128 = 1 << 25;
    let divisor = ((f64::from(rate) * SCALE as f64) as i128) * 1_000_000;
    let output_ms = |ns: i128| (ns * SCALE + divisor / 2) / divisor;
    let mut events = Vec::new();
    let mut texts = Vec::new();
    let mut offset = 0_i128;
    for span in timeline.spans() {
        let range = span.source();
        let start = document
            .cues()
            .partition_point(|cue| cue.start() <= range.start());
        let end = document
            .cues()
            .partition_point(|cue| cue.start() < range.end());
        let cues = document
            .active(range.start(), SubtitleDelay::default())
            .map(|(_, cue)| cue)
            .chain(document.cues()[start..end].iter());
        for cue in cues {
            check_cancelled(cancelled)?;
            let SubtitleContent::Text(text) = cue.content() else {
                continue;
            };
            let map = |time: MediaTime| {
                output_ms(
                    offset
                        + i128::from(time.as_nanoseconds() - range.start().as_nanoseconds())
                            * i128::from(span.duration().as_nanoseconds())
                            / i128::from(range.duration().as_nanoseconds()),
                )
            };
            let a = map(cue.start().max(range.start()));
            let b = map(cue.end().min(range.end()));
            if a >= b || text.trim().is_empty() {
                continue;
            }
            if texts.len() >= MAX_CUES {
                return Err(ExportError::Message(Text::SubtitleTooLarge));
            }
            let index = texts.len();
            texts.push(text.as_str());
            events.extend([(a, true, index), (b, false, index)]);
        }
        offset += i128::from(span.duration().as_nanoseconds());
    }
    events.sort_unstable();
    let mut active = BTreeSet::new();
    let mut output = String::new();
    let mut cursor = 0;
    let mut ordinal = 0;
    while cursor < events.len() {
        check_cancelled(cancelled)?;
        let time = events[cursor].0;
        while cursor < events.len() && events[cursor].0 == time {
            let (_, starts, index) = events[cursor];
            if starts {
                active.insert(index);
            } else {
                active.remove(&index);
            }
            cursor += 1;
        }
        if active.is_empty() || cursor == events.len() {
            continue;
        }
        ordinal += 1;
        if ordinal > MAX_CUES {
            return Err(ExportError::Message(Text::SubtitleTooLarge));
        }
        use std::fmt::Write as _;
        writeln!(
            output,
            "{ordinal}\n{} --> {}",
            timestamp(time),
            timestamp(events[cursor].0)
        )
        .expect("string");
        for index in &active {
            // Preserve literal angle brackets/entities through the SRT decoder;
            // blank rows cannot split an authored caption into a new SRT entry.
            for line in texts[*index].lines().filter(|line| !line.trim().is_empty()) {
                for (index, character) in line.chars().enumerate() {
                    if index % 4096 == 0 {
                        check_cancelled(cancelled)?;
                    }
                    match character {
                        '&' if escape_markup => output.push_str("&amp;"),
                        '<' if escape_markup => output.push_str("&lt;"),
                        '>' if escape_markup => output.push_str("&gt;"),
                        _ => output.push(character),
                    }
                    if output.len() > MAX_BYTES {
                        return Err(ExportError::Message(Text::SubtitleTooLarge));
                    }
                }
                output.push('\n');
            }
        }
        output.push('\n');
    }
    Ok(output)
}

fn timestamp(ms: i128) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}
