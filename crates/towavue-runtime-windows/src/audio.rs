use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use thiserror::Error;
use wasapi::{
    AudioClient, AudioClock, AudioRenderClient, DeviceEnumerator, DeviceEventCallbacks, Direction,
    Handle, Role, SampleType, StreamMode, WaveFormat,
};

use crate::{AudioChunk, AudioFormat};
use towavue_core::MediaTime;

const AUDIO_CHANNEL_CAPACITY: usize = 32;
const CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(20);
const EVENT_WAIT_MILLISECONDS: u32 = 20;
const BUFFERED_AUDIO_SECONDS: usize = 2;

/// Notifications produced by the event-driven audio thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioOutputEvent {
    Drained,
    EndpointChanged,
    Failed(String),
}

/// A failure while starting or controlling WASAPI output.
#[derive(Debug, Error)]
pub enum AudioOutputError {
    #[error("unsupported audio format: {0} Hz, {1} channels")]
    UnsupportedFormat(u32, u16),
    #[error("WASAPI output failed: {0}")]
    Wasapi(String),
    #[error("the audio output thread stopped")]
    Closed,
    #[error("the default audio endpoint changed")]
    EndpointChanged,
}

enum AudioMessage {
    Chunk(AudioChunk),
    End,
}

enum AudioControl {
    SetPaused(bool),
    Shutdown,
}

/// Safe command handle for the runtime-owned event-driven WASAPI thread.
pub struct AudioOutput {
    format: AudioFormat,
    audio_tx: SyncSender<AudioMessage>,
    control_tx: mpsc::Sender<AudioControl>,
    event_rx: Receiver<AudioOutputEvent>,
    position_nanoseconds: Arc<AtomicI64>,
    volume: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

/// Cloneable producer for the bounded audio queue.
#[derive(Clone)]
pub(crate) struct AudioOutputSender {
    format: AudioFormat,
    audio_tx: SyncSender<AudioMessage>,
}

impl AudioOutputSender {
    pub(crate) fn push(&self, chunk: AudioChunk) -> Result<(), AudioOutputError> {
        if chunk.format != self.format {
            return Err(AudioOutputError::UnsupportedFormat(
                chunk.format.sample_rate,
                chunk.format.channels,
            ));
        }
        self.audio_tx
            .send(AudioMessage::Chunk(chunk))
            .map_err(|_| AudioOutputError::Closed)
    }

    pub(crate) fn finish(&self) -> Result<(), AudioOutputError> {
        self.audio_tx
            .send(AudioMessage::End)
            .map_err(|_| AudioOutputError::Closed)
    }
}

impl AudioOutput {
    pub fn start(format: AudioFormat) -> Result<Self, AudioOutputError> {
        Self::start_at(format, MediaTime::ZERO)
    }

    pub fn start_at(
        format: AudioFormat,
        media_anchor: MediaTime,
    ) -> Result<Self, AudioOutputError> {
        Self::start_with_volume(format, media_anchor, 1.0)
    }

    pub(crate) fn start_with_volume(
        format: AudioFormat,
        media_anchor: MediaTime,
        volume: f32,
    ) -> Result<Self, AudioOutputError> {
        if format.sample_rate == 0 || format.channels != 2 {
            return Err(AudioOutputError::UnsupportedFormat(
                format.sample_rate,
                format.channels,
            ));
        }

        let (audio_tx, audio_rx) = mpsc::sync_channel(AUDIO_CHANNEL_CAPACITY);
        let (control_tx, control_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let position_nanoseconds = Arc::new(AtomicI64::new(media_anchor.as_nanoseconds()));
        let thread_position = Arc::clone(&position_nanoseconds);
        let volume = Arc::new(AtomicU32::new(normalized_volume(volume).to_bits()));
        let thread_volume = Arc::clone(&volume);
        let thread = thread::Builder::new()
            .name("towavue-wasapi".to_owned())
            .spawn(move || {
                let initialization = wasapi::initialize_mta()
                    .ok()
                    .map_err(|error| error.to_string());
                if let Err(error) = initialization {
                    let _ = ready_tx.send(Err(error));
                    return;
                }

                let result = run_audio_thread(
                    format,
                    media_anchor,
                    &thread_position,
                    &thread_volume,
                    audio_rx,
                    control_rx,
                    &ready_tx,
                );
                wasapi::deinitialize();
                match result {
                    Ok(()) => {
                        let _ = event_tx.send(AudioOutputEvent::Drained);
                    }
                    Err(AudioOutputError::EndpointChanged) => {
                        let _ = event_tx.send(AudioOutputEvent::EndpointChanged);
                    }
                    Err(error) => {
                        let _ = event_tx.send(AudioOutputEvent::Failed(error.to_string()));
                    }
                }
            })
            .map_err(|error| AudioOutputError::Wasapi(error.to_string()))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                format,
                audio_tx,
                control_tx,
                event_rx,
                position_nanoseconds,
                volume,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(AudioOutputError::Wasapi(error))
            }
            Err(_) => {
                let _ = thread.join();
                Err(AudioOutputError::Closed)
            }
        }
    }

