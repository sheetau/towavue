//! Bounded skipping of invalid AAC-LC packets in playback only.
use super::*;

const MAX_CONSECUTIVE: usize = 8;

pub(super) struct Recovery {
    enabled: bool,
    consecutive: usize,
    skipped_frames: usize,
    recovered: bool,
    decoded: bool,
}

impl Recovery {
    pub(super) fn new(playback: bool, config: &StreamConfig) -> Self {
        // SAFETY: the owning Parameters allocation stays live for this read;
        // no native pointer or reference is retained by the recovery policy.
        let profile = unsafe { (*config.parameters.as_ptr()).profile };
        Self {
            enabled: playback
                && config.parameters.id() == codec::Id::AAC
                && profile == ffmpeg::ffi::AV_PROFILE_AAC_LOW
                && config.has_sample_timestamps(),
            consecutive: 0,
            skipped_frames: 0,
            recovered: false,
            decoded: false,
        }
    }

    pub(super) fn decoded(&mut self) {
        self.consecutive = 0;
        self.skipped_frames = 0;
        self.decoded = true;
    }

    pub(super) fn finish(&self) -> Result<(), DecodeError> {
        if self.recovered && !self.decoded {
            Err(ffmpeg::Error::InvalidData.into())
        } else {
            Ok(())
        }
    }

    pub(super) fn replace(
        &mut self,
        error: DecodeError,
        packet: &ffmpeg::Packet,
        pipeline: &mut AudioPipeline,
        emit: &mut impl FnMut(ParallelDecodeOutput) -> bool,
    ) -> Result<(), DecodeError> {
        if !self.enabled
            || !matches!(error, DecodeError::Ffmpeg(ffmpeg::Error::InvalidData))
            || self.consecutive >= MAX_CONSECUTIVE
            || packet.side_data().next().is_some()
        {
            return Err(error);
        }
        let Some(pts) = packet.pts() else {
            return Err(error);
        };
        let rate = pipeline.output_format.sample_rate;
        let base = Rational(1, rate as i32);
        let duration = packet.duration().rescale(pipeline.time_base, base);
        // Do not allocate from untrusted duration metadata or guess missing time.
        // A damaged MP4 sample can span missing access units. Bound the entire
        // consecutive skipped interval to five seconds, not just its packets.
        if duration <= 0 || duration > i64::from(rate) * 5 - self.skipped_frames as i64 {
            return Err(error);
        }
        let start = pts.rescale(pipeline.time_base, base);
        let end = start.saturating_add(duration);
        // A receive error can follow partial output from this packet. Skip
        // only its remaining samples, preserving already emitted PCM.
        let start = if pipeline.next_sample == ffmpeg::ffi::AV_NOPTS_VALUE {
            start
        } else {
            start.max(pipeline.next_sample)
        };
        if start >= end {
            return Err(error);
        }
        let frames = (end - start) as usize;
        if !emit(ParallelDecodeOutput::AudioSkipped(
            timestamp_to_media_time(Some(start), base),
            timestamp_to_media_time(Some(end), base),
        )) {
            return Err(DecodeError::ConsumerClosed);
        }
        self.skipped_frames += frames;
        pipeline.next_sample = end;
        pipeline.next_presentation_time = timestamp_to_media_time(Some(end), base);
        pipeline.sample_preroll = None;
        self.consecutive += 1;
        self.recovered = true;
        crate::diagnostic!(
            "Skipped invalid AAC packet: frames={frames} consecutive={}",
            self.consecutive
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
