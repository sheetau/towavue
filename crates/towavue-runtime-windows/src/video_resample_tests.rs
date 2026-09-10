use super::*;
use towavue_core::{ResampleFilter, VideoResize};

const FILTERS: [(ResampleFilter, &str); 4] = [
    (ResampleFilter::Nearest, "neighbor"),
    (ResampleFilter::Bilinear, "bilinear"),
    (ResampleFilter::Bicubic, "bicubic"),
    (ResampleFilter::Lanczos, "lanczos"),
];

#[test]
#[ignore = "requires a hardware D3D11 adapter; windowless completion-latency measurement"]
fn hardware_video_resize_completion_latency_1080p_and_4k() {
    use std::time::{Duration, Instant};
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11CreateDevice,
    };
    let mut device = None;
    // This opt-in test owns a windowless hardware device, never opens media/audio,
    // and only uses its immediate context on the calling thread.
    if let Err(error) = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
    } {
        eprintln!("SKIP hardware resize: hardware D3D11 device unavailable: {error}");
        return;
    }
    let device = device.expect("device");
    let dxgi: IDXGIDevice = device.cast().expect("DXGI");
    // The owned adapter is queried synchronously; the copied descriptor has no pointers.
    let luid = unsafe {
        dxgi.GetAdapter()
            .expect("adapter")
            .GetDesc()
            .expect("description")
            .AdapterLuid
    };
    let mut gpu = Gpu::with_graphics(GraphicsDevice {
        device,
        adapter_luid: super::super::super::AdapterLuid {
            low_part: luid.LowPart,
            high_part: luid.HighPart,
        },
    });
    let mut event = None;
    // The owned event query contains only completion state, never media pixels.
    unsafe {
        gpu.graphics
            .device
            .CreateQuery(
                &D3D11_QUERY_DESC {
                    Query: D3D11_QUERY_EVENT,
                    MiscFlags: 0,
                },
                Some(&mut event),
            )
            .expect("completion query");
    }
    let event = event.expect("query");
    let finish = |context: &ID3D11DeviceContext| {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut complete = BOOL(0);
        // End/Flush serialize the prior draw commands. GetData's BOOL is borrowed
        // only for the call; waiting for true also distinguishes S_FALSE from S_OK.
        unsafe {
            context.End(&event);
            context.Flush();
            while !complete.as_bool() {
                context
                    .GetData(&event, Some((&mut complete as *mut BOOL).cast()), 4, 0)
                    .expect("completion state");
                assert!(
                    Instant::now() < deadline,
                    "hardware draw completion timed out"
                );
                if !complete.as_bool() {
                    std::thread::yield_now();
                }
            }
        }
    };
    for (input, output) in [
        ((1920, 1080), (1280, 720)),
        ((1920, 1080), (3840, 2160)),
        ((3840, 2160), (1920, 1080)),
        ((3840, 2160), (240, 136)),
    ] {
        let rgba = pixels(input);
        let source = gpu.upload(input, &rgba);
        for (filter, flag) in FILTERS {
            let p = plan(
                input,
                1.0,
                &[Edit::ResizeVideo(
                    VideoResize::new(output, filter, input, 1.0).expect("resize"),
                )],
            );
            let mut timings = Vec::new();
            for _ in 0..13 {
                let start = Instant::now();
                gpu.raster
                    .draw(
                        &gpu.graphics.device,
                        &gpu.context,
                        &gpu.blitter,
                        &source,
                        &p,
                    )
                    .expect("hardware resize");
                finish(&gpu.context);
                timings.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            let cold = timings.remove(0);
            timings.sort_by(f64::total_cmp);
            eprintln!(
                "PASS hardware resize {:?} {input:?}->{output:?} {filter:?}: cold {cold:.3} ms, warm median {:.3}/max {:.3} ms (CPU submission through GPU completion, 12 frames, no pixel readback)",
                gpu.graphics.adapter_luid(),
                timings[6],
                timings[11]
            );
            // Quality readback is outside the measured section and exists only in tests.
            let actual = gpu.draw(&source, &p);
            let expected = reference(
                input,
                &rgba,
                &format!(
                    "format=gbrp,scale={}:{}:flags={flag}+full_chroma_inp,format=gbrp",
                    output.0, output.1
                ),
            );
            assert_eq!(actual.len(), expected.len());
            let error = actual
                .iter()
                .zip(expected)
                .map(|(a, b)| a.abs_diff(b))
                .max()
                .expect("pixels");
            assert!(
                error
                    <= if filter == ResampleFilter::Nearest {
                        0
                    } else {
                        3
                    },
                "hardware {input:?}->{output:?} {filter:?}: {error}"
            );
            eprintln!("PASS hardware resize reference: maximum channel error {error}");
        }
    }
}

