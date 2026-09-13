use super::*;
use std::path::PathBuf;
use std::time::Instant;

#[test]
#[ignore = "read-only JPEG comparison; set TOWAVUE_NAV_REFERENCE_DIR and use Release; no pixel output"]
#[allow(clippy::assertions_on_constants)]
fn reference_jpegs_compare_direct_rgba_pixels_and_decode_cost() {
    assert!(!cfg!(debug_assertions), "use the Release test binary");
    let source = std::env::var_os("TOWAVUE_NAV_REFERENCE_DIR").expect("explicit source directory");
    let mut paths: Vec<_> = std::fs::read_dir(source)
        .expect("source directory")
        .map(|entry| entry.expect("source entry").path())
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    let mut samples: [Vec<Duration>; 2] = Default::default();
    let mut count = 0;
    for path in paths {
        let before = std::fs::metadata(&path).expect("source metadata");
        let reader = image::ImageReader::new(open(&path, &|| true).expect("read-only source"))
            .with_guessed_format()
            .expect("sniff content");
        if reader.format() != Some(ImageFormat::Jpeg) {
            continue;
        }
        let mut bytes = Vec::new();
        reader
            .into_inner()
            .read_to_end(&mut bytes)
            .expect("JPEG bytes");
        // Both variants receive the same already-read compressed bytes. Reverse
        // the order for each pair so neither always owns the first allocation.
        for order in [[0, 1], [1, 0]] {
            let mut previous = None;
            for variant in order {
                let started = Instant::now();
                let frame = if variant == 0 {
                    static_frame(
                        image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(&bytes))
                            .expect("baseline JPEG"),
                        IMAGE_BYTE_LIMIT,
                        &|| true,
                    )
                } else {
                    jpeg_static::decode(bytes.as_slice(), IMAGE_BYTE_LIMIT, &|| true)
                }
                .expect("complete JPEG frame");
                samples[variant].push(started.elapsed());
                if let Some(previous) = &previous {
                    // Never format frames on failure: the reference is private.
                    assert!(&frame == previous, "JPEG RGBA/dimension/delay mismatch");
                } else {
                    previous = Some(frame);
                }
            }
        }
        let after = std::fs::metadata(&path).expect("source metadata after reading");
        assert_eq!(before.len(), after.len());
        assert_eq!(
            before.modified().expect("before time"),
            after.modified().expect("after time")
        );
        count += 1;
    }
    assert!(count > 0, "JPEG references required");
    for (variant, mut samples) in samples.into_iter().enumerate() {
        samples.sort_unstable();
        eprintln!(
            "REFERENCE_JPEG variant={variant} files={count} samples={} median_ms={:.3} p95_ms={:.3} total_ms={:.3}; two alternating pairs per file, compressed bytes in memory, exact full RGBA/dimensions/delay equality, source length/mtime unchanged, no pixel output; excludes file I/O, workers, previews and GPU",
            samples.len(),
            samples[samples.len() / 2].as_secs_f64() * 1000.0,
            samples[(samples.len() * 95).div_ceil(100) - 1].as_secs_f64() * 1000.0,
            samples.iter().sum::<Duration>().as_secs_f64() * 1000.0,
        );
    }
}

