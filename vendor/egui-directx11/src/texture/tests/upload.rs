use super::*;
use std::time::{Duration, Instant};

#[test]
#[cfg(feature = "render-verification")]
fn upload_timings_are_nested_and_do_not_retain_the_source() -> Result<()> {
    let (device, context) = device(D3D_DRIVER_TYPE_WARP)?.expect("WARP");
    let mut pool = TexturePool::new(&device);
    let image = Arc::new(egui::ColorImage::filled([65, 3], Color32::WHITE));
    let weak = Arc::downgrade(&image);
    let before = pool.verification_upload_times();
    pool.update(
        &context,
        TexturesDelta {
            set: vec![(
                TextureId::Managed(9),
                egui::epaint::ImageDelta::full(image, egui::TextureOptions::LINEAR),
            )],
            ..Default::default()
        },
    )?;
    let after = pool.verification_upload_times();
    let elapsed: [Duration; 3] = std::array::from_fn(|i| after[i] - before[i]);
    assert!(elapsed[0] > Duration::ZERO);
    assert!(elapsed[2] >= elapsed[0] + elapsed[1]);
    assert!(weak.upgrade().is_none());
    Ok(())
}

#[test]
#[ignore = "large generated-image upload comparison; use Release with hardware D3D11"]
#[allow(clippy::assertions_on_constants)]
fn large_texture_upload_compares_default_and_immutable() -> Result<()> {
    assert!(!cfg!(debug_assertions), "use the Release test binary");
    let (device, context) = device(D3D_DRIVER_TYPE_HARDWARE)?.expect("hardware device required");
    // Match representative dimensions, never load reference or desktop pixels.
    for [width, height] in [[4096, 2304], [8706, 5949]] {
        let image = Arc::new(egui::ColorImage::new(
            [width, height],
            (0..width * height)
                .map(|n| {
                    Color32::from_rgba_unmultiplied(
                        n as u8,
                        (n / width) as u8,
                        (n / 7) as u8,
                        (n / 13) as u8,
                    )
                })
                .collect(),
        ));
        let measure = |immutable: bool| -> Result<Duration> {
            IMMUTABLE_UPLOAD_PROBE.set(immutable);
            let started = Instant::now();
            let result = TexturePool::create_managed_texture(
                &device,
                ImageData::Color(image.clone()),
                egui::TextureOptions::LINEAR,
            );
            let elapsed = started.elapsed();
            IMMUTABLE_UPLOAD_PROBE.set(false);
            let Texture::Managed(texture) = result? else {
                unreachable!()
            };
            // Readback is outside the timer and completes GPU work before the
            // next sample. Compare all owned pixels, without printing buffers.
            let actual = readback(&device, &context, &texture.tex)?;
            assert!(actual == image.pixels, "uploaded pixels differ");
            assert_eq!(Arc::strong_count(&image), 1, "upload retains no CPU shadow");
            Ok(elapsed)
        };
        for immutable in [false, true] {
            let elapsed = measure(immutable)?;
            eprintln!(
                "TEXTURE_UPLOAD_FIRST width={width} height={height} immutable={immutable} elapsed_ms={:.3}; first upload for this size and mode on the reused owned device, not a cold driver/system claim",
                elapsed.as_secs_f64() * 1000.0,
            );
        }
        for (batch, immutable) in [false, true, true, false].into_iter().enumerate() {
            let mut times = (0..5)
                .map(|_| measure(immutable))
                .collect::<Result<Vec<_>>>()?;
            times.sort_unstable();
            eprintln!(
                "TEXTURE_UPLOAD width={width} height={height} immutable={immutable} batch={batch} count={} median_ms={:.3} min_ms={:.3} max_ms={:.3}; generated RGBA, whole native texture+SRV creation, completed full-pixel readback outside timer; no UI/Present, disk decode or process-memory claim",
                times.len(),
                times[times.len() / 2].as_secs_f64() * 1000.0,
                times[0].as_secs_f64() * 1000.0,
                times[times.len() - 1].as_secs_f64() * 1000.0,
            );
        }
        for trial in 0..5 {
            let started = Instant::now();
            let source = Arc::new((*image).clone());
            let prepare = started.elapsed();
            let started = Instant::now();
            let texture = TexturePool::create_managed_texture(
                &device,
                ImageData::Color(source.clone()),
                egui::TextureOptions::LINEAR,
            )?;
            let upload = started.elapsed();
            let started = Instant::now();
            drop(source);
            let release = started.elapsed();
            let Texture::Managed(texture) = texture else {
                unreachable!()
            };
            assert!(readback(&device, &context, &texture.tex)? == image.pixels);
            eprintln!(
                "TEXTURE_SOURCE_LIFETIME width={width} height={height} trial={trial} clone_ms={:.3} upload_ms={:.3} source_release_ms={:.3}; DEFAULT texture, separately timed owned ColorImage clone and final source drop; no production color-conversion timing",
                prepare.as_secs_f64() * 1000.0,
                upload.as_secs_f64() * 1000.0,
                release.as_secs_f64() * 1000.0,
            );
        }
    }
    Ok(())
}
