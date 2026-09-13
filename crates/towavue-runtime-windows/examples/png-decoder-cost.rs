//! Read-only PNG decoder comparison; no application backend change.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};
use towavue_runtime_windows::{DecodedImageFrame, decode_image};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::core::PCWSTR;

#[path = "png_decoder/libpng.rs"]
mod libpng;

struct Apartment(std::marker::PhantomData<std::rc::Rc<()>>);

impl Apartment {
    fn new() -> windows::core::Result<Self> {
        // The !Send guard balances both S_OK and S_FALSE on this thread only.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self(std::marker::PhantomData))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn decode_wic(
    factory: &IWICImagingFactory,
    path: &Path,
    native_bgra: bool,
) -> windows::core::Result<DecodedImageFrame> {
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // All interfaces stay on the initialized caller thread and drop before its
    // apartment guard. The decoder owns its stream reference during CopyPixels;
    // the destination is initialized, size-checked and exclusively borrowed.
    unsafe {
        let stream = factory.CreateStream()?;
        stream.InitializeFromFilename(PCWSTR(name.as_ptr()), GENERIC_READ.0)?;
        let decoder: IWICBitmapDecoder =
            CoCreateInstance(&CLSID_WICPngDecoder, None, CLSCTX_INPROC_SERVER)?;
        decoder.Initialize(&stream, WICDecodeMetadataCacheOnDemand)?;
        assert_eq!(decoder.GetFrameCount()?, 1, "single PNG frame required");
        let frame = decoder.GetFrame(0)?;
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height)?;
        let stride = width.checked_mul(4).expect("RGBA stride");
        let bytes = (stride as usize)
            .checked_mul(height as usize)
            .expect("RGBA length");
        assert!(width > 0 && height > 0 && bytes <= 512 * 1024 * 1024);
        let mut rgba = vec![0; bytes];
        if native_bgra && frame.GetPixelFormat()? == GUID_WICPixelFormat32bppBGRA {
            frame.CopyPixels(std::ptr::null(), stride, &mut rgba)?;
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        } else {
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
        }
        Ok(DecodedImageFrame {
            width,
            height,
            rgba,
            delay: Duration::ZERO,
        })
    }
}

fn current(path: &Path) -> DecodedImageFrame {
    let Ok(mut decoded) = decode_image(path) else {
        panic!("current PNG decode failed");
    };
    assert_eq!(decoded.frames.len(), 1, "static PNG required");
    decoded.frames.remove(0)
}

fn equal(actual: &DecodedImageFrame, expected: &DecodedImageFrame) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    // A failed comparison must never format private pixel buffers or paths.
    assert!(actual.rgba == expected.rgba, "decoded RGBA differs");
}

fn check_generated(
    decode: &impl Fn(&Path) -> DecodedImageFrame,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("towavue-decoder-png-{}", std::process::id()));
    std::fs::create_dir(&root)?;
    for (name, color, channels) in [
        ("rgb.png", png::ColorType::Rgb, 3),
        ("rgba.png", png::ColorType::Rgba, 4),
    ] {
        let path = root.join(name);
        let mut encoder = png::Encoder::new(std::fs::File::create_new(&path)?, 17, 13);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        let pixels: Vec<u8> = (0..17 * 13)
            .flat_map(|n| {
                [
                    n as u8,
                    (n * 7) as u8,
                    (n * 13) as u8,
                    [0, 1, 127, 255][n % 4],
                ]
                .into_iter()
                .take(channels)
            })
            .collect();
        writer.write_image_data(&pixels)?;
        writer.finish()?;
        let expected = current(&path);
        equal(&decode(&path), &expected);
        std::fs::remove_file(&path)?;
    }
    std::fs::remove_dir(&root)?;
    eprintln!(
        "PNG_DECODER generated RGB/RGBA, odd rows and hidden RGB at zero alpha match exactly"
    );
    Ok(())
}

#[allow(clippy::assertions_on_constants)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    assert!(!cfg!(debug_assertions), "use --release");
    let path = std::path::PathBuf::from(
        std::env::var_os("TOWAVUE_PNG_REFERENCE_PATH").expect("explicit read-only static PNG"),
    );
    let stamp = || {
        let metadata = std::fs::metadata(&path).expect("source metadata");
        (metadata.len(), metadata.modified().expect("modified time"))
    };
    let before = stamp();
    let backend = std::env::var("TOWAVUE_PNG_BACKEND").unwrap_or_else(|_| "wic".into());
    assert!(matches!(backend.as_str(), "wic" | "wic-bgra" | "libpng"));
    let _apartment = Apartment::new()?;
    // Created once like a worker-local decoder service, outside per-file timings.
    // Declaration/drop order releases the factory before COM uninitialization.
    let factory: IWICImagingFactory =
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
    let libpng = (backend == "libpng").then(libpng::Decoder::load);
    let candidate = |path: &Path| {
        if let Some(decoder) = &libpng {
            decoder.decode(path)
        } else {
            // The native BGRA probe applies to RGBA inputs; use the converter for
            // the generated RGB control, which does not have a native BGRA frame.
            decode_wic(&factory, path, backend == "wic-bgra").expect("WIC decode failed")
        }
    };
    check_generated(&candidate)?;
    let expected = current(&path);
    equal(&candidate(&path), &expected);
    for (batch, alternative) in [false, true, true, false].into_iter().enumerate() {
        let mut times = Vec::new();
        for _ in 0..3 {
            let started = Instant::now();
            let actual = if alternative {
                candidate(&path)
            } else {
                current(&path)
            };
            times.push(started.elapsed());
            equal(&actual, &expected);
        }
        times.sort_unstable();
        eprintln!(
            "PNG_DECODER backend={backend} alternative={alternative} batch={batch} width={} height={} median_ms={:.3} min_ms={:.3} max_ms={:.3}; open/decode/RGBA allocation+copy/decoder release, worker COM/factory/DLL setup excluded; full equality outside timing, warm file, no private paths/pixels or GPU/whole-process memory claim",
            expected.width,
            expected.height,
            times[1].as_secs_f64() * 1000.0,
            times[0].as_secs_f64() * 1000.0,
            times[2].as_secs_f64() * 1000.0,
        );
    }
    assert_eq!(before, stamp(), "source changed");
    Ok(())
}