#[test]
#[ignore = "read-only image timing; set TOWAVUE_NAV_REFERENCE_DIR and use Release; no image output"]
#[allow(clippy::assertions_on_constants)]
fn reference_images_report_read_decode_and_optional_preview_cost() {
    assert!(!cfg!(debug_assertions), "use the Release test binary");
    let source = std::env::var_os("TOWAVUE_NAV_REFERENCE_DIR").expect("explicit source directory");
    let mut paths: Vec<_> = std::fs::read_dir(source)
        .expect("read source directory")
        .map(|entry| entry.expect("source entry").path())
        .filter(|path| {
            path.is_file()
                && towavue_core::MediaKind::from_path(path) == Some(towavue_core::MediaKind::Image)
        })
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "reference images required");
    #[derive(Default)]
    struct Samples {
        read: Vec<Duration>,
        decode: Vec<Duration>,
        preview: Vec<Duration>,
        bytes: u64,
        max_rgba: usize,
        previews: usize,
        slowest: Option<(Duration, PathBuf)>,
    }
    fn summary(values: &[Duration]) -> [f64; 3] {
        let mut values = values.to_vec();
        values.sort_unstable();
        [
            values[values.len() / 2],
            values[(values.len() * 95).div_ceil(100) - 1],
            values.iter().sum(),
        ]
        .map(|duration| duration.as_secs_f64() * 1000.0)
    }
    let mut groups = std::collections::BTreeMap::<&str, Samples>::new();
    for path in &paths {
        let before = std::fs::metadata(path).expect("source metadata");
        let started = Instant::now();
        let mut input = File::open(path).expect("read-only source");
        let mut buffer = [0; 64 * 1024];
        let mut bytes = 0;
        loop {
            let count = input.read(&mut buffer).expect("source read");
            if count == 0 {
                break;
            }
            std::hint::black_box(&buffer[..count]);
            bytes += count as u64;
        }
        drop(input);
        let read = started.elapsed();
        assert_eq!(bytes, before.len());
        let started = Instant::now();
        let image = decode_image(path).expect("complete original");
        let decode = started.elapsed();
        let samples = groups.entry(image.format).or_default();
        if samples
            .slowest
            .as_ref()
            .is_none_or(|(elapsed, _)| decode > *elapsed)
        {
            samples.slowest = Some((decode, path.clone()));
        }
        samples.max_rgba = samples.max_rgba.max(image.retained_bytes());
        samples.read.push(read);
        samples.decode.push(decode);
        samples.bytes += bytes;
        // No pixels leave the process. Drop the original before measuring the
        // optional cold-thumbnail preparation used ahead of a foreground decode.
        drop(image);
        let started = Instant::now();
        let preview =
            first_image_preview(path, IMAGE_BYTE_LIMIT, &|| true).expect("optional preview");
        samples.preview.push(started.elapsed());
        samples.previews += usize::from(preview.is_some());
        drop(preview);
        let after = std::fs::metadata(path).expect("source metadata after reading");
        assert_eq!(
            (
                before.len(),
                before.modified().expect("original modification time")
            ),
            (
                after.len(),
                after.modified().expect("final modification time")
            )
        );
    }
    for (format, samples) in groups {
        eprintln!(
            "REFERENCE_DECODE format={format} count={} compressed_mib={:.3} max_retained_rgba_mib={:.3} read_median_p95_total_ms={:?} warmed_decode_median_p95_total_ms={:?} optional_preview_median_p95_total_ms={:?} previews={}; separate serial probes, read primes filesystem cache, decode includes its own I/O/color/orientation, no GPU/worker/cache/persistence costs; source length/mtime unchanged, no pixel output",
            samples.decode.len(),
            samples.bytes as f64 / 1048576.0,
            samples.max_rgba as f64 / 1048576.0,
            summary(&samples.read),
            summary(&samples.decode),
            summary(&samples.preview),
            samples.previews,
        );
        let (first, path) = samples.slowest.expect("nonempty format group");
        let before = std::fs::metadata(&path).expect("outlier metadata");
        let mut repeated = Vec::new();
        let mut dimensions = (0, 0);
        let mut rgba = 0;
        for _ in 0..2 {
            let started = Instant::now();
            let image = decode_image(&path).expect("repeat complete original");
            repeated.push(started.elapsed().as_secs_f64() * 1000.0);
            dimensions = (image.frames[0].width, image.frames[0].height);
            rgba = image.retained_bytes();
        }
        if format == "PNG" {
            compare_png_outlier_backends(&path);
        }
        let after = std::fs::metadata(&path).expect("outlier metadata after reading");
        assert_eq!(before.len(), after.len());
        assert_eq!(
            before.modified().expect("before time"),
            after.modified().expect("after time")
        );
        eprintln!(
            "REFERENCE_OUTLIER format={format} over_33ms={} over_100ms={} max_ms={:.3} repeated_ms={repeated:?} dimensions={dimensions:?} compressed_mib={:.3} rgba_mib={:.3}; select the slowest first-pass file per format, repeat twice after the serial scan, dimensions and timing only; no path/pixel output or app-latency attribution",
            samples
                .decode
                .iter()
                .filter(|&&elapsed| elapsed > Duration::from_millis(33))
                .count(),
            samples
                .decode
                .iter()
                .filter(|&&elapsed| elapsed > Duration::from_millis(100))
                .count(),
            first.as_secs_f64() * 1000.0,
            before.len() as f64 / 1048576.0,
            rgba as f64 / 1048576.0,
        );
    }
}

// The existing FFmpeg software path is an available alternative, not a new
// production policy. Include its demux/scaling/packed-copy cost and compare
// complete pixels without ever formatting reference frames on failure.
fn compare_png_outlier_backends(path: &Path) {
    let mut samples: [Vec<f64>; 2] = Default::default();
    let mut expected = None;
    for variant in [0, 1, 1, 0] {
        let started = Instant::now();
        let frame = if variant == 0 {
            let mut image = decode_image(path).expect("current PNG decoder");
            assert_eq!(image.frames.len(), 1, "static PNG required");
            image.frames.remove(0)
        } else {
            let mut output = None;
            let result = crate::decode::decode_file(path, |item| {
                if let crate::decode::DecodeOutput::Video(frame) = item {
                    output = Some(frame);
                }
                false
            });
            assert!(
                matches!(result, Err(DecodeError::ConsumerClosed)),
                "one-frame decode must stop at the consumer"
            );
            let frame = output.expect("software RGBA frame");
            DecodedImageFrame {
                width: frame.width,
                height: frame.height,
                rgba: frame.rgba,
                delay: Duration::ZERO,
            }
        };
        samples[variant].push(started.elapsed().as_secs_f64() * 1000.0);
        if let Some(expected) = &expected {
            assert!(&frame == expected, "PNG decoder pixel/dimension mismatch");
        } else {
            expected = Some(frame);
        }
    }
    eprintln!(
        "REFERENCE_PNG_BACKENDS current_ms={:?} ffmpeg_software_ms={:?}; same selected outlier, current/FFmpeg/FFmpeg/current, complete RGBA/dimensions equal, decoder/demux/scaling/packed-copy time; no pixels emitted, peak-memory/GPU/app-latency or general PNG compatibility claim",
        samples[0], samples[1],
    );
}
