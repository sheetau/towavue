use std::path::Path;

use ffmpeg::codec;
use ffmpeg::format::{self, Pixel, Sample};
use ffmpeg::frame;
use ffmpeg::media::Type;
use ffmpeg::software::resampling;
use ffmpeg::software::scaling::{self, flag::Flags};
use ffmpeg::{ChannelLayout, Rational};
use ffmpeg_next as ffmpeg;
use thiserror::Error;
use towavue_core::MediaTime;

use crate::GraphicsDevice;

const OUTPUT_AUDIO_CHANNELS: usize = 2;
const BYTES_PER_F32: usize = size_of::<f32>();

/// One tightly packed RGBA frame produced by software decoding.
#[derive(Debug)]
pub struct VideoFrame {
    pub presentation_time: MediaTime,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Runtime-only owner of one FFmpeg-referenced D3D11 decode surface.
pub(crate) struct HardwareVideoFrame {
    pub(crate) presentation_time: MediaTime,
    pub(crate) width: u32,
    pub(crate) height: u32,
    frame: frame::Video,
}

impl HardwareVideoFrame {
    pub(crate) fn texture_and_slice(&self) -> Option<(*mut std::ffi::c_void, u32)> {
        // AV_PIX_FMT_D3D11 defines data[0] as ID3D11Texture2D* and data[1]
        // as an integer array-slice index. The AVFrame keeps the texture alive.
        unsafe {
            let frame = self.frame.as_ptr();
            let texture: *mut std::ffi::c_void = (*frame).data[0].cast();
            (!texture.is_null()).then_some((texture, (*frame).data[1] as usize as u32))
        }
    }
}

pub(crate) enum RuntimeDecodeOutput {
    Video(HardwareVideoFrame),
    Audio(AudioChunk),
}

/// The fixed packed sample layout supplied to WASAPI for one stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// One packed interleaved f32 audio block.
#[derive(Debug)]
pub struct AudioChunk {
    pub format: AudioFormat,
    pub frames: usize,
    pub bytes: Vec<u8>,
}

/// A decoded item delivered to the presentation and audio queues.
#[derive(Debug)]
pub enum DecodeOutput {
    Video(VideoFrame),
    Audio(AudioChunk),
}

/// Counters returned after the input reaches EOF and all decoders are drained.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DecodeSummary {
    pub video_frames: u64,
    pub audio_frames: u64,
}

/// A failure in the M1 software decode path.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("FFmpeg failed: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("the input has no decodable audio or video stream")]
    NoMediaStream,
    #[error("decoded frame dimensions exceed the addressable buffer size")]
    FrameTooLarge,
    #[error("the output consumer stopped accepting decoded data")]
    ConsumerClosed,
    #[error("D3D11VA is unavailable: {0}")]
    HardwareUnavailable(String),
}

struct VideoPipeline {
    stream_index: usize,
    time_base: Rational,
    decoder: codec::decoder::Video,
    scaler: scaling::Context,
}

struct HardwareVideoPipeline {
    stream_index: usize,
    time_base: Rational,
    decoder: codec::decoder::Video,
}

