//! First-call decoder boundary for a process-scoped debugger comparison.
//! No window, preview cache, warm-up decode or image output is created.
//! Optional repetitions support process-scoped sampling, not first-call timing.

use std::{hint::black_box, path::PathBuf};

// Stable debugger boundaries, not a public library ABI. The end marker borrows
// pixels only while main retains the decoded frame; it never dereferences them.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn towavue_decode_begin() {
    black_box(0);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn towavue_decode_end(width: u32, height: u32, rgba: *const u8, bytes: usize) {
    black_box((width, height, rgba, bytes));
}

#[allow(clippy::assertions_on_constants)]
fn main() {
    assert!(!cfg!(debug_assertions), "use --release");
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("explicit static image path"),
    );
    let stamp = || {
        let metadata = std::fs::metadata(&path).expect("source metadata");
        (
            metadata.len(),
            metadata.modified().expect("source timestamp"),
        )
    };
    let before = stamp();
    let repeats = std::env::var("TOWAVUE_DECODE_REPEATS")
        .map(|value| value.parse::<usize>().expect("integer repeat count"))
        .unwrap_or(1);
    assert!(
        (1..=1000).contains(&repeats),
        "repeat count must be 1..=1000"
    );
    let mut size = None;
    for _ in 0..repeats {
        towavue_decode_begin();
        let decoded = towavue_runtime_windows::decode_image(&path)
            .unwrap_or_else(|_| panic!("source decode failed"));
        assert_eq!(decoded.frames.len(), 1, "static source required");
        let frame = &decoded.frames[0];
        towavue_decode_end(
            frame.width,
            frame.height,
            frame.rgba.as_ptr(),
            frame.rgba.len(),
        );
        if let Some(previous) = size {
            assert_eq!(previous, (frame.width, frame.height), "dimensions changed");
        }
        size = Some((frame.width, frame.height));
    }
    assert_eq!(stamp(), before, "source changed");
    let (width, height) = size.expect("decoded source");
    println!("DECODE_CALL size={width}x{height} repeats={repeats} complete");
}
