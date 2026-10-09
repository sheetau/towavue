//! Private native preroll integration, confined to the video-only decode owner.
use super::*;
use ffmpeg::packet::Mut;
use std::sync::OnceLock;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    GetProcAddress,
};
use windows_core::{PCWSTR, s};

const OMITTED: i32 = 1 << 30;

type Allocate = unsafe extern "C" fn(i64) -> *mut ffmpeg::ffi::AVBufferRef;
type Stop = unsafe extern "C" fn(*mut ffmpeg::ffi::AVBufferRef) -> i64;
type Acknowledge = unsafe extern "C" fn(*const ffmpeg::ffi::AVFrame) -> i32;
type Codecs = unsafe extern "C" fn() -> u32;

struct Api {
    codecs: u32,
    allocate: Allocate,
    stop: Stop,
    acknowledge: Acknowledge,
}

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        // SAFETY: the linked codec returns its static configuration string. This
        // resolves a delayed import before discovery. FROM_ADDRESS treats the
        // pointer only as an address, not UTF-16 text, and identifies that exact
        // codec module rather than choosing by an ambiguous DLL basename.
        // PIN retains the module for all cached entry points and frame-buffer
        // destruction callbacks. The app never unloads its FFmpeg dependencies.
        let module = unsafe {
            let address = ffmpeg::ffi::avcodec_configuration();
            if address.is_null() {
                return None;
            }
            let mut module = HMODULE::default();
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                PCWSTR(address.cast()),
                &mut module,
            )
            .ok()?;
            module
        };
        // SAFETY: these exact versioned export names define the pinned private
        // v1 C ABI: allocate(i64)->AVBufferRef*, stop(AVBufferRef*)->i64 and
        // acknowledge(const AVFrame*)->int32. All three must exist in the same
        // loaded codec. Standard FFmpeg has none and keeps ordinary decoding.
        unsafe {
            let allocate = GetProcAddress(module, s!("avcodec_towavue_preroll_v1_alloc"))?;
            let stop = GetProcAddress(module, s!("avcodec_towavue_preroll_v1_stop"))?;
            let acknowledge = GetProcAddress(module, s!("avcodec_towavue_preroll_v1_ack"))?;
            // The initial v1 protocol supported HEVC only. Later compatible
            // implementations explicitly advertise additional codec bits.
            let codecs = GetProcAddress(module, s!("avcodec_towavue_preroll_v1_codecs"))
                .map_or(1, |function| {
                    let function = std::mem::transmute::<
                        unsafe extern "system" fn() -> isize,
                        Codecs,
                    >(function);
                    function()
                });
            Some(Api {
                codecs,
                allocate: std::mem::transmute::<unsafe extern "system" fn() -> isize, Allocate>(allocate),
                stop: std::mem::transmute::<unsafe extern "system" fn() -> isize, Stop>(stop),
                acknowledge: std::mem::transmute::<unsafe extern "system" fn() -> isize, Acknowledge>(acknowledge),
            })
        }
    })
    .as_ref()
}

pub(super) fn requested() -> bool {
    #[cfg(test)]
    {
        ENABLED.load(std::sync::atomic::Ordering::Relaxed) && api().is_some()
    }
    #[cfg(not(test))]
    {
        api().is_some()
    }
}

pub(super) fn supports(id: codec::Id) -> bool {
    let bit = match id {
        codec::Id::HEVC => 1,
        codec::Id::H264 => 2,
        codec::Id::AV1 => 4,
        _ => return false,
    };
    requested() && api().is_some_and(|api| api.codecs & bit != 0)
}

pub(super) fn omitted(frame: &frame::Video) -> bool {
    // SAFETY: the borrowed owned frame remains live throughout this scalar read.
    api().is_some() && unsafe { (*frame.as_ptr()).flags & OMITTED != 0 }
}

pub(super) struct Policy {
    api: &'static Api,
    reference: *mut ffmpeg::ffi::AVBufferRef,
    minimum: MediaTime,
    stopped: bool,
}

// SAFETY: this Rust owner is moved, never shared. Native packet/frame references
// use FFmpeg's thread-safe reference counting and the policy's atomic protocol.
// No Rust allocation or borrowed decoder/device address is retained by native code.
unsafe impl Send for Policy {}

impl Drop for Policy {
    fn drop(&mut self) {
        // SAFETY: one uniquely owned reference; native packets keep their own refs.
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut self.reference) };
    }
}