impl HardwareVideoPipeline {
    fn receive(
        &mut self,
        emit: &mut impl FnMut(RuntimeDecodeOutput) -> bool,
        summary: &mut DecodeSummary,
    ) -> Result<(), DecodeError> {
        loop {
            let mut decoded = frame::Video::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    if decoded.format() != Pixel::D3D11 {
                        return Err(DecodeError::HardwareUnavailable(format!(
                            "decoder selected {:?} instead of D3D11",
                            decoded.format()
                        )));
                    }
                    let output = HardwareVideoFrame {
                        presentation_time: timestamp_to_media_time(
                            decoded.timestamp(),
                            self.time_base,
                        ),
                        width: decoded.width(),
                        height: decoded.height(),
                        frame: decoded,
                    };
                    if !emit(RuntimeDecodeOutput::Video(output)) {
                        return Err(DecodeError::ConsumerClosed);
                    }
                    summary.video_frames += 1;
                }
                Err(error) if decoder_is_drained(error) => return Ok(()),
                Err(error) if summary.video_frames == 0 => {
                    return Err(DecodeError::HardwareUnavailable(error.to_string()));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl VideoPipeline {
    fn receive(
        &mut self,
        emit: &mut impl FnMut(DecodeOutput) -> bool,
        summary: &mut DecodeSummary,
    ) -> Result<(), DecodeError> {
        loop {
            let mut decoded = frame::Video::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let mut rgba = frame::Video::empty();
                    self.scaler.run(&decoded, &mut rgba)?;
                    let output = copy_video_frame(&decoded, &rgba, self.time_base)?;
                    if !emit(DecodeOutput::Video(output)) {
                        return Err(DecodeError::ConsumerClosed);
                    }
                    summary.video_frames += 1;
                }
                Err(error) if decoder_is_drained(error) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
    }
}

struct AudioPipeline {
    stream_index: usize,
    decoder: codec::decoder::Audio,
    resampler: resampling::Context,
    output_format: AudioFormat,
}

impl AudioPipeline {
    fn receive(
        &mut self,
        emit: &mut impl FnMut(DecodeOutput) -> bool,
        summary: &mut DecodeSummary,
    ) -> Result<(), DecodeError> {
        loop {
            let mut decoded = frame::Audio::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let mut converted = frame::Audio::empty();
                    self.resampler.run(&decoded, &mut converted)?;
                    emit_audio_frame(&converted, self.output_format, emit, summary)?;
                }
                Err(error) if decoder_is_drained(error) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn flush_resampler(
        &mut self,
        emit: &mut impl FnMut(DecodeOutput) -> bool,
        summary: &mut DecodeSummary,
    ) -> Result<(), DecodeError> {
        while let Some(delay) = self.resampler.delay() {
            let mut converted = frame::Audio::new(
                Sample::F32(ffmpeg::format::sample::Type::Packed),
                delay.output as usize,
                ChannelLayout::STEREO,
            );
            converted.set_rate(self.output_format.sample_rate);
            let remaining = self.resampler.flush(&mut converted)?;
            emit_audio_frame(&converted, self.output_format, emit, summary)?;
            if remaining.is_none() {
                break;
            }
        }
        Ok(())
    }
}

pub fn decode_file(
    path: &Path,
    mut emit: impl FnMut(DecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    ffmpeg::init()?;
    let mut input = format::input(path)?;
    let mut video = create_video_pipeline(&input)?;
    let mut audio = create_audio_pipeline(&input)?;
    if video.is_none() && audio.is_none() {
        return Err(DecodeError::NoMediaStream);
    }

    let mut summary = DecodeSummary::default();
    for (stream, packet) in input.packets() {
        if let Some(pipeline) = video.as_mut()
            && stream.index() == pipeline.stream_index
        {
            pipeline.decoder.send_packet(&packet)?;
            pipeline.receive(&mut emit, &mut summary)?;
        } else if let Some(pipeline) = audio.as_mut()
            && stream.index() == pipeline.stream_index
        {
            pipeline.decoder.send_packet(&packet)?;
            pipeline.receive(&mut emit, &mut summary)?;
        }
    }

    if let Some(pipeline) = video.as_mut() {
        pipeline.decoder.send_eof()?;
        pipeline.receive(&mut emit, &mut summary)?;
    }
    if let Some(pipeline) = audio.as_mut() {
        pipeline.decoder.send_eof()?;
        pipeline.receive(&mut emit, &mut summary)?;
        pipeline.flush_resampler(&mut emit, &mut summary)?;
    }

    Ok(summary)
}

pub(crate) fn decode_file_hardware(
    path: &Path,
    device: &GraphicsDevice,
    mut emit: impl FnMut(RuntimeDecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    ffmpeg::init()?;
    let mut input = format::input(path)?;
    let mut video = create_hardware_video_pipeline(&input, device)?;
    let mut audio = create_audio_pipeline(&input)?;
    let mut summary = DecodeSummary::default();

    for (stream, packet) in input.packets() {
        if stream.index() == video.stream_index {
            if let Err(error) = video.decoder.send_packet(&packet) {
                if summary.video_frames == 0 {
                    return Err(DecodeError::HardwareUnavailable(error.to_string()));
                }
                return Err(error.into());
            }
            video.receive(&mut emit, &mut summary)?;
        } else if let Some(pipeline) = audio.as_mut()
            && stream.index() == pipeline.stream_index
        {
            pipeline.decoder.send_packet(&packet)?;
            pipeline.receive(
                &mut |output| match output {
                    DecodeOutput::Audio(chunk) => emit(RuntimeDecodeOutput::Audio(chunk)),
                    DecodeOutput::Video(_) => unreachable!("audio pipeline emitted video"),
                },
                &mut summary,
            )?;
        }
    }

    if let Err(error) = video.decoder.send_eof() {
        if summary.video_frames == 0 {
            return Err(DecodeError::HardwareUnavailable(error.to_string()));
        }
        return Err(error.into());
    }
    video.receive(&mut emit, &mut summary)?;
    if let Some(pipeline) = audio.as_mut() {
        pipeline.decoder.send_eof()?;
        let mut emit_audio = |output| match output {
            DecodeOutput::Audio(chunk) => emit(RuntimeDecodeOutput::Audio(chunk)),
            DecodeOutput::Video(_) => unreachable!("audio pipeline emitted video"),
        };
        pipeline.receive(&mut emit_audio, &mut summary)?;
        pipeline.flush_resampler(&mut emit_audio, &mut summary)?;
    }
    Ok(summary)
}

pub(crate) fn probe_audio_format(path: &Path) -> Result<Option<AudioFormat>, DecodeError> {
    ffmpeg::init()?;
    let input = format::input(path)?;
    Ok(create_audio_pipeline(&input)?.map(|pipeline| pipeline.output_format))
}

fn create_video_pipeline(
    input: &format::context::Input,
) -> Result<Option<VideoPipeline>, DecodeError> {
    let Some(stream) = input.streams().best(Type::Video) else {
        return Ok(None);
    };
    let stream_index = stream.index();
    let time_base = stream.time_base();
    let context = codec::context::Context::from_parameters(stream.parameters())?;
    let decoder = context.decoder().video()?;
    let scaler = scaling::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        Pixel::RGBA,
        decoder.width(),
        decoder.height(),
        Flags::BILINEAR,
    )?;
    Ok(Some(VideoPipeline {
        stream_index,
        time_base,
        decoder,
        scaler,
    }))
}

fn create_hardware_video_pipeline(
    input: &format::context::Input,
    device: &GraphicsDevice,
) -> Result<HardwareVideoPipeline, DecodeError> {
    let stream = input
        .streams()
        .best(Type::Video)
        .ok_or_else(|| DecodeError::HardwareUnavailable("input has no video stream".to_owned()))?;
    let stream_index = stream.index();
    let time_base = stream.time_base();
    let mut context = codec::context::Context::from_parameters(stream.parameters())?;
    if !codec_supports_d3d11va(&context) {
        return Err(DecodeError::HardwareUnavailable(format!(
            "codec {:?} does not expose a D3D11VA configuration",
            context.id()
        )));
    }
    configure_d3d11va(&mut context, device)?;
    let decoder = context.decoder().video()?;
    Ok(HardwareVideoPipeline {
        stream_index,
        time_base,
        decoder,
    })
}

fn codec_supports_d3d11va(context: &codec::context::Context) -> bool {
    let Some(decoder) = codec::decoder::find(context.id()) else {
        return false;
    };
    // AVCodecHWConfig entries are immutable codec metadata terminated by null.
    unsafe {
        let mut index = 0;
        loop {
            let configuration = ffmpeg::ffi::avcodec_get_hw_config(decoder.as_ptr(), index);
            if configuration.is_null() {
                return false;
            }
            let supports_device_context = (*configuration).methods
                & ffmpeg::ffi::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32
                != 0;
            if supports_device_context
                && (*configuration).device_type
                    == ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA
                && (*configuration).pix_fmt == ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_D3D11
            {
                return true;
            }
            index += 1;
        }
    }
}

fn configure_d3d11va(
    context: &mut codec::context::Context,
    device: &GraphicsDevice,
) -> Result<(), DecodeError> {
    // FFmpeg takes ownership of both the AVBufferRef assigned to AVCodecContext
    // and the cloned COM reference stored in AVD3D11VADeviceContext.
    unsafe {
        let mut device_ref = ffmpeg::ffi::av_hwdevice_ctx_alloc(
            ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
        );
        if device_ref.is_null() {
            return Err(DecodeError::HardwareUnavailable(
                "FFmpeg could not allocate a D3D11VA device context".to_owned(),
            ));
        }
        let hardware_context = (*device_ref).data.cast::<ffmpeg::ffi::AVHWDeviceContext>();
        let d3d11_context = (*hardware_context)
            .hwctx
            .cast::<ffmpeg::ffi::AVD3D11VADeviceContext>();
        (*d3d11_context).device = device.clone_raw().cast();
        let result = ffmpeg::ffi::av_hwdevice_ctx_init(device_ref);
        if result < 0 {
            ffmpeg::ffi::av_buffer_unref(&mut device_ref);
            return Err(DecodeError::HardwareUnavailable(
                ffmpeg::Error::from(result).to_string(),
            ));
        }

        let codec_context = context.as_mut_ptr();
        (*codec_context).hw_device_ctx = device_ref;
        (*codec_context).get_format = Some(select_d3d11_pixel_format);
        (*codec_context).extra_hw_frames = 4;
    }
    Ok(())
}

unsafe extern "C" fn select_d3d11_pixel_format(
    _context: *mut ffmpeg::ffi::AVCodecContext,
    formats: *const ffmpeg::ffi::AVPixelFormat,
) -> ffmpeg::ffi::AVPixelFormat {
    // FFmpeg owns a valid AV_PIX_FMT_NONE-terminated array for the duration
    // of this callback; no pointer or element is retained after returning.
    let mut current = formats;
    while unsafe { *current } != ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_NONE {
        if unsafe { *current } == ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_D3D11 {
            return ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_D3D11;
        }
        current = unsafe { current.add(1) };
    }
    ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_NONE
}

fn create_audio_pipeline(
    input: &format::context::Input,
) -> Result<Option<AudioPipeline>, DecodeError> {
    let Some(stream) = input.streams().best(Type::Audio) else {
        return Ok(None);
    };
    let stream_index = stream.index();
    let context = codec::context::Context::from_parameters(stream.parameters())?;
    let decoder = context.decoder().audio()?;
    let source_layout = if decoder.channel_layout().is_empty() {
        ChannelLayout::default(decoder.channels() as i32)
    } else {
        decoder.channel_layout()
    };
    let sample_rate = decoder.rate();
    let output_format = AudioFormat {
        sample_rate,
        channels: OUTPUT_AUDIO_CHANNELS as u16,
    };
    let resampler = resampling::Context::get(
        decoder.format(),
        source_layout,
        sample_rate,
        Sample::F32(ffmpeg::format::sample::Type::Packed),
        ChannelLayout::STEREO,
        sample_rate,
    )?;
    Ok(Some(AudioPipeline {
        stream_index,
        decoder,
        resampler,
        output_format,
    }))
}

fn copy_video_frame(
    decoded: &frame::Video,
    rgba: &frame::Video,
    time_base: Rational,
) -> Result<VideoFrame, DecodeError> {
    let width = rgba.width() as usize;
    let height = rgba.height() as usize;
    let row_bytes = width.checked_mul(4).ok_or(DecodeError::FrameTooLarge)?;
    let buffer_len = row_bytes
        .checked_mul(height)
        .ok_or(DecodeError::FrameTooLarge)?;
    let mut pixels = vec![0; buffer_len];
    let source = rgba.data(0);
    let stride = rgba.stride(0);
    for row in 0..height {
        let source_start = row * stride;
        let target_start = row * row_bytes;
        pixels[target_start..target_start + row_bytes]
            .copy_from_slice(&source[source_start..source_start + row_bytes]);
    }

    Ok(VideoFrame {
        presentation_time: timestamp_to_media_time(decoded.timestamp(), time_base),
        width: rgba.width(),
        height: rgba.height(),
        rgba: pixels,
    })
}

fn emit_audio_frame(
    frame: &frame::Audio,
    format: AudioFormat,
    emit: &mut impl FnMut(DecodeOutput) -> bool,
    summary: &mut DecodeSummary,
) -> Result<(), DecodeError> {
    if frame.samples() == 0 {
        return Ok(());
    }
    let byte_count = frame
        .samples()
        .checked_mul(OUTPUT_AUDIO_CHANNELS)
        .and_then(|samples| samples.checked_mul(BYTES_PER_F32))
        .ok_or(DecodeError::FrameTooLarge)?;
    let bytes = frame.data(0)[..byte_count].to_vec();
    let frames = frame.samples();
    if !emit(DecodeOutput::Audio(AudioChunk {
        format,
        frames,
        bytes,
    })) {
        return Err(DecodeError::ConsumerClosed);
    }
    summary.audio_frames += frames as u64;
    Ok(())
}

fn timestamp_to_media_time(timestamp: Option<i64>, time_base: Rational) -> MediaTime {
    let timestamp = i128::from(timestamp.unwrap_or(0));
    let numerator = i128::from(time_base.numerator());
    let denominator = i128::from(time_base.denominator());
    let nanoseconds = timestamp
        .saturating_mul(numerator)
        .saturating_mul(1_000_000_000)
        / denominator;
    MediaTime::from_nanoseconds(nanoseconds.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

fn decoder_is_drained(error: ffmpeg::Error) -> bool {
    error == ffmpeg::Error::Eof
        || matches!(error, ffmpeg::Error::Other { errno } if errno == ffmpeg::error::EAGAIN)
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::Rational;
    use ffmpeg_next::ffi::AVPixelFormat;

    use super::{select_d3d11_pixel_format, timestamp_to_media_time};

    #[test]
    fn timestamp_conversion_uses_stream_time_base() {
        let time = timestamp_to_media_time(Some(90_000), Rational::new(1, 90_000));

        assert_eq!(time.as_nanoseconds(), 1_000_000_000);
    }

    #[test]
    fn missing_timestamp_maps_to_zero() {
        let time = timestamp_to_media_time(None, Rational::new(1, 1_000));

        assert_eq!(time.as_nanoseconds(), 0);
    }

    #[test]
    fn hardware_format_callback_selects_d3d11() {
        let formats = [
            AVPixelFormat::AV_PIX_FMT_YUV420P,
            AVPixelFormat::AV_PIX_FMT_D3D11,
            AVPixelFormat::AV_PIX_FMT_NONE,
        ];

        let selected = unsafe { select_d3d11_pixel_format(std::ptr::null_mut(), formats.as_ptr()) };

        assert_eq!(selected, AVPixelFormat::AV_PIX_FMT_D3D11);
    }
}
