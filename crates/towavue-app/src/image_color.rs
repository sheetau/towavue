#[cfg(test)]
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "image_color/parallel_trial.rs"]
pub(crate) mod parallel_trial;
#[cfg(test)]
#[path = "image_color/scalar_trial.rs"]
pub(crate) mod scalar_trial;

#[cfg(test)]
thread_local! {
    pub(crate) static COLOR_IMAGE_PARALLEL_TRIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static COLOR_IMAGE_LOOKUP_TRIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static COLOR_IMAGE_SCALAR_TRIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static COLOR_IMAGE_CONVERSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static COLOR_IMAGE_CONVERSION_TIME: std::cell::Cell<Duration> = const { std::cell::Cell::new(Duration::ZERO) };
    pub(crate) static COLOR_IMAGE_CPU_TIME: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
    pub(crate) static PACKED_PREVIEW_COLORS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub(crate) fn preview_color_image(
    image: &towavue_runtime_windows::PreviewImage,
) -> egui::ColorImage {
    let size = [image.width as usize, image.height as usize];
    #[cfg(test)]
    if !PACKED_PREVIEW_COLORS.get() {
        return egui::ColorImage::from_rgba_unmultiplied(size, &image.rgba);
    }
    towavue_runtime_windows::premultiplied_rgba_image(size, &image.rgba)
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
    {
        if COLOR_IMAGE_PARALLEL_TRIAL.get() {
            return parallel_trial::color_image(frame, true);
        }
        if COLOR_IMAGE_LOOKUP_TRIAL.get() || COLOR_IMAGE_SCALAR_TRIAL.get() {
            return scalar_trial::color_image(frame, COLOR_IMAGE_LOOKUP_TRIAL.get());
        }
    }
    towavue_runtime_windows::premultiplied_color_image(frame)
}

#[test]
fn preview_colors_preserve_dimensions_alpha_and_original_bytes() {
    for (width, height) in [(0, 0), (1, 1), (31, 3), (240, 160), (960, 640)] {
        let image = towavue_runtime_windows::PreviewImage {
            width,
            height,
            rgba: (0..width * height)
                .flat_map(|n| [n as u8, (n / 3) as u8, (n / 7) as u8, n as u8])
                .collect::<Vec<_>>()
                .into(),
        };
        let original = image.rgba.as_ref().clone();
        let expected =
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &original);
        for packed in [false, true] {
            PACKED_PREVIEW_COLORS.set(packed);
            assert!(
                preview_color_image(&image) == expected,
                "exact preview colors"
            );
            assert!(
                image.rgba.as_slice() == original,
                "borrowed input remains unchanged"
            );
        }
    }
    PACKED_PREVIEW_COLORS.set(true);
}

#[test]
fn parallel_trial_uses_the_instrumented_entry_without_changing_pixels() {
    let before = COLOR_IMAGE_CONVERSIONS.get();
    let mut calls = 0;
    assert!(!COLOR_IMAGE_PARALLEL_TRIAL.get());
    assert!(!COLOR_IMAGE_LOOKUP_TRIAL.get());
    assert!(!COLOR_IMAGE_SCALAR_TRIAL.get());
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
            for (parallel, lookup, scalar) in [
                (false, false, false),
                (false, true, false),
                (true, false, false),
                (false, false, true),
            ] {
                COLOR_IMAGE_PARALLEL_TRIAL.set(parallel);
                COLOR_IMAGE_LOOKUP_TRIAL.set(lookup);
                COLOR_IMAGE_SCALAR_TRIAL.set(scalar);
                let result = color_image(&frame);
                COLOR_IMAGE_PARALLEL_TRIAL.set(false);
                COLOR_IMAGE_LOOKUP_TRIAL.set(false);
                COLOR_IMAGE_SCALAR_TRIAL.set(false);
                assert!(result == expected, "entry pixels differ");
                calls += 1;
            }
        }
    }
    assert_eq!(COLOR_IMAGE_CONVERSIONS.get() - before, calls);
}
