//! Reprobe completed audio inputs without decoding an unchanged video again.
use super::*;
use crate::export::frame::{FrameSource, SourceLease};
use std::ffi::{CStr, CString};
use std::sync::Arc;

mod signature;
#[cfg(test)]
mod tests;

/// Bounded owned metadata only. Completion drops every native/file owner.
pub(super) struct Proof {
    source: Arc<FrameSource>,
    header: Vec<u8>,
    complete: Vec<u8>,
    video: usize,
    pixel_format: i32,
    raw_bits: i32,
    chroma_location: ffmpeg::ffi::AVChromaLocation,
    h264: bool,
    #[cfg(test)]
    pub(super) reused: bool,
}

#[cfg(test)]
thread_local! {
    static ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

fn enabled() -> bool {
    #[cfg(test)]
    if !ENABLED.get() {
        return false;
    }
    #[cfg(feature = "presentation-verification")]
    if std::env::var_os("TOWAVUE_VERIFY_AUDIO_REPROBE").is_some_and(|v| v == "0") {
        return false;
    }
    true
}

pub(super) fn candidate(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        ["mp4", "m4v", "mov", "mkv", "webm"]
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
    })
}

fn header_input(path: &Path) -> Result<format::context::Input, DecodeError> {
    let path = path.to_str().ok_or(ffmpeg::Error::InvalidData)?;
    let path = CString::new(path).map_err(|_| ffmpeg::Error::InvalidData)?;
    let mut context = std::ptr::null_mut();
    // SAFETY: FFmpeg initializes the output pointer, owns its nested allocations,
    // and frees a failed open. On success Input owns the exclusive close path.
    unsafe {
        let result = ffmpeg::ffi::avformat_open_input(
            &mut context,
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
        );
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
        Ok(format::context::Input::wrap(context))
    }
}

fn find_info(
    input: &mut format::context::Input,
    skip_video: Option<usize>,
) -> Result<(), DecodeError> {
    let mut options: Vec<*mut ffmpeg::ffi::AVDictionary> =
        vec![std::ptr::null_mut(); input.nb_streams() as usize];
    // SAFETY: the input is exclusive, with no workers. The options array has one
    // entry per original stream; FFmpeg bounds access by that original count.
    // Dictionaries are freed after the synchronous call, on success or failure.
    unsafe {
        if let Some(index) = skip_video {
            let result = ffmpeg::ffi::av_dict_set(
                &mut options[index],
                c"skip_frame".as_ptr(),
                c"all".as_ptr(),
                0,
            );
            if result < 0 {
                for option in &mut options {
                    ffmpeg::ffi::av_dict_free(option);
                }
                return Err(ffmpeg::Error::from(result).into());
            }
        }
        let result = ffmpeg::ffi::avformat_find_stream_info(
            input.as_mut_ptr(),
            if skip_video.is_some() {
                options.as_mut_ptr()
            } else {
                std::ptr::null_mut()
            },
        );
        for option in &mut options {
            ffmpeg::ffi::av_dict_free(option);
        }
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
    }
    Ok(())
}

pub(super) fn open(
    path: &Path,
    previous: Option<Proof>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(format::context::Input, Option<Proof>), DecodeError> {
    check_cancelled(cancelled)?;
    ffmpeg::init()?;
    #[cfg(feature = "presentation-verification")]
    let _probe = crate::burst_verification::InputProbeTrace::start(path);
    let lease = enabled().then(|| SourceLease::open(path).ok()).flatten();
    let Some(lease) = lease else {
        let input = format::input(path)?;
        check_cancelled(cancelled)?;
        return Ok((input, None));
    };
    let mut input = header_input(path)?;
    check_cancelled(cancelled)?;
    let header = signature::read(&input).filter(|_| {
        input
            .streams()
            .find(|s| s.parameters().medium() == Type::Video)
            .is_some_and(|s| {
                // SAFETY: the input owns the borrowed initialized parameters.
                unsafe { (*s.parameters().as_ptr()).format == -1 }
            })
    });
    let previous = previous
        .filter(|p| p.source == lease.source && header.as_ref().is_some_and(|h| h == &p.header));
    if let Some(previous) = previous {
        // SAFETY: matching bounded header signatures fix the stream index, codec
        // and missing pixel-format field. This exclusively owned audio input has
        // no decoder workers. The old value was obtained by a complete ordinary
        // probe under the same unchanged-file identity, not inferred from a tag.
        unsafe {
            let stream = &mut **(*input.as_mut_ptr()).streams.add(previous.video);
            (*stream.codecpar).format = previous.pixel_format;
            if previous.h264 {
                // Full H264 probing also establishes these fields. Seed the
                // verified values before native initialization; the complete
                // post-probe signature still has to match without exceptions.
                (*stream.codecpar).bits_per_raw_sample = previous.raw_bits;
                (*stream.codecpar).chroma_location = previous.chroma_location;
            }
        }
        let result = find_info(&mut input, previous.h264.then_some(previous.video));
        check_cancelled(cancelled)?;
        if result.is_ok()
            && signature::read(&input).as_ref() == Some(&previous.complete)
            && lease.verify_path().is_ok()
        {
            #[cfg(test)]
            let previous = Proof {
                reused: true,
                ..previous
            };
            return Ok((input, Some(previous)));
        }
        // Discard every altered native state before ordinary probing. Do not
        // expose a partially initialized input or trust the stale proof again.
        drop(input);
        check_cancelled(cancelled)?;
        let input = format::input(path)?;
        check_cancelled(cancelled)?;
        return Ok((input, None));
    }
    find_info(&mut input, None)?;
    check_cancelled(cancelled)?;
    let proof = header.and_then(|header| {
        let complete = signature::read(&input)?;
        let video = input
            .streams()
            .position(|s| s.parameters().medium() == Type::Video)?;
        let stream = input.stream(video)?;
        let parameters = stream.parameters();
        // SAFETY: the borrowed parameters remain owned by this live input.
        let (pixel_format, raw_bits, chroma_location) = unsafe {
            let p = &*parameters.as_ptr();
            (p.format, p.bits_per_raw_sample, p.chroma_location)
        };
        if pixel_format < 0 || lease.verify_path().is_err() {
            return None;
        }
        Some(Proof {
            source: Arc::clone(&lease.source),
            header,
            complete,
            video,
            pixel_format,
            raw_bits,
            chroma_location,
            h264: parameters.id() == codec::Id::H264,
            #[cfg(test)]
            reused: false,
        })
    });
    Ok((input, proof))
}
