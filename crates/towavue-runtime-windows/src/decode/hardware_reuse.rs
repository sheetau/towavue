//! Exclusively owned hardware decoder returned only from an interrupted run.
//! Container signatures cannot predict in-band changes: before publishing any
//! target frame, a rejected reuse gets one ordinary fresh-hardware attempt.
use super::*;

pub(super) struct Retained {
    pub(super) pipeline: HardwareVideoPipeline,
    pub(super) key: Signature,
    pub(super) device: GraphicsDevice,
}

pub(super) fn requested() -> bool {
    #[cfg(test)]
    if let Some(value) = ENABLED.with(std::cell::Cell::get) {
        return value;
    }
    #[cfg(feature = "presentation-verification")]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var("TOWAVUE_VERIFY_DECODER_REUSE").as_deref() != Ok("0"))
    }
    #[cfg(not(feature = "presentation-verification"))]
    true
}

pub(super) fn supports(codec: codec::Id) -> bool {
    matches!(
        codec,
        codec::Id::H264 | codec::Id::HEVC | codec::Id::VP9 | codec::Id::AV1
    )
}

#[cfg(test)]
thread_local! { pub(super) static ENABLED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) fn next_creation_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Eq, PartialEq)]
pub(super) struct Signature {
    index: usize,
    time_base: Rational,
    scalar: Vec<i64>,
    extra: Vec<u8>,
    side_data: Vec<(i32, Vec<u8>)>,
    matrix: Option<Vec<u8>>,
}

impl Signature {
    pub(super) fn new(config: &StreamConfig) -> Self {
        // SAFETY: config owns the live parameters and nested allocations. Copy
        // initialized fields/payloads only, never padding or native addresses.
        unsafe {
            let p = &*config.parameters.as_ptr();
            let scalar = vec![
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
                p.alpha_mode as i64,
            ];
            let extra = if p.extradata_size > 0 {
                std::slice::from_raw_parts(p.extradata, p.extradata_size as usize).to_vec()
            } else {
                Vec::new()
            };
            let side_data = if p.nb_coded_side_data > 0 {
                std::slice::from_raw_parts(p.coded_side_data, p.nb_coded_side_data as usize)
                    .iter()
                    .map(|side| {
                        (
                            side.type_ as i32,
                            if side.size > 0 {
                                std::slice::from_raw_parts(side.data, side.size).to_vec()
                            } else {
                                Vec::new()
                            },
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            Self {
                index: config.index,
                time_base: config.time_base,
                scalar,
                extra,
                side_data,
                matrix: config.display_matrix.clone(),
            }
        }
    }
}
