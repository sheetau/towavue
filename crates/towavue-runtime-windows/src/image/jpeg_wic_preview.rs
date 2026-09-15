//! Baseline YCbCr 4:4:4 thumbnails on the existing preview worker only.
use crate::PreviewImage;
use ffmpeg_next as ffmpeg;
use std::cell::{Cell, RefCell};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use windows::{
    Win32::{Graphics::Imaging::*, System::Com::*},
    core::Interface,
};

#[cfg(test)]
pub(crate) static ENABLED: AtomicBool = AtomicBool::new(true);
#[cfg(test)]
pub(crate) static FINISHED: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
pub(crate) static CREATED: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
pub(crate) static DECODED: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn worker_enabled() -> bool {
    #[cfg(test)]
    return ENABLED.load(Ordering::SeqCst);
    #[cfg(not(test))]
    true
}

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static SERVICE: RefCell<Option<Service>> = const { RefCell::new(None) };
}

// The scope is created/dropped inside the worker closure. No COM interface
// escapes that thread; lazy initialization leaves non-JPEG/cache-only work alone.
pub(crate) struct WorkerScope(bool);
impl WorkerScope {
    pub(crate) fn new(enabled: bool) -> Self {
        assert!(!ACTIVE.replace(enabled), "worker scopes cannot nest");
        Self(enabled)
    }
}
impl Drop for WorkerScope {
    fn drop(&mut self) {
        if self.0 {
            SERVICE.with_borrow_mut(|slot| drop(slot.take()));
            ACTIVE.set(false);
        }
        #[cfg(test)]
        FINISHED.fetch_add(1, Ordering::SeqCst);
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // Successful initialization and uninitialization occur on this worker.
        unsafe { CoUninitialize() };
    }
}

