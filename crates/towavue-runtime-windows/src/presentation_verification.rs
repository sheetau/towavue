//! Opt-in debugger boundary for actual-app image submission, not scanout timing.

use std::{cell::RefCell, time::Instant};

thread_local! {
    // The app calls these markers on its UI thread. No image data is retained.
    static STAGES: RefCell<([Option<Instant>; 24], bool)> = const { RefCell::new(([None; 24], false)) };
}

/// Stages: 0 = main, 1 = request, 2 = accepted original, 3 = prepared texture.
/// Startup: 10 = window start, 11 = caption ready, 12 = renderer ready,
/// 13 = fonts installed, 14 = UI/accessibility ready and window shown.
/// Renderer: 20 = device start, 21 = device ready, 22 = swap chain ready,
/// 23 = shader resources ready. Native self-timings include debugger pauses if attached.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn towavue_presentation_stage(stage: u32) {
    STAGES.with_borrow_mut(|(stages, _)| {
        if let Some(slot) = stages.get_mut(stage as usize) {
            *slot = Some(Instant::now());
        }
    });
    std::hint::black_box(stage);
}

// SAFETY: this uniquely named symbol exists only in verification builds. It has
// no pointer arguments or external resources and is not a supported library ABI.
// Display dimensions describe the unedited initial image before viewport clipping.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn towavue_original_submitted(
    width: u32,
    height: u32,
    client_width: u32,
    client_height: u32,
    display_width: f32,
    display_height: f32,
) {
    let submitted = Instant::now();
    STAGES.with_borrow_mut(|(stages, reported)| {
        if *reported {
            return;
        }
        *reported = true;
        if let Some(start) = stages[0] {
            for (stage, time) in stages.iter().enumerate() {
                if let Some(time) = time {
                    println!("PRESENT_STAGE id={stage} elapsed_ms={:.3}", time.duration_since(start).as_secs_f64() * 1000.0);
                }
            }
            println!("ORIGINAL_SUBMITTED size={width}x{height} client={client_width}x{client_height} display={display_width:.3}x{display_height:.3} elapsed_ms={:.3}", submitted.duration_since(start).as_secs_f64() * 1000.0);
        }
    });
    std::hint::black_box((
        width,
        height,
        client_width,
        client_height,
        display_width,
        display_height,
    ));
}
