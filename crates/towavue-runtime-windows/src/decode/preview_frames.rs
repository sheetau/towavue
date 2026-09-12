use super::*;
use ffmpeg::filter;

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
    for (slot, target) in targets.iter().enumerate() {
        check_cancelled(cancelled)?;
        // Match the existing CLI's six-decimal seek precision.
        let nanos = ((target.as_nanos() + 500) / 1000 * 1000).min(i64::MAX as u128) as i64;
        let target = MediaTime::from_nanoseconds(nanos);
        if target == MediaTime::ZERO {
            input.seek(origin, ..origin)?;
        } else {
            seek_input(&mut input, target, true, cancelled)?;
        }
        decoder.flush();
        let mut selected = None;
        let mut receive = |decoder: &mut codec::decoder::Video| -> Result<bool, DecodeError> {
            loop {
                check_cancelled(cancelled)?;
                let mut decoded = frame::Video::empty();
                match decoder.receive_frame(&mut decoded) {
                    Ok(()) => {
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
        let mut reached = false;
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
        if !reached {
            check_cancelled(cancelled)?;
            decoder.send_eof()?;
            receive(&mut decoder)?;
        }
        // Beyond video EOF, retain its last decoded frame, as the single-image path does.
        let decoded = selected.ok_or(ffmpeg::Error::Eof)?;
        let frame_orientation = frame_orientation(&decoded, orientation)?;
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
        graph
            .get("in")
            .expect("preview source")
            .source()
            .add(&decoded)?;
        let mut reduced = frame::Video::empty();
        graph
            .get("out")
            .expect("preview sink")
            .sink()
            .frame(&mut reduced)?;
        let frame = copy_video_frame(&decoded, &reduced, config.time_base, orientation)?;
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
