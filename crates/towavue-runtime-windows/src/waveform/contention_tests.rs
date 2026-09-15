use crate::{ImageLoader, LatestTask, PreviewCache, PreviewLoader};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use towavue_core::{EditTimeline, MediaKind, MediaTime};

#[test]
#[ignore = "Release generated waveform/foreground worker contention; run without concurrent builds"]
fn waveform_jobs_report_foreground_image_and_thumbnail_overlap() {
    use std::os::windows::process::CommandExt;
    if cfg!(debug_assertions) {
        panic!("use Release");
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "towavue-waveform-overlap-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&root).expect("owned directory");
    let audio = root.join("tone.wav");
    let ffmpeg = std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg"))
        .join("bin/ffmpeg.exe");
    let generated = std::process::Command::new(ffmpeg)
        .creation_flags(0x0800_0000)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.2*sin(2*PI*440*t)|0.3*sin(2*PI*880*t):s=48000",
            "-t",
            "900",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&audio)
        .output()
        .expect("owned audio fixture");
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let pixels = image::RgbaImage::from_fn(4096, 2304, |x, y| {
        image::Rgba([
            (x.wrapping_mul(73) ^ y.wrapping_mul(19)) as u8,
            (x / 7 + y / 3) as u8,
            (x ^ y) as u8,
            255,
        ])
    });
    let originals: Vec<_> = (0..8)
        .map(|i| root.join(format!("original-{i}.png")))
        .collect();
    let thumbnails: Vec<_> = (0..8)
        .map(|i| root.join(format!("thumbnail-{i}.png")))
        .collect();
    pixels.save(&originals[0]).expect("owned PNG");
    for path in originals.iter().chain(&thumbnails).skip(1) {
        std::fs::copy(&originals[0], path).expect("distinct cache identity");
    }
    let stamp = |path: &std::path::Path| {
        let meta = std::fs::metadata(path).expect("metadata");
        (meta.len(), meta.modified().expect("mtime"))
    };
    let sources: Vec<_> = std::iter::once(&audio)
        .chain(&originals)
        .chain(&thumbnails)
        .collect();
    let before: Vec<_> = sources.iter().map(|p| stamp(p)).collect();
    let plan = EditTimeline::from_operations(MediaTime::from_nanoseconds(900_000_000_000), &[])
        .expect("plan");
    let control = PreviewCache::new(root.join("control")).expect("control cache");
    let expected_thumbnail = control
        .filmstrip(&thumbnails[0], MediaKind::Image)
        .expect("thumbnail");
    let expected_overview = control.waveform(&audio, 640, 96).expect("overview");
    let expected_detail = super::timeline_waveform(
        &audio,
        &plan,
        1.0,
        1.0,
        960,
        &crate::Cancellation::default(),
    )
    .expect("detail");
    // Quiet/overview/detail/detail/overview/quiet, with fresh worker caches each run.
    for (round, mode) in [0, 1, 2, 2, 1, 0].into_iter().enumerate() {
        let cache = PreviewCache::new(root.join(format!("cache-{round}"))).expect("cache");
        let (image_send, image_events) = mpsc::channel();
        let images = ImageLoader::new(cache.clone(), move || {
            let _ = image_send.send(());
        })
        .expect("image worker");
        let (preview_send, preview_events) = mpsc::channel();
        let previews = PreviewLoader::new(cache.clone(), move || {
            let _ = preview_send.send(());
        })
        .expect("preview worker");
        let waveform = LatestTask::new("towavue-waveform-overlap").expect("waveform worker");
        let (started_send, started) = mpsc::channel();
        let (finished_send, finished) = mpsc::channel();
        let wave_audio = audio.clone();
        let wave_plan = plan.clone();
        waveform.submit(move |cancellation| {
            let begin = Instant::now();
            started_send.send(begin).expect("start");
            let overview = (mode == 1).then(|| {
                cache
                    .cancellable(cancellation.clone())
                    .waveform(&wave_audio, 640, 96)
                    .expect("overview")
            });
            let detail = (mode == 2).then(|| {
                super::timeline_waveform(&wave_audio, &wave_plan, 1.0, 1.0, 960, &cancellation)
                    .expect("detail")
            });
            finished_send
                .send((begin, Instant::now(), overview, detail))
                .expect("finish");
        });
        let wave_start = started
            .recv_timeout(Duration::from_secs(5))
            .expect("task started");
        let start = Instant::now();
        previews.request(
            thumbnails
                .iter()
                .cloned()
                .map(|p| (p, MediaKind::Image))
                .collect(),
        );
        let mut completed_images = Vec::new();
        let mut completed_previews = Vec::new();
        let mut preview_times = Vec::new();
        let mut image_times = Vec::new();
        let mut image_ends = Vec::new();
        for path in &originals {
            let request = Instant::now();
            let generation =
                images.request_originals_with_retained_bytes(vec![path.clone()], 0, &[]);
            loop {
                collect_previews(
                    &previews,
                    &mut completed_previews,
                    start,
                    &mut preview_times,
                );
                if let Some(result) = images.take_completed() {
                    assert_eq!(result.generation, generation);
                    image_times.push(request.elapsed().as_secs_f64() * 1000.0);
                    image_ends.push(Instant::now());
                    completed_images.push(result);
                    break;
                }
                assert!(
                    request.elapsed() < Duration::from_secs(10),
                    "image completes"
                );
                let _ = image_events.recv_timeout(Duration::from_millis(1));
            }
        }
        while completed_previews.len() < thumbnails.len() {
            collect_previews(
                &previews,
                &mut completed_previews,
                start,
                &mut preview_times,
            );
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "thumbnails complete"
            );
            if completed_previews.len() < thumbnails.len() {
                let _ = preview_events.recv_timeout(Duration::from_millis(1));
            }
        }
        let foreground_end = Instant::now();
        let (_, wave_end, overview, detail) = finished
            .recv_timeout(Duration::from_secs(20))
            .expect("waveform completes");
        // Full validation is outside measured request-to-result intervals.
        for result in completed_images {
            assert_eq!(result.images.len(), 1);
            let decoded = result.images[0].1.as_ref().expect("original");
            assert!(
                decoded.frames[0].rgba.as_slice() == pixels.as_raw().as_slice(),
                "exact original pixels"
            );
        }
        for result in completed_previews {
            assert!(thumbnails.contains(&result.path));
            let actual = result.result.expect("preview");
            assert_eq!(actual.image, expected_thumbnail.image);
            assert_eq!(actual.duration, expected_thumbnail.duration);
        }
        if let Some(result) = overview {
            assert_eq!(result, expected_overview);
        }
        if let Some(result) = detail {
            assert_eq!(result, expected_detail);
        }
        let overlap = wave_end
            .min(foreground_end)
            .saturating_duration_since(start);
        let overlapping_images = image_ends.iter().filter(|&&end| end <= wave_end).count();
        println!(
            "WAVEFORM_OVERLAP round={round} mode={mode} foreground_ms={:.4} waveform_ms={:.4} overlap_ms={:.4} images_completed_during_waveform={overlapping_images} image_ms={image_times:?} thumbnail_ready_ms={preview_times:?}",
            foreground_end.duration_since(start).as_secs_f64() * 1000.0,
            wave_end.duration_since(wave_start).as_secs_f64() * 1000.0,
            overlap.as_secs_f64() * 1000.0
        );
        while !images.is_idle() {
            assert!(start.elapsed() < Duration::from_secs(20));
            let _ = image_events.recv_timeout(Duration::from_millis(1));
        }
        drop((waveform, images, previews));
    }
    assert_eq!(sources.iter().map(|p| stamp(p)).collect::<Vec<_>>(), before);
    println!(
        "WAVEFORM_OVERLAP_CHECKS originals=48 thumbnails=48 waveform_outputs=4 exact_pixels_envelopes=true source_stamps=true"
    );
    std::fs::remove_dir_all(root).expect("owned fixture/cache cleanup");
}

fn collect_previews(
    loader: &PreviewLoader,
    completed: &mut Vec<crate::preview_loader::LoadedPreview>,
    start: Instant,
    times: &mut Vec<f64>,
) {
    for preview in loader.take_completed() {
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        completed.push(preview);
    }
}
