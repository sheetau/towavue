//! Opt-in native color-conversion cost probe. No production display path calls it.
use std::hint::black_box;
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::core::GUID;

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // Created once on this test/worker thread, after a successful CoInitializeEx.
        unsafe { CoUninitialize() };
    }
}

struct Session {
    // COM objects must drop before this thread's apartment.
    factory: IWICImagingFactory,
    _apartment: Apartment,
}
impl Session {
    fn new() -> Self {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }.expect("MTA initialization");
        let apartment = Apartment;
        let factory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .expect("WIC factory");
        Self {
            factory,
            _apartment: apartment,
        }
    }

    fn transform<'a>(
        &self,
        pixels: &'a [u8],
        size: (u32, u32),
        format: &GUID,
        source: &[u8],
        destination: &[u8],
    ) -> Trial<'a> {
        let (width, height) = size;
        let stride = width.checked_mul(4).expect("bounded stride");
        assert_eq!(pixels.len(), (stride * height) as usize);
        // All input/output buffers, profiles and COM objects are local to this
        // thread. Keep the input borrow through the final copy even though WIC
        // creates an owned bitmap. No COM interface crosses worker boundaries.
        unsafe {
            let input_context = self.factory.CreateColorContext().expect("input context");
            input_context
                .InitializeFromMemory(source)
                .expect("input ICC");
            let output_context = self.factory.CreateColorContext().expect("output context");
            output_context
                .InitializeFromMemory(destination)
                .expect("output ICC");
            let bitmap = self
                .factory
                .CreateBitmapFromMemory(width, height, format, stride, pixels)
                .expect("owned bitmap");
            let transform = self
                .factory
                .CreateColorTransformer()
                .expect("color transform");
            transform
                .Initialize(
                    &bitmap,
                    &input_context,
                    &output_context,
                    &GUID_WICPixelFormat32bppRGBA,
                )
                .expect("profile-compatible transform");
            Trial {
                transform,
                bitmap,
                _input_context: input_context,
                _output_context: output_context,
                pixels,
                stride,
            }
        }
    }
}

struct Trial<'a> {
    transform: IWICColorTransform,
    bitmap: IWICBitmap,
    _input_context: IWICColorContext,
    _output_context: IWICColorContext,
    pixels: &'a [u8],
    stride: u32,
}
impl Trial<'_> {
    fn copy(&self, output: &mut [u8], managed: bool) {
        assert_eq!(output.len(), self.pixels.len());
        // Full-image CopyPixels receives the exact stride and initialized slice;
        // native writes cannot outlive this call or reach another worker's rows.
        unsafe {
            if managed {
                self.transform
                    .CopyPixels(std::ptr::null(), self.stride, output)
            } else {
                self.bitmap
                    .CopyPixels(std::ptr::null(), self.stride, output)
            }
            .expect("native pixel copy");
        }
        black_box(&output);
    }
}

fn profile(variable: &str, signature: &[u8]) -> Vec<u8> {
    let path = std::env::var_os(variable)
        .unwrap_or_else(|| panic!("set {variable} to a read-only installed ICC profile"));
    let bytes = std::fs::read(path).expect("read profile");
    assert!(bytes.len() >= 128);
    assert_eq!(&bytes[36..40], b"acsp");
    assert_eq!(&bytes[16..20], signature);
    eprintln!(
        "PROFILE {variable} bytes={} version={:02x}{:02x} intent={}",
        bytes.len(),
        bytes[8],
        bytes[9],
        u32::from_be_bytes(bytes[64..68].try_into().expect("intent"))
    );
    bytes
}

fn pixels(width: u32, height: u32, cmyk: bool) -> Vec<u8> {
    (0..width * height)
        .flat_map(|index| {
            let x = index % width;
            let y = index / width;
            [
                ((x * 251 / width + y * 17) % 256) as u8,
                ((y * 239 / height + x * 7) % 256) as u8,
                ((x * 13 + y * 31) % 256) as u8,
                if cmyk {
                    ((x + y) % 128) as u8
                } else {
                    (index % 256) as u8
                },
            ]
        })
        .collect()
}

fn median(samples: &[Duration]) -> f64 {
    let mut samples = samples.to_vec();
    samples.sort();
    samples[samples.len() / 2].as_secs_f64() * 1000.0
}

fn parallel(
    pixels: &[u8],
    size: (u32, u32),
    format: &GUID,
    source: &[u8],
    destination: &[u8],
    output: &mut [u8],
    workers: usize,
) {
    let rows = (size.1 as usize).div_ceil(workers);
    let chunk = rows * size.0 as usize * 4;
    std::thread::scope(|scope| {
        for (pixels, output) in pixels.chunks(chunk).zip(output.chunks_mut(chunk)) {
            scope.spawn(move || {
                let session = Session::new();
                let height = (pixels.len() / (size.0 as usize * 4)) as u32;
                session
                    .transform(pixels, (size.0, height), format, source, destination)
                    .copy(output, true);
            });
        }
    });
}

