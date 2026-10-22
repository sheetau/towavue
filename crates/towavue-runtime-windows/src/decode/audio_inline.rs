//! Audio-only decoding on the existing feed owner, with no intermediate queues.
use super::*;

#[cfg(test)]
thread_local! {
    pub(super) static ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub(super) fn enabled(input: &format::context::Input) -> bool {
    // The direct prototype exceeded the existing repeated Opus seek-work bound
    // in Ogg. Preserve that container's parallel behavior until its direct seek
    // state is separately qualified; do not relax its PCM or work assertions.
    if input.format().name().split(',').any(|name| name == "ogg") {
        return false;
    }
    #[cfg(test)]
    if !ENABLED.get() {
        return false;
    }
    #[cfg(feature = "presentation-verification")]
    if std::env::var("TOWAVUE_VERIFY_AUDIO_INLINE").as_deref() == Ok("0") {
        return false;
    }
    true
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    input: &mut format::context::Input,
    config: StreamConfig,
    sample_preroll: Option<(MediaTime, PrerollSamples)>,
    checkpoints: &mut AudioCheckpoints,
    resume: Option<(i64, ffmpeg::Packet)>,
    intervals: &[TimeRange],
    recover_corrupt_audio: bool,
    minimum: MediaTime,
    maximum: Option<MediaTime>,
    cancelled: &(dyn Fn() -> bool + Sync),
    emit: &mut impl FnMut(ParallelDecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    let origin = input_origin(input);
    let selected = config.index;
    let trusted_axis = minimum <= MediaTime::ZERO || sample_preroll.is_some();
    // The feed exclusively owns input, decoder and checkpoints during this call.
    // Selected packet normalization is identical to the parallel demux path.
    // No stream borrow escapes an iterator step; no packet survives this run.
    let mut packets = input.packets();
    let packets = std::iter::from_fn(|| {
        while !cancelled() {
            let (stream, mut packet) = packets.next()?;
            if stream.index() == selected {
                normalize_packet_time(&mut packet, stream.time_base(), origin);
                return Some(packet);
            }
        }
        None
    });
    let mut summary = DecodeSummary::default();
    let mut finished = false;
    let mut refused = false;
    let mut receive = |output| {
        if cancelled() {
            return false;
        }
        let accepted = match output {
            ParallelDecodeOutput::Audio(mut chunk) => {
                summary.audio_frames += chunk.frames as u64;
                finished = maximum.is_some_and(|end| {
                    chunk.presentation_time >= end
                        || chunk
                            .presentation_time
                            .saturating_add(Duration::from_secs_f64(
                                chunk.frames as f64 / chunk.format.sample_rate as f64,
                            ))
                            >= end
                });
                clip_audio_chunk(&mut chunk, minimum, maximum);
                chunk.frames == 0 || emit(ParallelDecodeOutput::Audio(chunk))
            }
            ParallelDecodeOutput::AudioSkipped(start, end) => {
                finished = maximum.is_some_and(|limit| end >= limit);
                let start = start.max(minimum);
                let end = maximum.map_or(end, |limit| end.min(limit));
                start >= end || emit(ParallelDecodeOutput::AudioSkipped(start, end))
            }
            ParallelDecodeOutput::AudioFinished => {
                finished = true;
                true
            }
            _ => unreachable!("audio packet worker must emit only audio"),
        };
        let accepted = accepted && (!finished || emit(ParallelDecodeOutput::AudioFinished));
        refused |= !accepted;
        // Reaching the exact interval end stops further demux and decode. The
        // parallel consumer similarly closes its receiver after AudioFinished.
        accepted && !finished
    };
    let result = run_audio_packets(
        config,
        packets,
        &mut receive,
        sample_preroll,
        checkpoints,
        resume,
        trusted_axis,
        intervals,
        recover_corrupt_audio,
    );
    check_cancelled(cancelled)?;
    if finished && !refused && matches!(result, Ok(()) | Err(DecodeError::ConsumerClosed)) {
        Ok(summary)
    } else {
        result.map(|()| summary)
    }
}

#[cfg(test)]
mod tests;