    pub fn push(&self, chunk: AudioChunk) -> Result<(), AudioOutputError> {
        self.sender().push(chunk)
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), AudioOutputError> {
        self.control_tx
            .send(AudioControl::SetPaused(paused))
            .map_err(|_| AudioOutputError::Closed)
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume
            .store(normalized_volume(volume).to_bits(), Ordering::Relaxed);
    }

    pub fn finish(&self) -> Result<(), AudioOutputError> {
        self.sender().finish()
    }

    pub fn try_event(&self) -> Option<AudioOutputEvent> {
        self.event_rx.try_recv().ok()
    }

    pub fn position(&self) -> MediaTime {
        MediaTime::from_nanoseconds(self.position_nanoseconds.load(Ordering::Relaxed))
    }

    pub(crate) fn sender(&self) -> AudioOutputSender {
        AudioOutputSender {
            format: self.format,
            audio_tx: self.audio_tx.clone(),
        }
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        let _ = self.control_tx.send(AudioControl::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_audio_thread(
    format: AudioFormat,
    media_anchor: MediaTime,
    position_nanoseconds: &AtomicI64,
    volume: &AtomicU32,
    audio_rx: Receiver<AudioMessage>,
    control_rx: Receiver<AudioControl>,
    ready_tx: &SyncSender<Result<(), String>>,
) -> Result<(), AudioOutputError> {
    let (endpoint_tx, endpoint_rx) = mpsc::channel();
    let setup = (|| {
        let enumerator = DeviceEnumerator::new().map_err(wasapi_error)?;
        let device = enumerator
            .get_default_device(&Direction::Render)
            .map_err(wasapi_error)?;
        let device_id = device.get_id().map_err(wasapi_error)?;
        let mut callbacks = DeviceEventCallbacks::new();
        let default_endpoint_tx = endpoint_tx.clone();
        callbacks.set_default_device_callback(move |direction, role, _| {
            if is_default_render_endpoint(direction, role) {
                let _ = default_endpoint_tx.send(());
            }
        });
        let removed_endpoint_tx = endpoint_tx.clone();
        callbacks.set_device_removed_callback(move |removed_id| {
            if is_current_endpoint(&device_id, &removed_id) {
                let _ = removed_endpoint_tx.send(());
            }
        });
        let registration = enumerator
            .register_notification_callback(callbacks)
            .map_err(wasapi_error)?;
        let mut client = device.get_iaudioclient().map_err(wasapi_error)?;
        let wave_format = WaveFormat::new(
            32,
            32,
            &SampleType::Float,
            format.sample_rate as usize,
            format.channels as usize,
            None,
        );
        let (default_period, _) = client.get_device_period().map_err(wasapi_error)?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: default_period,
        };
        client
            .initialize_client(&wave_format, &Direction::Render, &mode)
            .map_err(wasapi_error)?;
        let event = client.set_get_eventhandle().map_err(wasapi_error)?;
        let render_client = client.get_audiorenderclient().map_err(wasapi_error)?;
        let clock = client.get_audioclock().map_err(wasapi_error)?;
        Ok::<_, AudioOutputError>((client, event, render_client, clock, registration))
    })();
    let (client, event, render_client, clock, _registration) = match setup {
        Ok(resources) => resources,
        Err(error) => {
            let _ = ready_tx.send(Err(error.to_string()));
            return Err(error);
        }
    };
    ready_tx
        .send(Ok(()))
        .map_err(|_| AudioOutputError::Closed)?;

    render_audio_loop(
        format,
        audio_rx,
        control_rx,
        &client,
        &render_client,
        &event,
        &clock,
        media_anchor,
        position_nanoseconds,
        &endpoint_rx,
        volume,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_audio_loop(
    format: AudioFormat,
    audio_rx: Receiver<AudioMessage>,
    control_rx: Receiver<AudioControl>,
    client: &AudioClient,
    render_client: &AudioRenderClient,
    event: &Handle,
    clock: &AudioClock,
    media_anchor: MediaTime,
    position_nanoseconds: &AtomicI64,
    endpoint_rx: &Receiver<()>,
    volume: &AtomicU32,
) -> Result<(), AudioOutputError> {
    let bytes_per_frame = format.channels as usize * size_of::<f32>();
    let queue_limit = format.sample_rate as usize * bytes_per_frame * BUFFERED_AUDIO_SECONDS;
    let mut queue = VecDeque::new();
    let mut output = Vec::new();
    let mut gain = VolumeRamp::new(
        f32::from_bits(volume.load(Ordering::Relaxed)),
        format.sample_rate,
    );
    let mut input_ended = false;
    let mut paused = false;
    let mut running = false;
    let mut wall_elapsed = Duration::ZERO;
    let mut wall_started_at: Option<Instant> = None;
    let clock_frequency = clock.get_frequency().map_err(wasapi_error)?;
    if clock_frequency == 0 {
        return Err(AudioOutputError::Wasapi(
            "IAudioClock returned a zero frequency".to_owned(),
        ));
    }
    let (clock_origin, _) = clock.get_position().map_err(wasapi_error)?;

    loop {
        if endpoint_rx.try_recv().is_ok() {
            return Err(AudioOutputError::EndpointChanged);
        }
        while let Ok(control) = control_rx.try_recv() {
            match control {
                AudioControl::SetPaused(value) => {
                    paused = value;
                    if paused && running {
                        client.stop_stream().map_err(wasapi_error)?;
                        if let Some(started_at) = wall_started_at.take() {
                            wall_elapsed += started_at.elapsed();
                        }
                        running = false;
                    }
                }
                AudioControl::Shutdown => {
                    if running {
                        client.stop_stream().map_err(wasapi_error)?;
                    }
                    return Err(AudioOutputError::Closed);
                }
            }
        }

        while !input_ended && queue.len() < queue_limit {
            match audio_rx.try_recv() {
                Ok(AudioMessage::Chunk(chunk)) => queue.extend(chunk.bytes),
                Ok(AudioMessage::End) => input_ended = true,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => input_ended = true,
            }
        }

        if !paused {
            let available = client
                .get_available_space_in_frames()
                .map_err(wasapi_error)? as usize;
            let queued_frames = queue.len() / bytes_per_frame;
            let write_frames = available.min(queued_frames);
            output.clear();
            output.extend(queue.drain(..write_frames * bytes_per_frame));
            gain.apply(&mut output, f32::from_bits(volume.load(Ordering::Relaxed)));
            render_client
                .write_to_device(write_frames, &output, None)
                .map_err(wasapi_error)?;

            if !running
                && (write_frames > 0 || client.get_current_padding().map_err(wasapi_error)? > 0)
            {
                client.start_stream().map_err(wasapi_error)?;
                wall_started_at = Some(Instant::now());
                running = true;
            }

            if input_ended
                && queue.is_empty()
                && client.get_current_padding().map_err(wasapi_error)? == 0
            {
                if running {
                    client.stop_stream().map_err(wasapi_error)?;
                    if let Some(started_at) = wall_started_at.take() {
                        wall_elapsed += started_at.elapsed();
                    }
                }
                return Ok(());
            }
        }

        if running {
            let _ = event.wait_for_event(EVENT_WAIT_MILLISECONDS);
            let (device_position, _) = clock.get_position().map_err(wasapi_error)?;
            let elapsed = device_position.saturating_sub(clock_origin) as u128 * 1_000_000_000u128
                / u128::from(clock_frequency);
            let wall_nanoseconds = wall_elapsed.as_nanos().saturating_add(
                wall_started_at.map_or(0, |started_at| started_at.elapsed().as_nanos()),
            );
            let elapsed = elapsed.max(wall_nanoseconds).min(i64::MAX as u128) as i64;
            position_nanoseconds.store(
                media_anchor.as_nanoseconds().saturating_add(elapsed),
                Ordering::Relaxed,
            );
        } else {
            match control_rx.recv_timeout(CONTROL_POLL_INTERVAL) {
                Ok(AudioControl::SetPaused(value)) => paused = value,
                Ok(AudioControl::Shutdown) => return Err(AudioOutputError::Closed),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(AudioOutputError::Closed);
                }
            }
        }
    }
}

fn normalized_volume(volume: f32) -> f32 {
    volume.clamp(0.0, 2.0).max(0.0)
}

struct VolumeRamp {
    current: f32,
    target: f32,
    remaining: u32,
    ramp_frames: u32,
}

impl VolumeRamp {
    fn new(volume: f32, sample_rate: u32) -> Self {
        Self {
            current: volume,
            target: volume,
            remaining: 0,
            ramp_frames: (sample_rate / 200).max(1),
        }
    }

    fn apply(&mut self, bytes: &mut [u8], target: f32) {
        if target != self.target {
            self.target = target;
            self.remaining = self.ramp_frames;
        }
        if self.current == 1.0 && self.remaining == 0 {
            return;
        }
        for frame in bytes.as_chunks_mut::<8>().0 {
            if self.remaining > 0 {
                self.current += (self.target - self.current) / self.remaining as f32;
                self.remaining -= 1;
                if self.remaining == 0 {
                    self.current = self.target;
                }
            }
            for sample in frame.as_chunks_mut::<4>().0 {
                let value = f32::from_le_bytes(*sample);
                let scaled = if self.current == 0.0 {
                    0.0
                } else {
                    value * self.current
                };
                sample.copy_from_slice(&scaled.to_le_bytes());
            }
        }
    }
}

fn wasapi_error(error: wasapi::WasapiError) -> AudioOutputError {
    AudioOutputError::Wasapi(error.to_string())
}

fn is_default_render_endpoint(direction: Direction, role: Role) -> bool {
    direction == Direction::Render && role == Role::Console
}

fn is_current_endpoint(current_id: &str, removed_id: &str) -> bool {
    current_id == removed_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_ramp_preserves_stereo_and_reaches_exact_mute_across_buffers() {
        let samples = [0.5_f32, -0.25].repeat(4);
        let source = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        let mut gain = VolumeRamp::new(1.0, 800);
        let mut unity = source.clone();
        gain.apply(&mut unity, 1.0);
        assert_eq!(unity, source);
        let mut first = source[..16].to_vec();
        let mut second = source[16..].to_vec();
        gain.apply(&mut first, 0.0);
        gain.apply(&mut second, 0.0);
        let actual = first.iter().chain(&second).copied().collect::<Vec<_>>();
        let expected = [0.375_f32, -0.1875, 0.25, -0.125, 0.125, -0.0625, 0.0, 0.0];
        assert_eq!(
            actual,
            expected
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<_>>()
        );
        let mut muted = source.clone();
        gain.apply(&mut muted, 0.0);
        assert!(muted.iter().all(|byte| *byte == 0));
        let mut louder = source.clone();
        gain.apply(&mut louder, 2.0);
        assert_eq!(
            &louder[louder.len() - 8..],
            [1.0_f32, -0.5]
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            source,
            samples
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn initial_volume_is_applied_without_a_full_volume_startup() {
        let mut gain = VolumeRamp::new(0.0, 48_000);
        let mut output = [0.5_f32, -0.5]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        gain.apply(&mut output, 0.0);
        assert_eq!(output, vec![0; 8]);
        assert_eq!(normalized_volume(f32::NAN), 0.0);
        assert_eq!(normalized_volume(-1.0), 0.0);
        assert_eq!(normalized_volume(3.0), 2.0);
    }

    #[test]
    fn rejects_non_stereo_format_before_opening_device() {
        let error = AudioOutput::start(AudioFormat {
            sample_rate: 48_000,
            channels: 1,
        })
        .err()
        .expect("mono must be rejected");

        assert!(matches!(
            error,
            AudioOutputError::UnsupportedFormat(48_000, 1)
        ));
    }

    #[test]
    fn filters_endpoint_notifications_to_the_active_console_renderer() {
        assert!(is_default_render_endpoint(Direction::Render, Role::Console));
        assert!(!is_default_render_endpoint(
            Direction::Capture,
            Role::Console
        ));
        assert!(!is_default_render_endpoint(
            Direction::Render,
            Role::Multimedia
        ));
        assert!(is_current_endpoint("active", "active"));
        assert!(!is_current_endpoint("active", "other"));
    }
}
