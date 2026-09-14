//! Selected audio stream directly to the bounded mono-s16 source envelope.

use super::Envelope;
use crate::{Cancellation, PreviewError};
use ffmpeg_next::{ChannelLayout, Error, codec, format, frame, media, software::resampling};
use std::path::Path;

pub(crate) fn decode(
    source: &Path,
    width: u32,
    height: u32,
    cancellation: &Cancellation,
) -> Result<image::RgbaImage, PreviewError> {
    let cancellation = cancellation.clone();
    decode_cancellable(source, width, height, move || cancellation.is_cancelled())
}

fn decode_cancellable(
    source: &Path,
    width: u32,
    height: u32,
    cancelled: impl Fn() -> bool + Clone + Send + 'static,
) -> Result<image::RgbaImage, PreviewError> {
    if cancelled() {
        return Err(PreviewError::Cancelled);
    }
    if width == 0 || height == 0 {
        return Err(PreviewError::Generate("invalid waveform dimensions".into()));
    }
    let mut envelope = Envelope::new(width);
    // All FFmpeg objects and borrowed frame data remain on this calling worker.
    // Retain only bounded envelope bins, not PCM for the whole source.
    let result = (|| -> Result<(), Error> {
        let check = || {
            if cancelled() {
                Err(Error::Exit)
            } else {
                Ok(())
            }
        };
        ffmpeg_next::init()?;
        let mut input = format::input_with_interrupt(source, cancelled.clone())?;
        check()?;
        let stream = input
            .streams()
            .best(media::Type::Audio)
            .ok_or(Error::StreamNotFound)?;
        let index = stream.index();
        let mut context = codec::context::Context::from_parameters(stream.parameters())?.decoder();
        context.set_packet_time_base(stream.time_base());
        let mut decoder = context.audio()?;
        let sample = format::Sample::I16(format::sample::Type::Packed);
        let mut resampler: Option<resampling::Context> = None;
        let mut mix_levels = None;
        {
            let mut receive = |decoder: &mut codec::decoder::Audio| -> Result<(), Error> {
                loop {
                    check()?;
                    let mut decoded = frame::Audio::empty();
                    match decoder.receive_frame(&mut decoded) {
                        Ok(()) => {
                            if decoded.channel_layout().is_empty() {
                                decoded.set_channel_layout(ChannelLayout::default(i32::from(
                                    decoded.channels(),
                                )));
                            }
                            let definition = resampling::context::Definition {
                                format: decoded.format(),
                                channel_layout: decoded.channel_layout(),
                                rate: decoded.rate(),
                            };
                            let levels = mono_mix_levels(&decoded)?;
                            if resampler
                                .as_ref()
                                .is_none_or(|current| *current.input() != definition)
                                || levels != mix_levels
                            {
                                // Like the CLI's reinitialized audio filter, discard the old
                                // converter delay but keep the first frame's output rate.
                                let rate = resampler
                                    .as_ref()
                                    .map_or(decoded.rate(), |current| current.output().rate);
                                let mut options = ffmpeg_next::Dictionary::new();
                                if let Some(levels) = levels {
                                    for (key, level) in
                                        ["clev", "slev", "lfe_mix_level"].into_iter().zip(levels)
                                    {
                                        options.set(key, &level.to_string());
                                    }
                                }
                                resampler = Some(resampling::Context::get_with(
                                    definition.format,
                                    definition.channel_layout,
                                    definition.rate,
                                    sample,
                                    ChannelLayout::MONO,
                                    rate,
                                    options,
                                )?);
                                mix_levels = levels;
                            }
                            let current = resampler.as_mut().expect("frame converter");
                            // Reserve the rate-converted input plus delayed output, so an
                            // upsampling segment cannot accumulate PCM inside swresample.
                            let count = (decoded.samples() as u64
                                * u64::from(current.output().rate))
                            .div_ceil(u64::from(current.input().rate))
                                + current.delay().map_or(0, |delay| delay.output as u64);
                            let mut converted =
                                frame::Audio::new(sample, count as usize, ChannelLayout::MONO);
                            current.run(&decoded, &mut converted)?;
                            if converted.samples() != 0 {
                                envelope.push(&converted.data(0)[..converted.samples() * 2]);
                            }
                        }
                        Err(Error::Eof)
                        | Err(Error::Other {
                            errno: ffmpeg_next::error::EAGAIN,
                        }) => return Ok(()),
                        Err(error) => return Err(error),
                    }
                }
            };
            loop {
                check()?;
                let mut packet = ffmpeg_next::Packet::empty();
                match packet.read(&mut input) {
                    Ok(()) if packet.stream() == index => {
                        decoder.send_packet(&packet)?;
                        receive(&mut decoder)?;
                    }
                    Ok(()) => {}
                    Err(Error::Eof) => break,
                    Err(error) => return Err(error),
                }
            }
            decoder.send_eof()?;
            receive(&mut decoder)?;
        }
        while let Some(current) = resampler.as_mut() {
            let Some(delay) = current.delay() else {
                break;
            };
            check()?;
            let mut converted =
                frame::Audio::new(sample, delay.output as usize, ChannelLayout::MONO);
            converted.set_rate(current.output().rate);
            let remaining = current.flush(&mut converted)?;
            if converted.samples() == 0 {
                break;
            }
            envelope.push(&converted.data(0)[..converted.samples() * 2]);
            if remaining.is_none() {
                break;
            }
        }
        Ok(())
    })();
    if cancelled() {
        return Err(PreviewError::Cancelled);
    }
    result.map_err(|error| PreviewError::Generate(error.to_string()))?;
    super::rasterize(&envelope, width, height)
        .map_err(|error| PreviewError::Generate(error.to_string()))
}

fn mono_mix_levels(decoded: &frame::Audio) -> Result<Option<[f64; 3]>, Error> {
    let Some(data) = decoded.side_data(frame::side_data::Type::DownMixInfo) else {
        return Ok(None);
    };
    // These are native-ABI doubles in decoder-owned AVDownmixInfo, not file-endian
    // bytes. Read fields by their bound ABI offsets without casting borrowed data.
    // Mono uses the regular levels, not the stereo-only Lt/Rt matrix coefficients.
    let offsets = [
        std::mem::offset_of!(ffmpeg_next::ffi::AVDownmixInfo, center_mix_level),
        std::mem::offset_of!(ffmpeg_next::ffi::AVDownmixInfo, surround_mix_level),
        std::mem::offset_of!(ffmpeg_next::ffi::AVDownmixInfo, lfe_mix_level),
    ];
    let mut levels = [0.0; 3];
    for (level, offset) in levels.iter_mut().zip(offsets) {
        let bytes = data
            .data()
            .get(offset..offset + 8)
            .ok_or(Error::InvalidData)?;
        *level = f64::from_ne_bytes(bytes.try_into().expect("double field"));
    }
    Ok(Some(levels))
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
