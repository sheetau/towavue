use super::*;
use std::time::Instant;

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
    }
}
