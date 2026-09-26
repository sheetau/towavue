//! Preview-only mixing on one source-time/sample axis. No native handle crosses
//! workers: each track owns its decoder/resampler and sends bounded stereo PCM.

use crate::decode::{self, AudioChunk, AudioFormat, AudioSeekPolicy, DecodeError};
use ffmpeg_next::{ChannelLayout, format::Sample, frame, software::resampling};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;
use towavue_core::{AudioTrackId, MediaTime};

const BLOCK: usize = 1024;
const QUEUE: usize = 2;
const POLL: Duration = Duration::from_millis(20);

struct Samples {
    start: i64,
    bytes: Vec<u8>,
}

impl Samples {
    fn end(&self) -> i64 {
        self.start + (self.bytes.len() / 8) as i64
    }
}

struct Feed {
    receiver: Receiver<Result<Samples, DecodeError>>,
    current: Option<Samples>,
    ended: bool,
}

impl Feed {
    fn advance(&mut self, position: i64, cancelled: &impl Fn() -> bool) -> Result<(), DecodeError> {
        while !self.ended
            && self
                .current
                .as_ref()
                .is_none_or(|chunk| chunk.end() <= position)
        {
            if cancelled() {
                return Err(DecodeError::ConsumerClosed);
            }
            match self.receiver.recv_timeout(POLL) {
                Ok(result) => self.current = Some(result?),
                Err(RecvTimeoutError::Disconnected) => {
                    self.ended = true;
                    self.current = None;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        Ok(())
    }
}

struct StopOnDrop<'a>(&'a AtomicBool);
impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode(
    path: &Path,
    tracks: &[AudioTrackId],
    target: MediaTime,
    end: Option<MediaTime>,
    format: AudioFormat,
    policy: AudioSeekPolicy,
    cancelled: &(dyn Fn() -> bool + Sync),
    mut emit: impl FnMut(AudioChunk) -> bool,
) -> Result<(), DecodeError> {
    if cancelled() {
        return Err(DecodeError::ConsumerClosed);
    }
    if tracks.is_empty() || format.channels != 2 || format.sample_rate == 0 {
        return Err(ffmpeg_next::Error::InvalidData.into());
    }
    let stopped = AtomicBool::new(false);
    thread::scope(|scope| {
        // On every exit (including spawn failure/panic), receivers retire before
        // scope joins the producers. Blocked sends then fail; the shared stop flag
        // also interrupts decode/preroll. No detached reader survives this call.
        let _stop = StopOnDrop(&stopped);
        let stop = &stopped;
        let mut feeds = Vec::with_capacity(tracks.len());
        for &track in tracks {
            let (sender, receiver) = mpsc::sync_channel(QUEUE);
            thread::Builder::new()
                .name("towavue-audio-mix-track".into())
                .spawn_scoped(scope, move || {
                    let cancelled = || stop.load(Ordering::Relaxed) || cancelled();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut converter = Converter::new(format.sample_rate);
                        let mut failure = None;
                        let mut send = |samples| {
                            if cancelled() {
                                return Err(DecodeError::ConsumerClosed);
                            }
                            sender
                                .send(Ok(samples))
                                .map_err(|_| DecodeError::ConsumerClosed)
                        };
                        let receive = |output| {
                            if let decode::ParallelSoftwareDecodeOutput::Item(
                                decode::DecodeOutput::Audio(chunk),
                            ) = output
                                && let Err(error) = converter.push(chunk, &mut send)
                            {
                                failure = Some(error);
                                return false;
                            }
                            !cancelled()
                        };
                        // Keep ordinary selected-track seek policies. All mixing is
                        // listening-only; waveform and export never call this path.
                        let result = match policy {
                            AudioSeekPolicy::Exact => decode::decode_audio_track_cancellable(
                                path,
                                Some(track),
                                target,
                                end,
                                &cancelled,
                                receive,
                            ),
                            AudioSeekPolicy::Playback => {
                                decode::decode_playback_audio_track_cancellable(
                                    path,
                                    Some(track),
                                    target,
                                    end,
                                    &cancelled,
                                    receive,
                                )
                            }
                        };
                        if let Some(error) = failure {
                            return Err(error);
                        }
                        result?;
                        converter.finish(&mut send)
                    }))
                    .unwrap_or(Err(DecodeError::WorkerPanicked));
                    if let Err(error) = result {
                        let _ = sender.send(Err(error));
                    }
                })?;
            feeds.push(Feed {
                receiver,
                current: None,
                ended: false,
            });
        }
        let cancelled = || stopped.load(Ordering::Relaxed) || cancelled();
        let mut position = sample_ceil(target, format.sample_rate);
        let end = end.map(|time| sample_ceil(time, format.sample_rate));
        loop {
            if cancelled() {
                return Err(DecodeError::ConsumerClosed);
            }
            if end.is_some_and(|end| position >= end) {
                return Ok(());
            }
            for feed in &mut feeds {
                feed.advance(position, &cancelled)?;
            }
            if feeds.iter().all(|feed| feed.ended) {
                return Ok(());
            }
            let mut limit = position.saturating_add(BLOCK as i64);
            if let Some(end) = end {
                limit = limit.min(end);
            }
            for chunk in feeds.iter().filter_map(|feed| feed.current.as_ref()) {
                limit = limit.min(if chunk.start > position {
                    chunk.start
                } else {
                    chunk.end()
                });
            }
            let frames = (limit - position) as usize;
            let mut values = vec![0_f64; frames * 2];
            for chunk in feeds
                .iter()
                .filter_map(|feed| feed.current.as_ref())
                .filter(|chunk| chunk.start <= position)
            {
                let first = (position - chunk.start) as usize * 8;
                for (sum, sample) in values
                    .iter_mut()
                    .zip(chunk.bytes[first..first + frames * 8].as_chunks::<4>().0)
                {
                    *sum += f64::from(f32::from_le_bytes(*sample));
                }
            }
            let mut bytes = Vec::with_capacity(frames * 8);
            for value in values {
                let value = value as f32;
                if !value.is_finite() {
                    return Err(ffmpeg_next::Error::InvalidData.into());
                }
                // Preserve each track's gain. Do not normalize by track count or
                // change levels as shorter tracks end; listening volume applies once.
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            let presentation_time = sample_time(position, format.sample_rate);
            if !emit(AudioChunk {
                presentation_time,
                format,
                frames,
                bytes,
            }) {
                return Err(DecodeError::ConsumerClosed);
            }
            position = limit;
        }
    })
}

