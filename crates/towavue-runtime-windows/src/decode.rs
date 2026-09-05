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
use ffmpeg::{ChannelLayout, Rational};
use ffmpeg_next as ffmpeg;
use thiserror::Error;
use towavue_core::MediaTime;

use crate::GraphicsDevice;

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
    pub rgba: Vec<u8>,
}

/// Runtime-only owner of one FFmpeg-referenced D3D11 decode surface.
pub(crate) struct HardwareVideoFrame {
    pub(crate) presentation_time: MediaTime,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixel_aspect: f32,
    pub(crate) transfer: VideoTransfer,
    frame: frame::Video,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VideoTransfer {
    Sdr,
    Pq,
    Hlg,
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

pub(crate) enum ParallelRuntimeDecodeOutput {
    Item(RuntimeDecodeOutput),
    VideoFinished,
    AudioFinished,
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
}

struct HardwareVideoPipeline {
    time_base: Rational,
    decoder: codec::decoder::Video,
    transfer: VideoTransfer,
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
                        transfer: video_transfer(&decoded).unwrap_or(self.transfer),
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

fn video_transfer(frame: &frame::Video) -> Option<VideoTransfer> {
    classify_transfer(frame.color_transfer_characteristic())
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
    time_base: Rational,
    decoder: codec::decoder::Audio,
    resampler: resampling::Context,
    output_format: AudioFormat,
    next_presentation_time: MediaTime,
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
                    let presentation_time = decoded
                        .timestamp()
                        .map_or(self.next_presentation_time, |timestamp| {
                            timestamp_to_media_time(Some(timestamp), self.time_base)
                        });
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
    let mut video = create_video_pipeline(&input)?;
    let mut audio = create_audio_pipeline(&input)?;
    if video.is_none() && audio.is_none() {
        return Err(DecodeError::NoMediaStream);
    }
    seek_input(&mut input, minimum_time)?;
    let mut filtered_emit = |output| {
        let presentation_time = match &output {
            DecodeOutput::Video(frame) => frame.presentation_time,
            DecodeOutput::Audio(chunk) => chunk.presentation_time,
        };
        minimum_time > MediaTime::ZERO && presentation_time < minimum_time || emit(output)
    };

    let mut summary = DecodeSummary::default();
    for (stream, packet) in input.packets() {
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

pub(crate) fn decode_file_parallel(
    path: &Path,
    minimum_time: MediaTime,
    maximum_time: Option<MediaTime>,
    mut emit: impl FnMut(ParallelSoftwareDecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    decode_file_parallel_inner(
        path,
        None,
        minimum_time,
        maximum_time,
        |output| match output {
            ParallelDecodeOutput::SoftwareVideo(frame) => emit(ParallelSoftwareDecodeOutput::Item(
                DecodeOutput::Video(frame),
            )),
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

pub(crate) fn decode_file_hardware_parallel(
    path: &Path,
    device: &GraphicsDevice,
    minimum_time: MediaTime,
    maximum_time: Option<MediaTime>,
    mut emit: impl FnMut(ParallelRuntimeDecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    decode_file_parallel_inner(
        path,
        Some(device),
        minimum_time,
        maximum_time,
        |output| match output {
            ParallelDecodeOutput::HardwareVideo(frame) => emit(ParallelRuntimeDecodeOutput::Item(
                RuntimeDecodeOutput::Video(frame),
            )),
            ParallelDecodeOutput::Audio(chunk) => emit(ParallelRuntimeDecodeOutput::Item(
                RuntimeDecodeOutput::Audio(chunk),
            )),
            ParallelDecodeOutput::AudioFinished => emit(ParallelRuntimeDecodeOutput::AudioFinished),
            ParallelDecodeOutput::VideoFinished => emit(ParallelRuntimeDecodeOutput::VideoFinished),
            ParallelDecodeOutput::SoftwareVideo(_) | ParallelDecodeOutput::Failed(_) => {
                unreachable!("parallel decode output is handled internally")
            }
        },
    )
}

fn decode_file_parallel_inner(
    path: &Path,
    hardware_device: Option<&GraphicsDevice>,
    minimum_time: MediaTime,
    maximum_time: Option<MediaTime>,
    mut emit: impl FnMut(ParallelDecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
    ffmpeg::init()?;
    let mut input = format::input(path)?;
    let video_config = best_stream_config(&input, Type::Video);
    let video = match (hardware_device, video_config) {
        (Some(device), Some(config)) => Some(ParallelVideoConfig::Hardware(config, device.clone())),
        (Some(_), None) => {
            return Err(DecodeError::HardwareUnavailable(
                "input has no video stream".to_owned(),
            ));
        }
        (None, config) => config.map(ParallelVideoConfig::Software),
    };
    let audio = best_stream_config(&input, Type::Audio);
    if video.is_none() && audio.is_none() {
        return Err(DecodeError::NoMediaStream);
    }
    seek_input(&mut input, minimum_time)?;
    run_parallel_workers(input, video, audio, minimum_time, maximum_time, &mut emit)
}

fn run_parallel_workers(
    mut input: format::context::Input,
    video: Option<ParallelVideoConfig>,
    audio: Option<StreamConfig>,
    minimum_time: MediaTime,
    maximum_time: Option<MediaTime>,
    emit: &mut impl FnMut(ParallelDecodeOutput) -> bool,
) -> Result<DecodeSummary, DecodeError> {
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
                for (stream, packet) in input.packets() {
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
        for mut output in output_rx {
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
                accepted = emit(output);
            }
            if finished && accepted {
                if is_video {
                    video_finished = true;
                } else {
                    audio_finished = true;
                }
                accepted = emit(if is_video {
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
        result.map(|()| summary)
    })
}

fn clip_audio_chunk(chunk: &mut AudioChunk, start: MediaTime, end: Option<MediaTime>) {
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
                        RuntimeDecodeOutput::Audio(_) => unreachable!("video worker emitted audio"),
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
                    RuntimeDecodeOutput::Audio(_) => unreachable!("video worker emitted audio"),
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

fn seek_input(input: &mut format::context::Input, target: MediaTime) -> Result<(), DecodeError> {
    if target <= MediaTime::ZERO {
        return Ok(());
    }
    let timestamp_microseconds = target.as_nanoseconds() / 1_000;
    input.seek(timestamp_microseconds, ..timestamp_microseconds)?;
    Ok(())
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
    })
}

fn create_hardware_video_pipeline_from(
    config: StreamConfig,
    device: &GraphicsDevice,
) -> Result<HardwareVideoPipeline, DecodeError> {
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
    })
}

fn best_stream_config(input: &format::context::Input, media_type: Type) -> Option<StreamConfig> {
    let stream = input.streams().best(media_type)?;
    Some(StreamConfig {
        index: stream.index(),
        time_base: stream.time_base(),
        parameters: stream.parameters(),
    })
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
        pixel_aspect: pixel_aspect(decoded.aspect_ratio()),
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
                let (mut videos, mut samples, mut video_ends, mut audio_ends) = (0, 0, 0, 0);
                super::decode_file_parallel(&path, start, Some(end), |output| {
                    match output {
                        ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) => {
                            assert!(
                                frame.presentation_time >= start && frame.presentation_time < end
                            );
                            assert_eq!(video_ends, 0);
                            videos += 1;
                        }
                        ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) => {
                            assert!(
                                chunk.presentation_time >= start && chunk.presentation_time < end
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
                assert_eq!(videos, expected_video);
                let expected_samples = if audio_seconds == 1 && start_ns == 2_000_000_000 {
                    0
                } else {
                    ((end_ns - start_ns) * 48_000 / 1_000_000_000) as usize
                };
                assert_eq!(samples, expected_samples);
                assert_eq!((video_ends, audio_ends), (1, 1));
            }
            assert!(
                matches!(
                    super::decode_file_parallel(
                        &path,
                        MediaTime::ZERO,
                        Some(MediaTime::from_nanoseconds(500_000_000)),
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
            let parallel = super::decode_file_parallel(&path, MediaTime::ZERO, None, |output| {
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

        let summary = super::decode_file_parallel(&path, MediaTime::ZERO, None, |output| {
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
