//! Read-only WIC JPEG comparison. No production decoder or pixel policy change.

use image::ImageDecoder;
use std::{os::windows::ffi::OsStrExt, path::Path, time::Instant};
use towavue_runtime_windows::DecodedImageFrame;
use windows::{
    Win32::{Foundation::GENERIC_READ, Graphics::Imaging::*, System::Com::*},
    core::PCWSTR,
};

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: constructed only after successful initialization on main's thread.
        unsafe { CoUninitialize() };
    }
}

fn wic(factory: &IWICImagingFactory, path: &Path) -> windows::core::Result<DecodedImageFrame> {
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: every COM object stays on the initialized caller thread and drops
    // before its apartment. The decoder retains the stream through CopyPixels;
    // the size-checked output is exclusively borrowed for that synchronous call.
    unsafe {
        let stream = factory.CreateStream()?;
        stream.InitializeFromFilename(PCWSTR(name.as_ptr()), GENERIC_READ.0)?;
        let decoder: IWICBitmapDecoder =
            CoCreateInstance(&CLSID_WICJpegDecoder, None, CLSCTX_INPROC_SERVER)?;
        decoder.Initialize(&stream, WICDecodeMetadataCacheOnDemand)?;
        assert_eq!(decoder.GetFrameCount()?, 1);
        let frame = decoder.GetFrame(0)?;
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height)?;
        let stride = width.checked_mul(4).expect("RGBA stride");
        let length = u64::from(stride) * u64::from(height);
        assert!(width > 0 && height > 0 && length <= 512 * 1024 * 1024);
        let mut rgba = vec![0; length as usize];
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        converter.CopyPixels(std::ptr::null(), stride, &mut rgba)?;
        Ok(DecodedImageFrame {
            width,
            height,
            rgba,
            delay: std::time::Duration::ZERO,
        })
    }
}

fn current(path: &Path) -> DecodedImageFrame {
    let mut decoded = towavue_runtime_windows::decode_image(path)
        .unwrap_or_else(|_| panic!("reference decode failed"));
    assert_eq!(decoded.format, "JPEG", "JPEG input required");
    assert_eq!(decoded.frames.len(), 1);
    decoded.frames.remove(0)
}

fn fresh_service(path: &Path) -> windows::core::Result<DecodedImageFrame> {
    // SAFETY: this fresh comparison worker owns COM and drops its factory first.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let _apartment = Apartment;
    let factory: IWICImagingFactory =
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
    wic(&factory, path)
}

#[allow(clippy::assertions_on_constants)]
fn main() -> windows::core::Result<()> {
    assert!(!cfg!(debug_assertions), "use --release");
    let path = std::path::PathBuf::from(std::env::args_os().nth(1).expect("explicit JPEG path"));
    let stamp = || {
        let metadata = std::fs::metadata(&path).expect("source metadata");
        (
            metadata.len(),
            metadata.modified().expect("source timestamp"),
        )
    };
    let before = stamp();
    let mut header = image::codecs::jpeg::JpegDecoder::new(std::io::BufReader::new(
        std::fs::File::open(&path).expect("source open"),
    ))
    .expect("JPEG header");
    assert_eq!(
        header.orientation().expect("JPEG orientation"),
        image::metadata::Orientation::NoTransforms,
        "probe requires an untransformed JPEG"
    );
    drop(header);
    // SAFETY: this single-threaded example owns initialization; the factory and
    // all its objects are declared after the guard, so they drop before COM.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let _apartment = Apartment;
    let factory: IWICImagingFactory =
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
    let expected = current(&path);
    let fresh_worker = std::env::var_os("TOWAVUE_JPEG_FRESH_WORKER").is_some();
    let decode_wic = || {
        if fresh_worker {
            // Include thread creation, first COM setup, factory and teardown as
            // a conservative service-cost control, not a proposed app worker.
            std::thread::scope(|scope| {
                scope
                    .spawn(|| fresh_service(&path))
                    .join()
                    .expect("WIC worker")
            })
        } else {
            wic(&factory, &path)
        }
    };
    let candidate = decode_wic()?;
    assert_eq!(
        (candidate.width, candidate.height),
        (expected.width, expected.height),
        "orientation/geometry differs; this probe does not apply EXIF transforms"
    );
    assert_eq!(candidate.rgba.len(), expected.rgba.len());
    let (mut changed, mut total, mut maximum) = (0_u64, 0_u64, 0_u8);
    for (a, b) in candidate
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.rgba.as_chunks::<4>().0.iter())
    {
        assert!(a[3] == 255 && b[3] == 255, "opaque JPEG output required");
        for channel in 0..3 {
            let difference = a[channel].abs_diff(b[channel]);
            changed += u64::from(difference != 0);
            total += u64::from(difference);
            maximum = maximum.max(difference);
        }
    }
    println!(
        "JPEG_DIFFERENCE size={}x{} changed_rgb_components={} max_abs={} mean_abs_rgb={:.6}",
        expected.width,
        expected.height,
        changed,
        maximum,
        total as f64 / (expected.rgba.len() / 4 * 3) as f64
    );
    for native in [false, true, true, false] {
        let mut samples = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            let result = if native {
                decode_wic()?
            } else {
                current(&path)
            };
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
            let reference = if native { &candidate } else { &expected };
            assert!(result == *reference, "unstable decode; pixels not printed");
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "JPEG_DECODER wic={native} fresh_worker={fresh_worker} median_ms={:.3}",
            samples[2]
        );
    }
    assert_eq!(stamp(), before, "source changed");
    Ok(())
}
