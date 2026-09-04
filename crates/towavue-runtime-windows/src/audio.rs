use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use thiserror::Error;
use wasapi::{
    AudioClient, AudioRenderClient, DeviceEnumerator, Direction, Handle, SampleType, StreamMode,
    WaveFormat,
};

use crate::{AudioChunk, AudioFormat};

const AUDIO_CHANNEL_CAPACITY: usize = 32;
const CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(20);
const EVENT_WAIT_MILLISECONDS: u32 = 20;
const BUFFERED_AUDIO_SECONDS: usize = 2;

/// Notifications produced by the event-driven audio thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioOutputEvent {
    Drained,
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

                let result = run_audio_thread(format, audio_rx, control_rx, &ready_tx);
                wasapi::deinitialize();
                match result {
                    Ok(()) => {
                        let _ = event_tx.send(AudioOutputEvent::Drained);
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

    pub fn finish(&self) -> Result<(), AudioOutputError> {
        self.sender().finish()
    }

    pub fn try_event(&self) -> Option<AudioOutputEvent> {
        self.event_rx.try_recv().ok()
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
    audio_rx: Receiver<AudioMessage>,
    control_rx: Receiver<AudioControl>,
    ready_tx: &SyncSender<Result<(), String>>,
) -> Result<(), AudioOutputError> {
    let setup = (|| {
        let enumerator = DeviceEnumerator::new().map_err(wasapi_error)?;
        let device = enumerator
            .get_default_device(&Direction::Render)
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
        Ok::<_, AudioOutputError>((client, event, render_client))
    })();
    let (client, event, render_client) = match setup {
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
    )
}

fn render_audio_loop(
    format: AudioFormat,
    audio_rx: Receiver<AudioMessage>,
    control_rx: Receiver<AudioControl>,
    client: &AudioClient,
    render_client: &AudioRenderClient,
    event: &Handle,
) -> Result<(), AudioOutputError> {
    let bytes_per_frame = format.channels as usize * size_of::<f32>();
    let queue_limit = format.sample_rate as usize * bytes_per_frame * BUFFERED_AUDIO_SECONDS;
    let mut queue = VecDeque::new();
    let mut input_ended = false;
    let mut paused = false;
    let mut running = false;

    loop {
        while let Ok(control) = control_rx.try_recv() {
            match control {
                AudioControl::SetPaused(value) => {
                    paused = value;
                    if paused && running {
                        client.stop_stream().map_err(wasapi_error)?;
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
            render_client
                .write_to_device_from_deque(write_frames, &mut queue, None)
                .map_err(wasapi_error)?;

            if !running
                && (write_frames > 0 || client.get_current_padding().map_err(wasapi_error)? > 0)
            {
                client.start_stream().map_err(wasapi_error)?;
                running = true;
            }

            if input_ended
                && queue.is_empty()
                && client.get_current_padding().map_err(wasapi_error)? == 0
            {
                if running {
                    client.stop_stream().map_err(wasapi_error)?;
                }
                return Ok(());
            }
        }

        if running {
            let _ = event.wait_for_event(EVENT_WAIT_MILLISECONDS);
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

fn wasapi_error(error: wasapi::WasapiError) -> AudioOutputError {
    AudioOutputError::Wasapi(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