#[test]
fn resize_coefficients_are_normalized_bounded_and_widen_when_shrinking() {
    for source in [1, 2, 3, 7, 64, 257, 16384] {
        for target in [16, 18, 64, 96, 1024, 16384] {
            for (filter, _) in FILTERS {
                let spec = resample::FilterSpec {
                    source,
                    target,
                    filter,
                };
                let table = spec.coefficients();
                assert_eq!(table.len() as u64 * 8, spec.bytes());
                for row in table.chunks_exact(spec.taps() as usize) {
                    let sum: f32 = row.iter().map(|entry| entry[1]).sum();
                    assert!((sum - 1.0).abs() < 0.00002, "{spec:?} sum {sum}");
                    assert!(row.iter().all(|entry| entry[0] >= 0.0
                        && entry[0] < source as f32
                        && entry[1].is_finite()));
                }
            }
        }
    }
    let down = resample::FilterSpec {
        source: 4096,
        target: 16,
        filter: ResampleFilter::Lanczos,
    };
    assert!(down.taps() > 1500);
    let up = resample::FilterSpec {
        source: 64,
        target: 96,
        filter: ResampleFilter::Bicubic,
    };
    assert!(up.coefficients().iter().any(|entry| entry[1] < 0.0));
}

#[test]
fn resize_plan_budgets_float_intermediates_coefficients_and_reuses_specs() {
    let mut edits = Vec::new();
    for _ in 0..1000 {
        edits.push(Edit::ResizeVideo(
            VideoResize::new((128, 64), ResampleFilter::Lanczos, (64, 32), 1.0).expect("up"),
        ));
        edits.push(Edit::ResizeVideo(
            VideoResize::new((64, 32), ResampleFilter::Lanczos, (128, 64), 1.0).expect("down"),
        ));
    }
    let p = plan((64, 32), 1.0, &edits);
    assert_eq!(p.slots.len(), 4);
    assert_eq!(p.filters.len(), 4);
    assert!(p.slots.iter().any(|slot| slot.floating_point));
    assert!(
        p.stages
            .windows(2)
            .all(|stages| stages[0].slot != stages[1].slot)
    );
    let huge = VideoResize::new((16384, 16), ResampleFilter::Lanczos, (16, 16384), 1.0)
        .expect("huge intermediate");
    assert!(matches!(
        Plan::new(
            (16, 16384),
            1.0,
            crate::VideoOrientation::default(),
            &[Edit::ResizeVideo(huge)],
            16384
        ),
        Err(RenderError::VideoEditBudget)
    ));
    let almost = VideoResize::new((8192, 4096), ResampleFilter::Lanczos, (8192, 8192), 1.0)
        .expect("float exceeds budget");
    assert!(matches!(
        Plan::new(
            (8192, 8192),
            1.0,
            crate::VideoOrientation::default(),
            &[Edit::ResizeVideo(almost)],
            16384
        ),
        Err(RenderError::VideoEditBudget)
    ));
    let wrong =
        VideoResize::new((96, 48), ResampleFilter::Bilinear, (64, 48), 2.0).expect("wrong SAR");
    assert!(matches!(
        Plan::new(
            (64, 48),
            1.0,
            crate::VideoOrientation::default(),
            &[Edit::ResizeVideo(wrong)],
            16384
        ),
        Err(RenderError::InvalidVideoEdit)
    ));
}

