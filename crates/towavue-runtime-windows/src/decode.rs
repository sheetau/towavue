use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;
use std::time::Duration;

use ffmpeg::codec;
use ffmpeg::format::{self, Pixel, Sample};
use ffmpeg::frame;
use ffmpeg::media::Type;
use ffmpeg::software::resampling;
use ffmpeg::software::scaling::{self, flag::Flags};
use ffmpeg::{ChannelLayout, Rational, Rescale, Rounding};
use ffmpeg_next as ffmpeg;
use thiserror::Error;
use towavue_core::MediaTime;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::core::Interface;

use crate::{GraphicsDevice, VideoOrientation};

mod frame_step;
pub use frame_step::adjacent_video_frame;

const OUTPUT_AUDIO_CHANNELS: usize = 2;
const BYTES_PER_F32: usize = size_of::<f32>();
const PACKET_QUEUE_CAPACITY: usize = 32;
const DECODED_QUEUE_CAPACITY: usize = 2;

/// One tightly packed RGBA frame produced by software decoding.
#[derive(Debug)]
pub struct VideoFrame {
    pub presentation_time: MediaTime,
    pub width: u32,
    pub height: u32,
    pub pixel_aspect: f32,
    pub orientation: VideoOrientation,
    pub rgba: Vec<u8>,
}

/// Runtime-only owner of a decoded or independently retained D3D11 surface.
pub(crate) struct HardwareVideoFrame {
    pub(crate) presentation_time: MediaTime,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixel_aspect: f32,
    pub(crate) orientation: VideoOrientation,
    pub(crate) transfer: VideoTransfer,
    backing: HardwareFrameBacking,
}

enum HardwareFrameBacking {
    Decoded(frame::Video),
    Retained(ID3D11Texture2D),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VideoTransfer {
    Sdr,
    Pq,
    Hlg,
}

impl HardwareVideoFrame {
    pub(crate) fn texture_and_slice(&self) -> Option<(*mut std::ffi::c_void, u32)> {
        let frame = match &self.backing {
            HardwareFrameBacking::Decoded(frame) => frame,
            HardwareFrameBacking::Retained(texture) => return Some((texture.as_raw(), 0)),
        };
        // AV_PIX_FMT_D3D11 defines data[0] as ID3D11Texture2D* and data[1]
        // as an integer array-slice index. The AVFrame keeps the texture alive.
        unsafe {
            let frame = frame.as_ptr();
            let texture: *mut std::ffi::c_void = (*frame).data[0].cast();
            (!texture.is_null()).then_some((texture, (*frame).data[1] as usize as u32))
        }
    }