#[test]
#[ignore = "isolated WIC worker lifetime/eligibility/cancellation control; uses existing Pillow fixtures"]
fn worker_service_preserves_guards_orientation_and_apartment_lifetime() {
    use super::IMAGE_BYTE_LIMIT;
    use super::jpeg_preview::{jpeg_preview, jpeg_thumbnail};
    use std::fs;
    let root =
        std::env::temp_dir().join(format!("towavue-wic-worker-guards-{}", std::process::id()));
    fs::create_dir(&root).expect("unique owned test directory");
    let path = root.join("source.jpg");
    image::RgbImage::from_fn(503, 317, |x, y| {
        image::Rgb(
            [[240, 10, 20], [10, 230, 30], [20, 30, 220], [230, 220, 20]]
                [usize::from(x >= 251) + 2 * usize::from(y >= 158)],
        )
    })
    .save(&path)
    .expect("owned JPEG");
    let child_path = path.clone();
    std::thread::spawn(move || {
        let apartment_state = || unsafe {
            let mut kind = APTTYPE::default();
            let mut qualifier = APTTYPEQUALIFIER::default();
            CoGetApartmentType(&mut kind, &mut qualifier)
                .map(|()| (kind, qualifier))
                .map_err(|error| error.code())
        };
        let before = apartment_state();
        let bytes = fs::read(&child_path).expect("source bytes");
        let baseline = jpeg_thumbnail(&child_path, IMAGE_BYTE_LIMIT, &|| true)
            .expect("baseline")
            .expect("reduced");
        let created = CREATED.load(Ordering::SeqCst);
        {
            let _scope = WorkerScope::new(true);
            assert!(
                jpeg_preview(&child_path, IMAGE_BYTE_LIMIT, &|| true)
                    .expect("foreground speculation")
                    .is_none()
            );
            assert_eq!(CREATED.load(Ordering::SeqCst), created);
            assert!(
                jpeg_thumbnail(&child_path, 1, &|| true)
                    .expect("budget")
                    .is_none()
            );
            assert_eq!(CREATED.load(Ordering::SeqCst), created);
            for tag in 1..=8 {
                let mut exif =
                    *b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0";
                exif[24] = tag;
                let mut encoded = vec![0xff, 0xd8, 0xff, 0xe1];
                encoded.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
                encoded.extend_from_slice(&exif);
                encoded.extend_from_slice(&bytes[2..]);
                fs::write(&child_path, &encoded).expect("owned orientation fixture");
                let decoded = DECODED.load(Ordering::SeqCst);
                let small = jpeg_thumbnail(&child_path, IMAGE_BYTE_LIMIT, &|| true)
                    .expect("WIC preview")
                    .expect("reduced");
                assert_eq!(DECODED.load(Ordering::SeqCst), decoded + 1);
                let full = super::decode_image(&child_path).expect("independent original");
                assert_eq!(small.source_size, full.dimensions());
                assert!(small.image.width <= 240 && small.image.height <= 160);
                for (x, y) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                    let a = ((small.image.height * y / 4 * small.image.width
                        + small.image.width * x / 4)
                        * 4) as usize;
                    let (w, h) = full.dimensions();
                    let b = ((h * y / 4 * w + w * x / 4) * 4) as usize;
                    for c in 0..4 {
                        assert!(small.image.rgba[a + c].abs_diff(full.frames[0].rgba[b + c]) <= 5);
                    }
                }
                assert_eq!(
                    fs::read(&child_path).expect("unchanged buffer/source"),
                    encoded
                );
            }
            assert_eq!(
                CREATED.load(Ordering::SeqCst),
                created + 1,
                "one reused service"
            );
            fs::write(&child_path, &bytes).expect("restore owned untransformed fixture");
            for cutoff in 1..=20 {
                let calls = Cell::new(0);
                let current = || {
                    calls.set(calls.get() + 1);
                    calls.get() < cutoff
                };
                let decoded = DECODED.load(Ordering::SeqCst);
                assert!(jpeg_thumbnail(&child_path, IMAGE_BYTE_LIMIT, &current).is_err());
                assert_eq!(
                    DECODED.load(Ordering::SeqCst),
                    decoded,
                    "no accepted WIC image after cancellation"
                );
            }
            let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/generated/jpeg-preview");
            for name in [
                "RGB-False",
                "RGB-True",
                "L-False",
                "CMYK-False",
                "RGB-direct",
            ] {
                let decoded = DECODED.load(Ordering::SeqCst);
                assert!(
                    jpeg_thumbnail(
                        &fixtures.join(format!("{name}.jpg")),
                        IMAGE_BYTE_LIMIT,
                        &|| true
                    )
                    .expect("fallback")
                    .is_some()
                );
                assert_eq!(
                    DECODED.load(Ordering::SeqCst),
                    decoded,
                    "unsupported class stays on existing decoder"
                );
            }
            fs::write(&child_path, &bytes[..100]).expect("owned truncated header");
            assert!(jpeg_thumbnail(&child_path, IMAGE_BYTE_LIMIT, &|| true).is_err());
            fs::write(&child_path, &bytes).expect("restore owned JPEG");
        }
        assert!(!ACTIVE.get());
        assert!(SERVICE.with_borrow(|slot| slot.is_none()));
        assert_eq!(
            apartment_state(),
            before,
            "COM count restored after service teardown"
        );
        let outside = jpeg_thumbnail(&child_path, IMAGE_BYTE_LIMIT, &|| true)
            .expect("outside scope")
            .expect("fallback");
        assert_eq!(outside.source_size, baseline.source_size);
        assert_eq!(outside.image, baseline.image);
        // A caller-owned STA must survive an incompatible MTA setup attempt.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .expect("owned STA")
        };
        let _sta = Apartment;
        let sta_before = apartment_state();
        let created = CREATED.load(Ordering::SeqCst);
        {
            let _scope = WorkerScope::new(true);
            let fallback = jpeg_thumbnail(&child_path, IMAGE_BYTE_LIMIT, &|| true)
                .expect("apartment fallback")
                .expect("preview");
            assert_eq!(fallback.source_size, baseline.source_size);
            assert_eq!(fallback.image, baseline.image);
        }
        assert_eq!(CREATED.load(Ordering::SeqCst), created);
        assert_eq!(apartment_state(), sta_before);
        assert_eq!(
            fs::read(child_path).expect("unchanged original bytes"),
            bytes
        );
    })
    .join()
    .expect("owned lifetime test worker");
    fs::remove_file(path).expect("owned fixture cleanup");
    fs::remove_dir(root).expect("empty owned fixture directory");
}
struct Service {
    // Struct fields drop in declaration order: factory before apartment.
    factory: IWICImagingFactory,
    _apartment: Apartment,
}
impl Service {
    fn new() -> windows::core::Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        let apartment = Apartment;
        let factory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
        #[cfg(test)]
        CREATED.fetch_add(1, Ordering::SeqCst);
        Ok(Self {
            factory,
            _apartment: apartment,
        })
    }
}

