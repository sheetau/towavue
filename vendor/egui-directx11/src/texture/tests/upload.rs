use super::*;
use std::time::{Duration, Instant};

#[test]
#[ignore = "same-size replacement/reuse comparison; use Release with hardware D3D11"]
#[allow(clippy::assertions_on_constants)]
fn repeated_texture_upload_compares_replacement_and_reuse() -> Result<()> {
    use windows::core::BOOL;

    assert!(!cfg!(debug_assertions), "use Release");
    let (device, context) = device_with_flags(
        D3D_DRIVER_TYPE_HARDWARE,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
        true,
    )?
    .expect("hardware device required");
    let mut query = None;
    // One test-owned immediate context; queries are reused only after completion.
    unsafe {
        device.CreateQuery(
            &D3D11_QUERY_DESC {
                Query: D3D11_QUERY_EVENT,
                MiscFlags: 0,
            },
            Some(&mut query),
        )?;
    }
    let query = query.expect("event query");
    for [width, height] in [[1919, 1079], [4096, 2304], [8706, 5949]] {
        let frames: Vec<_> = (0..2)
            .map(|phase| {
                Arc::new(egui::ColorImage::new(
                    [width, height],
                    (0..width * height)
                        .map(|n| {
                            Color32::from_rgba_unmultiplied(
                                (n + phase * 71) as u8,
                                (n / width + phase * 43) as u8,
                                (n / 7 + phase * 113) as u8,
                                (n / 13 + phase * 19) as u8,
                            )
                        })
                        .collect(),
                ))
            })
            .collect();
        let mut texture = TexturePool::create_managed_texture(
            &device,
            ImageData::Color(frames[0].clone()),
            egui::TextureOptions::LINEAR,
        )?;
        let Texture::Managed(initial) = &texture else {
            unreachable!()
        };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        let mut consumer = None;
        // This separate same-sized GPU resource captures a prior queued reader's
        // pixels; it never aliases the replacement and is not a production buffer.
        unsafe {
            initial.tex.GetDesc(&mut desc);
            device.CreateTexture2D(&desc, None, Some(&mut consumer))?;
        }
        let consumer = consumer.expect("queued reader destination");
        let mut previous = 0;
        let mut measure = |reuse: bool, queued_read: bool| -> Result<(Duration, Duration)> {
            let next = 1 - previous;
            let Texture::Managed(old) = &texture else {
                unreachable!()
            };
            if queued_read {
                // Queue GPU consumption immediately before the CPU update. D3D must
                // preserve that read even when UpdateSubresource reuses the resource.
                unsafe {
                    context.CopyResource(&consumer, &old.tex);
                }
            }
            let started = Instant::now();
            if reuse {
                TexturePool::update_partial(
                    &context,
                    &mut texture,
                    ImageData::Color(frames[next].clone()),
                    [0, 0],
                )?;
            } else {
                texture = TexturePool::create_managed_texture(
                    &device,
                    ImageData::Color(frames[next].clone()),
                    egui::TextureOptions::LINEAR,
                )?;
            }
            let submission = started.elapsed();
            let mut complete = BOOL(0);
            let deadline = Instant::now() + Duration::from_secs(10);
            unsafe {
                context.End(&query);
                context.Flush();
                while !complete.as_bool() {
                    context.GetData(
                        &query,
                        Some((&mut complete as *mut BOOL).cast()),
                        mem::size_of::<BOOL>() as u32,
                        0,
                    )?;
                    assert!(Instant::now() < deadline, "GPU completion deadline");
                    if !complete.as_bool() {
                        std::thread::yield_now();
                    }
                }
            }
            let completion = started.elapsed();
            let Texture::Managed(current) = &texture else {
                unreachable!()
            };
            assert!(
                readback(&device, &context, &current.tex)? == frames[next].pixels,
                "new pixels match exactly"
            );
            if queued_read {
                assert!(
                    readback(&device, &context, &consumer)? == frames[previous].pixels,
                    "queued reader retains prior pixels"
                );
            }
            assert!(
                frames.iter().all(|frame| Arc::strong_count(frame) == 1),
                "no retained CPU shadow"
            );
            previous = next;
            Ok((submission, completion))
        };
        for queued_read in [false, true] {
            for reuse in [false, true] {
                measure(reuse, queued_read)?;
            }
            for (batch, reuse) in [false, true, true, false].into_iter().enumerate() {
                let samples = (0..5)
                    .map(|_| measure(reuse, queued_read))
                    .collect::<Result<Vec<_>>>()?;
                let mut submission: Vec<_> = samples.iter().map(|sample| sample.0).collect();
                let mut completion: Vec<_> = samples.iter().map(|sample| sample.1).collect();
                submission.sort_unstable();
                completion.sort_unstable();
                eprintln!(
                    "TEXTURE_REUSE width={width} height={height} queued_read={queued_read} reuse={reuse} batch={batch} submission_ms={:.3} completion_ms={:.3}; five generated-frame samples, app device flags/protection; prior queued GPU copy excluded from submission but included in completion; exact current/prior readback and source checks outside timing; no window/animation/navigation or memory measurement",
                    submission[2].as_secs_f64() * 1000.0,
                    completion[2].as_secs_f64() * 1000.0
                );
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "generated upload submission/completion comparison; use Release with hardware D3D11"]
#[allow(clippy::assertions_on_constants)]
fn large_texture_upload_compares_initial_data_and_update() -> Result<()> {
    use windows::core::BOOL;

    assert!(!cfg!(debug_assertions), "use the Release test binary");
    let (device, context) = device_with_flags(
        D3D_DRIVER_TYPE_HARDWARE,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
        true,
    )?
    .expect("hardware device required");
    let mut query = None;
    // This same-device event is reused only after each sample completes.
    unsafe {
        device.CreateQuery(
            &D3D11_QUERY_DESC {
                Query: D3D11_QUERY_EVENT,
                MiscFlags: 0,
            },
            Some(&mut query),
        )?;
    }
    let query = query.expect("completion query");
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
        let measure = |update: bool| -> Result<(Duration, Duration)> {
            let started = Instant::now();
            let texture = if update {
                // Identical DEFAULT/SRV descriptor to production, but upload through
                // the owned, serialized immediate context after allocating the texture.
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: width as u32,
                    Height: height as u32,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    ..Default::default()
                };
                let mut texture = None;
                let mut srv = None;
                // The generated RGBA buffer has exactly width * height pixels and
                // stays alive through UpdateSubresource, which snapshots its source.
                unsafe {
                    device.CreateTexture2D(&desc, None, Some(&mut texture))?;
                    let texture = texture.as_ref().expect("allocated texture");
                    context.UpdateSubresource(
                        texture,
                        0,
                        None,
                        image.pixels.as_ptr().cast(),
                        (width * mem::size_of::<Color32>()) as u32,
                        0,
                    );
                    device.CreateShaderResourceView(texture, None, Some(&mut srv))?;
                }
                ManagedTexture {
                    tex: texture.expect("texture"),
                    srv: srv.expect("view"),
                    size: [width, height],
                    options: egui::TextureOptions::LINEAR,
                }
            } else {
                let Texture::Managed(texture) = TexturePool::create_managed_texture(
                    &device,
                    ImageData::Color(image.clone()),
                    egui::TextureOptions::LINEAR,
                )?
                else {
                    unreachable!()
                };
                texture
            };
            let submitted = started.elapsed();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut complete = BOOL(0);
            // Flush and wait for GPU completion, not merely a successful/S_FALSE HRESULT.
            // The query output is borrowed for each call; no production UI waits are added.
            unsafe {
                context.End(&query);
                context.Flush();
                while !complete.as_bool() {
                    context.GetData(
                        &query,
                        Some((&mut complete as *mut BOOL).cast()),
                        mem::size_of::<BOOL>() as u32,
                        0,
                    )?;
                    assert!(Instant::now() < deadline, "upload completion timed out");
                    if !complete.as_bool() {
                        std::thread::yield_now();
                    }
                }
            }
            let completed = started.elapsed();
            // All-pixel readback and equality stay outside both measured intervals.
            assert!(
                readback(&device, &context, &texture.tex)? == image.pixels,
                "uploaded pixels differ, update={update}"
            );
            assert_eq!(Arc::strong_count(&image), 1, "no retained CPU shadow");
            Ok((submitted, completed))
        };
        for update in [false, true] {
            measure(update)?;
        }
        for (batch, update) in [false, true, true, false].into_iter().enumerate() {
            let samples = (0..5)
                .map(|_| measure(update))
                .collect::<Result<Vec<_>>>()?;
            let mut submitted = samples.iter().map(|sample| sample.0).collect::<Vec<_>>();
            let mut completed = samples.iter().map(|sample| sample.1).collect::<Vec<_>>();
            submitted.sort_unstable();
            completed.sort_unstable();
            eprintln!(
                "TEXTURE_UPDATE width={width} height={height} update={update} batch={batch} submission_median_ms={:.3} completion_median_ms={:.3}; five samples, app device flags/protection, generated RGBA, GPU event completion includes submission, full equality/readback excluded; no swap chain, media load, cold-driver or memory claim",
                submitted[2].as_secs_f64() * 1000.0,
                completed[2].as_secs_f64() * 1000.0
            );
        }
    }
    Ok(())
}

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
    let app_device = std::env::var_os("TOWAVUE_UPLOAD_APP_DEVICE").is_some();
    let (device, context) = if app_device {
        device_with_flags(
            D3D_DRIVER_TYPE_HARDWARE,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            true,
        )?
    } else {
        device(D3D_DRIVER_TYPE_HARDWARE)?
    }
    .expect("hardware device required");
    eprintln!(
        "TEXTURE_DEVICE app_settings={app_device}; app settings mean BGRA+VIDEO support and multithread protection, not a swap chain or media workload"
    );
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
