use egui::Color32;
#[cfg(test)]
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "image_color/parallel_trial.rs"]
pub(crate) mod parallel_trial;

#[cfg(test)]
thread_local! {
    pub(crate) static COLOR_IMAGE_PARALLEL_TRIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static COLOR_IMAGE_LOOKUP_TRIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static COLOR_IMAGE_SSE2_TRIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static COLOR_IMAGE_CONVERSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static COLOR_IMAGE_CONVERSION_TIME: std::cell::Cell<Duration> = const { std::cell::Cell::new(Duration::ZERO) };
    pub(crate) static COLOR_IMAGE_CPU_TIME: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn color_image(frame: &towavue_runtime_windows::DecodedImageFrame) -> egui::ColorImage {
    #[cfg(test)]
    let cpu_started = COLOR_IMAGE_CPU_TIME.get().map(|_| {
        towavue_runtime_windows::verification_thread_cpu_time()
            .expect("color thread CPU accounting")
    });
    #[cfg(test)]
    let started = Instant::now();
    #[cfg(test)]
    COLOR_IMAGE_CONVERSIONS.set(COLOR_IMAGE_CONVERSIONS.get() + 1);
    let image = convert(frame);
    #[cfg(test)]
    COLOR_IMAGE_CONVERSION_TIME.set(COLOR_IMAGE_CONVERSION_TIME.get() + started.elapsed());
    #[cfg(test)]
    if let Some(cpu_started) = cpu_started {
        let elapsed = towavue_runtime_windows::verification_thread_cpu_time()
            .expect("color thread CPU accounting")
            - cpu_started;
        COLOR_IMAGE_CPU_TIME.set(Some(
            COLOR_IMAGE_CPU_TIME.get().expect("enabled accounting") + elapsed,
        ));
    }
    image
}

fn convert(frame: &towavue_runtime_windows::DecodedImageFrame) -> egui::ColorImage {
    #[cfg(test)]
    if COLOR_IMAGE_SSE2_TRIAL.get() {
        return towavue_runtime_windows::verification_color_image_sse2(frame);
    }
    #[cfg(test)]
    if COLOR_IMAGE_PARALLEL_TRIAL.get() {
        return parallel_trial::color_image(frame, true);
    }
    let size = [frame.width as usize, frame.height as usize];
    assert_eq!(size[0] * size[1] * 4, frame.rgba.len());
    let mut pixels = Vec::with_capacity(size[0] * size[1]);
    for row in frame.rgba.chunks_exact(size[0].max(1) * 4) {
        let row = row.as_chunks::<4>().0;
        // Opaque rows need no alpha conversion; mixed rows keep egui's exact rounding.
        // Reduce whole pixels in bounded blocks while retaining early exit for mixed rows.
        let alpha_mask = u32::from_ne_bytes([0, 0, 0, 255]);
        if row.chunks(32).all(|block| {
            block
                .iter()
                .fold(u32::MAX, |bits, pixel| bits & u32::from_ne_bytes(*pixel))
                & alpha_mask
                == alpha_mask
        }) {
            pixels.extend(
                row.iter()
                    .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])),
            );
        } else {
            #[cfg(test)]
            if COLOR_IMAGE_LOOKUP_TRIAL.get() {
                pixels.extend(
                    row.iter()
                        .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])),
                );
                continue;
            }
            pixels.extend(row.iter().copied().map(premultiplied_color));
        }
    }
    egui::ColorImage::new(size, pixels)
}

fn premultiplied_color([r, g, b, a]: [u8; 4]) -> Color32 {
    // Rounded component * alpha / 255 matches egui, including transparent hidden RGB.
    // Bounded u16 arithmetic avoids per-pixel table lookups and initialization checks.
    let channel = |value: u8| {
        let product = u16::from(value) * u16::from(a) + 128;
        ((product + (product >> 8)) >> 8) as u8
    };
    Color32::from_rgba_premultiplied(channel(r), channel(g), channel(b), a)
}

#[test]
fn integer_premultiplication_matches_every_egui_component_and_alpha() {
    for alpha in 0_u8..=255 {
        for value in 0_u8..=255 {
            let pixel = [value, 255 - value, value.wrapping_mul(73), alpha];
            assert_eq!(
                premultiplied_color(pixel),
                Color32::from_rgba_unmultiplied(pixel[0], pixel[1], pixel[2], pixel[3]),
                "component={value}, alpha={alpha}"
            );
        }
    }
}

#[test]
fn parallel_trial_uses_the_instrumented_entry_without_changing_pixels() {
    let before = COLOR_IMAGE_CONVERSIONS.get();
    let mut calls = 0;
    assert!(!COLOR_IMAGE_PARALLEL_TRIAL.get());
    assert!(!COLOR_IMAGE_LOOKUP_TRIAL.get());
    assert!(!COLOR_IMAGE_SSE2_TRIAL.get());
    for width in [0, 1, 31, 32, 33] {
        for height in [0, 1, 3] {
            let frame = towavue_runtime_windows::DecodedImageFrame {
                width,
                height,
                rgba: (0..width * height)
                    .flat_map(|n| [n as u8, (n / 3) as u8, (n / 7) as u8, n as u8])
                    .collect(),
                delay: Duration::ZERO,
            };
            let expected = egui::ColorImage::from_rgba_unmultiplied(
                [width as usize, height as usize],
                &frame.rgba,
            );
            for (parallel, lookup, sse2) in [
                (false, false, false),
                (false, true, false),
                (true, false, false),
                (false, false, true),
            ] {
                COLOR_IMAGE_PARALLEL_TRIAL.set(parallel);
                COLOR_IMAGE_LOOKUP_TRIAL.set(lookup);
                COLOR_IMAGE_SSE2_TRIAL.set(sse2);
                let result = color_image(&frame);
                COLOR_IMAGE_PARALLEL_TRIAL.set(false);
                COLOR_IMAGE_LOOKUP_TRIAL.set(false);
                COLOR_IMAGE_SSE2_TRIAL.set(false);
                assert!(result == expected, "entry pixels differ");
                calls += 1;
            }
        }
    }
    assert_eq!(COLOR_IMAGE_CONVERSIONS.get() - before, calls);
}
