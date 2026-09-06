use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};

use thiserror::Error;
use towavue_core::{MediaTime, PlaybackGeneration, PlaybackRange};

use crate::audio::{AudioOutput, AudioOutputError, AudioOutputEvent, AudioOutputSender};
use crate::decode::{
    self, AudioFormat, DecodeOutput, DecodeStream, HardwareVideoFrame, ParallelRuntimeDecodeOutput,
    ParallelSoftwareDecodeOutput, RuntimeDecodeOutput, VideoFrame,
};
use crate::{AdapterLuid, FrameRenderer, GraphicsDevice, RenderError};

const VIDEO_QUEUE_CAPACITY: usize = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackEvent {
    VideoReady(PlaybackGeneration),
    AudioReady(PlaybackGeneration),
    DecodePathSelected(PlaybackGeneration, DecodePath),
    DecodeFinished(PlaybackGeneration),
    DeviceRemoved(PlaybackGeneration, String),
    Failed(PlaybackGeneration, String),
}

impl PlaybackEvent {
    pub fn generation(&self) -> PlaybackGeneration {
        match self {
            Self::VideoReady(generation)
            | Self::AudioReady(generation)
            | Self::DecodePathSelected(generation, _)
            | Self::DecodeFinished(generation)
            | Self::DeviceRemoved(generation, _)
            | Self::Failed(generation, _) => *generation,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodePath {
    D3d11va,
    Software,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackMetrics {
    pub adapter_luid: AdapterLuid,
    pub hardware_frame_count: u64,
    pub cpu_transfer_count: u64,
    pub presented_frame_count: u64,
    pub dropped_frame_count: u64,
}

struct SharedMetrics {
    hardware_frame_count: AtomicU64,
    cpu_transfer_count: AtomicU64,
    presented_frame_count: AtomicU64,
    dropped_frame_count: AtomicU64,
}

impl SharedMetrics {
    fn reset(&self) {
        self.hardware_frame_count.store(0, Ordering::Relaxed);
        self.cpu_transfer_count.store(0, Ordering::Relaxed);
        self.presented_frame_count.store(0, Ordering::Relaxed);
        self.dropped_frame_count.store(0, Ordering::Relaxed);
    }
}

enum PresentationFrame {
    Software(VideoFrame),
    Hardware(HardwareVideoFrame),
}

impl PresentationFrame {
    fn presentation_time(&self) -> MediaTime {
        match self {
            Self::Software(frame) => frame.presentation_time,
            Self::Hardware(frame) => frame.presentation_time,
        }
    }
}

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("media probing failed: {0}")]
    Probe(#[from] decode::DecodeError),
    #[error("audio output failed: {0}")]
    Audio(#[from] AudioOutputError),
    #[error("the decode thread could not start: {0}")]
    Thread(#[from] std::io::Error),
}

pub struct PlaybackSession {
    path: PathBuf,
    graphics_device: GraphicsDevice,
    notify: Arc<dyn Fn(PlaybackEvent) + Send + Sync>,
    audio_format: Option<AudioFormat>,
    video_rx: Option<Receiver<PresentationFrame>>,
    pending_video: Option<PresentationFrame>,
    current_video: Option<PresentationFrame>,
    current_video_unpresented: bool,
    audio: Option<AudioOutput>,
    decode_thread: Option<JoinHandle<()>>,
    decode_cancel: Arc<AtomicBool>,
    adapter_luid: AdapterLuid,
    metrics: Arc<SharedMetrics>,
    generation: PlaybackGeneration,
    target: MediaTime,
    paused: bool,
    volume: f32,
    rate: f32,
    range: PlaybackRange,
}

impl PlaybackSession {
    pub fn open(
        path: &Path,
        graphics_device: GraphicsDevice,
        volume: f32,
        rate: f32,
        range: PlaybackRange,
        notify: impl Fn(PlaybackEvent) + Send + Sync + 'static,
    ) -> Result<Self, PlaybackError> {
        let audio_format = decode::probe_audio_format(path)?;
        let adapter_luid = graphics_device.adapter_luid();
        let metrics = Arc::new(SharedMetrics {
            hardware_frame_count: AtomicU64::new(0),
            cpu_transfer_count: AtomicU64::new(0),
            presented_frame_count: AtomicU64::new(0),
            dropped_frame_count: AtomicU64::new(0),
        });
        let mut session = Self {
            path: path.to_owned(),
            graphics_device,
            notify: Arc::new(notify),
            audio_format,
            video_rx: None,
            pending_video: None,
            current_video: None,
            current_video_unpresented: false,
            audio: None,
            decode_thread: None,
            decode_cancel: Arc::new(AtomicBool::new(false)),
            adapter_luid,
            metrics,
            generation: PlaybackGeneration::INITIAL,
            target: range.start,
            paused: false,
            volume,
            rate: rate.clamp(0.25, 4.0).max(0.25),
            range,
        };
        session.start_pipeline()?;
        Ok(session)
    }

    pub fn generation(&self) -> PlaybackGeneration {
        self.generation
    }

    pub fn seek(&mut self, target: MediaTime) -> Result<PlaybackGeneration, PlaybackError> {
        self.stop_pipeline();
        self.generation = self.generation.next();
        self.target = target.max(MediaTime::ZERO);
        self.metrics.reset();
        self.start_pipeline()?;
        Ok(self.generation)
    }

    pub fn rate(&self) -> f32 {
        self.rate
    }

    pub fn range(&self) -> PlaybackRange {
        self.range
    }

    pub fn target(&self) -> MediaTime {
        self.target
    }

    pub fn range_end(&self) -> Option<MediaTime> {
        self.range
            .contains(self.target)
            .then_some(self.range.end)
            .flatten()
    }

    pub fn seek_with_edits(
        &mut self,
        target: MediaTime,
        rate: f32,
        range: PlaybackRange,
        pause: bool,
    ) -> Result<PlaybackGeneration, PlaybackError> {
        self.rate = rate.clamp(0.25, 4.0).max(0.25);
        self.range = range;
        // Seek may require pausing, but resuming remains an explicit control.
        self.paused |= pause;
        self.seek(target)
    }

    pub fn replace_graphics_device(
        &mut self,
        graphics_device: GraphicsDevice,
        target: MediaTime,
    ) -> Result<PlaybackGeneration, PlaybackError> {
        self.stop_pipeline();
        self.adapter_luid = graphics_device.adapter_luid();
        self.graphics_device = graphics_device;
        self.generation = self.generation.next();
        self.target = target.max(MediaTime::ZERO);
        self.metrics.reset();
        self.start_pipeline()?;
        Ok(self.generation)
    }

    /// Quiesce the old device before its window surface is released.
    pub fn suspend_for_graphics_recovery(&mut self) {
        self.stop_pipeline();
    }

    fn start_pipeline(&mut self) -> Result<(), PlaybackError> {
        let audio = self
            .audio_format
            .map(|format| {
                let notify = Arc::clone(&self.notify);
                let generation = self.generation;
                AudioOutput::start_with_settings(
                    format,
                    self.target,
                    self.volume,
                    self.rate,
                    move || {
                        notify(PlaybackEvent::AudioReady(generation));
                    },
                )
            })
            .transpose()?;
        if self.paused
            && let Some(audio) = &audio
        {
            audio.set_paused(true)?;
        }
        let audio_sender = audio.as_ref().map(AudioOutput::sender);
        let (video_tx, video_rx) = mpsc::sync_channel(VIDEO_QUEUE_CAPACITY);
        let path = self.path.clone();
        let graphics_device = self.graphics_device.clone();
        let notify = Arc::clone(&self.notify);
        let metrics = Arc::clone(&self.metrics);
        let generation = self.generation;
        let target = self.target;
        let end = self.range_end();
        self.decode_cancel = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&self.decode_cancel);
        let decode_thread = thread::Builder::new()
            .name("towavue-decode".to_owned())
            .spawn(move || {
                run_decode_thread(
                    &path,
                    &graphics_device,
                    &video_tx,
                    audio_sender.as_ref(),
                    &metrics,
                    generation,
                    target,
                    end,
                    &cancelled,
                    notify.as_ref(),
                );
            })?;
        self.video_rx = Some(video_rx);
        self.audio = audio;
        self.decode_thread = Some(decode_thread);
        Ok(())
    }

    fn stop_pipeline(&mut self) {
        self.decode_cancel.store(true, Ordering::Relaxed);
        self.pending_video.take();
        self.current_video.take();
        self.video_rx.take();
        self.audio.take();
        if let Some(thread) = self.decode_thread.take() {
            let _ = thread.join();
        }
    }

    pub fn pending_video_time(&mut self) -> Option<MediaTime> {
        if self.pending_video.is_none() {
            self.pending_video = self.video_rx.as_ref()?.try_recv().ok();
        }
        self.pending_video
            .as_ref()
            .map(PresentationFrame::presentation_time)
    }

    pub fn advance_pending(&mut self) -> bool {
        let Some(frame) = self.pending_video.take() else {
            return false;
        };
        self.current_video = Some(frame);
        self.current_video_unpresented = true;
        true
    }

    pub fn video_geometry(&self) -> Option<(u32, u32, f32)> {
        Some(match self.current_video.as_ref()? {
            PresentationFrame::Software(frame) => (frame.width, frame.height, frame.pixel_aspect),
            PresentationFrame::Hardware(frame) => (frame.width, frame.height, frame.pixel_aspect),
        })
    }

    pub fn video_orientation(&self) -> Option<crate::VideoOrientation> {
        Some(match self.current_video.as_ref()? {
            PresentationFrame::Software(frame) => frame.orientation,
            PresentationFrame::Hardware(frame) => frame.orientation,
        })
    }

    pub fn draw_current(
        &mut self,
        renderer: &mut FrameRenderer,
        destination: egui::Rect,
        uv: [towavue_core::UnitPoint; 4],
    ) -> Result<bool, RenderError> {
        let Some(frame) = self.current_video.as_ref() else {
            return Ok(false);
        };
        let result = match frame {
            PresentationFrame::Software(frame) => renderer.draw_software(frame, destination, uv),
            PresentationFrame::Hardware(frame) => renderer.draw_hardware(frame, destination, uv),
        };
        if let Err(error) = result {
            return match renderer.device_removed_reason() {
                Some(reason) => Err(RenderError::DeviceRemoved(reason)),
                None => Err(error),
            };
        }
        if self.current_video_unpresented {
            self.metrics
                .presented_frame_count
                .fetch_add(1, Ordering::Relaxed);
            self.current_video_unpresented = false;
        }
        Ok(true)
    }

    pub fn drop_video_before(&mut self, cutoff: MediaTime) -> u64 {
        let mut dropped = 0;
        while let Some(time) = self.pending_video_time() {
            if time >= cutoff {
                break;
            }
            self.pending_video.take();
            dropped += 1;
        }
        self.metrics
            .dropped_frame_count
            .fetch_add(dropped, Ordering::Relaxed);
        dropped
    }

    pub fn try_audio_event(&self) -> Option<AudioOutputEvent> {
        self.audio.as_ref()?.try_event()
    }

    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    pub fn audio_position(&self) -> Option<MediaTime> {
        self.audio.as_ref().map(AudioOutput::position)
    }

    pub fn set_paused(&mut self, paused: bool) -> Result<(), AudioOutputError> {
        if let Some(audio) = &self.audio {
            audio.set_paused(paused)?;
        }
        self.paused = paused;
        Ok(())
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        if let Some(audio) = &self.audio {
            audio.set_volume(volume);
        }
    }

    pub fn metrics(&self) -> PlaybackMetrics {
        PlaybackMetrics {
            adapter_luid: self.adapter_luid,
            hardware_frame_count: self.metrics.hardware_frame_count.load(Ordering::Relaxed),
            cpu_transfer_count: self.metrics.cpu_transfer_count.load(Ordering::Relaxed),
            presented_frame_count: self.metrics.presented_frame_count.load(Ordering::Relaxed),
            dropped_frame_count: self.metrics.dropped_frame_count.load(Ordering::Relaxed),
        }
    }
}

impl Drop for PlaybackSession {
    fn drop(&mut self) {
        self.stop_pipeline();
    }
}

#[allow(clippy::too_many_arguments)]
fn run_decode_thread(
    path: &Path,
    graphics_device: &GraphicsDevice,
    video_tx: &SyncSender<PresentationFrame>,
    audio: Option<&AudioOutputSender>,
    metrics: &SharedMetrics,
    generation: PlaybackGeneration,
    target: MediaTime,
    end: Option<MediaTime>,
    cancelled: &AtomicBool,
    notify: &(impl Fn(PlaybackEvent) + Sync + ?Sized),
) {
    thread::scope(|scope| {
        let (start_tx, start_rx) = mpsc::sync_channel(1);
        let audio_thread = audio
            .map(|audio| {
                thread::Builder::new()
                    .name("towavue-audio-feed".into())
                    .spawn_scoped(scope, move || {
                        if start_rx.recv().is_err() {
                            return Err(decode::DecodeError::ConsumerClosed);
                        }
                        let result = decode::decode_file_parallel_cancellable(
                            path,
                            target,
                            end,
                            Some(DecodeStream::Audio),
                            &|| cancelled.load(Ordering::Relaxed),
                            |output| match output {
                                ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) => {
                                    audio.push(chunk).is_ok()
                                }
                                ParallelSoftwareDecodeOutput::AudioFinished => {
                                    audio.finish().is_ok()
                                }
                                _ => unreachable!("audio-only decoder emitted video"),
                            },
                        );
                        if let Err(error) = &result
                            && !matches!(error, decode::DecodeError::ConsumerClosed)
                            && !cancelled.load(Ordering::Relaxed)
                        {
                            notify(PlaybackEvent::Failed(generation, error.to_string()));
                        }
                        result
                    })
            })
            .transpose();
        let audio_thread = match audio_thread {
            Ok(handle) => handle,
            Err(error) => {
                notify(PlaybackEvent::Failed(generation, error.to_string()));
                return;
            }
        };
        let mut start_audio = Some(start_tx);
        let mut ready = || {
            if let Some(sender) = start_audio.take() {
                let _ = sender.send(());
            }
        };
        let mut hardware_output_seen = false;
        let hardware_result = decode::decode_file_hardware_parallel(
            path,
            graphics_device,
            target,
            end,
            &|| cancelled.load(Ordering::Relaxed),
            |output| {
                if !hardware_output_seen {
                    hardware_output_seen = true;
                    notify(PlaybackEvent::DecodePathSelected(
                        generation,
                        DecodePath::D3d11va,
                    ));
                    ready();
                }
                match output {
                    ParallelRuntimeDecodeOutput::VideoFinished => true,
                    ParallelRuntimeDecodeOutput::Item(RuntimeDecodeOutput::Video(frame)) => {
                        metrics.hardware_frame_count.fetch_add(1, Ordering::Relaxed);
                        send_video_frame(
                            PresentationFrame::Hardware(frame),
                            video_tx,
                            generation,
                            notify,
                        )
                    }
                }
            },
        );

        let result = match hardware_result {
            _ if cancelled.load(Ordering::Relaxed) => Err(decode::DecodeError::ConsumerClosed),
            Err(decode::DecodeError::HardwareUnavailable(_)) if !hardware_output_seen => {
                notify(PlaybackEvent::DecodePathSelected(
                    generation,
                    DecodePath::Software,
                ));
                decode::decode_file_parallel_cancellable(
                    path,
                    target,
                    end,
                    Some(DecodeStream::Video),
                    &|| cancelled.load(Ordering::Relaxed),
                    |output| match output {
                        ParallelSoftwareDecodeOutput::VideoFinished => {
                            ready();
                            true
                        }
                        ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) => {
                            ready();
                            metrics.cpu_transfer_count.fetch_add(1, Ordering::Relaxed);
                            send_video_frame(
                                PresentationFrame::Software(frame),
                                video_tx,
                                generation,
                                notify,
                            )
                        }
                        _ => unreachable!("video-only decoder emitted audio"),
                    },
                )
            }
            other => other,
        };

        drop(start_audio);
        match &result {
            _ if cancelled.load(Ordering::Relaxed) => {}
            Ok(_) => {}
            Err(decode::DecodeError::ConsumerClosed) => {}
            Err(error) => match graphics_device.device_removed_reason() {
                Some(reason) => notify(PlaybackEvent::DeviceRemoved(generation, reason)),
                None => notify(PlaybackEvent::Failed(generation, error.to_string())),
            },
        }
        let audio_ok = match audio_thread {
            Some(handle) => match handle.join() {
                Ok(result) => result.is_ok(),
                Err(_) => {
                    notify(PlaybackEvent::Failed(
                        generation,
                        decode::DecodeError::WorkerPanicked.to_string(),
                    ));
                    false
                }
            },
            None => true,
        };
        if result.is_ok() && audio_ok && !cancelled.load(Ordering::Relaxed) {
            notify(PlaybackEvent::DecodeFinished(generation));
        }
    });
}

fn send_video_frame(
    frame: PresentationFrame,
    video_tx: &SyncSender<PresentationFrame>,
    generation: PlaybackGeneration,
    notify: &(impl Fn(PlaybackEvent) + ?Sized),
) -> bool {
    if video_tx.send(frame).is_err() {
        return false;
    }
    notify(PlaybackEvent::VideoReady(generation));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_events_retain_their_playback_generation() {
        let generation = PlaybackGeneration::INITIAL.next();

        assert_eq!(
            PlaybackEvent::AudioReady(generation).generation(),
            generation
        );
        assert_eq!(
            PlaybackEvent::DeviceRemoved(generation, "removed".to_owned()).generation(),
            generation
        );
        assert_eq!(
            PlaybackEvent::Failed(generation, "failed".to_owned()).generation(),
            generation
        );
    }
}
