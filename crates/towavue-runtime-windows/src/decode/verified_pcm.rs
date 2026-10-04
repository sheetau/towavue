//! Reopen an unchanged, fully probed PCM WAV without repeating format guessing.
use super::*;
use crate::export::frame::{FrameSource, SourceLease};
use std::sync::Arc;

/// No file, demux context, decoder or sample payload survives completion here.
/// The source identity was recorded under a deny-write/delete lease covering
/// the complete ordinary probe. A new lease must match before bounded reprobe.
pub(super) struct VerifiedPcm {
    source: Arc<FrameSource>,
    signature: Signature,
    #[cfg(test)]
    pub(super) reused: bool,
}

#[derive(Eq, PartialEq)]
struct Signature {
    scalars: [i64; 17],
    channel_mask: u64,
}

fn signature(input: &format::context::Input) -> Option<Signature> {
    if input.format().name() != "wav" || input.nb_streams() != 1 {
        return None;
    }
    let stream = input.stream(0)?;
    let parameters = stream.parameters();
    if parameters.id() != ffmpeg::codec::Id::PCM_S16LE {
        return None;
    }
    // SAFETY: only read initialized public fields from the borrowed live input.
    // Restrict the channel union read to its native-mask discriminator.
    unsafe {
        let p = &*parameters.as_ptr();
        let context = &*input.as_ptr();
        if p.extradata_size != 0
            || p.sample_rate <= 0
            || p.ch_layout.nb_channels <= 0
            || !matches!(
                p.ch_layout.order,
                ffmpeg::ffi::AVChannelOrder::AV_CHANNEL_ORDER_NATIVE
                    | ffmpeg::ffi::AVChannelOrder::AV_CHANNEL_ORDER_UNSPEC
            )
            || p.format != ffmpeg::ffi::AVSampleFormat::AV_SAMPLE_FMT_S16 as i32
            || p.bits_per_coded_sample != 16
            || p.block_align != p.ch_layout.nb_channels.checked_mul(2)?
        {
            return None;
        }
        Some(Signature {
            scalars: [
                p.codec_tag as i64,
                p.sample_rate as i64,
                p.ch_layout.nb_channels as i64,
                p.bits_per_raw_sample as i64,
                p.bit_rate,
                p.block_align as i64,
                p.frame_size as i64,
                p.initial_padding as i64,
                p.trailing_padding as i64,
                p.seek_preroll as i64,
                stream.duration(),
                stream.start_time(),
                stream.time_base().numerator() as i64,
                stream.time_base().denominator() as i64,
                context.duration,
                context.start_time,
                p.ch_layout.order as i64,
            ],
            channel_mask: if p.ch_layout.order
                == ffmpeg::ffi::AVChannelOrder::AV_CHANNEL_ORDER_NATIVE
            {
                p.ch_layout.u.mask
            } else {
                0
            },
        })
    }
}

pub(super) fn open(
    path: &Path,
    previous: Option<VerifiedPcm>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(format::context::Input, Option<VerifiedPcm>), DecodeError> {
    check_cancelled(cancelled)?;
    ffmpeg::init()?;
    // This local optimization is only for ordinary .wav playback. Other paths
    // preserve their existing open behavior without acquiring another lease.
    let lease = path
        .extension()
        .filter(|ext| ext.eq_ignore_ascii_case("wav"))
        .and_then(|_| SourceLease::open(path).ok());
    let previous = previous.filter(|previous| {
        lease
            .as_ref()
            .is_some_and(|lease| lease.source == previous.source)
    });
    #[cfg(feature = "presentation-verification")]
    let probe = crate::burst_verification::InputProbeTrace::start(path);
    #[cfg(test)]
    let mut reused = false;
    let input = if let Some(previous) = previous.as_ref() {
        let mut options = ffmpeg::Dictionary::new();
        // Still run find_stream_info and its decoder/parameter initialization.
        // Only repeated codec guessing is bounded, after a full probe of these
        // exact unchanged bytes. Never apply this to an unverified first open.
        options.set("max_probe_packets", "1");
        let fast = format::input_with_dictionary(path, options);
        match fast {
            Ok(input)
                if signature(&input).as_ref() == Some(&previous.signature)
                    && lease
                        .as_ref()
                        .is_some_and(|lease| lease.verify_path().is_ok()) =>
            {
                #[cfg(test)]
                {
                    reused = true;
                }
                input
            }
            other => {
                drop(other);
                check_cancelled(cancelled)?;
                format::input(path)?
            }
        }
    } else {
        format::input(path)?
    };
    #[cfg(feature = "presentation-verification")]
    drop(probe);
    check_cancelled(cancelled)?;
    let verified = lease
        .filter(|lease| lease.verify_path().is_ok())
        .and_then(|lease| {
            signature(&input).map(|signature| VerifiedPcm {
                source: lease.source,
                signature,
                #[cfg(test)]
                reused,
            })
        });
    Ok((input, verified))
}
