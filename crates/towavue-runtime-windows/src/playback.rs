use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
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

#[derive(Default)]
struct DecodeCompletion(AtomicU8);

impl DecodeCompletion {
    const VIDEO: u8 = 1;
    const AUDIO: u8 = 2;

    fn finish(&self, stream: u8) -> bool {
        let previous = self.0.fetch_or(stream, Ordering::AcqRel);
        previous != 3 && previous | stream == 3
    }

    fn restart_video(&self) {
        self.0.fetch_and(!Self::VIDEO, Ordering::AcqRel);
    }

    fn finished(&self) -> bool {
        self.0.load(Ordering::Acquire) == 3
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackEvent {
    VideoReady(PlaybackGeneration),
    AudioReady(PlaybackGeneration),
    DecodePathSelected(PlaybackGeneration, DecodePath),
    DecodeFinished(PlaybackGeneration),
    DeviceRemoved(PlaybackGeneration, String),
    VideoFailed(PlaybackGeneration, String),
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
            | Self::VideoFailed(generation, _)
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
    video_refresh_pending: bool,
    retained_video_position: Option<MediaTime>,
    audio: Option<AudioOutput>,
    video_thread: Option<JoinHandle<Option<decode::ParallelInput>>>,
    video_input: Option<decode::ParallelInput>,
    audio_thread: Option<JoinHandle<()>>,
    decode_cancel: Arc<AtomicBool>,
    video_cancel: Arc<AtomicBool>,
    completion: Arc<DecodeCompletion>,
    video_visible: bool,
    video_target: MediaTime,
    adapter_luid: AdapterLuid,
    metrics: Arc<SharedMetrics>,
    generation: PlaybackGeneration,
    video_generation: PlaybackGeneration,
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
            video_refresh_pending: true,
            retained_video_position: None,
            audio: None,
            video_thread: None,
            video_input: None,
            audio_thread: None,
            decode_cancel: Arc::new(AtomicBool::new(false)),
            video_cancel: Arc::new(AtomicBool::new(false)),
            completion: Arc::new(DecodeCompletion::default()),
            video_visible: true,
            video_target: range.start,
            adapter_luid,
            metrics,
            generation: PlaybackGeneration::INITIAL,
            video_generation: PlaybackGeneration::INITIAL,
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

    /// Check this session's stream generation after the caller checks session identity.
    pub fn accepts_event(&self, event: &PlaybackEvent) -> bool {
        let generation = match event {
            PlaybackEvent::VideoReady(_)
            | PlaybackEvent::DecodePathSelected(..)
            | PlaybackEvent::DeviceRemoved(..)
            | PlaybackEvent::VideoFailed(..) => self.video_generation,
            _ => self.generation,
        };
        event.generation() == generation
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
        self.audio = audio;
        self.decode_cancel = Arc::new(AtomicBool::new(false));
        self.completion = Arc::new(DecodeCompletion::default());
        if self.audio.is_none() {
            self.completion.finish(DecodeCompletion::AUDIO);
        }
        if self.video_visible
            && let Err(error) = self.start_video(self.target)
        {
            self.stop_pipeline();
            return Err(error);
        }
        if let Some(audio) = self.audio.as_ref().map(AudioOutput::sender) {
            let path = self.path.clone();
            let target = self.target;
            let end = self.range_end();
            let cancelled = Arc::clone(&self.decode_cancel);
            let completion = Arc::clone(&self.completion);
            let notify = Arc::clone(&self.notify);
            let generation = self.generation;
            match thread::Builder::new()
                .name("towavue-audio-feed".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_audio_decode(&path, &audio, target, end, &cancelled)
                    }))
                    .unwrap_or(Err(decode::DecodeError::WorkerPanicked));
                    if cancelled.load(Ordering::Relaxed) {
                        return;
                    }
                    match result {
                        Ok(()) if completion.finish(DecodeCompletion::AUDIO) => {
                            notify(PlaybackEvent::DecodeFinished(generation));
                        }
                        Ok(()) | Err(decode::DecodeError::ConsumerClosed) => {}
                        Err(error) => notify(PlaybackEvent::Failed(generation, error.to_string())),
                    }
                }) {
                Ok(thread) => self.audio_thread = Some(thread),
                Err(error) => {
                    self.stop_pipeline();
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    fn start_video(&mut self, target: MediaTime) -> Result<(), PlaybackError> {
        self.video_target = target;
        self.completion.restart_video();
        let (video_tx, video_rx) = mpsc::sync_channel(VIDEO_QUEUE_CAPACITY);
        let path = self.path.clone();
        let graphics_device = self.graphics_device.clone();
        let notify = Arc::clone(&self.notify);
        let metrics = Arc::clone(&self.metrics);
        let generation = self.generation;
        let video_generation = self.video_generation;
        let end = self.range_end();
        self.video_cancel = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&self.video_cancel);
        let completion = Arc::clone(&self.completion);
        let mut input = self.video_input.take();
        let video_thread = thread::Builder::new()
            .name("towavue-video-feed".to_owned())
            .spawn(move || {
                let result = (|| {
                    if input.is_none() {
                        input = Some(decode::ParallelInput::open(&path, &|| {
                            cancelled.load(Ordering::Relaxed)
                        })?);
                    }
                    run_video_decode(
                        input.as_mut().expect("opened video input"),
                        &graphics_device,
                        &video_tx,
                        &metrics,
                        video_generation,
                        target,
                        end,
                        &cancelled,
                        notify.as_ref(),
                    )
                })();
                if cancelled.load(Ordering::Relaxed) {
                    return input;
                }
                match result {
                    Ok(()) if completion.finish(DecodeCompletion::VIDEO) => {
                        notify(PlaybackEvent::DecodeFinished(generation));
                    }
                    Ok(()) | Err(decode::DecodeError::ConsumerClosed) => {}
                    Err(error) => match graphics_device.device_removed_reason() {
                        Some(reason) => {
                            notify(PlaybackEvent::DeviceRemoved(video_generation, reason))
                        }
                        None => notify(PlaybackEvent::VideoFailed(
                            video_generation,
                            error.to_string(),
                        )),
                    },
                }
                input
            })?;
        self.video_rx = Some(video_rx);
        self.video_thread = Some(video_thread);
        Ok(())
    }

    fn stop_pipeline(&mut self) {
        self.decode_cancel.store(true, Ordering::Relaxed);
        // Release the bounded audio receiver before joining its producer.
        self.audio.take();
        self.stop_video(false);
        if let Some(thread) = self.audio_thread.take() {
            let _ = thread.join();
        }
    }

    fn stop_video(&mut self, retain_frame: bool) {
        self.video_generation = self.video_generation.next();
        self.video_cancel.store(true, Ordering::Relaxed);
        self.pending_video.take();
        if !retain_frame {
            self.current_video.take();
            self.retained_video_position = None;
        }
        self.current_video_unpresented = false;
        self.video_refresh_pending = true;
        self.video_rx.take();
        if let Some(thread) = self.video_thread.take() {
            self.video_input = thread.join().ok().flatten();
        }
    }

    /// Suspend video decode while leaving the audio producer, output, and clock intact.
    /// On return, the caller supplies its current source position, including paused state.
    pub fn set_video_visible(
        &mut self,
        visible: bool,
        position: MediaTime,
    ) -> Result<(), PlaybackError> {
        if self.video_visible == visible {
            return Ok(());
        }
        if !visible {
            if !self.video_refresh_pending && self.current_video.is_some() {
                self.retained_video_position = Some(position);
            } else if self.retained_video_position != Some(position) {
                self.retained_video_position = None;
            }
        }
        let target = if visible && self.paused && self.retained_video_position == Some(position) {
            // Preserve the exact paused frame, even when its timestamp differs from the clock.
            self.current_video
                .as_ref()
                .map_or(position, PresentationFrame::presentation_time)
        } else {
            position
        };
        self.stop_video(true);
        self.completion.restart_video();
        self.video_visible = false;
        if visible {
            self.start_video(target.max(MediaTime::ZERO))?;
            self.video_visible = true;
        }
        Ok(())
    }

    /// DecodeFinished is a wake-up; visibility changes can invalidate an already queued one.
    pub fn decode_finished(&self) -> bool {
        self.completion.finished()
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
        self.video_refresh_pending = false;
        true
    }

    /// A retained frame is drawable, but is not the result for the resumed position.
    pub fn video_refresh_pending(&self) -> bool {
        self.video_refresh_pending
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
            // Only terminal source preview may precede the seek target.
            if time >= cutoff || (time < self.video_target && self.video_refresh_pending) {
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

fn run_audio_decode(
    path: &Path,
    audio: &AudioOutputSender,
    target: MediaTime,
    end: Option<MediaTime>,
    cancelled: &AtomicBool,
) -> Result<(), decode::DecodeError> {
    decode::decode_file_parallel_cancellable(
        path,
        target,
        end,
        Some(DecodeStream::Audio),
        &|| cancelled.load(Ordering::Relaxed),
        |output| match output {
            ParallelSoftwareDecodeOutput::Item(DecodeOutput::Audio(chunk)) => {
                audio.push(chunk).is_ok()
            }
            ParallelSoftwareDecodeOutput::AudioFinished => audio.finish().is_ok(),
            _ => unreachable!("audio-only decoder emitted video"),
        },
    )
    .map(|_| ())
}

#[allow(clippy::too_many_arguments)]
fn run_video_decode(
    input: &mut decode::ParallelInput,
    graphics_device: &GraphicsDevice,
    video_tx: &SyncSender<PresentationFrame>,
    metrics: &SharedMetrics,
    generation: PlaybackGeneration,
    target: MediaTime,
    end: Option<MediaTime>,
    cancelled: &AtomicBool,
    notify: &(impl Fn(PlaybackEvent) + Sync + ?Sized),
) -> Result<(), decode::DecodeError> {
    let mut hardware_output_seen = false;
    let hardware_result = input.decode_hardware(
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
            }
            match output {
                ParallelRuntimeDecodeOutput::VideoFinished => true,
                ParallelRuntimeDecodeOutput::Item(RuntimeDecodeOutput::Video(frame)) => {
                    metrics.hardware_frame_count.fetch_add(1, Ordering::Relaxed);
                    send_video_frame(
                        PresentationFrame::Hardware(frame),
                        video_tx,
                        generation,
                        cancelled,
                        notify,
                    )
                }
            }
        },
    );

    match hardware_result {
        _ if cancelled.load(Ordering::Relaxed) => Err(decode::DecodeError::ConsumerClosed),
        Err(decode::DecodeError::HardwareUnavailable(_)) if !hardware_output_seen => {
            notify(PlaybackEvent::DecodePathSelected(
                generation,
                DecodePath::Software,
            ));
            input
                .decode_software(
                    target,
                    end,
                    Some(DecodeStream::Video),
                    &|| cancelled.load(Ordering::Relaxed),
                    |output| match output {
                        ParallelSoftwareDecodeOutput::VideoFinished => true,
                        ParallelSoftwareDecodeOutput::Item(DecodeOutput::Video(frame)) => {
                            metrics.cpu_transfer_count.fetch_add(1, Ordering::Relaxed);
                            send_video_frame(
                                PresentationFrame::Software(frame),
                                video_tx,
                                generation,
                                cancelled,
                                notify,
                            )
                        }
                        _ => unreachable!("video-only decoder emitted audio"),
                    },
                )
                .map(|_| ())
        }
        other => other.map(|_| ()),
    }
}

fn send_video_frame(
    frame: PresentationFrame,
    video_tx: &SyncSender<PresentationFrame>,
    generation: PlaybackGeneration,
    cancelled: &AtomicBool,
    notify: &(impl Fn(PlaybackEvent) + ?Sized),
) -> bool {
    // stop_video drops the receiver before joining this bounded producer.
    if cancelled.load(Ordering::Relaxed) || video_tx.send(frame).is_err() {
        return false;
    }
    if !cancelled.load(Ordering::Relaxed) {
        notify(PlaybackEvent::VideoReady(generation));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4")
    }

    fn wait_for_video(session: &mut PlaybackSession) -> MediaTime {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(time) = session.pending_video_time() {
                return time;
            }
            assert!(Instant::now() < deadline, "video frame deadline");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn completion_notifies_once_and_video_restart_invalidates_queued_completion() {
        for _ in 0..32 {
            let completion = DecodeCompletion::default();
            let notifications = AtomicU8::new(0);
            thread::scope(|scope| {
                for stream in [DecodeCompletion::AUDIO, DecodeCompletion::VIDEO] {
                    let completion = &completion;
                    let notifications = &notifications;
                    scope.spawn(move || {
                        if completion.finish(stream) {
                            notifications.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                }
            });
            assert!(completion.finished());
            assert_eq!(notifications.load(Ordering::Relaxed), 1);
            completion.restart_video();
            assert!(!completion.finished());
            assert!(!completion.finish(DecodeCompletion::AUDIO));
            assert!(completion.finish(DecodeCompletion::VIDEO));
            assert!(completion.finished());
        }
    }

    #[test]
    fn hidden_video_releases_a_full_queue_and_resumes_at_the_requested_source_position() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("towavue-video-visibility-{unique}.mp4"));
        let output = std::process::Command::new(
            crate::media_tools::tool_path("ffmpeg.exe").expect("fixed FFmpeg"),
        )
        .args(["-v", "error", "-i"])
        .arg(fixture())
        .args(["-map", "0:v:0", "-c", "copy", "-an"])
        .arg(&path)
        .output()
        .expect("video-only fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let device = GraphicsDevice::warp_for_test().expect("Windows WARP device");
        let (sender, events) = mpsc::channel();
        let mut session = PlaybackSession::open(
            &path,
            device,
            0.0,
            1.0,
            PlaybackRange::default(),
            move |event| {
                let _ = sender.send(event);
            },
        )
        .expect("video session");
        let generation = session.generation();
        let old_video_generation = session.video_generation;
        assert_eq!(wait_for_video(&mut session), MediaTime::ZERO);
        assert!(session.advance_pending());
        let Some(PresentationFrame::Software(first)) = &session.current_video else {
            panic!("WARP software frame");
        };
        let first_pixels = first.rgba.as_ptr();
        let geometry = session.video_geometry();
        assert!(!session.video_refresh_pending());
        let between_frames = MediaTime::from_nanoseconds(17_000_000);
        thread::sleep(Duration::from_millis(50));
        assert!(
            session.metrics().cpu_transfer_count <= 4,
            "bounded output queue"
        );
        session
            .set_video_visible(false, between_frames)
            .expect("hide");
        assert!(session.video_thread.is_none() && session.video_rx.is_none());
        assert!(
            session.video_input.is_some(),
            "joined worker returns its input"
        );
        // Any path reopen from this point would fail; the open input must suffice.
        session.path = path.with_extension("absent");
        assert!(!session.path.exists());
        assert!(session.pending_video_time().is_none());
        assert_eq!(session.video_geometry(), geometry);
        assert!(session.video_refresh_pending() && !session.current_video_unpresented);
        let hidden_count = session.metrics().cpu_transfer_count;
        thread::sleep(Duration::from_millis(50));
        assert_eq!(session.metrics().cpu_transfer_count, hidden_count);
        session.set_paused(true).expect("pause behind another tab");
        session
            .set_video_visible(true, between_frames)
            .expect("return to the same paused clock");
        let Some(PresentationFrame::Software(retained)) = &session.current_video else {
            panic!("retained frame");
        };
        assert_eq!(
            retained.rgba.as_ptr(),
            first_pixels,
            "no copy or blank on return"
        );
        session
            .set_video_visible(false, between_frames)
            .expect("hide before fresh frame");
        session
            .set_video_visible(true, between_frames)
            .expect("rapid paused return");
        assert_eq!(
            wait_for_video(&mut session),
            MediaTime::ZERO,
            "no one-frame step from a between-frame clock"
        );
        session.advance_pending();
        session
            .set_video_visible(false, between_frames)
            .expect("hide paused frame again");
        let target = MediaTime::from_nanoseconds(1_000_000_000);
        session.set_paused(true).expect("pause");
        session.set_video_visible(true, target).expect("show");
        let Some(PresentationFrame::Software(retained)) = &session.current_video else {
            panic!("retained frame");
        };
        assert_eq!(retained.presentation_time, MediaTime::ZERO);
        assert!(session.video_refresh_pending());
        assert_eq!(session.generation(), generation);
        assert!(!session.accepts_event(&PlaybackEvent::VideoReady(old_video_generation)));
        assert!(!session.accepts_event(&PlaybackEvent::VideoFailed(
            old_video_generation,
            "stale".into()
        )));
        assert!(session.accepts_event(&PlaybackEvent::VideoReady(session.video_generation)));
        assert!(session.accepts_event(&PlaybackEvent::AudioReady(generation)));
        assert!(session.paused && !session.decode_finished());
        assert!(wait_for_video(&mut session) >= target);
        let mut observed = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            while let Some(time) = session.pending_video_time() {
                session.advance_pending();
                let Some(PresentationFrame::Software(frame)) = &session.current_video else {
                    panic!("WARP must use the software fallback");
                };
                observed.push((time, frame.rgba.clone()));
            }
            if session.decode_finished() && session.pending_video_time().is_none() {
                break;
            }
            assert!(Instant::now() < deadline, "completion deadline");
            thread::sleep(Duration::from_millis(5));
        }
        let mut expected = Vec::new();
        decode::decode_file(&path, |output| {
            if let DecodeOutput::Video(frame) = output
                && frame.presentation_time >= target
            {
                expected.push((frame.presentation_time, frame.rgba));
            }
            true
        })
        .expect("reference decode");
        assert_eq!(observed, expected);
        assert!(!session.video_refresh_pending());
        assert!(events.try_iter().all(|event| !matches!(
            event,
            PlaybackEvent::Failed(..)
                | PlaybackEvent::VideoFailed(..)
                | PlaybackEvent::DeviceRemoved(..)
        )));
        session
            .set_video_visible(false, target)
            .expect("hide at EOF");
        assert!(
            !session.decode_finished(),
            "old queued EOF is no longer authoritative"
        );
        let next_generation = session.seek(target).expect("seek while hidden");
        assert_ne!(next_generation, generation);
        assert!(session.video_thread.is_none() && session.paused);
        session
            .set_video_visible(true, target)
            .expect("show after hidden seek");
        assert!(wait_for_video(&mut session) >= target);
        for target in [MediaTime::ZERO, target, MediaTime::ZERO] {
            session
                .set_video_visible(false, target)
                .expect("hide again");
            session
                .set_video_visible(true, target)
                .expect("resume open input");
            assert_eq!(wait_for_video(&mut session), target);
        }
        session
            .set_video_visible(false, MediaTime::ZERO)
            .expect("hide before recovery");
        session
            .replace_graphics_device(
                GraphicsDevice::warp_for_test().expect("replacement WARP device"),
                MediaTime::ZERO,
            )
            .expect("recover with retained input");
        assert!(session.video_thread.is_none());
        assert!(
            session.video_geometry().is_none(),
            "old device frame released"
        );
        session
            .set_video_visible(true, MediaTime::ZERO)
            .expect("return after recovery");
        assert_eq!(wait_for_video(&mut session), MediaTime::ZERO);
        session.advance_pending();
        let end = MediaTime::from_nanoseconds(3_000_000_000);
        session
            .set_video_visible(false, MediaTime::ZERO)
            .expect("hide first frame");
        session
            .set_video_visible(true, end)
            .expect("return after background EOF");
        assert!(wait_for_video(&mut session) < end);
        assert_eq!(
            session.drop_video_before(end),
            0,
            "retain terminal replacement despite old preview"
        );
        session.advance_pending();
        assert!(!session.video_refresh_pending());
        let Some(PresentationFrame::Software(terminal)) = &session.current_video else {
            panic!("terminal frame");
        };
        let last = expected.last().expect("reference terminal");
        assert_eq!(terminal.presentation_time, last.0);
        assert_eq!(terminal.rgba, last.1);
        session.seek(MediaTime::ZERO).expect("seek clears preview");
        assert!(session.video_geometry().is_none() && session.video_refresh_pending());
        drop(session);
        std::fs::remove_file(path).expect("remove owned video-only fixture");
    }

    #[test]
    #[ignore = "requires a live Windows shared-mode audio endpoint; plays the owned fixture muted"]
    fn video_visibility_keeps_the_live_audio_workers_and_source_clock() {
        let device = GraphicsDevice::warp_for_test().expect("Windows WARP device");
        let mut session = match PlaybackSession::open(
            &fixture(),
            device,
            0.0,
            1.0,
            PlaybackRange::default(),
            |_| {},
        ) {
            Ok(session) => session,
            Err(PlaybackError::Audio(error)) => {
                eprintln!("SKIP: shared-mode audio endpoint unavailable: {error}");
                return;
            }
            Err(error) => panic!("session: {error}"),
        };
        wait_for_video(&mut session);
        let output_worker = session.audio.as_ref().expect("audio output").worker_id();
        let feed_worker = session
            .audio_thread
            .as_ref()
            .expect("audio feed")
            .thread()
            .id();
        let generation = session.generation();
        let before = session.audio_position().expect("source clock");
        session.set_video_visible(false, before).expect("hide");
        thread::sleep(Duration::from_millis(250));
        let hidden = session.audio_position().expect("hidden clock");
        assert!(hidden > before.saturating_add(Duration::from_millis(100)));
        session.set_paused(true).expect("pause hidden audio");
        thread::sleep(Duration::from_millis(50));
        let paused = session.audio_position().expect("paused clock");
        session
            .set_video_visible(true, paused)
            .expect("show paused video");
        assert!(wait_for_video(&mut session) >= paused);
        thread::sleep(Duration::from_millis(50));
        assert_eq!(session.audio_position(), Some(paused));
        assert_eq!(
            session.audio.as_ref().expect("retained output").worker_id(),
            output_worker
        );
        assert_eq!(
            session
                .audio_thread
                .as_ref()
                .expect("retained audio feed")
                .thread()
                .id(),
            feed_worker
        );
        assert_eq!(session.generation(), generation);
        session.set_paused(false).expect("resume");
        session
            .set_video_visible(false, paused)
            .expect("hide again");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match session.try_audio_event() {
                Some(AudioOutputEvent::Drained) => break,
                Some(other) => panic!("unexpected audio event: {other:?}"),
                None => {}
            }
            assert!(
                Instant::now() < deadline,
                "audio must drain while video is hidden"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(session.audio_position().expect("drained audio clock") > paused);
        eprintln!(
            "PASS: hidden video stops; audio worker identities/paused clock remain and audio drains"
        );
    }

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
