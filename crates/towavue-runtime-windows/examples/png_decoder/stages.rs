//! Stage isolation through png's own streaming and benchmark APIs.
//! The full filtered canvas is diagnostic-only, not a production memory policy.

use std::cell::RefCell;
use std::path::Path;
use std::time::{Duration, Instant};
use towavue_runtime_windows::DecodedImageFrame;

#[path = "../../src/image/png_static/paeth.rs"]
mod paeth;

#[derive(Clone, Copy)]
pub(super) struct Timing {
    parts: [Duration; 5],
    filters: [usize; 5],
    kernel: &'static str,
}

thread_local! {
    static LAST: RefCell<Option<Timing>> = const { RefCell::new(None) };
}

pub(super) fn take_timing() -> Timing {
    LAST.with_borrow_mut(|last| last.take().expect("completed stage sample"))
}

pub(super) fn report(batch: usize, samples: &[Timing]) {
    let medians: Vec<f64> = (0..5)
        .map(|part| {
            let mut values: Vec<Duration> = samples.iter().map(|t| t.parts[part]).collect();
            values.sort_unstable();
            values[values.len() / 2].as_secs_f64() * 1000.0
        })
        .collect();
    eprintln!(
        "PNG_STAGES batch={batch} kernel={} read_ms={:.3} raw_alloc_ms={:.3} inflate_parse_ms={:.3} unfilter_ms={:.3} pack_alloc_copy_ms={:.3} filters_none_sub_up_avg_paeth={:?}; warm full-buffer stage isolation, png inflate and current or experimental unfilter, not production buffering or whole-process memory",
        samples[0].kernel,
        medians[0],
        medians[1],
        medians[2],
        medians[3],
        medians[4],
        samples[0].filters,
    );
}

pub(super) fn decode(path: &Path, mode: &str) -> DecodedImageFrame {
    let kernel = match mode {
        "stages-sse2" => "sse2",
        _ => "current",
    };
    let mut parts = [Duration::ZERO; 5];
    let started = Instant::now();
    let encoded = std::fs::read(path).expect("read PNG");
    parts[0] = started.elapsed();
    let info = png::Decoder::new(std::io::Cursor::new(&encoded))
        .read_info()
        .expect("PNG header")
        .info()
        .clone();
    assert!(!info.interlaced && info.animation_control.is_none());
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    let bpp = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => panic!("stage probe requires RGB/RGBA8"),
    };
    let (width, height) = (info.width, info.height);
    let rgba_length = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .expect("RGBA size");
    assert!(width > 0 && height > 0 && rgba_length <= 512 * 1024 * 1024);
    let stride = width as usize * bpp + 1;
    let raw_length = stride * height as usize;
    let started = Instant::now();
    // Additional room permits the inflater to finish/check its stream; the
    // exact filled length below rejects any extra decompressed image data.
    let mut raw = vec![0; raw_length + 32768];
    parts[1] = started.elapsed();
    let started = Instant::now();
    let mut decoder = png::StreamingDecoder::new();
    let mut region = png::UnfilterRegion::default();
    let mut input = encoded.as_slice();
    loop {
        assert!(!input.is_empty(), "missing PNG end");
        let filled = region.filled;
        let (used, event) = decoder
            .update(input, Some(&mut region.as_buf(&mut raw)))
            .expect("PNG stream");
        assert!(
            used > 0
                || region.filled > filled
                || !matches!(event, png::Decoded::Nothing | png::Decoded::ImageData),
            "PNG stream made no progress within the bounded output"
        );
        input = &input[used..];
        if matches!(event, png::Decoded::ChunkComplete(png::chunk::IEND)) {
            break;
        }
    }
    assert_eq!(region.filled, raw_length, "complete filtered canvas");
    assert_eq!(region.available, raw_length, "inflate history released");
    parts[2] = started.elapsed();
    let filters = [
        png::Filter::NoFilter,
        png::Filter::Sub,
        png::Filter::Up,
        png::Filter::Avg,
        png::Filter::Paeth,
    ];
    let mut counts = [0; 5];
    let started = Instant::now();
    for row in 0..height as usize {
        let (before, remaining) = raw.split_at_mut(row * stride);
        let current = &mut remaining[..stride];
        let method = current[0] as usize;
        counts[method] += 1;
        let previous = if row == 0 {
            &[]
        } else {
            &before[before.len() - stride + 1..]
        };
        if kernel != "current" && method == 4 && bpp == 4 && row != 0 {
            paeth::unfilter_rgba(previous, &mut current[1..]);
        } else {
            png::benchable_apis::unfilter(filters[method], bpp as u8, previous, &mut current[1..]);
        }
    }
    parts[3] = started.elapsed();
    let started = Instant::now();
    let mut rgba = vec![0; rgba_length];
    for (source, target) in raw[..raw_length]
        .chunks_exact(stride)
        .zip(rgba.chunks_exact_mut(width as usize * 4))
    {
        if bpp == 4 {
            target.copy_from_slice(&source[1..]);
        } else {
            for (rgb, rgba) in source[1..]
                .as_chunks::<3>()
                .0
                .iter()
                .zip(target.as_chunks_mut::<4>().0.iter_mut())
            {
                *rgba = [rgb[0], rgb[1], rgb[2], 255];
            }
        }
    }
    parts[4] = started.elapsed();
    LAST.with_borrow_mut(|last| {
        *last = Some(Timing {
            parts,
            filters: counts,
            kernel,
        })
    });
    DecodedImageFrame {
        width,
        height,
        rgba,
        delay: Duration::ZERO,
    }
}
