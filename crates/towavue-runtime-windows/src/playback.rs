use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};

use thiserror::Error;
use towavue_core::MediaTime;

use crate::audio::{AudioOutput, AudioOutputError, AudioOutputEvent, AudioOutputSender};
use crate::decode::{self, DecodeOutput, HardwareVideoFrame, RuntimeDecodeOutput, VideoFrame};
use crate::{AdapterLuid, FrameRenderer, GraphicsDevice, RenderError};

const VIDEO_QUEUE_CAPACITY: usize = 2;

/// Runtime events containing no native Windows or FFmpeg handles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackEvent {
    VideoReady,
    DecodePathSelected(DecodePath),
    DecodeFinished,
    Failed(String),
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
}

struct SharedMetrics {
    hardware_frame_count: AtomicU64,
    cpu_transfer_count: AtomicU64,
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

/// A failure while opening the M1 playback pipeline.
#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("media probing failed: {0}")]
    Probe(#[from] decode::DecodeError),
    #[error("audio output failed: {0}")]
    Audio(#[from] AudioOutputError),
    #[error("the decode thread could not start: {0}")]
    Thread(#[from] std::io::Error),
}

/// Owns decoder and audio threads plus the bounded presentation queue.
pub struct PlaybackSession {
    video_rx: Option<Receiver<PresentationFrame>>,
    pending_video: Option<PresentationFrame>,
    audio: Option<AudioOutput>,
    decode_thread: Option<JoinHandle<()>>,
    adapter_luid: AdapterLuid,
    metrics: Arc<SharedMetrics>,
}

impl PlaybackSession {
    pub fn open(
        path: &Path,
        graphics_device: GraphicsDevice,
        notify: impl Fn(PlaybackEvent) + Send + 'static,
    ) -> Result<Self, PlaybackError> {
        let audio = decode::probe_audio_format(path)?
            .map(AudioOutput::start)
            .transpose()?;
        let audio_sender = audio.as_ref().map(AudioOutput::sender);
        let (video_tx, video_rx) = mpsc::sync_channel(VIDEO_QUEUE_CAPACITY);
        let path = path.to_owned();
        let adapter_luid = graphics_device.adapter_luid();
        let metrics = Arc::new(SharedMetrics {
            hardware_frame_count: AtomicU64::new(0),
            cpu_transfer_count: AtomicU64::new(0),
        });
        let thread_metrics = Arc::clone(&metrics);
        let decode_thread = thread::Builder::new()
            .name("towavue-decode".to_owned())
            .spawn(move || {
                run_decode_thread(
                    &path,
                    &graphics_device,
                    &video_tx,
                    audio_sender.as_ref(),
                    &thread_metrics,
                    &notify,
                );
            })?;

        Ok(Self {
            video_rx: Some(video_rx),
            pending_video: None,
            audio,
            decode_thread: Some(decode_thread),
            adapter_luid,
            metrics,
        })
    }

    pub fn pending_video_time(&mut self) -> Option<MediaTime> {
        if self.pending_video.is_none() {
            self.pending_video = self.video_rx.as_ref()?.try_recv().ok();
        }
        self.pending_video
            .as_ref()
            .map(PresentationFrame::presentation_time)
    }

    pub fn present_pending(&mut self, renderer: &mut FrameRenderer) -> Result<bool, RenderError> {
        let Some(frame) = self.pending_video.take() else {
            return Ok(false);
        };
        match frame {
            PresentationFrame::Software(frame) => renderer.present(&frame)?,
            PresentationFrame::Hardware(frame) => renderer.present_hardware(&frame)?,
        }
        Ok(true)
    }

    pub fn try_audio_event(&self) -> Option<AudioOutputEvent> {
        self.audio.as_ref()?.try_event()
    }

    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), AudioOutputError> {
        if let Some(audio) = &self.audio {
            audio.set_paused(paused)?;
        }
        Ok(())
    }

    pub fn metrics(&self) -> PlaybackMetrics {
        PlaybackMetrics {
            adapter_luid: self.adapter_luid,
            hardware_frame_count: self.metrics.hardware_frame_count.load(Ordering::Relaxed),
            cpu_transfer_count: self.metrics.cpu_transfer_count.load(Ordering::Relaxed),
        }
    }
}

impl Drop for PlaybackSession {
    fn drop(&mut self) {
        self.pending_video.take();
        self.video_rx.take();
        self.audio.take();
        if let Some(thread) = self.decode_thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_decode_thread(
    path: &Path,
    graphics_device: &GraphicsDevice,
    video_tx: &SyncSender<PresentationFrame>,
    audio: Option<&AudioOutputSender>,
    metrics: &SharedMetrics,
    notify: &impl Fn(PlaybackEvent),
) {
    let mut hardware_output_seen = false;
    let mut pending_audio = Vec::new();
    let hardware_result =
        decode::decode_file_hardware(path, graphics_device, |output| match output {
            RuntimeDecodeOutput::Video(frame) => {
                if !hardware_output_seen {
                    if let Some(audio) = audio {
                        for chunk in pending_audio.drain(..) {
                            if audio.push(chunk).is_err() {
                                return false;
                            }
                        }
                    }
                    hardware_output_seen = true;
                    notify(PlaybackEvent::DecodePathSelected(DecodePath::D3d11va));
                }
                metrics.hardware_frame_count.fetch_add(1, Ordering::Relaxed);
                send_video_frame(PresentationFrame::Hardware(frame), video_tx, notify)
            }
            RuntimeDecodeOutput::Audio(chunk) => {
                if hardware_output_seen {
                    audio.is_some_and(|sender| sender.push(chunk).is_ok())
                } else {
                    pending_audio.push(chunk);
                    true
                }
            }
        });

    let result = match hardware_result {
        Err(decode::DecodeError::HardwareUnavailable(_)) if !hardware_output_seen => {
            pending_audio.clear();
            notify(PlaybackEvent::DecodePathSelected(DecodePath::Software));
            decode::decode_file(path, |output| match output {
                DecodeOutput::Video(frame) => {
                    metrics.cpu_transfer_count.fetch_add(1, Ordering::Relaxed);
                    send_video_frame(PresentationFrame::Software(frame), video_tx, notify)
                }
                DecodeOutput::Audio(chunk) => {
                    audio.is_some_and(|sender| sender.push(chunk).is_ok())
                }
            })
        }
        other => other,
    };

    if let Some(audio) = audio
        && audio.finish().is_err()
    {
        return;
    }

    match result {
        Ok(_) => notify(PlaybackEvent::DecodeFinished),
        Err(decode::DecodeError::ConsumerClosed) => {}
        Err(error) => notify(PlaybackEvent::Failed(error.to_string())),
    }
}

fn send_video_frame(
    frame: PresentationFrame,
    video_tx: &SyncSender<PresentationFrame>,
    notify: &impl Fn(PlaybackEvent),
) -> bool {
    if video_tx.send(frame).is_err() {
        return false;
    }
    notify(PlaybackEvent::VideoReady);
    true
}