#[test]
#[allow(
    clippy::assertions_on_constants,
    reason = "Reject explicit Debug invocation without preventing ordinary Debug test compilation"
)]
#[ignore = "isolated Release ICC speed gate; requires three installed-profile environment variables, no concurrent builds/tests"]
fn report_native_icc_cost_and_pixel_controls() {
    assert!(!cfg!(debug_assertions), "run this probe in Release");
    let rgb = profile("TOWAVUE_ICC_RGB_PROFILE", b"RGB ");
    let cmyk = profile("TOWAVUE_ICC_CMYK_PROFILE", b"CMYK");
    let destination = profile("TOWAVUE_ICC_OUTPUT_PROFILE", b"RGB ");
    let available = std::thread::available_parallelism()
        .expect("CPU count")
        .get();
    let workers = available.min(8);
    let all_workers = available.min(32);
    let session = Session::new();
    eprintln!(
        "CONDITIONS generated RGBA/CMYK8, output RGBA8, preallocated output; samples=5; row_workers={workers}; no display, source mutation or monitor-profile change"
    );
    for (name, source, format, cmyk_input) in [
        ("rgb", &rgb, GUID_WICPixelFormat32bppRGBA, false),
        (
            "identity",
            &destination,
            GUID_WICPixelFormat32bppRGBA,
            false,
        ),
        ("cmyk", &cmyk, GUID_WICPixelFormat32bppCMYK, true),
    ] {
        for size in [(240, 160), (1920, 1080), (3000, 2000), (6000, 4000)] {
            let pixels = pixels(size.0, size.1, cmyk_input);
            let mut output = vec![0; pixels.len()];
            let started = Instant::now();
            let trial = session.transform(&pixels, size, &format, source, &destination);
            let setup = started.elapsed();
            let started = Instant::now();
            trial.copy(&mut output, true);
            let first_copy = started.elapsed();
            if !cmyk_input {
                assert!(
                    pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(output.as_chunks::<4>().0)
                        .all(|(a, b)| a[3] == b[3]),
                    "alpha must survive conversion"
                );
            }
            let changed = pixels.iter().zip(&output).filter(|(a, b)| a != b).count();
            if name != "identity" {
                assert!(
                    changed > pixels.len() / 100,
                    "not an identity/no-op transform"
                );
            }
            if name == "identity" {
                assert_eq!(changed, 0, "matching profiles must preserve every sample");
            }
            let expected = output.clone();
            let mut copies = Vec::new();
            let mut transforms = Vec::new();
            for iteration in 0..5 {
                for managed in if iteration % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let started = Instant::now();
                    trial.copy(&mut output, managed);
                    let elapsed = started.elapsed();
                    if managed {
                        transforms.push(elapsed);
                        assert_eq!(output, expected);
                    } else {
                        copies.push(elapsed);
                        assert_eq!(output, pixels);
                    }
                }
            }
            let mut parallel_times = Vec::new();
            for _ in 0..3 {
                let started = Instant::now();
                parallel(
                    &pixels,
                    size,
                    &format,
                    source,
                    &destination,
                    &mut output,
                    workers,
                );
                parallel_times.push(started.elapsed());
                assert_eq!(
                    output, expected,
                    "row partitioning must preserve all converted pixels"
                );
            }
            let mut all_parallel_times = Vec::new();
            for _ in 0..3 {
                let started = Instant::now();
                parallel(
                    &pixels,
                    size,
                    &format,
                    source,
                    &destination,
                    &mut output,
                    all_workers,
                );
                all_parallel_times.push(started.elapsed());
                assert_eq!(
                    output, expected,
                    "all-worker partitioning must preserve pixels"
                );
            }
            eprintln!(
                "ICC_ALL_WORKERS kind={name} width={} height={} workers={all_workers} fresh_median_ms={:.4}",
                size.0,
                size.1,
                median(&all_parallel_times)
            );
            eprintln!(
                "ICC_COST kind={name} width={} height={} setup_ms={:.4} first_copy_ms={:.4} copy_median_ms={:.4} transform_median_ms={:.4} parallel_fresh_median_ms={:.4} changed_bytes={changed}",
                size.0,
                size.1,
                setup.as_secs_f64() * 1000.0,
                first_copy.as_secs_f64() * 1000.0,
                median(&copies),
                median(&transforms),
                median(&parallel_times)
            );
        }
    }
}
