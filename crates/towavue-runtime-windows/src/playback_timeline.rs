use crate::decode::{
    self, AudioChunk, AudioFormat, DecodeError, DecodeOutput, DecodeStream,
    ParallelSoftwareDecodeOutput,
};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use towavue_core::{EditTimeline, MediaTime, TimeRange};

#[derive(Clone, Copy)]
pub(super) struct Segment {
    source: TimeRange,
    start: MediaTime,
    duration: MediaTime,
    volume: f32,
}

impl Segment {
    pub(super) fn source_end(self) -> MediaTime {
        self.source.end()
    }
    fn end(self) -> MediaTime {
        MediaTime::from_nanoseconds(self.start.as_nanoseconds() + self.duration.as_nanoseconds())
    }
    pub(super) fn source_target(self, edited: MediaTime) -> MediaTime {
        let local =
            edited.max(self.start).min(self.end()).as_nanoseconds() - self.start.as_nanoseconds();
        MediaTime::from_nanoseconds(
            self.source.start().as_nanoseconds()
                + (i128::from(local) * i128::from(self.source.duration().as_nanoseconds())
                    / i128::from(self.duration.as_nanoseconds())) as i64,
        )
    }
    pub(super) fn edited_time(self, source: MediaTime) -> Option<MediaTime> {
        if source < self.source.start() || source >= self.source.end() {
            return None;
        }
        let local = i128::from(source.as_nanoseconds() - self.source.start().as_nanoseconds())
            * i128::from(self.duration.as_nanoseconds())
            / i128::from(self.source.duration().as_nanoseconds());
        Some(MediaTime::from_nanoseconds(
            self.start.as_nanoseconds() + local as i64,
        ))
    }
}

pub(super) fn segments(
    plan: &EditTimeline,
    target: MediaTime,
    terminal_frame: bool,
) -> Vec<Segment> {
    let mut start = 0_i64;
    plan.spans()
        .iter()
        .enumerate()
        .filter_map(|(index, span)| {
            let segment = Segment {
                source: span.source(),
                start: MediaTime::from_nanoseconds(start),
                duration: span.duration(),
                volume: span.volume(),
            };
            start += span.duration().as_nanoseconds();
            (segment.end() > target
                || (terminal_frame && index + 1 == plan.spans().len() && target == plan.duration()))
            .then_some(segment)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_audio(
    path: &Path,
    plan: &EditTimeline,
    target: MediaTime,
    master_rate: f32,
    format: AudioFormat,
    cancelled: &AtomicBool,
    mut emit: impl FnMut(AudioChunk) -> bool,
) -> Result<(), DecodeError> {
    let cancelled = || cancelled.load(Ordering::Relaxed);
    let segments = segments(plan, target, false);
    let Some(first) = segments.first() else {
        return Ok(());
    };
    let source_start = first.source_target(target);
    let source_end = segments.last().expect("nonempty segments").source.end();
    let sample_at = |time: MediaTime| {
        (time.as_seconds_f64() / f64::from(master_rate) * f64::from(format.sample_rate)).ceil()
            as u64
    };
    let mut emitted = 0_u64;
    let mut index = 0;
    let mut tempo = None;
    let mut queue = VecDeque::new();
    let mut process = |chunk: Option<AudioChunk>| -> Result<(), DecodeError> {
        while let Some(segment) = segments.get(index) {
            if cancelled() {
                return Err(DecodeError::ConsumerClosed);
            }
            let limit = sample_at(segment.end()) - sample_at(target);
            if tempo.is_none() {
                let rate = segment.source.duration().as_nanoseconds() as f64
                    / segment.duration.as_nanoseconds() as f64
                    * f64::from(master_rate);
                tempo = Some(crate::tempo::AudioTempo::timeline(
                    format.sample_rate,
                    rate,
                )?);
            }
            let complete = if let Some(chunk) = &chunk {
                let end = chunk
                    .presentation_time
                    .saturating_add(std::time::Duration::from_nanos(
                        (chunk.frames as u64 * 1_000_000_000)
                            .div_ceil(u64::from(format.sample_rate)),
                    ));
                let mut part = AudioChunk {
                    presentation_time: chunk.presentation_time,
                    format: chunk.format,
                    frames: chunk.frames,
                    bytes: chunk.bytes.clone(),
                };
                decode::clip_audio_chunk(
                    &mut part,
                    segment.source_target(target),
                    Some(segment.source.end()),
                );
                if part.frames > 0 {
                    tempo
                        .as_mut()
                        .expect("tempo")
                        .push(&part.bytes, &mut queue)?;
                }
                end >= segment.source.end()
            } else {
                true
            };
            if complete {
                tempo.take().expect("tempo").finish(&mut queue)?;
            }
            send_audio(
                &mut queue,
                &mut emitted,
                limit,
                segment.volume,
                target,
                master_rate,
                format,
                &cancelled,
                &mut emit,
            )?;
            if !complete {
                break;
            }
            while emitted < limit {
                queue.clear();
                queue.resize(((limit - emitted).min(1024) as usize) * 8, 0);
                send_audio(
                    &mut queue,
                    &mut emitted,
                    limit,
                    1.0,
                    target,
                    master_rate,
                    format,
                    &cancelled,
                    &mut emit,
                )?;
            }
            index += 1;
        }
        Ok(())
    };
    let mut failure = None;
    // Decode once through ordered source intervals; repeated coarse-PTS seeks lose sample phase.
    let result = decode::decode_file_parallel_cancellable(
        path,
        source_start,
        Some(source_end),
        Some(DecodeStream::Audio),
        &cancelled,
        |output| {
            if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) = output
                && let Err(error) = process(Some(chunk))
            {
                failure = Some(error);
                return false;
            }
            true
        },
    );
    if let Some(error) = failure {
        return Err(error);
    }
    result?;
    process(None)
}

#[allow(clippy::too_many_arguments)]
fn send_audio(
    queue: &mut VecDeque<u8>,
    emitted: &mut u64,
    limit: u64,
    volume: f32,
    target: MediaTime,
    master_rate: f32,
    format: AudioFormat,
    cancelled: &impl Fn() -> bool,
    emit: &mut impl FnMut(AudioChunk) -> bool,
) -> Result<(), DecodeError> {
    while queue.len() >= 8 && *emitted < limit {
        if cancelled() {
            return Err(DecodeError::ConsumerClosed);
        }
        let frames = (queue.len() / 8).min(1024).min((limit - *emitted) as usize);
        let mut bytes: Vec<_> = queue.drain(..frames * 8).collect();
        if volume != 1.0 {
            for sample in bytes.as_chunks_mut::<4>().0 {
                *sample = (f32::from_le_bytes(*sample) * volume).to_le_bytes();
            }
        }
        let offset = (*emitted as f64 * 1_000_000_000.0 * f64::from(master_rate)
            / f64::from(format.sample_rate)) as i64;
        let presentation_time =
            MediaTime::from_nanoseconds(target.as_nanoseconds().saturating_add(offset));
        if !emit(AudioChunk {
            presentation_time,
            format,
            frames,
            bytes,
        }) {
            return Err(DecodeError::ConsumerClosed);
        }
        *emitted += frames as u64;
    }
    // The producer may decode a coarse-PTS tail past the planned output boundary.
    if *emitted == limit {
        queue.clear();
    }
    Ok(())
}
