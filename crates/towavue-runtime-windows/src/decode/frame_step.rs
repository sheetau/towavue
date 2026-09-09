use super::*;
use towavue_core::EditTimeline;

/// Find the adjacent distinct video PTS on the source or edited timeline.
/// Run on a worker: seeking and codec calls can block. Only scalar timestamps
/// leave this query; playback keeps its own input, device and presentation path.
pub fn adjacent_video_frame(
    path: &Path,
    target: MediaTime,
    forward: bool,
    timeline: Option<&EditTimeline>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Option<MediaTime>, DecodeError> {
    check_cancelled(cancelled)?;
    ffmpeg::init()?;
    let source_target = if let Some(plan) = timeline {
        let Some(source) = plan.source_time(target.clamp(MediaTime::ZERO, plan.duration())) else {
            return Ok(None);
        };
        source
    } else {
        target.max(MediaTime::ZERO)
    };
    let mut intervals = timeline.map_or_else(
        || vec![(MediaTime::ZERO, None)],
        |plan| {
            plan.spans()
                .iter()
                .map(|span| (span.source().start(), Some(span.source().end())))
                .collect()
        },
    );
    if !forward {
        intervals.reverse();
    }
    for (start, end) in intervals {
        check_cancelled(cancelled)?;
        if (forward && end.is_some_and(|end| source_target >= end))
            || (!forward && source_target < start)
        {
            continue;
        }
        let anchor = if forward {
            source_target.max(start)
        } else {
            end.map_or(source_target, |end| source_target.min(end))
        };
        // A keyframe at the current PTS need not include its predecessor. If
        // preroll yields none, widen backwards to the interval's actual start.
        let mut lookback = if forward { 0_i64 } else { 1 };
        loop {
            let seek =
                MediaTime::from_nanoseconds(anchor.as_nanoseconds().saturating_sub(lookback))
                    .max(start);
            let candidate = scan(path, seek, start, end, target, forward, timeline, cancelled)?;
            if candidate.is_some() {
                return Ok(candidate);
            }
            if forward || seek == start {
                break;
            }
            lookback = lookback.saturating_mul(2).max(1_000_000_000);
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn scan(
    path: &Path,
    seek: MediaTime,
    start: MediaTime,
    end: Option<MediaTime>,
    target: MediaTime,
    forward: bool,
    timeline: Option<&EditTimeline>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Option<MediaTime>, DecodeError> {
    check_cancelled(cancelled)?;
    let mut input = format::input(path)?;
    check_cancelled(cancelled)?;
    let config = best_stream_config(&input, Type::Video).ok_or(DecodeError::NoMediaStream)?;
    let mut decoder = codec::context::Context::from_parameters(config.parameters)?
        .decoder()
        .video()?;
    let origin = input_origin(&input);
    seek_video_stream(
        &mut input,
        config.index,
        config.time_base,
        origin,
        seek,
        cancelled,
    )?;
    let mut candidate = None;
    let mut receive = |decoder: &mut codec::decoder::Video| -> Result<bool, DecodeError> {
        loop {
            check_cancelled(cancelled)?;
            let mut decoded = frame::Video::empty();
            match decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let timestamp = decoded
                        .timestamp()
                        .ok_or(DecodeError::MissingVideoTimestamp)?;
                    let source = timestamp_to_media_time(Some(timestamp), config.time_base);
                    if source < start {
                        continue;
                    }
                    if end.is_some_and(|end| source >= end) {
                        return Ok(true);
                    }
                    let Some(time) = timeline.map_or(Some(source), |plan| plan.edited_time(source))
                    else {
                        continue;
                    };
                    if forward {
                        if time > target {
                            candidate = Some(time);
                            return Ok(true);
                        }
                    } else if time < target {
                        candidate = Some(time);
                    } else {
                        return Ok(true);
                    }
                }
                Err(error) if decoder_is_drained(error) => return Ok(false),
                Err(error) => return Err(error.into()),
            }
        }
    };
    for (stream, mut packet) in input.packets() {
        check_cancelled(cancelled)?;
        if stream.index() == config.index {
            normalize_packet_time(&mut packet, stream.time_base(), origin);
            decoder.send_packet(&packet)?;
            if receive(&mut decoder)? {
                return Ok(candidate);
            }
        }
    }
    check_cancelled(cancelled)?;
    decoder.send_eof()?;
    receive(&mut decoder)?;
    Ok(candidate)
}

#[cfg(test)]
mod tests;
