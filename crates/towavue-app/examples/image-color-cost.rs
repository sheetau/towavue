//! Opt-in Release comparison using the app's actual color conversion and owned pixels.
#![forbid(unsafe_code)]

#[path = "../src/image_color.rs"]
mod image_color;

use std::time::{Duration, Instant};
use towavue_runtime_windows::DecodedImageFrame;

#[test]
#[ignore = "Release color benchmark with the same cfg(test) diagnostics as the app harness"]
fn measured_with_application_test_instrumentation() {
    main();
}

fn convert(frame: &DecodedImageFrame, strategy: usize) -> egui::ColorImage {
    // Strategy 1 is an experimental comparison, never the application path.
    let size = [frame.width as usize, frame.height as usize];
    match strategy {
        0 => image_color::color_image(frame),
        1 => {
            let alpha_mask = u32::from_ne_bytes([0, 0, 0, 255]);
            let opaque = |block: &[[u8; 4]]| {
                block
                    .iter()
                    .fold(u32::MAX, |bits, pixel| bits & u32::from_ne_bytes(*pixel))
                    & alpha_mask
                    == alpha_mask
            };
            let mut pixels = Vec::with_capacity(size[0] * size[1]);
            for row in frame.rgba.chunks_exact(size[0] * 4) {
                let row = row.as_chunks::<4>().0;
                if row.chunks(32).all(opaque) {
                    pixels.extend(
                        row.iter().map(|p| {
                            egui::Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])
                        }),
                    );
                } else {
                    for block in row.chunks(256) {
                        if opaque(block) {
                            pixels.extend(block.iter().map(|p| {
                                egui::Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])
                            }));
                        } else {
                            pixels.extend(block.iter().map(|p| {
                                egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])
                            }));
                        }
                    }
                }
            }
            egui::ColorImage::new(size, pixels)
        }
        2 => egui::ColorImage::from_rgba_unmultiplied(size, &frame.rgba),
        _ => unreachable!(),
    }
}

#[allow(clippy::assertions_on_constants)]
fn main() {
    assert!(!cfg!(debug_assertions), "run this example with --release");
    if let Some(path) = std::env::var_os("TOWAVUE_COLOR_REFERENCE_PATH") {
        let path = std::path::PathBuf::from(path);
        let stamp = || {
            let metadata = std::fs::metadata(&path).expect("source metadata");
            (metadata.len(), metadata.modified().expect("modified time"))
        };
        let before = stamp();
        let Ok(decoded) = towavue_runtime_windows::decode_image(&path) else {
            panic!("selected reference decode failed");
        };
        assert_eq!(decoded.frames.len(), 1, "static reference required");
        if std::env::var_os("TOWAVUE_COLOR_CONCURRENT_DECODE").is_some() {
            use std::sync::atomic::{AtomicBool, Ordering};
            let stop = AtomicBool::new(false);
            let (started, ready) = std::sync::mpsc::channel();
            std::thread::scope(|scope| {
                let worker = scope.spawn(|| {
                    let mut completed = 0;
                    started.send(()).expect("probe started");
                    while !stop.load(Ordering::Relaxed) {
                        let result = towavue_runtime_windows::decode_image(&path);
                        assert!(result.is_ok(), "concurrent reference decode failed");
                        completed += 1;
                    }
                    completed
                });
                ready.recv().expect("concurrent worker");
                // A failed pixel comparison must still stop/join the stress worker.
                let measured = std::panic::catch_unwind(|| {
                    measure(&decoded.frames[0], "read-only-reference-concurrent-decode");
                });
                stop.store(true, Ordering::Relaxed);
                let completed = worker.join().expect("decode worker");
                if let Err(payload) = measured {
                    std::panic::resume_unwind(payload);
                }
                assert!(completed > 0, "concurrent decode must execute");
                eprintln!(
                    "IMAGE_COLOR_CONCURRENT completed_decodes={completed}; one looping same-source CPU decoder, not the app's bounded neighbor sequence"
                );
            });
        } else {
            measure(&decoded.frames[0], "read-only-reference");
        }
        assert_eq!(before, stamp(), "source changed");
        return;
    }
    for (width, height) in [(4096, 2304), (8706, 5949)] {
        for alpha in ["opaque", "sparse", "dense"] {
            let rgba = (0..width * height)
                .flat_map(|n| {
                    [
                        n as u8,
                        (n / width) as u8,
                        (n / 7) as u8,
                        match alpha {
                            "sparse" if n % width == width - 1 => 128,
                            "dense" => (n / 13) as u8,
                            _ => 255,
                        },
                    ]
                })
                .collect();
            let frame = DecodedImageFrame {
                width,
                height,
                rgba,
                delay: Duration::ZERO,
            };
            measure(&frame, alpha);
        }
    }
}

fn measure(frame: &DecodedImageFrame, case: &str) {
    let started = Instant::now();
    let first = image_color::color_image(frame);
    let first_elapsed = started.elapsed();
    let expected = convert(frame, 2);
    assert!(first == expected, "first conversion pixels differ");
    drop(first);
    eprintln!(
        "IMAGE_COLOR_FIRST width={} height={} case={case} elapsed_ms={:.3}; before comparison buffers and warmups, not a cold system or allocation-only measurement",
        frame.width,
        frame.height,
        first_elapsed.as_secs_f64() * 1000.0,
    );
    for strategy in 0..3 {
        assert!(convert(frame, strategy) == expected, "warmup pixels differ");
    }
    for (batch, strategy) in [0, 1, 2, 2, 1, 0].into_iter().enumerate() {
        let mut times = Vec::new();
        for _ in 0..5 {
            let started = Instant::now();
            let actual = convert(frame, strategy);
            times.push(started.elapsed());
            assert!(actual == expected, "converted pixels differ");
        }
        times.sort_unstable();
        eprintln!(
            "IMAGE_COLOR width={} height={} case={case} strategy={strategy} batch={batch} median_ms={:.3}; 0=current rows, 1=mixed rows split into opaque/transparent blocks, 2=egui unmultiplied; full equality outside timing, no pixels/path output, decode/GPU/display or memory claim",
            frame.width,
            frame.height,
            times[2].as_secs_f64() * 1000.0,
        );
    }
}