pub(crate) fn thumbnail(
    bytes: &[u8],
    width: u32,
    height: u32,
    swap: bool,
    current: &dyn Fn() -> bool,
) -> Option<PreviewImage> {
    if !ACTIVE.get() || !current() {
        return None;
    }
    // Parse the bounded, borrowed input to exclude unverified color/coding paths.
    // The pinned SOF enum's default is BaselineDct, excluding extended/progressive.
    let mut header = zune_jpeg::JpegDecoder::new(zune_core::bytestream::ZCursor::new(bytes));
    header.decode_headers().ok()?;
    let info = header.info()?;
    if info.sof != Default::default()
        || info.sample_ratio != zune_jpeg::SampleRatios::None
        || header.input_colorspace() != Some(zune_core::colorspace::ColorSpace::YCbCr)
    {
        return None;
    }
    drop(header);
    let (bw, bh) = if swap { (160.0, 240.0) } else { (240.0, 160.0) };
    let scale = (bw / f64::from(width)).min(bh / f64::from(height));
    let (tw, th) = (
        (f64::from(width) * scale).round().max(1.0) as u32,
        (f64::from(height) * scale).round().max(1.0) as u32,
    );
    let shift = (1..=3)
        .rev()
        .find(|shift| width.div_ceil(1 << shift) >= tw && height.div_ceil(1 << shift) >= th)?;
    if !current() {
        return None;
    }
    SERVICE.with_borrow_mut(|slot| {
        if slot.is_none() {
            *slot = Service::new().ok();
        }
        let service = slot.as_ref()?;
        decode(
            &service.factory,
            bytes,
            (width, height),
            (tw, th),
            shift,
            current,
        )
        .ok()
    })
}

fn decode(
    factory: &IWICImagingFactory,
    bytes: &[u8],
    source: (u32, u32),
    target: (u32, u32),
    shift: u32,
    current: &dyn Fn() -> bool,
) -> windows::core::Result<PreviewImage> {
    let cancelled = || windows::core::Error::from_hresult(windows::Win32::Foundation::E_ABORT);
    // The input is the caller's already bounded JPEG buffer. Decoder/frame/stream
    // stay on this worker and drop before return, so WIC cannot outlive its borrowed
    // input. Only read operations are used. Factory retains no per-image interface.
    // CopyPixels synchronously borrows the owned AVFrame plane with its real stride.
    unsafe {
        let stream = factory.CreateStream()?;
        stream.InitializeFromMemory(bytes)?;
        let decoder: IWICBitmapDecoder =
            CoCreateInstance(&CLSID_WICJpegDecoder, None, CLSCTX_INPROC_SERVER)?;
        decoder.Initialize(&stream, WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h)?;
        if (w, h) != source {
            return Err(cancelled());
        }
        let transform: IWICBitmapSourceTransform = frame.cast()?;
        let wanted = (w.div_ceil(1 << shift), h.div_ceil(1 << shift));
        (w, h) = wanted;
        transform.GetClosestSize(&mut w, &mut h)?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        transform.GetClosestPixelFormat(&mut format)?;
        if (w, h) != wanted
            || format != GUID_WICPixelFormat24bppBGR
            || !transform
                .DoesSupportTransform(WICBitmapTransformRotate0)?
                .as_bool()
            || !current()
        {
            return Err(cancelled());
        }
        let mut decoded = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::BGR24, w, h);
        let stride = decoded.stride(0) as u32;
        transform.CopyPixels(
            std::ptr::null(),
            w,
            h,
            &format,
            WICBitmapTransformRotate0,
            stride,
            decoded.data_mut(0),
        )?;
        if !current() {
            return Err(cancelled());
        }
        let (tw, th) = target;
        let mut scaler = ffmpeg::software::scaling::Context::get(
            decoded.format(),
            w,
            h,
            ffmpeg::format::Pixel::RGBA,
            tw,
            th,
            ffmpeg::software::scaling::Flags::BILINEAR,
        )
        .map_err(|_| cancelled())?;
        let mut rgba = ffmpeg::frame::Video::empty();
        scaler.run(&decoded, &mut rgba).map_err(|_| cancelled())?;
        let mut pixels = Vec::with_capacity((tw * th * 4) as usize);
        for row in 0..th as usize {
            if !current() {
                return Err(cancelled());
            }
            let start = row * rgba.stride(0);
            pixels.extend_from_slice(&rgba.data(0)[start..start + tw as usize * 4]);
        }
        #[cfg(test)]
        DECODED.fetch_add(1, Ordering::SeqCst);
        Ok(PreviewImage {
            width: tw,
            height: th,
            rgba: pixels.into(),
        })
    }
}
