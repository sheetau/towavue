use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};

use thiserror::Error;

use crate::audio::{AudioOutput, AudioOutputError, AudioOutputEvent, AudioOutputSender};
use crate::decode::{self, DecodeOutput, VideoFrame};

const VIDEO_QUEUE_CAPACITY: usize = 2;

/// Runtime events containing no native Windows or FFmpeg handles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackEvent {
    VideoReady,
    DecodeFinished,
    Failed(String),
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

/// Owns M1 decoder and audio threads plus the bounded presentation queue.
pub struct PlaybackSession {
    video_rx: Option<Receiver<VideoFrame>>,
    audio: Option<AudioOutput>,
    decode_thread: Option<JoinHandle<()>>,
}

impl PlaybackSession {
    pub fn open(
        path: &Path,
        notify: impl Fn(PlaybackEvent) + Send + 'static,
    ) -> Result<Self, PlaybackError> {
        let audio = decode::probe_audio_format(path)?
            .map(AudioOutput::start)
            .transpose()?;
        let audio_sender = audio.as_ref().map(AudioOutput::sender);
        let (video_tx, video_rx) = mpsc::sync_channel(VIDEO_QUEUE_CAPACITY);
        let path = path.to_owned();
        let decode_thread = thread::Builder::new()
            .name("towavue-decode".to_owned())
            .spawn(move || {
                run_decode_thread(&path, &video_tx, audio_sender.as_ref(), &notify);
            })?;

        Ok(Self {
            video_rx: Some(video_rx),
            audio,
            decode_thread: Some(decode_thread),
        })
    }

    pub fn try_video_frame(&self) -> Option<VideoFrame> {
        self.video_rx.as_ref()?.try_recv().ok()
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
}

impl Drop for PlaybackSession {
    fn drop(&mut self) {
        self.video_rx.take();
        self.audio.take();
        if let Some(thread) = self.decode_thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_decode_thread(
    path: &Path,
    video_tx: &SyncSender<VideoFrame>,
    audio: Option<&AudioOutputSender>,
    notify: &impl Fn(PlaybackEvent),
) {
    let result = decode::decode_file(path, |output| match output {
        DecodeOutput::Video(frame) => {
            if video_tx.send(frame).is_err() {
                return false;
            }
            notify(PlaybackEvent::VideoReady);
            true
        }
        DecodeOutput::Audio(chunk) => audio.is_some_and(|sender| sender.push(chunk).is_ok()),
    });

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