    pub(crate) fn retain_surface(
        &mut self,
        device: &GraphicsDevice,
    ) -> Result<(), crate::RenderError> {
        if matches!(self.backing, HardwareFrameBacking::Decoded(_)) {
            // Replace ownership only after allocation and submission succeed. Dropping the
            // AVFrame then releases its pool reference; D3D11 retains in-flight copy resources.
            let texture = device.copy_video_surface(self)?;
            self.backing = HardwareFrameBacking::Retained(texture);
        }
        Ok(())
    }
}

pub(crate) enum RuntimeDecodeOutput {
    Video(HardwareVideoFrame),
}

pub(crate) enum ParallelRuntimeDecodeOutput {
    Item(RuntimeDecodeOutput),
    VideoFinished,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum DecodeStream {
    Video,
    Audio,
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
    pub presentation_time: MediaTime,
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

pub(crate) enum ParallelSoftwareDecodeOutput {
    Item(DecodeOutput),
    VideoFinished,
    AudioFinished,
}

/// Counters returned after natural EOF or the requested playback range ends.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DecodeSummary {
    pub video_frames: u64,
    pub audio_frames: u64,
}

/// A failure in the M1 software decode path.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("frame stepping requires video presentation timestamps")]
    MissingVideoTimestamp,
    #[error("video display matrix is not a supported quarter-turn or reflection")]
    UnsupportedOrientation,
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
    #[error("a decode worker could not start: {0}")]
    WorkerStart(#[from] std::io::Error),
    #[error("a decode worker panicked")]
    WorkerPanicked,
}

enum ParallelDecodeOutput {
    SoftwareVideo(VideoFrame),
    HardwareVideo(HardwareVideoFrame),
    Audio(AudioChunk),
    VideoFinished,
    AudioFinished,
    Failed(DecodeError),
}

enum ParallelVideoPipeline {
    Software(VideoPipeline),
    Hardware(HardwareVideoPipeline),
}

struct StreamConfig {
    index: usize,
    time_base: Rational,
    parameters: codec::Parameters,
    display_matrix: Option<Vec<u8>>,
}

enum ParallelVideoConfig {
    Software(StreamConfig),
    Hardware(StreamConfig, GraphicsDevice),
}

struct VideoPipeline {
    stream_index: usize,
    time_base: Rational,
    decoder: codec::decoder::Video,
    scaler: scaling::Context,
    orientation: VideoOrientation,
}

struct HardwareVideoPipeline {
    time_base: Rational,
    decoder: codec::decoder::Video,
    transfer: VideoTransfer,
    orientation: VideoOrientation,
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
                        pixel_aspect: pixel_aspect(decoded.aspect_ratio()),
                        orientation: frame_orientation(&decoded, self.orientation)?,
                        transfer: video_transfer(&decoded).unwrap_or(self.transfer),
                        backing: HardwareFrameBacking::Decoded(decoded),
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

fn video_transfer(frame: &frame::Video) -> Option<VideoTransfer> {
    classify_transfer(frame.color_transfer_characteristic())
}

fn frame_orientation(
    frame: &frame::Video,
    fallback: VideoOrientation,
) -> Result<VideoOrientation, DecodeError> {
    frame
        .side_data(ffmpeg::util::frame::side_data::Type::DisplayMatrix)
        .map_or(Ok(fallback), |data| {
            VideoOrientation::from_bytes(Some(data.data()))
        })
}

fn classify_transfer(
    characteristic: ffmpeg::color::TransferCharacteristic,
) -> Option<VideoTransfer> {
    match characteristic {
        ffmpeg::color::TransferCharacteristic::SMPTE2084 => Some(VideoTransfer::Pq),
        ffmpeg::color::TransferCharacteristic::ARIB_STD_B67 => Some(VideoTransfer::Hlg),
        ffmpeg::color::TransferCharacteristic::Unspecified => None,
        _ => Some(VideoTransfer::Sdr),
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
                    let output =
                        copy_video_frame(&decoded, &rgba, self.time_base, self.orientation)?;
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
    time_base: Rational,
    decoder: codec::decoder::Audio,
    resampler: resampling::Context,
    output_format: AudioFormat,
    next_presentation_time: MediaTime,
    next_sample: i64,
}

impl AudioPipeline {
    fn sample_presentation_time(&mut self, timestamp: Option<i64>, samples: usize) -> MediaTime {
        let sample_base = Rational(1, self.output_format.sample_rate as i32);
        let sample = if let Some(timestamp) = timestamp {
            if self.next_sample != ffmpeg::ffi::AV_NOPTS_VALUE
                && timestamp
                    > self
                        .next_sample
                        .rescale_with(sample_base, self.time_base, Rounding::Up)
            {
                self.next_sample = ffmpeg::ffi::AV_NOPTS_VALUE;
            }
            // The worker exclusively owns next_sample for this pipeline. FFmpeg borrows
            // its valid mutable pointer only for this call and retains nothing. Decoded
            // timestamps exclude AV_NOPTS_VALUE; sample counts are non-negative i32 values.
            unsafe {
                ffmpeg::ffi::av_rescale_delta(
                    self.time_base.into(),
                    timestamp,
                    sample_base.into(),
                    samples as i32,
                    &mut self.next_sample,
                    sample_base.into(),
                )
            }
        } else {
            let sample = if self.next_sample == ffmpeg::ffi::AV_NOPTS_VALUE {
                0
            } else {
                self.next_sample
            };
            self.next_sample = sample.saturating_add(samples as i64);
            sample
        };
        timestamp_to_media_time(Some(sample), sample_base)
    }

    fn receive(
        &mut self,
        emit: &mut impl FnMut(DecodeOutput) -> bool,
        summary: &mut DecodeSummary,
    ) -> Result<(), DecodeError> {
        loop {
            let mut decoded = frame::Audio::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let presentation_time =
                        self.sample_presentation_time(decoded.timestamp(), decoded.samples());
                    let mut converted = frame::Audio::empty();
                    // Match the default layout used at resampler setup when PCM supplies
                    // only a channel count. Preserve explicitly declared speaker layouts.
                    if decoded.channel_layout().is_empty() {
                        decoded.set_channel_layout(ChannelLayout::default(i32::from(
                            decoded.channels(),
                        )));
                    }
                    self.resampler.run(&decoded, &mut converted)?;
                    self.next_presentation_time =
                        presentation_time.saturating_add(Duration::from_secs_f64(
                            converted.samples() as f64 / self.output_format.sample_rate as f64,
                        ));
                    emit_audio_frame(
                        &converted,
                        presentation_time,
                        self.output_format,
                        emit,
                        summary,
                    )?;
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
            emit_audio_frame(
                &converted,
                self.next_presentation_time,
                self.output_format,
                emit,
                summary,
            )?;
            self.next_presentation_time =
                self.next_presentation_time
                    .saturating_add(Duration::from_secs_f64(
                        converted.samples() as f64 / self.output_format.sample_rate as f64,
                    ));
            if remaining.is_none() {
                break;
            }
        }
        Ok(())
    }
}

pub fn decode_file(
    path: &Path,
    emit: impl FnMut(DecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    decode_file_from(path, MediaTime::ZERO, emit)
}

pub(crate) fn decode_file_from(
    path: &Path,
    minimum_time: MediaTime,
    mut emit: impl FnMut(DecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    ffmpeg::init()?;
    let mut input = format::input(path)?;
    let origin = input_origin(&input);
    let mut video = create_video_pipeline(&input)?;
    let mut audio = create_audio_pipeline(&input)?;
    if video.is_none() && audio.is_none() {
        return Err(DecodeError::NoMediaStream);
    }
    seek_input(&mut input, minimum_time, video.is_some(), &|| false)?;
    let mut filtered_emit = |output| {
        let presentation_time = match &output {
            DecodeOutput::Video(frame) => frame.presentation_time,
            DecodeOutput::Audio(chunk) => chunk.presentation_time,
        };
        minimum_time > MediaTime::ZERO && presentation_time < minimum_time || emit(output)
    };

    let mut summary = DecodeSummary::default();
    for (stream, mut packet) in input.packets() {
        normalize_packet_time(&mut packet, stream.time_base(), origin);
        if let Some(pipeline) = video.as_mut()
            && stream.index() == pipeline.stream_index
        {
            pipeline.decoder.send_packet(&packet)?;
            pipeline.receive(&mut filtered_emit, &mut summary)?;
        } else if let Some(pipeline) = audio.as_mut()
            && stream.index() == pipeline.stream_index
        {
            pipeline.decoder.send_packet(&packet)?;
            pipeline.receive(&mut filtered_emit, &mut summary)?;
        }
    }

    if let Some(pipeline) = video.as_mut() {
        pipeline.decoder.send_eof()?;
        pipeline.receive(&mut filtered_emit, &mut summary)?;
    }
    if let Some(pipeline) = audio.as_mut() {
        pipeline.decoder.send_eof()?;
        pipeline.receive(&mut filtered_emit, &mut summary)?;
        pipeline.flush_resampler(&mut filtered_emit, &mut summary)?;
    }

    Ok(summary)
}

#[cfg(test)]
pub(crate) fn decode_file_parallel(
    path: &Path,
    minimum_time: MediaTime,
    maximum_time: Option<MediaTime>,
    stream: Option<DecodeStream>,
    emit: impl FnMut(ParallelSoftwareDecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    decode_file_parallel_cancellable(path, minimum_time, maximum_time, stream, &|| false, emit)
}

pub(crate) fn decode_file_parallel_cancellable(
    path: &Path,
    minimum_time: MediaTime,
    maximum_time: Option<MediaTime>,
    stream: Option<DecodeStream>,
    cancelled: &(dyn Fn() -> bool + Sync),
    emit: impl FnMut(ParallelSoftwareDecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    ParallelInput::open(path, cancelled)?.decode_software(
        minimum_time,
        maximum_time,
        stream,
        cancelled,
        emit,
    )
}

/// An exclusively owned demux input, moved back to its session after workers join.
/// Decoders and packet queues are per run; no stream borrow escapes a run.
pub(crate) struct ParallelInput {
    input: format::context::Input,
    started: bool,
}

impl ParallelInput {
    pub(crate) fn open(
        path: &Path,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Self, DecodeError> {
        check_cancelled(cancelled)?;
        ffmpeg::init()?;
        let input = format::input(path)?;
        check_cancelled(cancelled)?;
        Ok(Self {
            input,
            started: false,
        })
    }

    pub(crate) fn decode_software(
        &mut self,
        minimum_time: MediaTime,
        maximum_time: Option<MediaTime>,
        stream: Option<DecodeStream>,
        cancelled: &(dyn Fn() -> bool + Sync),
        mut emit: impl FnMut(ParallelSoftwareDecodeOutput) -> bool,
    ) -> Result<DecodeSummary, DecodeError> {
        self.decode(
            None,
            minimum_time,
            maximum_time,
            stream,
            cancelled,
            |output| match output {
                ParallelDecodeOutput::SoftwareVideo(frame) => emit(
                    ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)),
                ),
                ParallelDecodeOutput::Audio(chunk) => emit(ParallelSoftwareDecodeOutput::Item(
                    DecodeOutput::Audio(chunk),
                )),
                ParallelDecodeOutput::AudioFinished => {
                    emit(ParallelSoftwareDecodeOutput::AudioFinished)
                }
                ParallelDecodeOutput::VideoFinished => {
                    emit(ParallelSoftwareDecodeOutput::VideoFinished)
                }
                ParallelDecodeOutput::HardwareVideo(_) | ParallelDecodeOutput::Failed(_) => {
                    unreachable!("parallel decode output is handled internally")
                }
            },
        )
    }

    pub(crate) fn decode_hardware(
        &mut self,
        device: &GraphicsDevice,
        minimum_time: MediaTime,
        maximum_time: Option<MediaTime>,
        cancelled: &(dyn Fn() -> bool + Sync),
        mut emit: impl FnMut(ParallelRuntimeDecodeOutput) -> bool,
    ) -> Result<DecodeSummary, DecodeError> {
        self.decode(
            Some(device),
            minimum_time,
            maximum_time,
            Some(DecodeStream::Video),
            cancelled,
            |output| match output {
                ParallelDecodeOutput::HardwareVideo(frame) => emit(
                    ParallelRuntimeDecodeOutput::Item(RuntimeDecodeOutput::Video(frame)),
                ),
                ParallelDecodeOutput::VideoFinished => {
                    emit(ParallelRuntimeDecodeOutput::VideoFinished)
                }
                ParallelDecodeOutput::SoftwareVideo(_)
                | ParallelDecodeOutput::Audio(_)
                | ParallelDecodeOutput::AudioFinished
                | ParallelDecodeOutput::Failed(_) => {
                    unreachable!("parallel decode output is handled internally")
                }
            },
        )
    }

    fn decode(
        &mut self,
        hardware_device: Option<&GraphicsDevice>,
        minimum_time: MediaTime,
        maximum_time: Option<MediaTime>,
        stream: Option<DecodeStream>,
        cancelled: &(dyn Fn() -> bool + Sync),
        mut emit: impl FnMut(ParallelDecodeOutput) -> bool,
    ) -> Result<DecodeSummary, DecodeError> {
        check_cancelled(cancelled)?;
        let input = &mut self.input;
        let video_config = (stream != Some(DecodeStream::Audio))
            .then(|| best_stream_config(input, Type::Video))
            .flatten();
        let video = match (hardware_device, video_config) {
            (Some(device), Some(config)) => {
                Some(ParallelVideoConfig::Hardware(config, device.clone()))
            }
            (Some(_), None) => {
                return Err(DecodeError::HardwareUnavailable(
                    "input has no video stream".to_owned(),
                ));
            }
            (None, config) => config.map(ParallelVideoConfig::Software),
        };
        let audio = (stream != Some(DecodeStream::Video))
            .then(|| best_stream_config(input, Type::Audio))
            .flatten();
        if video.is_none() && audio.is_none() {
            if let Some(stream) = stream
                && best_stream_config(
                    input,
                    match stream {
                        DecodeStream::Video => Type::Audio,
                        DecodeStream::Audio => Type::Video,
                    },
                )
                .is_some()
            {
                return if emit(match stream {
                    DecodeStream::Video => ParallelDecodeOutput::VideoFinished,
                    DecodeStream::Audio => ParallelDecodeOutput::AudioFinished,
                }) {
                    Ok(DecodeSummary::default())
                } else {
                    Err(DecodeError::ConsumerClosed)
                };
            }
            return Err(DecodeError::NoMediaStream);
        }
        if self.started && minimum_time <= MediaTime::ZERO {
            if input.format().name() == "mpegts" {
                seek_byte_position(input, 0)?;
            } else {
                let origin = input_origin(input);
                input.seek(origin, ..origin)?;
            }
        }
        self.started = true;
        let seek_target =
            if stream == Some(DecodeStream::Video) && maximum_time == Some(minimum_time) {
                minimum_time.saturating_sub(Duration::from_nanos(1))
            } else {
                minimum_time
            };
        seek_input(input, seek_target, video.is_some(), cancelled)?;
        check_cancelled(cancelled)?;
        run_parallel_workers(
            input,
            video,
            audio,
            minimum_time,
            maximum_time,
            cancelled,
            &mut emit,
        )
    }
}

fn run_parallel_workers(
    input: &mut format::context::Input,
    video: Option<ParallelVideoConfig>,
    audio: Option<StreamConfig>,
    minimum_time: MediaTime,
    maximum_time: Option<MediaTime>,
    cancelled: &(dyn Fn() -> bool + Sync),
    emit: &mut impl FnMut(ParallelDecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    let origin = input_origin(input);
    let terminal_preview = maximum_time == Some(minimum_time) && audio.is_none();
    let video_stream_index = video.as_ref().map(|video| match video {
        ParallelVideoConfig::Software(config) => config.index,
        ParallelVideoConfig::Hardware(config, _) => config.index,
    });
    let audio_stream_index = audio.as_ref().map(|config| config.index);
    let mut video_finished = video.is_none();
    let mut audio_finished = audio.is_none();
    let (video_packet_tx, video_packet_rx) = mpsc::sync_channel(PACKET_QUEUE_CAPACITY);
    let (audio_packet_tx, audio_packet_rx) = mpsc::sync_channel(PACKET_QUEUE_CAPACITY);
    let (output_tx, output_rx) = mpsc::sync_channel(DECODED_QUEUE_CAPACITY);

    thread::scope(|scope| {
        let demux_handle = thread::Builder::new()
            .name("towavue-demux".to_owned())
            .spawn_scoped(scope, move || {
                let mut pending_video = VecDeque::new();
                let mut pending_audio = VecDeque::new();
                for (stream, mut packet) in input.packets() {
                    if cancelled() {
                        return;
                    }
                    normalize_packet_time(&mut packet, stream.time_base(), origin);
                    if !flush_available_packets(&video_packet_tx, &mut pending_video)
                        || !flush_available_packets(&audio_packet_tx, &mut pending_audio)
                    {
                        break;
                    }
                    let routed = if Some(stream.index()) == video_stream_index {
                        queue_demux_packet(&video_packet_tx, &mut pending_video, packet)
                    } else if Some(stream.index()) == audio_stream_index {
                        queue_demux_packet(&audio_packet_tx, &mut pending_audio, packet)
                    } else {
                        true
                    };
                    if !routed {
                        break;
                    }
                }
                while !pending_video.is_empty() || !pending_audio.is_empty() {
                    if cancelled() {
                        return;
                    }
                    if !send_one_pending_packet(&video_packet_tx, &mut pending_video)
                        || !send_one_pending_packet(&audio_packet_tx, &mut pending_audio)
                    {
                        break;
                    }
                }
            })?;

        let video_handle = video.map(|video| {
            let video_output_tx = output_tx.clone();
            thread::Builder::new()
                .name("towavue-video-decode".to_owned())
                .spawn_scoped(scope, move || {
                    if let Err(error) =
                        run_parallel_video_worker(video, video_packet_rx, &video_output_tx)
                    {
                        let _ = video_output_tx.send(ParallelDecodeOutput::Failed(error));
                    } else {
                        let _ = video_output_tx.send(ParallelDecodeOutput::VideoFinished);
                    }
                })
        });

        let audio_handle = audio.map(|audio| {
            let audio_output_tx = output_tx.clone();
            thread::Builder::new()
                .name("towavue-audio-decode".to_owned())
                .spawn_scoped(scope, move || {
                    if let Err(error) =
                        run_parallel_audio_worker(audio, audio_packet_rx, &audio_output_tx)
                    {
                        let _ = audio_output_tx.send(ParallelDecodeOutput::Failed(error));
                    }
                })
        });
        drop(output_tx);

        let mut summary = DecodeSummary::default();
        let mut result = Ok(());
        let mut last_preroll_video = None;
        let mut emitted_video = false;
        for mut output in output_rx {
            if cancelled() {
                result = Err(DecodeError::ConsumerClosed);
                break;
            }
            if let ParallelDecodeOutput::Failed(error) = output {
                result = Err(error);
                break;
            }
            let is_video = matches!(
                output,
                ParallelDecodeOutput::SoftwareVideo(_)
                    | ParallelDecodeOutput::HardwareVideo(_)
                    | ParallelDecodeOutput::VideoFinished
            );
            if (is_video && video_finished) || (!is_video && audio_finished) {
                continue;
            }
            match &output {
                ParallelDecodeOutput::SoftwareVideo(_) | ParallelDecodeOutput::HardwareVideo(_) => {
                    summary.video_frames += 1
                }
                ParallelDecodeOutput::Audio(chunk) => summary.audio_frames += chunk.frames as u64,
                _ => {}
            }
            let time = match &output {
                ParallelDecodeOutput::SoftwareVideo(frame) => Some(frame.presentation_time),
                ParallelDecodeOutput::HardwareVideo(frame) => Some(frame.presentation_time),
                ParallelDecodeOutput::Audio(chunk) => Some(chunk.presentation_time),
                _ => None,
            };
            let at_end = time.is_some_and(|time| maximum_time.is_some_and(|end| time >= end));
            let mut finished = time.is_none() || at_end;
            let mut accepted = true;
            if let ParallelDecodeOutput::Audio(chunk) = &mut output {
                finished |= maximum_time.is_some_and(|end| {
                    chunk
                        .presentation_time
                        .saturating_add(Duration::from_secs_f64(
                            chunk.frames as f64 / chunk.format.sample_rate as f64,
                        ))
                        >= end
                });
                clip_audio_chunk(chunk, minimum_time, maximum_time);
                if chunk.frames > 0 {
                    accepted = emit(output);
                }
            } else if !finished && time.is_some_and(|time| time >= minimum_time) {
                emitted_video = true;
                last_preroll_video = None;
                accepted = emit(output);
            } else if is_video
                && !finished
                && (maximum_time.is_none() || terminal_preview)
                && !emitted_video
            {
                // Keep one owned software/native frame until a usable frame or EOF.
                // A degenerate video interval previews its last frame before the end.
                // Nonempty bounded trims still exclude all preroll.
                last_preroll_video = Some(output);
            }
            if finished && accepted {
                if is_video && let Some(frame) = last_preroll_video.take() {
                    accepted = emit(frame);
                }
                if is_video {
                    video_finished = true;
                } else {
                    audio_finished = true;
                }
                accepted = accepted
                    && emit(if is_video {
                        ParallelDecodeOutput::VideoFinished
                    } else {
                        ParallelDecodeOutput::AudioFinished
                    });
            }
            if !accepted {
                result = Err(DecodeError::ConsumerClosed);
                break;
            }
            if video_finished && audio_finished {
                break;
            }
        }

        let demux_joined = demux_handle.join().is_ok();
        let video_joined = match video_handle {
            Some(Ok(handle)) => handle.join().is_ok(),
            Some(Err(error)) => return Err(error.into()),
            None => true,
        };
        let audio_joined = match audio_handle {
            Some(Ok(handle)) => handle.join().is_ok(),
            Some(Err(error)) => return Err(error.into()),
            None => true,
        };
        if !demux_joined || !video_joined || !audio_joined {
            return Err(DecodeError::WorkerPanicked);
        }
        check_cancelled(cancelled)?;
        result.map(|()| summary)
    })
}

pub(crate) fn clip_audio_chunk(chunk: &mut AudioChunk, start: MediaTime, end: Option<MediaTime>) {
    let rate = i128::from(chunk.format.sample_rate);
    // Recover the sample index before ceil-ing boundaries: FFmpeg PTS-to-nanosecond
    // truncation must not add an extra sample at an exactly aligned trim end.
    let chunk_sample = (i128::from(chunk.presentation_time.as_nanoseconds()) * rate + 500_000_000)
        .div_euclid(1_000_000_000);
    let sample_offset = |time: MediaTime| {
        let sample =
            (i128::from(time.as_nanoseconds()) * rate + 999_999_999).div_euclid(1_000_000_000);
        (sample - chunk_sample).clamp(0, chunk.frames as i128) as usize
    };
    let first = sample_offset(start);
    let last = end.map_or(chunk.frames, sample_offset).max(first);
    let stride = usize::from(chunk.format.channels) * size_of::<f32>();
    chunk.bytes.truncate(last * stride);
    chunk.bytes.drain(..first * stride);
    chunk.frames = last - first;
    if first > 0 {
        let nanos = ((chunk_sample + first as i128) * 1_000_000_000 + rate - 1).div_euclid(rate);
        chunk.presentation_time =
            MediaTime::from_nanoseconds(nanos.clamp(i64::MIN as i128, i64::MAX as i128) as i64);
    }
}

fn queue_demux_packet(
    sender: &SyncSender<ffmpeg::Packet>,
    pending: &mut VecDeque<ffmpeg::Packet>,
    packet: ffmpeg::Packet,
) -> bool {
    if pending.is_empty() {
        match sender.try_send(packet) {
            Ok(()) => return true,
            Err(TrySendError::Full(packet)) => pending.push_back(packet),
            Err(TrySendError::Disconnected(_)) => return false,
        }
    } else {
        pending.push_back(packet);
    }
    if pending.len() >= PACKET_QUEUE_CAPACITY {
        send_one_pending_packet(sender, pending)
    } else {
        true
    }
}

fn flush_available_packets(
    sender: &SyncSender<ffmpeg::Packet>,
    pending: &mut VecDeque<ffmpeg::Packet>,
) -> bool {
    while let Some(packet) = pending.pop_front() {
        match sender.try_send(packet) {
            Ok(()) => {}
            Err(TrySendError::Full(packet)) => {
                pending.push_front(packet);
                return true;
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
    true
}

fn send_one_pending_packet(
    sender: &SyncSender<ffmpeg::Packet>,
    pending: &mut VecDeque<ffmpeg::Packet>,
) -> bool {
    pending
        .pop_front()
        .is_none_or(|packet| sender.send(packet).is_ok())
}

fn run_parallel_video_worker(
    config: ParallelVideoConfig,
    packets: mpsc::Receiver<ffmpeg::Packet>,
    output: &SyncSender<ParallelDecodeOutput>,
) -> Result<(), DecodeError> {
    let mut pipeline = match config {
        ParallelVideoConfig::Software(config) => {
            ParallelVideoPipeline::Software(create_video_pipeline_from(config)?)
        }
        ParallelVideoConfig::Hardware(config, device) => {
            ParallelVideoPipeline::Hardware(create_hardware_video_pipeline_from(config, &device)?)
        }
    };
    let mut summary = DecodeSummary::default();
    for packet in packets {
        match &mut pipeline {
            ParallelVideoPipeline::Software(pipeline) => {
                pipeline.decoder.send_packet(&packet)?;
                pipeline.receive(
                    &mut |output_frame| match output_frame {
                        DecodeOutput::Video(frame) => output
                            .send(ParallelDecodeOutput::SoftwareVideo(frame))
                            .is_ok(),
                        DecodeOutput::Audio(_) => unreachable!("video worker emitted audio"),
                    },
                    &mut summary,
                )?;
            }
            ParallelVideoPipeline::Hardware(pipeline) => {
                if let Err(error) = pipeline.decoder.send_packet(&packet) {
                    if summary.video_frames == 0 {
                        return Err(DecodeError::HardwareUnavailable(error.to_string()));
                    }
                    return Err(error.into());
                }
                pipeline.receive(
                    &mut |output_frame| match output_frame {
                        RuntimeDecodeOutput::Video(frame) => output
                            .send(ParallelDecodeOutput::HardwareVideo(frame))
                            .is_ok(),
                    },
                    &mut summary,
                )?;
            }
        }
    }
    match &mut pipeline {
        ParallelVideoPipeline::Software(pipeline) => {
            pipeline.decoder.send_eof()?;
            pipeline.receive(
                &mut |output_frame| match output_frame {
                    DecodeOutput::Video(frame) => output
                        .send(ParallelDecodeOutput::SoftwareVideo(frame))
                        .is_ok(),
                    DecodeOutput::Audio(_) => unreachable!("video worker emitted audio"),
                },
                &mut summary,
            )
        }
        ParallelVideoPipeline::Hardware(pipeline) => {
            if let Err(error) = pipeline.decoder.send_eof() {
                if summary.video_frames == 0 {
                    return Err(DecodeError::HardwareUnavailable(error.to_string()));
                }
                return Err(error.into());
            }
            pipeline.receive(
                &mut |output_frame| match output_frame {
                    RuntimeDecodeOutput::Video(frame) => output
                        .send(ParallelDecodeOutput::HardwareVideo(frame))
                        .is_ok(),
                },
                &mut summary,
            )
        }
    }
}

fn run_parallel_audio_worker(
    config: StreamConfig,
    packets: mpsc::Receiver<ffmpeg::Packet>,
    output: &SyncSender<ParallelDecodeOutput>,
) -> Result<(), DecodeError> {
    let mut pipeline = create_audio_pipeline_from(config)?;
    let mut summary = DecodeSummary::default();
    let mut emit_audio = |decoded| match decoded {
        DecodeOutput::Audio(chunk) => output.send(ParallelDecodeOutput::Audio(chunk)).is_ok(),
        DecodeOutput::Video(_) => unreachable!("audio worker emitted video"),
    };
    for packet in packets {
        pipeline.decoder.send_packet(&packet)?;
        pipeline.receive(&mut emit_audio, &mut summary)?;
    }
    pipeline.decoder.send_eof()?;
    pipeline.receive(&mut emit_audio, &mut summary)?;
    pipeline.flush_resampler(&mut emit_audio, &mut summary)?;
    output
        .send(ParallelDecodeOutput::AudioFinished)
        .map_err(|_| DecodeError::ConsumerClosed)
}

fn seek_input(
    input: &mut format::context::Input,
    target: MediaTime,
    video: bool,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(), DecodeError> {
    check_cancelled(cancelled)?;
    if target <= MediaTime::ZERO {
        return Ok(());
    }
    if video && let Some(point) = transport_seek_point(input, target, cancelled)? {
        return seek_byte_position(input, point.position);
    }
    if video
        && input.format().name() != "mpegts"
        && let Some(stream) = input.streams().best(Type::Video)
    {
        let (index, time_base) = (stream.index(), stream.time_base());
        return seek_video_stream(
            input,
            index,
            time_base,
            input_origin(input),
            target,
            cancelled,
        );
    }
    let timestamp_microseconds =
        (target.as_nanoseconds() / 1_000).saturating_add(input_origin(input));
    input.seek(timestamp_microseconds, ..timestamp_microseconds)?;
    Ok(())
}

fn seek_video_stream(
    input: &mut format::context::Input,
    index: usize,
    time_base: Rational,
    origin: i64,
    target: MediaTime,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(), DecodeError> {
    check_cancelled(cancelled)?;
    if target <= MediaTime::ZERO {
        return Ok(());
    }
    if input.format().name() == "mpegts" {
        return seek_input(input, target, true, cancelled);
    }
    let timestamp = target
        .as_nanoseconds()
        .rescale_with(Rational(1, 1_000_000_000), time_base, Rounding::Down)
        .saturating_add(origin.rescale(ffmpeg::rescale::TIME_BASE, time_base));
    let origin_tick = origin.rescale(ffmpeg::rescale::TIME_BASE, time_base);
    let mut seek = timestamp;
    loop {
        check_cancelled(cancelled)?;
        seek_video_packet(input, index, seek)?;
        let mut earlier = None;
        for (stream, packet) in input.packets() {
            check_cancelled(cancelled)?;
            if stream.index() != index || !packet.is_key() {
                continue;
            }
            // Container indexes can use DTS. A future I picture's DTS may be
            // before the requested B picture, which still needs the prior GOP.
            if packet.pts().is_some_and(|pts| pts > timestamp) && seek > origin_tick {
                earlier = Some(
                    packet
                        .dts()
                        .unwrap_or(seek)
                        .min(seek)
                        .saturating_sub(1)
                        .max(origin_tick),
                );
            }
            break;
        }
        if let Some(earlier) = earlier {
            seek = earlier;
        } else {
            check_cancelled(cancelled)?;
            // Packet inspection consumes input; restore the validated start.
            return seek_video_packet(input, index, seek);
        }
    }
}

fn seek_video_packet(
    input: &mut format::context::Input,
    index: usize,
    timestamp: i64,
) -> Result<(), DecodeError> {
    // Exclusive input ownership on this thread, no borrowed packets. Decoder
    // workers start after Seek; query decoders have received no data. FFmpeg
    // flushes demux/parser state here. No native pointer escapes this call.
    let result = unsafe {
        ffmpeg::ffi::av_seek_frame(
            input.as_mut_ptr(),
            index as i32,
            timestamp,
            ffmpeg::ffi::AVSEEK_FLAG_BACKWARD,
        )
    };
    if result < 0 {
        return Err(ffmpeg::Error::from(result).into());
    }
    Ok(())
}

fn seek_byte_position(
    input: &mut format::context::Input,
    position: i64,
) -> Result<(), DecodeError> {
    // Exclusive input ownership on this thread; FFmpeg flushes parser/demux
    // state before repositioning. No packet/stream borrow or pointer escapes.
    let result = unsafe {
        ffmpeg::ffi::av_seek_frame(
            input.as_mut_ptr(),
            -1,
            position,
            ffmpeg::ffi::AVSEEK_FLAG_BYTE,
        )
    };
    if result < 0 {
        Err(ffmpeg::Error::from(result).into())
    } else {
        Ok(())
    }
}

struct TransportSeekPoint {
    position: i64,
    start_microseconds: i64,
}

fn transport_seek_point(
    input: &mut format::context::Input,
    target: MediaTime,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Option<TransportSeekPoint>, DecodeError> {
    if input.format().name() != "mpegts" || target <= MediaTime::ZERO {
        return Ok(None);
    }
    let Some(stream) = input.streams().best(Type::Video) else {
        return Ok(None);
    };
    let index = stream.index();
    let time_base = stream.time_base();
    let origin = input_origin(input);
    let target_us = target.as_nanoseconds() / 1_000;
    let target_tick = target_us.saturating_add(origin).rescale_with(
        ffmpeg::rescale::TIME_BASE,
        time_base,
        Rounding::Down,
    );
    let mut preroll = 0_i64;
    loop {
        check_cancelled(cancelled)?;
        let start = target_us.saturating_sub(preroll);
        if start == 0 {
            return Ok(Some(TransportSeekPoint {
                position: 0,
                start_microseconds: 0,
            }));
        }
        let absolute_start = start.saturating_add(origin);
        input.seek(absolute_start, ..absolute_start)?;
        check_cancelled(cancelled)?;
        let mut position = None;
        for (stream, packet) in input.packets() {
            check_cancelled(cancelled)?;
            if stream.index() != index {
                continue;
            }
            let Some(time) = packet.pts().or_else(|| packet.dts()) else {
                continue;
            };
            if time > target_tick {
                break;
            }
            if packet.is_key() && packet.position() >= 0 {
                position = Some(packet.position() as i64);
            }
        }
        if let Some(position) = position {
            return Ok(Some(TransportSeekPoint {
                position,
                start_microseconds: start,
            }));
        }
        preroll = if preroll == 0 {
            1_000_000
        } else {
            preroll.saturating_mul(2)
        }
        .min(target_us);
    }
}

pub(crate) fn preview_input(
    path: &Path,
    media_type: Type,
    target: Duration,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(Duration, usize), DecodeError> {
    check_cancelled(cancelled)?;
    ffmpeg::init()?;
    let mut input = format::input(path)?;
    check_cancelled(cancelled)?;
    let stream = input
        .streams()
        .best(media_type)
        .ok_or(DecodeError::NoMediaStream)?
        .index();
    if media_type != Type::Video || target.is_zero() {
        return Ok((target, stream));
    }
    let target_time = MediaTime::from_nanoseconds(target.as_nanos().min(i64::MAX as u128) as i64);
    let start = transport_seek_point(&mut input, target_time, cancelled)?
        .map(|point| Duration::from_micros(point.start_microseconds as u64))
        .unwrap_or(target);
    Ok((start, stream))
}

pub(crate) fn preview_last_video_time(
    path: &Path,
    target: Duration,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Option<Duration>, DecodeError> {
    check_cancelled(cancelled)?;
    ffmpeg::init()?;
    let mut input = format::input(path)?;
    let config = best_stream_config(&input, Type::Video).ok_or(DecodeError::NoMediaStream)?;
    let mut decoder = codec::context::Context::from_parameters(config.parameters)?
        .decoder()
        .video()?;
    let origin = input_origin(&input);
    let target = MediaTime::from_nanoseconds(target.as_nanos().min(i64::MAX as u128) as i64);
    seek_input(&mut input, target, true, cancelled)?;
    let mut last = None;
    let mut receive = |decoder: &mut codec::decoder::Video| -> Result<(), DecodeError> {
        loop {
            check_cancelled(cancelled)?;
            let mut decoded = frame::Video::empty();
            match decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    if let Some(timestamp) = decoded.timestamp() {
                        let time = timestamp_to_media_time(Some(timestamp), config.time_base);
                        last = Some(Duration::from_nanos(time.as_nanoseconds().max(0) as u64));
                    }
                }
                Err(error) if decoder_is_drained(error) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
    };
    for (stream, mut packet) in input.packets() {
        check_cancelled(cancelled)?;
        if stream.index() == config.index {
            normalize_packet_time(&mut packet, stream.time_base(), origin);
            decoder.send_packet(&packet)?;
            receive(&mut decoder)?;
        }
    }
    check_cancelled(cancelled)?;
    decoder.send_eof()?;
    receive(&mut decoder)?;
    Ok(last)
}

fn check_cancelled(cancelled: &(dyn Fn() -> bool + Sync)) -> Result<(), DecodeError> {
    if cancelled() {
        Err(DecodeError::ConsumerClosed)
    } else {
        Ok(())
    }
}

pub(crate) fn input_origin(input: &format::context::Input) -> i64 {
    // The owning input is immutably borrowed on its current thread. Read only the
    // initialized scalar; no pointer escapes or concurrent demux access occurs.
    let start = unsafe { (*input.as_ptr()).start_time };
    if start == ffmpeg::ffi::AV_NOPTS_VALUE {
        0
    } else {
        start
    }
}

fn normalize_packet_time(packet: &mut ffmpeg::Packet, time_base: Rational, origin: i64) {
    let offset = origin.rescale(ffmpeg::rescale::TIME_BASE, time_base);
    packet.set_pts(packet.pts().map(|time| time.saturating_sub(offset)));
    packet.set_dts(packet.dts().map(|time| time.saturating_sub(offset)));
}

pub(crate) fn probe_audio_format(path: &Path) -> Result<Option<AudioFormat>, DecodeError> {
    ffmpeg::init()?;
    let input = format::input(path)?;
    Ok(create_audio_pipeline(&input)?.map(|pipeline| pipeline.output_format))
}

fn create_video_pipeline(
    input: &format::context::Input,
) -> Result<Option<VideoPipeline>, DecodeError> {
    best_stream_config(input, Type::Video)
        .map(create_video_pipeline_from)
        .transpose()
}

fn create_video_pipeline_from(config: StreamConfig) -> Result<VideoPipeline, DecodeError> {
    let orientation = VideoOrientation::from_bytes(config.display_matrix.as_deref())?;
    let context = codec::context::Context::from_parameters(config.parameters)?;
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
    Ok(VideoPipeline {
        stream_index: config.index,
        time_base: config.time_base,
        decoder,
        scaler,
        orientation,
    })
}

fn create_hardware_video_pipeline_from(
    config: StreamConfig,
    device: &GraphicsDevice,
) -> Result<HardwareVideoPipeline, DecodeError> {
    let orientation = VideoOrientation::from_bytes(config.display_matrix.as_deref())?;
    let mut context = codec::context::Context::from_parameters(config.parameters)?;
    if !codec_supports_d3d11va(&context) {
        return Err(DecodeError::HardwareUnavailable(format!(
            "codec {:?} does not expose a D3D11VA configuration",
            context.id()
        )));
    }
    configure_d3d11va(&mut context, device)?;
    let decoder = context.decoder().video()?;
    let transfer =
        classify_transfer(decoder.color_transfer_characteristic()).unwrap_or(VideoTransfer::Sdr);
    Ok(HardwareVideoPipeline {
        time_base: config.time_base,
        decoder,
        transfer,
        orientation,
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
    best_stream_config(input, Type::Audio)
        .map(create_audio_pipeline_from)
        .transpose()
}

fn create_audio_pipeline_from(config: StreamConfig) -> Result<AudioPipeline, DecodeError> {
    let context = codec::context::Context::from_parameters(config.parameters)?;
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
    Ok(AudioPipeline {
        stream_index: config.index,
        time_base: config.time_base,
        decoder,
        resampler,
        output_format,
        next_presentation_time: MediaTime::ZERO,
        next_sample: ffmpeg::ffi::AV_NOPTS_VALUE,
    })
}

fn best_stream_config(input: &format::context::Input, media_type: Type) -> Option<StreamConfig> {
    let stream = input.streams().best(media_type)?;
    Some(StreamConfig {
        index: stream.index(),
        time_base: stream.time_base(),
        parameters: stream.parameters(),
        display_matrix: stream
            .side_data()
            .find(|data| data.kind() == ffmpeg::codec::packet::side_data::Type::DisplayMatrix)
            .map(|data| data.data().to_vec()),
    })
}

fn copy_video_frame(
    decoded: &frame::Video,
    rgba: &frame::Video,
    time_base: Rational,
    orientation: VideoOrientation,
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
        pixel_aspect: pixel_aspect(decoded.aspect_ratio()),
        orientation: frame_orientation(decoded, orientation)?,
        rgba: pixels,
    })
}

fn pixel_aspect(ratio: Rational) -> f32 {
    if ratio.numerator() > 0 && ratio.denominator() > 0 {
        ratio.numerator() as f32 / ratio.denominator() as f32
    } else {
        1.0
    }
}

fn emit_audio_frame(
    frame: &frame::Audio,
    presentation_time: MediaTime,
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
        presentation_time,
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
    use std::path::Path;

    use ffmpeg_next::ffi::AVPixelFormat;
    use ffmpeg_next::{Rational, color};
    use towavue_core::MediaTime;

    use super::{
        DecodeOutput, ParallelSoftwareDecodeOutput, VideoTransfer, classify_transfer,
        decode_file_from, select_d3d11_pixel_format, timestamp_to_media_time,
    };

    #[test]
    fn timestamp_conversion_uses_stream_time_base() {
        let time = timestamp_to_media_time(Some(90_000), Rational::new(1, 90_000));

        assert_eq!(time.as_nanoseconds(), 1_000_000_000);
    }

    #[test]
    fn packet_origin_keeps_stream_offsets_preroll_and_missing_timestamps() {
        use ffmpeg_next::{self as ffmpeg, Rescale};

        for origin in [0_i64, 11_378_667, -1_000_000] {
            for time_base in [Rational(1, 90_000), Rational(1, 48_000)] {
                let offset = origin.rescale(ffmpeg::rescale::TIME_BASE, time_base);
                let mut packet = ffmpeg::Packet::empty();
                packet.set_pts(Some(offset + 1_920));
                packet.set_dts(Some(offset - 960));
                packet.set_duration(1_024);
                super::normalize_packet_time(&mut packet, time_base, origin);
                assert_eq!(packet.pts(), Some(1_920));
                assert_eq!(packet.dts(), Some(-960));
                assert_eq!(packet.duration(), 1_024);
                packet.set_pts(None);
                packet.set_dts(None);
                super::normalize_packet_time(&mut packet, time_base, origin);
                assert_eq!((packet.pts(), packet.dts()), (None, None));
            }
        }
    }

    #[test]
    fn cancellation_interrupts_preroll_before_output_and_does_not_poison_the_next_decode() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        let mut outputs = 0;
        let missing = path.with_extension("missing");
        let result = super::decode_file_parallel_cancellable(
            &missing,
            MediaTime::ZERO,
            None,
            None,
            &|| true,
            |_| {
                outputs += 1;
                true
            },
        );
        assert!(matches!(result, Err(super::DecodeError::ConsumerClosed)));
        assert_eq!(outputs, 0);

        let checks = AtomicUsize::new(0);
        let start = MediaTime::from_nanoseconds(1_800_000_000);
        let result = super::decode_file_parallel_cancellable(
            &path,
            start,
            None,
            Some(super::DecodeStream::Video),
            &|| checks.fetch_add(1, Ordering::Relaxed) >= 20,
            |_| {
                outputs += 1;
                true
            },
        );
        assert!(matches!(result, Err(super::DecodeError::ConsumerClosed)));
        assert_eq!(
            outputs, 0,
            "cancellation must not wait for target or emit EOF"
        );

        super::decode_file_parallel_cancellable(
            &path,
            start,
            None,
            Some(super::DecodeStream::Video),
            &|| false,
            |_| {
                outputs += 1;
                true
            },
        )
        .expect("fresh pipeline remains usable");
        assert!(outputs > 1, "fresh decode emits frames and EOF");
    }

    #[test]
    fn transport_seek_matches_full_decode_inside_short_and_long_gops() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let executable =
            std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
                .join("bin/ffmpeg.exe");
        for (codec, gop, b_frames) in [
            ("libopenh264", 30, 0),
            ("libopenh264", 120, 0),
            ("mpeg2video", 30, 2),
        ] {
            let path =
                std::env::temp_dir().join(format!("towavue-ts-seek-{unique}-{codec}-{gop}.ts"));
            let generated = std::process::Command::new(&executable)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=160x96:rate=30:duration=6",
                    "-c:v",
                    codec,
                    "-g",
                    &gop.to_string(),
                    "-bf",
                    &b_frames.to_string(),
                    "-output_ts_offset",
                    "10",
                ])
                .arg(&path)
                .output()
                .expect("generate GOP fixture");
            assert!(
                generated.status.success(),
                "{}",
                String::from_utf8_lossy(&generated.stderr)
            );
            let mut reference = Vec::new();
            super::decode_file(&path, |output| {
                if let DecodeOutput::Video(frame) = output {
                    reference.push((frame.presentation_time, frame.rgba));
                }
                true
            })
            .expect("full source decode");
            assert_eq!(reference.len(), 180);
            let mut retained =
                super::ParallelInput::open(&path, &|| false).expect("retained input");
            for start_ns in [0, 5_500_000_000, 0, 500_000_000, 0] {
                let start = MediaTime::from_nanoseconds(start_ns);
                let mut actual = Vec::new();
                retained
                    .decode_software(
                        start,
                        None,
                        Some(super::DecodeStream::Video),
                        &|| false,
                        |output| {
                            if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) =
                                output
                            {
                                actual.push((frame.presentation_time, frame.rgba));
                            }
                            true
                        },
                    )
                    .expect("reuse TS input after EOF");
                let expected: Vec<_> = reference
                    .iter()
                    .filter(|(time, _)| *time >= start)
                    .cloned()
                    .collect();
                assert!(
                    actual == expected,
                    "retained GOP {gop}, seek {start_ns}: {} versus {} frames",
                    actual.len(),
                    expected.len()
                );
            }
            drop(retained);
            let cache_path = path.with_extension("cache");
            let cache = crate::PreviewCache::new(cache_path.clone()).expect("preview cache");
            for start_ns in [500_000_000, 3_500_000_000, 5_500_000_000] {
                let start = MediaTime::from_nanoseconds(start_ns);
                let end = MediaTime::from_nanoseconds(start_ns + 200_000_000);
                let expected: Vec<_> = reference
                    .iter()
                    .filter(|(time, _)| *time >= start && *time < end)
                    .cloned()
                    .collect();
                let mut actual = Vec::new();
                super::decode_file_parallel(
                    &path,
                    start,
                    Some(end),
                    Some(super::DecodeStream::Video),
                    |output| {
                        if let ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) =
                            output
                        {
                            actual.push((frame.presentation_time, frame.rgba));
                        }
                        true
                    },
                )
                .expect("seek decode");
                assert!(
                    actual == expected,
                    "GOP {gop}, seek {start_ns}: {} frames instead of {}",
                    actual.len(),
                    expected.len()
                );
                let mut serial = None;
                let result = super::decode_file_from(&path, start, |output| {
                    if let DecodeOutput::Video(frame) = output {
                        serial = Some((frame.presentation_time, frame.rgba));
                        return false;
                    }
                    true
                });
                assert!(matches!(result, Err(super::DecodeError::ConsumerClosed)));
                assert!(
                    serial.as_ref() == expected.first(),
                    "serial seek at {start_ns}"
                );
                let thumbnail = cache
                    .thumbnail(&path, std::time::Duration::from_nanos(start_ns as u64), 160)
                    .expect("GOP thumbnail");
                assert_eq!((thumbnail.width, thumbnail.height), (160, 96));
                assert!(
                    thumbnail
                        .rgba
                        .iter()
                        .zip(&expected[0].1)
                        .all(|(actual, expected)| actual.abs_diff(*expected) <= 3),
                    "thumbnail chose a different frame at GOP {gop}, {start_ns}"
                );
            }
            let card = cache
                .filmstrip(&path, towavue_core::MediaKind::Video)
                .expect("GOP filmstrip");
            assert_eq!((card.image.width, card.image.height), (240, 160));
            let last = reference.last().expect("final TS frame");
            let thumbnail = cache
                .thumbnail(&path, std::time::Duration::from_secs(6), 160)
                .expect("terminal TS thumbnail");
            assert!(
                thumbnail
                    .rgba
                    .iter()
                    .zip(&last.1)
                    .all(|(actual, expected)| actual.abs_diff(*expected) <= 3),
                "terminal thumbnail at GOP {gop}"
            );
            let mut input = ffmpeg_next::format::input(&path).expect("inspect seek point");
            let point = super::transport_seek_point(
                &mut input,
                MediaTime::from_nanoseconds(5_500_000_000),
                &|| false,
            )
            .expect("find preroll")
            .expect("TS seek point");
            assert!(
                point.position > 0 && point.start_microseconds > 0,
                "late seek must not always decode from the beginning"
            );
            let checks = std::sync::atomic::AtomicUsize::new(0);
            let cancelled = super::transport_seek_point(
                &mut input,
                MediaTime::from_nanoseconds(5_500_000_000),
                &|| checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 1,
            );
            assert!(
                matches!(cancelled, Err(super::DecodeError::ConsumerClosed)),
                "cancel during TS seek preparation"
            );
            drop(input);
            std::fs::remove_dir_all(cache_path).expect("remove owned preview cache");
            std::fs::remove_file(path).expect("remove owned GOP fixture");
        }
    }

    #[test]
    fn trim_audio_clips_partial_chunks_on_sample_boundaries_without_changing_samples() {
        let mut chunk = super::AudioChunk {
            presentation_time: MediaTime::from_nanoseconds(1_000_000_000),
            format: super::AudioFormat {
                sample_rate: 48_000,
                channels: 2,
            },
            frames: 16,
            bytes: (0..32)
                .flat_map(|value| (value as f32).to_le_bytes())
                .collect(),
        };
        let expected = chunk.bytes[16..48].to_vec();
        super::clip_audio_chunk(
            &mut chunk,
            MediaTime::from_nanoseconds(1_000_031_250),
            Some(MediaTime::from_nanoseconds(1_000_114_583)),
        );
        assert_eq!(chunk.frames, 4);
        assert_eq!(chunk.bytes, expected);
        assert_eq!(chunk.presentation_time.as_nanoseconds(), 1_000_041_667);
        super::clip_audio_chunk(&mut chunk, MediaTime::from_nanoseconds(2_000_000_000), None);
        assert_eq!(chunk.frames, 0);
        assert!(chunk.bytes.is_empty());
    }

    #[test]
    fn coarse_audio_timestamps_follow_samples_but_preserve_gaps() {
        ffmpeg_next::init().expect("initialize FFmpeg");
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        let input = ffmpeg_next::format::input(&path).expect("generated fixture");
        let mut pipeline = super::create_audio_pipeline(&input)
            .expect("audio pipeline")
            .expect("audio stream");
        pipeline.time_base = Rational(1, 1000);
        for rate in [44_100, 48_000] {
            pipeline.output_format.sample_rate = rate;
            pipeline.next_sample = ffmpeg_next::ffi::AV_NOPTS_VALUE;
            for index in 0..3000_i64 {
                let sample = index * 1024;
                let timestamp = (sample * 1000 + i64::from(rate) / 2) / i64::from(rate);
                let actual = pipeline.sample_presentation_time(Some(timestamp), 1024);
                assert_eq!(
                    actual.as_nanoseconds(),
                    sample * 1_000_000_000 / i64::from(rate)
                );
            }
            let predicted = pipeline.next_sample;
            assert_eq!(
                pipeline
                    .sample_presentation_time(None, 1024)
                    .as_nanoseconds(),
                predicted * 1_000_000_000 / i64::from(rate)
            );
            assert_eq!(
                pipeline
                    .sample_presentation_time(Some(100_000), 1024)
                    .as_nanoseconds(),
                100_000_000_000
            );
            assert_eq!(
                pipeline.sample_presentation_time(Some(0), 1024),
                MediaTime::ZERO
            );
        }
    }

    #[test]
    fn parallel_trim_stops_at_both_stream_boundaries_and_handles_unequal_lengths() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let executable = std::env::var_os("FFMPEG_DIR")
            .map(std::path::PathBuf::from)
            .map(|path| path.join("bin/ffmpeg.exe"))
            .unwrap_or_else(|| "ffmpeg.exe".into());
        for (video_seconds, audio_seconds) in [(4, 4), (1, 4), (4, 1)] {
            let path = std::env::temp_dir().join(format!(
                "towavue-trim-{unique}-{video_seconds}-{audio_seconds}.nut"
            ));
            let generated = std::process::Command::new(&executable)
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("color=size=160x96:rate=10:duration={video_seconds}"),
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("sine=sample_rate=48000:duration={audio_seconds}"),
                    "-c:v",
                    "ffv1",
                    "-c:a",
                    "pcm_f32le",
                ])
                .arg(&path)
                .output()
                .expect("generate trim fixture");
            assert!(
                generated.status.success(),
                "{}",
                String::from_utf8_lossy(&generated.stderr)
            );
            for (start_ns, end_ns, expected_video) in [
                (205_000_000, 605_000_000, 4),
                (
                    2_000_000_000,
                    2_500_000_000,
                    if video_seconds == 1 { 0 } else { 5 },
                ),
            ] {
                let start = MediaTime::from_nanoseconds(start_ns);
                let end = MediaTime::from_nanoseconds(end_ns);
                for selected in [
                    None,
                    Some(super::DecodeStream::Video),
                    Some(super::DecodeStream::Audio),
                ] {
                    let (mut videos, mut samples, mut video_ends, mut audio_ends) = (0, 0, 0, 0);
                    super::decode_file_parallel(&path, start, Some(end), selected, |output| {
                        match output {
                            ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) => {
                                assert!(
                                    frame.presentation_time >= start
                                        && frame.presentation_time < end
                                );
                                assert_eq!(video_ends, 0);
                                videos += 1;
                            }
                            ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) => {
                                assert!(
                                    chunk.presentation_time >= start
                                        && chunk.presentation_time < end
                                );
                                assert_eq!(audio_ends, 0);
                                samples += chunk.frames;
                            }
                            ParallelSoftwareDecodeOutput::VideoFinished => video_ends += 1,
                            ParallelSoftwareDecodeOutput::AudioFinished => audio_ends += 1,
                        }
                        true
                    })
                    .expect("bounded trim decode");
                    let has_video = selected != Some(super::DecodeStream::Audio);
                    let has_audio = selected != Some(super::DecodeStream::Video);
                    assert_eq!(videos, if has_video { expected_video } else { 0 });
                    let expected_samples = if audio_seconds == 1 && start_ns == 2_000_000_000 {
                        0
                    } else {
                        ((end_ns - start_ns) * 48_000 / 1_000_000_000) as usize
                    };
                    assert_eq!(samples, if has_audio { expected_samples } else { 0 });
                    assert_eq!(
                        (video_ends, audio_ends),
                        (usize::from(has_video), usize::from(has_audio))
                    );
                }
            }
            assert!(
                matches!(
                    super::decode_file_parallel(
                        &path,
                        MediaTime::ZERO,
                        Some(MediaTime::from_nanoseconds(500_000_000)),
                        None,
                        |_| false
                    ),
                    Err(super::DecodeError::ConsumerClosed)
                ),
                "caller cancellation must not become successful range EOF"
            );
            std::fs::remove_file(path).expect("remove owned trim fixture");
        }
    }

    #[test]
    fn selected_stream_does_not_turn_subtitle_only_input_into_successful_media() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("towavue-subtitle-only-{unique}.srt"));
        std::fs::write(
            &path,
            "1\n00:00:00,000 --> 00:00:01,000\nFixture subtitle\n",
        )
        .expect("subtitle fixture");
        for stream in [super::DecodeStream::Video, super::DecodeStream::Audio] {
            assert!(matches!(
                super::decode_file_parallel(
                    &path,
                    MediaTime::ZERO,
                    None,
                    Some(stream),
                    |_| panic!("no media output")
                ),
                Err(super::DecodeError::NoMediaStream)
            ));
        }
        std::fs::remove_file(path).expect("remove owned subtitle fixture");
    }

    #[test]
    fn selected_video_remains_bounded_and_completes_while_audio_consumer_is_blocked() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, mpsc};
        use std::time::Duration;
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        let (audio_ready_tx, audio_ready_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (video_tx, video_rx) = mpsc::sync_channel(2);
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let produced = Arc::new(AtomicUsize::new(0));
        let video_produced = Arc::clone(&produced);
        std::thread::scope(|scope| {
            let path = &path;
            let audio = scope.spawn(move || {
                super::decode_file_parallel(
                    path,
                    MediaTime::ZERO,
                    None,
                    Some(super::DecodeStream::Audio),
                    |output| {
                        assert!(matches!(
                            output,
                            ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(_))
                        ));
                        audio_ready_tx
                            .send(())
                            .expect("audio reached blocked output");
                        let _ = release_rx.recv();
                        false
                    },
                )
            });
            let audio_ready = audio_ready_rx.recv_timeout(Duration::from_secs(5));
            if audio_ready.is_err() {
                let _ = release_tx.send(());
            }
            assert!(audio_ready.is_ok());
            scope.spawn(move || {
                let result = super::decode_file_parallel(
                    path,
                    MediaTime::ZERO,
                    None,
                    Some(super::DecodeStream::Video),
                    |output| match output {
                        ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) => {
                            video_produced.fetch_add(1, Ordering::Relaxed);
                            video_tx.send(frame.presentation_time).is_ok()
                        }
                        ParallelSoftwareDecodeOutput::VideoFinished => true,
                        _ => panic!("selected video emitted audio"),
                    },
                );
                let _ = done_tx.send(result);
            });
            let first = video_rx.recv_timeout(Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(100));
            let bounded = produced.load(Ordering::Relaxed) <= 4;
            let mut frames = usize::from(first.is_ok());
            let mut previous = first.ok();
            let mut ordered = true;
            while let Ok(time) = video_rx.recv_timeout(Duration::from_secs(5)) {
                ordered &= previous.is_none_or(|last| last <= time);
                previous = Some(time);
                frames += 1;
            }
            drop(video_rx);
            let _ = release_tx.send(());
            let result = done_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("video completion")
                .expect("video decode");
            assert!(matches!(
                audio.join().expect("audio worker"),
                Err(super::DecodeError::ConsumerClosed)
            ));
            assert!(
                bounded,
                "only two queued frames and one producer-held frame after first receive"
            );
            assert!(ordered);
            assert_eq!(frames, 60);
            assert_eq!(result.video_frames, 60);
            assert_eq!(result.audio_frames, 0);
        });
    }

