use super::*;
use ffmpeg::filter;

#[cfg(test)]
thread_local! {
    pub(crate) static REUSE_PREVIEW_GOP: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    pub(crate) static PREVIEW_WORK: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

fn preview_key(
    input: &format::context::Input,
    index: usize,
    target: MediaTime,
) -> Option<(i64, i64)> {
    // Limit continuation to the MOV/MP4 sample-table demuxer. Other formats
    // may expose sparse indexes that do not bound the work of a forward scan;
    // transport streams also require the existing specialized seek path.
    if !input.format().name().split(',').any(|name| name == "mov") {
        return None;
    }
    let stream = input.stream(index)?;
    let timestamp = target
        .as_nanoseconds()
        .rescale_with(
            Rational(1, 1_000_000_000),
            stream.time_base(),
            Rounding::Down,
        )
        .saturating_add(
            input_origin(input).rescale(ffmpeg::rescale::TIME_BASE, stream.time_base()),
        );
    // The worker owns this input. Copy the public index identity before any demux
    // call can invalidate its borrowed entry; no pointer survives this block.
    unsafe {
        let count = ffmpeg::ffi::avformat_index_get_entries_count(stream.as_ptr());
        let last = ffmpeg::ffi::avformat_index_get_entry(stream.as_ptr().cast_mut(), count - 1);
        if last
            .as_ref()
            .is_none_or(|entry| entry.timestamp < timestamp)
        {
            // Fragmented inputs may only have an indexed prefix. Do not infer
            // that an unknown future interval belongs to its last known GOP.
            return None;
        }
        let entry = ffmpeg::ffi::avformat_index_get_entry_from_timestamp(
            stream.as_ptr().cast_mut(),
            timestamp,
            ffmpeg::ffi::AVSEEK_FLAG_BACKWARD,
        );
        let entry = entry.as_ref()?;
        (entry.pos >= 0 && entry.timestamp <= timestamp).then_some((entry.pos, entry.timestamp))
    }
}

/// One worker-owned input/decoder/filter graph serves a bounded preview request.
/// Only reduced owned pixels leave this auxiliary software path, never playback frames.
pub(crate) fn preview_video_frames(
    path: &Path,
    targets: &[Duration],
    filter: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
    mut emit: impl FnMut(usize, crate::PreviewImage),
) -> Result<(), DecodeError> {
    check_cancelled(cancelled)?;
    ffmpeg::init()?;
    let mut input = format::input(path)?;
    let config = best_stream_config(&input, Type::Video).ok_or(DecodeError::NoMediaStream)?;
    let orientation = VideoOrientation::from_bytes(config.display_matrix.as_deref())?;
    let mut context = codec::context::Context::from_parameters(config.parameters)?;
    context.set_threading(codec::threading::Config::count(1));
    // This worker exclusively owns the unopened context. Bound decoder allocations
    // before opening it; no native pointer is retained or accessed by another thread.
    unsafe {
        (*context.as_mut_ptr()).max_pixels = 128 * 1024 * 1024;
    }
    let mut decoder = context.decoder().video()?;
    let origin = input_origin(&input);
    let mut graph: Option<(String, filter::Graph)> = None;
    let mut previous_target = None;
    let mut previous_key = None;
    let mut selected: Option<frame::Video> = None;
    let mut eof_sent = false;
    for (slot, target) in targets.iter().enumerate() {
        check_cancelled(cancelled)?;
        // Match the existing CLI's six-decimal seek precision.
        let nanos = ((target.as_nanos() + 500) / 1000 * 1000).min(i64::MAX as u128) as i64;
        let target = MediaTime::from_nanoseconds(nanos);
        let key = preview_key(&input, config.index, target);
        let reuse = previous_target.is_some_and(|previous| previous <= target)
            && key.is_some()
            && key == previous_key;
        #[cfg(test)]
        let reuse = reuse && REUSE_PREVIEW_GOP.get();
        if !reuse {
            if target == MediaTime::ZERO {
                input.seek(origin, ..origin)?;
            } else {
                seek_input(&mut input, target, true, cancelled)?;
            }
            decoder.flush();
            selected = None;
            eof_sent = false;
            #[cfg(test)]
            PREVIEW_WORK.set({
                let (seeks, frames) = PREVIEW_WORK.get();
                (seeks + 1, frames)
            });
        }
        previous_target = Some(target);
        previous_key = key;
        let mut reached = selected.as_ref().is_some_and(|frame| {
            timestamp_to_media_time(frame.timestamp(), config.time_base) >= target
        });
        let mut receive = |decoder: &mut codec::decoder::Video| -> Result<bool, DecodeError> {
            loop {
                check_cancelled(cancelled)?;
                let mut decoded = frame::Video::empty();
                match decoder.receive_frame(&mut decoded) {
                    Ok(()) => {
                        #[cfg(test)]
                        PREVIEW_WORK.set({
                            let (seeks, frames) = PREVIEW_WORK.get();
                            (seeks, frames + 1)
                        });
                        let timestamp = decoded
                            .timestamp()
                            .ok_or(DecodeError::MissingVideoTimestamp)?;
                        let reached =
                            timestamp_to_media_time(Some(timestamp), config.time_base) >= target;
                        selected = Some(decoded);
                        if reached {
                            return Ok(true);
                        }
                    }
                    Err(error) if decoder_is_drained(error) => return Ok(false),
                    Err(error) => return Err(error.into()),
                }
            }
        };
        if !reached {
            reached = receive(&mut decoder)?;
        }
        if !reached && !eof_sent {
            for (stream, mut packet) in input.packets() {
                check_cancelled(cancelled)?;
                if stream.index() == config.index {
                    normalize_packet_time(&mut packet, stream.time_base(), origin);
                    decoder.send_packet(&packet)?;
                    if receive(&mut decoder)? {
                        reached = true;
                        break;
                    }
                }
            }
        }
        if !reached && !eof_sent {
            check_cancelled(cancelled)?;
            decoder.send_eof()?;
            eof_sent = true;
            receive(&mut decoder)?;
        }
        // Beyond video EOF, retain its last decoded frame, as the single-image path does.
        let decoded = selected.as_ref().ok_or(ffmpeg::Error::Eof)?;
        let frame_orientation = frame_orientation(decoded, orientation)?;
        let pixel_format: ffmpeg::ffi::AVPixelFormat = decoded.format().into();
        let aspect = decoded.aspect_ratio();
        let aspect = if aspect.numerator() > 0 && aspect.denominator() > 0 {
            aspect
        } else {
            Rational(1, 1)
        };
        let space: ffmpeg::ffi::AVColorSpace = decoded.color_space().into();
        let range: ffmpeg::ffi::AVColorRange = decoded.color_range().into();
        let arguments = format!(
            "video_size={}x{}:pix_fmt={}:time_base={}:pixel_aspect={}:colorspace={}:range={}",
            decoded.width(),
            decoded.height(),
            pixel_format as i32,
            config.time_base,
            aspect,
            space as i32,
            range as i32
        );
        let filters = format!(
            "{}{filter},format=rgba,copy",
            frame_orientation.ffmpeg_filter()
        );
        let key = format!("{arguments}/{filters}");
        if graph.as_ref().is_none_or(|(current, _)| *current != key) {
            let mut next = filter::Graph::new();
            // Exclusive unconfigured graph on this worker; no filter threads exist yet.
            unsafe {
                (*next.as_mut_ptr()).nb_threads = 1;
            }
            next.add(
                &filter::find("buffer").ok_or(ffmpeg::Error::FilterNotFound)?,
                "in",
                &arguments,
            )?;
            next.add(
                &filter::find("buffersink").ok_or(ffmpeg::Error::FilterNotFound)?,
                "out",
                "",
            )?;
            next.output("in", 0)?.input("out", 0)?.parse(&filters)?;
            next.validate()?;
            graph = Some((key, next));
        }
        check_cancelled(cancelled)?;
        let graph = &mut graph.as_mut().expect("configured graph").1;
        // Keep the selected frame for duplicate targets and EOF. Source::add
        // consumes its references even through its shared Rust argument; write
        // instead gives the worker-owned graph its own reference, not a pixel copy.
        let result = unsafe {
            ffmpeg::ffi::av_buffersrc_write_frame(
                graph.get("in").expect("preview source").as_mut_ptr(),
                decoded.as_ptr(),
            )
        };
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
        let mut reduced = frame::Video::empty();
        graph
            .get("out")
            .expect("preview sink")
            .sink()
            .frame(&mut reduced)?;
        let frame = copy_video_frame(decoded, &reduced, config.time_base, orientation)?;
        check_cancelled(cancelled)?;
        emit(
            slot,
            crate::PreviewImage {
                width: frame.width,
                height: frame.height,
                rgba: frame.rgba,
            },
        );
    }
    check_cancelled(cancelled)
}
