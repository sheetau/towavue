use super::*;
use std::ptr::{self, NonNull};

/// A parser owned, used and dropped on one audio worker; never shared or sent.
pub(super) struct FlacPacketSamples(NonNull<ffmpeg::ffi::AVCodecParserContext>);

impl FlacPacketSamples {
    pub(super) fn new() -> Option<Self> {
        // FFmpeg returns a new owned parser allocation or null. Complete-frame
        // parsing reads only the FLAC header and does not retain packet storage.
        let parser = unsafe {
            NonNull::new(ffmpeg::ffi::av_parser_init(
                ffmpeg::ffi::AVCodecID::AV_CODEC_ID_FLAC,
            ))?
        };
        // The allocation is live, exclusively owned and not yet used by a worker.
        unsafe { (*parser.as_ptr()).flags = ffmpeg::ffi::PARSER_FLAG_COMPLETE_FRAMES };
        Some(Self(parser))
    }

    pub(super) fn samples(
        &mut self,
        decoder: &mut codec::decoder::Audio,
        packet: &ffmpeg::Packet,
    ) -> Option<usize> {
        let size = i32::try_from(packet.size()).ok()?;
        if size < 16 {
            return None;
        }
        let mut output = ptr::null_mut();
        let mut output_size = 0;
        // Parser and decoder are exclusively borrowed on this worker. FFmpeg's
        // refcounted packet has AV_INPUT_BUFFER_PADDING_SIZE trailing padding;
        // its buffer remains alive throughout this complete-frame parse. The
        // returned output borrows that buffer and is deliberately not retained.
        // Reset duration because an invalid header otherwise leaves its old value.
        unsafe {
            let parser = self.0.as_ptr();
            (*parser).duration = 0;
            let consumed = ffmpeg::ffi::av_parser_parse2(
                parser,
                decoder.as_mut_ptr(),
                &mut output,
                &mut output_size,
                packet.data()?.as_ptr(),
                size,
                ffmpeg::ffi::AV_NOPTS_VALUE,
                ffmpeg::ffi::AV_NOPTS_VALUE,
                -1,
            );
            (consumed == size && output_size == size && (*parser).duration > 0)
                .then_some((*parser).duration as usize)
        }
    }
}

impl Drop for FlacPacketSamples {
    fn drop(&mut self) {
        // This is the sole owner; no parser call or borrowed output survives here.
        unsafe { ffmpeg::ffi::av_parser_close(self.0.as_ptr()) };
    }
}