    #[test]
    fn pixel_aspect_preserves_valid_ratios_and_defaults_unspecified_values() {
        assert_eq!(super::pixel_aspect(Rational::new(2, 1)), 2.0);
        assert_eq!(super::pixel_aspect(Rational::new(0, 1)), 1.0);
        assert_eq!(super::pixel_aspect(Rational::new(1, 0)), 1.0);
        assert_eq!(super::pixel_aspect(Rational::new(-1, 2)), 1.0);
    }

    #[test]
    fn pcm_wav_without_channel_mask_decodes_mono_and_stereo() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        for channels in [1_u16, 2] {
            let data_len = 480 * u32::from(channels) * 2;
            let mut bytes = b"RIFF".to_vec();
            bytes.extend((36 + data_len).to_le_bytes());
            bytes.extend(b"WAVEfmt ");
            bytes.extend(16_u32.to_le_bytes());
            bytes.extend(1_u16.to_le_bytes());
            bytes.extend(channels.to_le_bytes());
            bytes.extend(48_000_u32.to_le_bytes());
            bytes.extend((48_000 * u32::from(channels) * 2).to_le_bytes());
            bytes.extend((channels * 2).to_le_bytes());
            bytes.extend(16_u16.to_le_bytes());
            bytes.extend(b"data");
            bytes.extend(data_len.to_le_bytes());
            for _ in 0..480 {
                bytes.extend(8192_i16.to_le_bytes());
                if channels == 2 {
                    bytes.extend((-4096_i16).to_le_bytes());
                }
            }
            let path = std::env::temp_dir().join(format!("towavue-pcm-{unique}-{channels}.wav"));
            std::fs::write(&path, bytes).expect("write PCM fixture");
            let verify = |output: DecodeOutput| {
                if let DecodeOutput::Audio(chunk) = output {
                    assert_eq!(chunk.format.sample_rate, 48_000);
                    assert_eq!(chunk.format.channels, 2);
                    for frame in chunk.bytes.as_chunks::<8>().0 {
                        let left = f32::from_le_bytes(frame[..4].try_into().expect("left sample"));
                        let right =
                            f32::from_le_bytes(frame[4..].try_into().expect("right sample"));
                        if channels == 1 {
                            assert!(left > 0.0);
                            assert_eq!(left, right);
                        } else {
                            assert_eq!(left, 0.25);
                            assert_eq!(right, -0.125);
                        }
                    }
                }
                true
            };
            let serial = super::decode_file(&path, verify);
            let parallel =
                super::decode_file_parallel(&path, MediaTime::ZERO, None, None, |output| {
                    if let ParallelSoftwareDecodeOutput::Item(item) = output {
                        verify(item);
                    }
                    true
                });
            let mut trimmed_samples = 0;
            let mut trim_ends = 0;
            super::decode_file_parallel(
                &path,
                MediaTime::from_nanoseconds(3_000_000),
                Some(MediaTime::from_nanoseconds(7_000_000)),
                None,
                |output| {
                    match output {
                        ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) => {
                            trimmed_samples += chunk.frames
                        }
                        ParallelSoftwareDecodeOutput::AudioFinished => trim_ends += 1,
                        _ => panic!("audio-only trim emitted video"),
                    }
                    true
                },
            )
            .expect("audio-only trim decode");
            assert_eq!(trimmed_samples, 192);
            assert_eq!(trim_ends, 1);
            std::fs::remove_file(path).expect("remove PCM fixture");
            assert_eq!(serial.expect("serial PCM decode").audio_frames, 480);
            assert_eq!(parallel.expect("parallel PCM decode").audio_frames, 480);
        }
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

    #[test]
    fn classifies_hdr_transfer_metadata() {
        assert_eq!(
            classify_transfer(color::TransferCharacteristic::SMPTE2084),
            Some(VideoTransfer::Pq)
        );
        assert_eq!(
            classify_transfer(color::TransferCharacteristic::ARIB_STD_B67),
            Some(VideoTransfer::Hlg)
        );
        assert_eq!(
            classify_transfer(color::TransferCharacteristic::Unspecified),
            None
        );
    }

    #[test]
    fn terminal_seek_retains_the_last_frame_only_without_a_bounded_range() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
        let mut reference = None;
        decode_file_from(&path, MediaTime::ZERO, |output| {
            if let DecodeOutput::Video(frame) = output {
                reference = Some(frame);
            }
            true
        })
        .expect("reference decode");
        let reference = reference.expect("last source frame");
        for target in [1_999_000_000, 2_000_000_000] {
            for end in [None, Some(MediaTime::from_nanoseconds(3_000_000_000))] {
                let mut frames = Vec::new();
                let mut finished = false;
                super::decode_file_parallel(
                    &path,
                    MediaTime::from_nanoseconds(target),
                    end,
                    Some(super::DecodeStream::Video),
                    |output| {
                        match output {
                            super::ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(
                                frame,
                            )) => {
                                assert!(!finished);
                                frames.push(frame);
                            }
                            super::ParallelSoftwareDecodeOutput::VideoFinished => finished = true,
                            _ => {}
                        }
                        true
                    },
                )
                .expect("terminal seek");
                assert!(finished);
                assert_eq!(frames.len(), usize::from(end.is_none()));
                if let Some(frame) = frames.first() {
                    assert_eq!(frame.presentation_time, reference.presentation_time);
                    assert_eq!(frame.rgba, reference.rgba);
                }
            }
        }
        let mut calls = 0;
        let result = super::decode_file_parallel(
            &path,
            MediaTime::from_nanoseconds(2_000_000_000),
            None,
            Some(super::DecodeStream::Video),
            |output| {
                assert!(matches!(
                    output,
                    super::ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(_))
                ));
                calls += 1;
                false
            },
        );
        assert!(matches!(result, Err(super::DecodeError::ConsumerClosed)));
        assert_eq!(calls, 1, "closed consumer must not receive a completion");
    }

    #[test]
    fn terminal_seek_matches_full_decode_with_unequal_streams_and_vfr() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let ffmpeg =
            std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
                .join("bin/ffmpeg.exe");
        for (extension, codec) in [("mkv", "ffv1"), ("mp4", "libopenh264")] {
            for (video_seconds, audio_seconds) in [(1, 4), (4, 1)] {
                let path = std::env::temp_dir().join(format!(
                    "towavue-terminal-{unique}-{video_seconds}-{audio_seconds}.{extension}"
                ));
                let generated = std::process::Command::new(&ffmpeg)
                    .args(["-v", "error", "-f", "lavfi", "-i"])
                    .arg(format!(
                        "testsrc2=size=160x96:rate=10:duration={video_seconds}"
                    ))
                    .args(["-f", "lavfi", "-i"])
                    .arg(format!("sine=sample_rate=48000:duration={audio_seconds}"))
                    .args([
                        "-vf",
                        "select='not(eq(mod(n,3),1))'",
                        "-fps_mode",
                        "vfr",
                        "-c:v",
                        codec,
                        "-c:a",
                        "aac",
                    ])
                    .arg(&path)
                    .output()
                    .expect("generate unequal VFR fixture");
                assert!(
                    generated.status.success(),
                    "{}",
                    String::from_utf8_lossy(&generated.stderr)
                );
                let mut reference = None;
                let mut times = Vec::new();
                decode_file_from(&path, MediaTime::ZERO, |output| {
                    if let DecodeOutput::Video(frame) = output {
                        times.push(frame.presentation_time.as_nanoseconds());
                        reference = Some(frame);
                    }
                    true
                })
                .expect("reference decode");
                assert!(
                    times.windows(3).any(|t| t[1] - t[0] != t[2] - t[1]),
                    "fixture must be VFR"
                );
                let reference = reference.expect("final source frame");
                for selected in [None, Some(super::DecodeStream::Video)] {
                    let mut frames = Vec::new();
                    super::decode_file_parallel(
                        &path,
                        MediaTime::from_nanoseconds(4_100_000_000),
                        None,
                        selected,
                        |output| {
                            if let super::ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(
                                frame,
                            )) = output
                            {
                                frames.push(frame);
                            }
                            true
                        },
                    )
                    .expect("terminal VFR seek");
                    assert_eq!(
                        frames.len(),
                        1,
                        "{path:?}, video-only: {}",
                        selected.is_some()
                    );
                    assert_eq!(frames[0].presentation_time, reference.presentation_time);
                    assert_eq!(frames[0].rgba, reference.rgba);
                }
                std::fs::remove_file(path).expect("remove generated terminal fixture");
            }
        }
    }

    #[test]
    fn seek_discards_output_before_target() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("generated")
            .join("m1")
            .join("h264-aac.mp4");
        assert!(path.is_file(), "run scripts/generate-m1-fixtures.ps1");
        let target = MediaTime::from_nanoseconds(1_000_000_000);
        let mut output_count = 0;

        decode_file_from(&path, target, |output| {
            let presentation_time = match output {
                DecodeOutput::Video(frame) => frame.presentation_time,
                DecodeOutput::Audio(chunk) => chunk.presentation_time,
            };
            assert!(presentation_time >= target);
            output_count += 1;
            true
        })
        .expect("seek decode must succeed");

        assert!(output_count > 0);
    }

    #[test]
    fn parallel_workers_decode_both_streams() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("generated")
            .join("m1")
            .join("h264-aac.mp4");
        assert!(path.is_file(), "run scripts/generate-m1-fixtures.ps1");
        let mut video_frames = 0;
        let mut audio_frames = 0;
        let mut audio_finished = 0;

        let summary = super::decode_file_parallel(&path, MediaTime::ZERO, None, None, |output| {
            match output {
                ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(_)) => video_frames += 1,
                ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) => {
                    audio_frames += chunk.frames as u64;
                }
                ParallelSoftwareDecodeOutput::AudioFinished => audio_finished += 1,
                ParallelSoftwareDecodeOutput::VideoFinished => {}
            }
            true
        })
        .expect("parallel decode must succeed");

        assert_eq!(summary.video_frames, video_frames);
        assert_eq!(summary.audio_frames, audio_frames);
        assert_eq!(video_frames, 60);
        assert_eq!(audio_frames, 96_000);
        assert_eq!(audio_finished, 1);
    }
}