impl Policy {
    pub(super) fn new(time_base: Rational, minimum: MediaTime) -> Option<Self> {
        let api = api()?;
        if minimum <= MediaTime::ZERO || time_base.numerator() <= 0 || time_base.denominator() <= 0
        {
            return None;
        }
        let numerator = i128::from(minimum.as_nanoseconds()) * i128::from(time_base.denominator());
        let denominator = i128::from(time_base.numerator()) * 1_000_000_000;
        let threshold = i64::try_from((numerator + denominator - 1) / denominator).ok()?;
        #[cfg(test)]
        let threshold = if FORCE_SELECTED.load(std::sync::atomic::Ordering::Relaxed) {
            i64::MAX
        } else {
            threshold
        };
        // SAFETY: the native allocator returns an owned ref or null. No pointer
        // from Rust is stored; a failed optional allocation keeps ordinary decode.
        let reference = unsafe { (api.allocate)(threshold) };
        (!reference.is_null()).then_some(Self {
            api,
            reference,
            minimum,
            stopped: false,
        })
    }

    pub(super) fn attach(&self, packet: &mut ffmpeg::Packet) -> Result<(), DecodeError> {
        if self.stopped {
            return Ok(());
        }
        // SAFETY: this worker exclusively owns the packet. The decoder receives
        // another buffer reference through COPY_OPAQUE; no policy lifetime is borrowed.
        unsafe {
            let raw = &mut *packet.as_mut_ptr();
            if !raw.opaque_ref.is_null() {
                return Err(DecodeError::PrerollUnavailable);
            }
            raw.opaque_ref = ffmpeg::ffi::av_buffer_ref(self.reference);
            if raw.opaque_ref.is_null() {
                return Err(DecodeError::PrerollUnavailable);
            }
        }
        Ok(())
    }

    pub(super) fn receive(
        &mut self,
        frame: &frame::Video,
        time: MediaTime,
    ) -> Result<(), DecodeError> {
        let missing = omitted(frame);
        if missing {
            // SAFETY: each native output is acknowledged exactly once, before it
            // enters a runtime channel or is cloned. Its owned opaque_ref is live.
            let result = unsafe { (self.api.acknowledge)(frame.as_ptr()) };
            if result < 0 {
                return Err(DecodeError::PrerollUnavailable);
            }
            #[cfg(test)]
            OMITTED_OUTPUTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if time >= self.minimum && !self.stopped {
            // Stop and admission use one atomic native word. Zero outstanding
            // proves that no earlier worker can publish another omitted output,
            // and all later admissions fail. A conservative nonzero retries.
            let outstanding = unsafe { (self.api.stop)(self.reference) };
            self.stopped = true;
            if outstanding != 0 || missing {
                return Err(DecodeError::PrerollUnavailable);
            }
        } else if missing && self.stopped {
            return Err(DecodeError::PrerollUnavailable);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
#[cfg(test)]
pub(super) static FORCE_SELECTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
pub(super) static DAMAGE_PTS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
pub(super) static OMITTED_OUTPUTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
pub(super) static RETRIES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(super) static DAMAGE_REFERENCE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
pub(super) fn damage_reference(packet: &mut ffmpeg::Packet, changed: &mut bool) {
    if *changed || !DAMAGE_REFERENCE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let bytes = packet.data_mut().expect("fixture packet bytes");
    let mut offset = 0;
    while offset + 6 <= bytes.len() {
        let length = u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("four-byte NAL length"),
        ) as usize;
        assert!(length >= 2 && length <= bytes.len() - offset - 4);
        if (bytes[offset + 4] >> 1) & 63 == 1 {
            bytes[offset + 4] &= !2;
            *changed = true;
        }
        offset += length + 4;
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires an explicitly selected native runtime and expected capability"]
fn native_capability_matches_loaded_codec() {
    let expected = std::env::var("TOWAVUE_PREROLL_EXPECT_AVAILABLE")
        .expect("explicit native capability expectation");
    let expected = match expected.as_str() {
        "0" => false,
        "1" => true,
        _ => panic!("capability expectation must be 0 or 1"),
    };
    assert_eq!(api().is_some(), expected);
    assert_eq!(requested(), expected);
    let codecs: u32 = std::env::var("TOWAVUE_PREROLL_EXPECT_CODECS")
        .expect("explicit codec capability mask")
        .parse()
        .expect("codec mask integer");
    assert_eq!(api().map_or(0, |api| api.codecs), codecs);
    assert_eq!(supports(codec::Id::HEVC), codecs & 1 != 0);
    assert_eq!(supports(codec::Id::H264), codecs & 2 != 0);
    assert_eq!(supports(codec::Id::AV1), codecs & 4 != 0);
    assert!(!supports(codec::Id::VP9));
    let policy = Policy::new(Rational(1, 30), MediaTime::from_nanoseconds(1_000_000_000));
    assert_eq!(policy.is_some(), expected);
    if let Some(policy) = policy {
        // SAFETY: this fresh policy has its live owned reference and no packets.
        assert_eq!(unsafe { (policy.api.stop)(policy.reference) }, 0);
    }
    println!("PREROLL_CAPABILITY available={expected} codecs={codecs} ordinary_if_absent=1");
}
