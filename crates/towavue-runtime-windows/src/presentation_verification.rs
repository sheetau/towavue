//! Opt-in debugger boundary for actual-app image submission, not scanout timing.

use std::{
    cell::RefCell,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

// SAFETY: this uniquely named, aligned scalar exists only in verification builds.
// An owned-process observer reads only this word, not pointers or image pixels.
#[unsafe(no_mangle)]
pub static TOWAVUE_FIRST_ORIGINAL_SIZE: AtomicU64 = AtomicU64::new(0);

/// Publish the first validated original completion before texture preparation.
/// High/low 32 bits are width/height. This is not a rendering-completion marker.
pub fn towavue_original_ready((width, height): (u32, u32)) {
    let size = (u64::from(width) << 32) | u64::from(height);
    // No other memory is published through this diagnostic scalar.
    let _ =
        TOWAVUE_FIRST_ORIGINAL_SIZE.compare_exchange(0, size, Ordering::Relaxed, Ordering::Relaxed);
}

#[test]
fn original_ready_marker_keeps_the_first_complete_dimensions() {
    assert_eq!(TOWAVUE_FIRST_ORIGINAL_SIZE.load(Ordering::Relaxed), 0);
    towavue_original_ready((503, 317));
    towavue_original_ready((4096, 2304));
    assert_eq!(
        TOWAVUE_FIRST_ORIGINAL_SIZE.load(Ordering::Relaxed),
        (503u64 << 32) | 317
    );
}

thread_local! {
    // The app calls these markers on its UI thread. No image data is retained.
    static STAGES: RefCell<([Option<Instant>; 27], bool)> = const { RefCell::new(([None; 27], false)) };
}

/// Stages: 0 = main, 1 = request, 2 = accepted original, 3 = prepared texture.
/// Startup: 10 = window start, 11 = caption ready, 12 = renderer ready,
/// 13 = fonts installed, 14 = UI/accessibility ready and window shown.
/// Renderer: 20 = device start, 21 = device ready, 22 = swap chain ready,
/// 23 = shader resources ready. Native self-timings include debugger pauses if attached.
/// UI: 24 = style ready, 25 = egui-winit state ready, 26 = AccessKit initialized.
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

#[test]
fn startup_markers_retain_each_ui_boundary_and_ignore_unknown_ids() {
    STAGES.with_borrow_mut(|(stages, _)| *stages = [None; 27]);
    for stage in [24, 25, 26, u32::MAX] {
        towavue_presentation_stage(stage);
    }
    STAGES.with_borrow(|(stages, _)| {
        assert_eq!(stages.iter().filter(|time| time.is_some()).count(), 3);
        assert!(stages[24] <= stages[25] && stages[25] <= stages[26]);
    });
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