#[test]
fn warp_video_resize_four_filters_match_export_up_down_odd_and_small_sources() {
    let mut gpu = Gpu::new();
    let mut errors = Vec::new();
    for (input, output) in [
        ((64, 48), (96, 72)),
        ((64, 48), (30, 18)),
        ((65, 49), (98, 50)),
        ((64, 48), (128, 16)),
        ((257, 129), (16, 16)),
        ((7, 5), (16, 16)),
        ((2, 2), (16, 16)),
        ((1, 1), (16, 16)),
        ((17, 19), (16, 16)),
    ] {
        let rgba = pixels(input);
        let source = gpu.upload(input, &rgba);
        for (filter, flag) in FILTERS {
            let resize = VideoResize::new(output, filter, input, 1.5).expect("resize");
            let p = plan(input, 1.5, &[Edit::ResizeVideo(resize)]);
            let actual = gpu.draw(&source, &p);
            let expected = reference(
                input,
                &rgba,
                &format!(
                    "format=gbrp,scale={}:{}:flags={flag}+full_chroma_inp,format=gbrp,setsar=1",
                    output.0, output.1
                ),
            );
            assert_eq!(actual.len(), expected.len());
            let maximum = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .expect("pixels");
            let mean = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| f64::from(a.abs_diff(*b)))
                .sum::<f64>()
                / actual.len() as f64;
            if maximum
                > if filter == ResampleFilter::Nearest {
                    0
                } else {
                    3
                }
                || mean > 0.8
            {
                errors.push(format!(
                    "{input:?}->{output:?} {filter:?}: max {maximum}, mean {mean:.4}"
                ));
            }
            assert_eq!((p.size, p.pixel_aspect), (output, 1.0));
            assert_eq!(gpu.read(&source, input), rgba);
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn warp_video_resize_reuses_coefficients_and_does_not_retain_old_frame_pixels() {
    let mut gpu = Gpu::new();
    let input = (64, 48);
    let p = plan(
        input,
        1.0,
        &[Edit::ResizeVideo(
            VideoResize::new((32, 24), ResampleFilter::Lanczos, input, 1.0).expect("resize"),
        )],
    );
    let source = gpu.upload(input, &pixels(input));
    gpu.draw(&source, &p);
    let textures: Vec<_> = gpu
        .raster
        .surfaces
        .iter()
        .map(|surface| surface.texture.as_raw())
        .collect();
    let coefficients: Vec<_> = gpu
        .raster
        .filters
        .iter()
        .map(|filter| filter.source.as_raw())
        .collect();
    let black = [0, 0, 0, 255].repeat((input.0 * input.1) as usize);
    let source = gpu.upload(input, &black);
    assert_eq!(
        gpu.draw(&source, &p),
        [0, 0, 0, 255].repeat((p.size.0 * p.size.1) as usize)
    );
    assert_eq!(
        textures,
        gpu.raster
            .surfaces
            .iter()
            .map(|surface| surface.texture.as_raw())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        coefficients,
        gpu.raster
            .filters
            .iter()
            .map(|filter| filter.source.as_raw())
            .collect::<Vec<_>>()
    );
    // Same dimensions do not make coefficients interchangeable between filters.
    let colored = pixels(input);
    let source = gpu.upload(input, &colored);
    for (filter, flag) in FILTERS {
        let changed = plan(
            input,
            1.0,
            &[Edit::ResizeVideo(
                VideoResize::new((32, 24), filter, input, 1.0).expect("changed filter"),
            )],
        );
        let actual = gpu.draw(&source, &changed);
        let expected = reference(
            input,
            &colored,
            &format!("format=gbrp,scale=32:24:flags={flag}+full_chroma_inp,format=gbrp"),
        );
        assert!(actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3));
        assert!(
            gpu.raster
                .filters
                .iter()
                .all(|table| table.specification.filter == filter)
        );
    }
    gpu.draw(&source, &plan(input, 1.0, &[]));
    assert!(gpu.raster.surfaces.is_empty() && gpu.raster.filters.is_empty());
}

#[test]
fn warp_video_resize_preserves_signed_filter_lobes_on_saturated_patterns() {
    let mut gpu = Gpu::new();
    let size = (64, 48);
    for pattern in 0..3 {
        let rgba: Vec<_> = (0..size.1)
            .flat_map(|y| {
                (0..size.0).flat_map(move |x| {
                    let on = match pattern {
                        0 => (x / 3 + y / 3) % 2 == 0,
                        1 => x == 32 && y == 24,
                        _ => x >= 32 && y >= 24,
                    };
                    if on {
                        [255, 0, 255, 255]
                    } else {
                        [0, 255, 0, 255]
                    }
                })
            })
            .collect();
        let source = gpu.upload(size, &rgba);
        for (filter, flag) in FILTERS {
            for output in [(98, 74), (30, 18)] {
                let p = plan(
                    size,
                    1.0,
                    &[Edit::ResizeVideo(
                        VideoResize::new(output, filter, size, 1.0).expect("resize"),
                    )],
                );
                let actual = gpu.draw(&source, &p);
                let expected = reference(
                    size,
                    &rgba,
                    &format!(
                        "format=gbrp,scale={}:{}:flags={flag}+full_chroma_inp,format=gbrp",
                        output.0, output.1
                    ),
                );
                let error = actual
                    .iter()
                    .zip(expected)
                    .map(|(a, b)| a.abs_diff(b))
                    .max()
                    .expect("pixels");
                assert!(
                    error <= 3,
                    "pattern {pattern} {filter:?} {output:?}: {error}"
                );
            }
        }
    }
}

#[test]
fn warp_video_resize_composes_with_metadata_crop_rotation_and_later_resize() {
    let mut gpu = Gpu::new();
    let size = (64, 48);
    let rgba = pixels(size);
    let source = gpu.upload(size, &rgba);
    let matrix = [0_i32, 65536, 0, -65536, 0, 0, 0, 0, 1 << 30];
    let bytes: Vec<_> = matrix.into_iter().flat_map(i32::to_ne_bytes).collect();
    let orientation = crate::VideoOrientation::from_bytes(Some(&bytes)).expect("orientation");
    for (filter, flag) in FILTERS {
        let rotation = VideoRotation::new(317, (48, 32), 1.0).expect("rotation");
        let edits = [
            Edit::ResizeVideo(
                VideoResize::new((48, 64), filter, (48, 64), 0.5).expect("SAR resize"),
            ),
            Edit::Crop(PixelCrop {
                x: 4,
                y: 6,
                width: 32,
                height: 48,
            }),
            Edit::RotateClockwise,
            Edit::RotateVideo(rotation),
            Edit::ResizeVideo(
                VideoResize::new((30, 18), filter, rotation.size(), 1.0).expect("later resize"),
            ),
            Edit::FlipVertical,
        ];
        let p = Plan::new(size, 2.0, orientation, &edits, 16384).expect("composed plan");
        let actual = gpu.draw(&source, &p);
        let expected = reference(
            size,
            &rgba,
            &format!(
                "transpose=clock,format=gbrp,scale=48:64:flags={flag}+full_chroma_inp,format=gbrp,setsar=1,crop=32:48:4:6:exact=1,transpose=clock,rotate=317*PI/1800:ow={}:oh={}:c=black:bilinear=1,pad={}:{}:0:0:color=black,scale=30:18:flags={flag}+full_chroma_inp,format=gbrp,setsar=1,vflip",
                rotation.raster_size().0,
                rotation.raster_size().1,
                rotation.size().0,
                rotation.size().1
            ),
        );
        let error = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .expect("pixels");
        assert_eq!(actual.len(), expected.len());
        assert!(error <= 3, "composed {filter:?}: {error}");
        assert_eq!((p.size, p.pixel_aspect), ((30, 18), 1.0));
        assert_eq!(gpu.read(&source, size), rgba);
    }
}
