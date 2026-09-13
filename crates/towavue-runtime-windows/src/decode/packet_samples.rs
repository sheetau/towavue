use super::*;
use std::ptr::{self, NonNull};

/// A parser owned, used and dropped on one audio worker; never shared or sent.
pub(super) struct PacketSamples {
    parser: NonNull<ffmpeg::ffi::AVCodecParserContext>,
    codec: codec::Id,
    first_packet: bool,
}

impl PacketSamples {
    pub(super) fn new(codec: codec::Id) -> Option<Self> {
        // FFmpeg returns a new owned parser allocation or null. Its complete-frame
        // FLAC/Vorbis parsers inspect headers, not compressed audio samples.
        let parser = unsafe { NonNull::new(ffmpeg::ffi::av_parser_init(codec.into()))? };
        // The allocation is live, exclusively owned and not yet used by a worker.
        unsafe { (*parser.as_ptr()).flags = ffmpeg::ffi::PARSER_FLAG_COMPLETE_FRAMES };
        Some(Self {
            parser,
            codec,
            first_packet: true,
        })
    }

    pub(super) fn samples(
        &mut self,
        decoder: &mut codec::decoder::Audio,
        packet: &ffmpeg::Packet,
    ) -> Option<usize> {
        let size = i32::try_from(packet.size()).ok()?;
        if size < if self.codec == codec::Id::FLAC { 16 } else { 1 } {
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
            let parser = self.parser.as_ptr();
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
            if consumed != size || output_size != size || (*parser).duration <= 0 {
                return None;
            }
            let mut priming = [0; 10];
            priming[..4].copy_from_slice(&((*parser).duration as u32).to_le_bytes());
            if packet.side_data().any(|side| {
                !(self.first_packet
                    && self.codec == codec::Id::VORBIS
                    && side.kind() == ffmpeg::codec::packet::side_data::Type::SkipSamples
                    && side.data() == priming)
            }) {
                return None;
            }
            let first = std::mem::replace(&mut self.first_packet, false);
            // FFmpeg's Vorbis decoder discards its first audio packet to warm
            // the overlap window. It must not advance the sequential sample axis.
            Some(if first && self.codec == codec::Id::VORBIS {
                0
            } else {
                (*parser).duration as usize
            })
        }
    }
}

impl Drop for PacketSamples {
    fn drop(&mut self) {
        // This is the sole owner; no parser call or borrowed output survives here.
        unsafe { ffmpeg::ffi::av_parser_close(self.parser.as_ptr()) };
    }
}
