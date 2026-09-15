use super::*;

// Owned decoded references stay inside the runtime's existing bounded channel
// and one last-preroll slot. No decoder/scaler or borrowed native pointer crosses
// threads. Each preroll frame still reaches the consumer, preserving counts and
// cancellation through channel disconnection even before the target is reached.
pub(super) struct PrerollVideoFrame {
    pub(super) presentation_time: MediaTime,
    decoded: frame::Video,
    time_base: Rational,
    orientation: VideoOrientation,
    input: scaling::context::Definition,
    output: scaling::context::Definition,
}

impl PrerollVideoFrame {
    pub(super) fn materialize(self) -> Result<VideoFrame, DecodeError> {
        let mut scaler = scaling::Context::get(
            self.input.format,
            self.input.width,
            self.input.height,
            self.output.format,
            self.output.width,
            self.output.height,
            Flags::BILINEAR,
        )?;
        let mut rgba = frame::Video::empty();
        scaler.run(&self.decoded, &mut rgba)?;
        copy_video_frame(&self.decoded, &rgba, self.time_base, self.orientation)
    }
}

impl VideoPipeline {
    pub(super) fn receive_parallel(
        &mut self,
        output: &SyncSender<ParallelDecodeOutput>,
        summary: &mut DecodeSummary,
        preroll_before: Option<MediaTime>,
    ) -> Result<(), DecodeError> {
        loop {
            let mut decoded = frame::Video::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let time = timestamp_to_media_time(decoded.timestamp(), self.time_base);
                    let output_frame = if preroll_before.is_some_and(|target| time < target) {
                        let input = *self.scaler.input();
                        if (decoded.format(), decoded.width(), decoded.height())
                            != (input.format, input.width, input.height)
                        {
                            return Err(ffmpeg::Error::InputChanged.into());
                        }
                        let orientation = frame_orientation(&decoded, self.orientation)?;
                        ParallelDecodeOutput::SoftwarePreroll(PrerollVideoFrame {
                            presentation_time: time,
                            decoded,
                            time_base: self.time_base,
                            orientation,
                            input,
                            output: *self.scaler.output(),
                        })
                    } else {
                        let mut rgba = frame::Video::empty();
                        self.scaler.run(&decoded, &mut rgba)?;
                        ParallelDecodeOutput::SoftwareVideo(copy_video_frame(
                            &decoded,
                            &rgba,
                            self.time_base,
                            self.orientation,
                        )?)
                    };
                    output
                        .send(output_frame)
                        .map_err(|_| DecodeError::ConsumerClosed)?;
                    summary.video_frames += 1;
                }
                Err(error) if decoder_is_drained(error) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
    }
}