fn sample_ceil(time: MediaTime, rate: u32) -> i64 {
    let value = i128::from(time.as_nanoseconds()) * i128::from(rate);
    ((value + 999_999_999).div_euclid(1_000_000_000)) as i64
}

fn sample_nearest(time: MediaTime, rate: u32) -> i64 {
    let value = i128::from(time.as_nanoseconds()) * i128::from(rate);
    ((value + 500_000_000).div_euclid(1_000_000_000)) as i64
}

fn sample_time(sample: i64, rate: u32) -> MediaTime {
    MediaTime::from_nanoseconds((i128::from(sample) * 1_000_000_000 / i128::from(rate)) as i64)
}

struct Converter {
    rate: u32,
    resampler: Option<resampling::Context>,
    expected: Option<MediaTime>,
    next: i64,
}

impl Converter {
    fn new(rate: u32) -> Self {
        Self {
            rate,
            resampler: None,
            expected: None,
            next: 0,
        }
    }

    fn push(
        &mut self,
        chunk: AudioChunk,
        send: &mut impl FnMut(Samples) -> Result<(), DecodeError>,
    ) -> Result<(), DecodeError> {
        let source_rate = chunk.format.sample_rate;
        if source_rate == 0 || chunk.format.channels != 2 || chunk.bytes.len() != chunk.frames * 8 {
            return Err(ffmpeg_next::Error::InvalidData.into());
        }
        let discontinuity = self.expected.is_none_or(|expected| {
            (i128::from(chunk.presentation_time.as_nanoseconds())
                - i128::from(expected.as_nanoseconds()))
            .abs()
                * i128::from(source_rate)
                > 1_000_000_000
        });
        if discontinuity
            || self
                .resampler
                .as_ref()
                .is_some_and(|resampler| resampler.input().rate != source_rate)
        {
            self.finish(send)?;
            self.resampler = None;
            self.next = sample_nearest(chunk.presentation_time, self.rate);
        }
        self.expected = Some(MediaTime::from_nanoseconds(
            chunk.presentation_time.as_nanoseconds().saturating_add(
                (chunk.frames as u64 * 1_000_000_000 / u64::from(source_rate)) as i64,
            ),
        ));
        if source_rate == self.rate {
            self.publish(&chunk.bytes, send)?;
            return Ok(());
        }
        let sample = Sample::F32(ffmpeg_next::format::sample::Type::Packed);
        if self.resampler.is_none() {
            self.resampler = Some(resampling::Context::get(
                sample,
                ChannelLayout::STEREO,
                source_rate,
                sample,
                ChannelLayout::STEREO,
                self.rate,
            )?);
        }
        let resampler = self.resampler.as_mut().expect("resampler");
        let mut input = frame::Audio::new(sample, chunk.frames, ChannelLayout::STEREO);
        input.set_rate(source_rate);
        input.data_mut(0)[..chunk.bytes.len()].copy_from_slice(&chunk.bytes);
        let count = (chunk.frames as u64 * u64::from(self.rate)).div_ceil(u64::from(source_rate))
            + resampler.delay().map_or(0, |delay| delay.output as u64);
        let mut output = frame::Audio::new(sample, count as usize, ChannelLayout::STEREO);
        output.set_rate(self.rate);
        resampler.run(&input, &mut output)?;
        self.publish(&output.data(0)[..output.samples() * 8], send)
    }

    fn finish(
        &mut self,
        send: &mut impl FnMut(Samples) -> Result<(), DecodeError>,
    ) -> Result<(), DecodeError> {
        while let Some(resampler) = self.resampler.as_mut() {
            let Some(delay) = resampler.delay() else {
                break;
            };
            let mut output = frame::Audio::new(
                Sample::F32(ffmpeg_next::format::sample::Type::Packed),
                delay.output.max(1) as usize,
                ChannelLayout::STEREO,
            );
            output.set_rate(self.rate);
            let remaining = resampler.flush(&mut output)?;
            if output.samples() == 0 {
                break;
            }
            self.publish(&output.data(0)[..output.samples() * 8], send)?;
            if remaining.is_none() {
                break;
            }
        }
        Ok(())
    }

    fn publish(
        &mut self,
        bytes: &[u8],
        send: &mut impl FnMut(Samples) -> Result<(), DecodeError>,
    ) -> Result<(), DecodeError> {
        for bytes in bytes.chunks(BLOCK * 8) {
            let samples = Samples {
                start: self.next,
                bytes: bytes.to_vec(),
            };
            self.next += (bytes.len() / 8) as i64;
            send(samples)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "playback_audio_mix/tests.rs"]
mod tests;
