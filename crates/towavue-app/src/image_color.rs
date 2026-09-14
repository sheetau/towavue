use egui::Color32;
#[cfg(test)]
use std::time::{Duration, Instant};

#[cfg(test)]
thread_local! {
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
            pixels.extend(
                row.iter()
                    .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])),
            );
        }
    }
    let image = egui::ColorImage::new(size, pixels);
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
