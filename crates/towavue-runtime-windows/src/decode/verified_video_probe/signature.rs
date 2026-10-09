//! Conservative, padding-free signatures bounded to 4 KiB each.
use super::*;
use ffmpeg::ffi::*;

const LIMIT: usize = 4096;

struct Bytes(Vec<u8>);
impl Bytes {
    fn integer(&mut self, value: i64) -> Option<()> {
        self.append(&value.to_le_bytes())
    }
    fn append(&mut self, bytes: &[u8]) -> Option<()> {
        if bytes.len() > LIMIT.saturating_sub(self.0.len()) {
            return None;
        }
        self.0.extend_from_slice(bytes);
        Some(())
    }
    fn payload(&mut self, bytes: &[u8]) -> Option<()> {
        self.integer(bytes.len() as i64)?;
        self.append(bytes)
    }
    unsafe fn dictionary(&mut self, dict: *mut AVDictionary) -> Option<()> {
        // SAFETY: caller borrows the native owner; dictionary entries remain live
        // for this read. FFmpeg entries are NUL-terminated strings. Copy no pointer.
        unsafe {
            self.integer(av_dict_count(dict) as i64)?;
            let mut entry = std::ptr::null();
            loop {
                entry = av_dict_get(dict, c"".as_ptr(), entry, AV_DICT_IGNORE_SUFFIX);
                if entry.is_null() {
                    break;
                }
                self.payload(CStr::from_ptr((*entry).key).to_bytes())?;
                self.payload(CStr::from_ptr((*entry).value).to_bytes())?;
            }
        }
        Some(())
    }
}

pub(super) fn read(input: &format::context::Input) -> Option<Vec<u8>> {
    if !matches!(
        input.format().name(),
        "mov,mp4,m4a,3gp,3g2,mj2" | "matroska,webm"
    ) || input.nb_streams() != 2
    {
        return None;
    }
    let video = input
        .streams()
        .filter(|s| s.parameters().medium() == Type::Video)
        .count();
    let audio = input
        .streams()
        .filter(|s| s.parameters().medium() == Type::Audio)
        .count();
    if video != 1 || audio != 1 {
        return None;
    }
    let mut bytes = Bytes(Vec::new());
    bytes.payload(input.format().name().as_bytes())?;
    // SAFETY: input exclusively owns all borrowed contexts, parameters and nested
    // payloads. Read initialized fields only, never padding or raw union storage.
    // Unsupported custom channel maps/topology use ordinary probing instead.
    unsafe {
        let c = &*input.as_ptr();
        if c.nb_programs != 0 || c.nb_stream_groups != 0 {
            return None;
        }
        for n in [
            c.start_time,
            c.duration,
            c.bit_rate,
            c.ctx_flags as i64,
            c.nb_chapters as i64,
        ] {
            bytes.integer(n)?;
        }
        bytes.dictionary(c.metadata)?;
        for stream in input.streams() {
            let s = &*stream.as_ptr();
            let p = &*s.codecpar;
            if p.codec_type == AVMediaType::AVMEDIA_TYPE_VIDEO
                && !matches!(
                    p.codec_id,
                    AVCodecID::AV_CODEC_ID_H264 | AVCodecID::AV_CODEC_ID_VP9
                )
            {
                return None;
            }
            if s.attached_pic.size != 0
                || s.disposition & AV_DISPOSITION_ATTACHED_PIC != 0
                || !matches!(
                    p.ch_layout.order,
                    AVChannelOrder::AV_CHANNEL_ORDER_NATIVE
                        | AVChannelOrder::AV_CHANNEL_ORDER_UNSPEC
                )
            {
                return None;
            }
            for n in [
                s.index as i64,
                s.id as i64,
                s.time_base.num as i64,
                s.time_base.den as i64,
                s.start_time,
                s.duration,
                s.nb_frames,
                s.disposition as i64,
                s.discard as i64,
                s.sample_aspect_ratio.num as i64,
                s.sample_aspect_ratio.den as i64,
                s.avg_frame_rate.num as i64,
                s.avg_frame_rate.den as i64,
                s.r_frame_rate.num as i64,
                s.r_frame_rate.den as i64,
                s.event_flags as i64,
                p.codec_type as i64,
                p.codec_id as i64,
                p.codec_tag as i64,
                p.format as i64,
                p.bit_rate,
                p.bits_per_coded_sample as i64,
                p.bits_per_raw_sample as i64,
                p.profile as i64,
                p.level as i64,
                p.width as i64,
                p.height as i64,
                p.sample_aspect_ratio.num as i64,
                p.sample_aspect_ratio.den as i64,
                p.framerate.num as i64,
                p.framerate.den as i64,
                p.field_order as i64,
                p.color_range as i64,
                p.color_primaries as i64,
                p.color_trc as i64,
                p.color_space as i64,
                p.chroma_location as i64,
                p.video_delay as i64,
                p.ch_layout.order as i64,
                p.ch_layout.nb_channels as i64,
                p.sample_rate as i64,
                p.block_align as i64,
                p.frame_size as i64,
                p.initial_padding as i64,
                p.trailing_padding as i64,
                p.seek_preroll as i64,
                p.alpha_mode as i64,
            ] {
                bytes.integer(n)?;
            }
            bytes.integer(
                if p.ch_layout.order == AVChannelOrder::AV_CHANNEL_ORDER_NATIVE {
                    p.ch_layout.u.mask as i64
                } else {
                    0
                },
            )?;
            if p.extradata_size < 0 || p.extradata_size as usize > LIMIT {
                return None;
            }
            bytes.payload(if p.extradata_size == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(p.extradata, p.extradata_size as usize)
            })?;
            if p.nb_coded_side_data < 0 || p.nb_coded_side_data > 32 {
                return None;
            }
            bytes.integer(p.nb_coded_side_data as i64)?;
            for i in 0..p.nb_coded_side_data as usize {
                let side = &*p.coded_side_data.add(i);
                if side.size > LIMIT {
                    return None;
                }
                bytes.integer(side.type_ as i64)?;
                bytes.payload(if side.size == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(side.data, side.size)
                })?;
            }
            bytes.dictionary(s.metadata)?;
        }
    }
    Some(bytes.0)
}
