use super::*;
use std::sync::mpsc;
use std::thread;

fn cache(name: &str) -> PreviewCache {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    PreviewCache::new(
        std::env::temp_dir().join(format!("towavue-{name}-{}-{nonce}", std::process::id())),
    )
    .expect("cache")
}

fn png() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(2, 2, image::Rgb([24, 48, 96]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("PNG");
    bytes.into_inner()
}

#[test]
fn direct_static_thumbnails_preserve_sampled_pixels_and_bound_original_bytes() {
    let cache = cache("direct-static-thumbnail");
    for extension in ["png", "bmp", "webp", "jpg"] {
        let path = cache.root.join(format!("source.{extension}"));
        let source = image::RgbaImage::from_fn(480, 320, |x, y| {
            image::Rgba([x as u8, y as u8, (x ^ y) as u8, (x + y) as u8])
        });
        if extension == "jpg" {
            image::DynamicImage::ImageRgba8(source)
                .to_rgb8()
                .save(&path)
                .expect("owned JPEG");
        } else {
            source.save(&path).expect("owned alpha image");
        }
        let source_bytes = fs::read(&path).expect("source bytes");
        let original = crate::decode_image(&path).expect("original");
        let required = 480 * 320 * 4;
        assert!(static_thumbnail_png(&path, required - 1, &|| true).is_none());
        assert!(static_thumbnail_png(&path, required, &|| false).is_none());
        let polls = std::cell::Cell::new(0);
        assert!(
            static_thumbnail_png(&path, required, &|| {
                polls.set(polls.get() + 1);
                polls.get() < 5
            })
            .is_none()
        );
        let direct = decode_png(
            &static_thumbnail_png(&path, required, &|| true).expect("bounded thumbnail"),
        )
        .expect("PNG");
        assert_eq!((direct.width, direct.height), (240, 160));
        for y in 0..160_usize {
            for x in 0..240_usize {
                let sample = ((y * 2 + 1) * 480 + x * 2 + 1) * 4;
                let target = (y * 240 + x) * 4;
                assert_eq!(
                    &direct.rgba[target..target + 4],
                    &original.frames[0].rgba[sample..sample + 4],
                    "{extension} ({x},{y})"
                );
            }
        }
        assert_eq!(
            cache
                .filmstrip(&path, MediaKind::Image)
                .expect("integrated direct thumbnail")
                .image,
            direct
        );
        let fresh = PreviewCache::new(cache.root.clone()).expect("fresh memory");
        assert_eq!(
            fresh
                .filmstrip(&path, MediaKind::Image)
                .expect("disk thumbnail")
                .image,
            direct
        );
        assert_eq!(fs::read(&path).expect("unchanged source"), source_bytes);
    }
    let gif = cache.root.join("animated.gif");
    {
        let file = fs::File::create(&gif).expect("owned GIF");
        let mut encoder = image::codecs::gif::GifEncoder::new(file);
        encoder
            .encode_frame(image::Frame::new(image::RgbaImage::from_pixel(
                32,
                16,
                image::Rgba([20, 40, 60, 255]),
            )))
            .expect("first frame");
        encoder
            .encode_frame(image::Frame::new(image::RgbaImage::from_pixel(
                32,
                16,
                image::Rgba([100, 120, 140, 255]),
            )))
            .expect("second frame");
    }
    assert!(static_thumbnail_png(&gif, STATIC_THUMBNAIL_BYTE_LIMIT, &|| true).is_none());
    let fallback = cache
        .filmstrip(&gif, MediaKind::Image)
        .expect("animation fallback");
    assert_eq!((fallback.image.width, fallback.image.height), (240, 120));
    assert!(
        fallback
            .image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [20, 40, 60, 255])
    );
    let malformed = cache.root.join("bad.png");
    fs::write(&malformed, b"not an image").expect("owned malformed file");
    assert!(static_thumbnail_png(&malformed, STATIC_THUMBNAIL_BYTE_LIMIT, &|| true).is_none());
    assert!(cache.filmstrip(&malformed, MediaKind::Image).is_err());
    fs::remove_dir_all(&cache.root).expect("remove owned fixtures");
}

#[test]
fn static_preview_fallback_does_not_seek_past_the_only_frame() {
    let cache = cache("static-preview-zero-seek");
    for extension in ["jpg", "bmp", "png", "webp"] {
        let path = cache.root.join(format!("small.{extension}"));
        image::RgbImage::from_pixel(32, 16, image::Rgb([12, 80, 190]))
            .save(&path)
            .expect("owned small image");
        let args = preview_input_arguments(&path, Duration::ZERO, None).expect("image arguments");
        assert!(
            !args.iter().any(|arg| arg == "-ss"),
            "no seek for first image frame: {extension}"
        );
        let later = preview_input_arguments(&path, Duration::from_secs(1), None)
            .expect("timed image arguments");
        assert_eq!(
            &later[..2],
            &["-ss", "1.000000"],
            "nonzero animation positions still seek"
        );
        let first = cache
            .filmstrip(&path, MediaKind::Image)
            .expect("small image fallback");
        assert_eq!((first.image.width, first.image.height), (240, 120));
        assert!(
            first
                .image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[0].abs_diff(12) <= 5
                    && pixel[1].abs_diff(80) <= 5
                    && pixel[2].abs_diff(190) <= 5
                    && pixel[3] == 255)
        );
        assert_eq!(
            cache
                .thumbnail(&path, Duration::ZERO, 16)
                .expect("generic image thumbnail")
                .rgba
                .len(),
            16 * 8 * 4
        );
        let fresh = PreviewCache::new(cache.root.clone()).expect("fresh memory");
        assert_eq!(
            fresh
                .filmstrip(&path, MediaKind::Image)
                .expect("disk reuse")
                .image,
            first.image
        );
    }
    fs::remove_dir_all(&cache.root).expect("remove owned fixtures");
}

#[test]
fn unvisited_static_filmstrips_share_the_bounded_first_preview() {
    for extension in ["jpg", "bmp"] {
        let cache = cache("unvisited-static-preview");
        let path = cache.root.join(format!("source.{extension}"));
        image::RgbImage::from_pixel(2560, 1920, image::Rgb([12, 80, 190]))
            .save(&path)
            .expect("large owned image");
        let first = cache.filmstrip(&path, MediaKind::Image).expect("filmstrip");
        assert!(first.duration.is_none());
        let shared = cache
            .cached_image(&path)
            .expect("lookup")
            .expect("preview includes source geometry before original decode");
        assert_eq!(shared.source_size, (2560, 1920));
        assert_eq!(first.image, shared.image);
        assert_eq!((first.image.width, first.image.height), (213, 160));
        assert!(first.image.rgba.as_chunks::<4>().0.iter().all(|pixel| {
            pixel[0].abs_diff(12) <= 5
                && pixel[1].abs_diff(80) <= 5
                && pixel[2].abs_diff(190) <= 5
                && pixel[3] == 255
        }));
        assert_eq!(
            cache
                .filmstrip(&path, MediaKind::Image)
                .expect("warm thumbnail")
                .image,
            first.image
        );
        assert_eq!(
            cache
                .prepare_image_preview(&path, crate::image::IMAGE_BYTE_LIMIT, &|| true)
                .expect("foreground reuse")
                .image,
            first.image
        );
        assert_eq!(
            fs::read_dir(&cache.root).expect("cache files").count(),
            1,
            "fast path does not encode a disk thumbnail"
        );
        let disk = PreviewCache::new(cache.root.join("disk")).expect("existing disk cache");
        let stored = disk
            .load_or_generate(
                cache_key(&path, IMAGE_PREVIEW_VARIANT).expect("key"),
                || Ok(png()),
            )
            .expect("seed an independently recognizable cached preview");
        let fresh = PreviewCache::new(disk.root.clone()).expect("fresh memory");
        for _ in 0..2 {
            assert_eq!(
                fresh
                    .filmstrip(&path, MediaKind::Image)
                    .expect("prefer disk, then memory, over reduced decoding")
                    .image,
                stored
            );
            assert!(
                fresh.cached_image(&path).expect("lookup").is_none(),
                "disk reuse does not invent source dimensions"
            );
        }
        let token = Cancellation::default();
        token.cancel();
        assert!(matches!(
            cache.cancellable(token).filmstrip(&path, MediaKind::Image),
            Err(PreviewError::Cancelled)
        ));
        fs::write(&path, b"changed source").expect("replace owned fixture");
        assert!(cache.cached_image(&path).expect("new key").is_none());
        assert!(cache.filmstrip(&path, MediaKind::Image).is_err());
        assert!(cache.in_flight.0.lock().expect("pending").is_empty());
        fs::remove_dir_all(&cache.root).expect("remove owned fixtures");
    }
}

#[test]
fn static_first_preview_reuses_pixels_without_waiting_for_another_generator() {
    for extension in ["jpg", "bmp"] {
        let cache = cache("static-first-sharing");
        let path = cache.root.join(format!("source.{extension}"));
        image::RgbImage::from_pixel(2560, 1920, image::Rgb([12, 80, 190]))
            .save(&path)
            .expect("large static image");
        let key = cache_key(&path, IMAGE_PREVIEW_VARIANT).expect("source key");
        let held = cache.claim_generation(&key).expect("another generator");
        let worker_cache = cache.clone();
        let worker_path = path.clone();
        let (sent, received) = mpsc::channel();
        let worker = thread::spawn(move || {
            sent.send(worker_cache.prepare_image_preview(
                &worker_path,
                crate::image::IMAGE_BYTE_LIMIT,
                &|| true,
            ))
            .expect("speculation result");
        });
        let immediate = received.recv_timeout(Duration::from_secs(1));
        drop(held);
        worker.join().expect("worker");
        assert!(
            immediate
                .expect("foreground must not wait for another generator")
                .is_none()
        );
        let preview = cache
            .prepare_image_preview(&path, crate::image::IMAGE_BYTE_LIMIT, &|| true)
            .expect("generate after release");
        let held = cache.claim_generation(&key).expect("hold generation again");
        assert_eq!(
            cache
                .prepare_image_preview(&path, crate::image::IMAGE_BYTE_LIMIT, &|| true)
                .expect("warm reuse")
                .image,
            preview.image
        );
        assert!(
            cache
                .prepare_image_preview(&path, crate::image::IMAGE_BYTE_LIMIT, &|| false)
                .is_none()
        );
        drop(held);
        fs::write(&path, b"changed input").expect("replace owned source");
        assert!(
            cache
                .prepare_image_preview(&path, crate::image::IMAGE_BYTE_LIMIT, &|| true)
                .is_none()
        );
        assert!(cache.in_flight.0.lock().expect("pending").is_empty());
        fs::remove_dir_all(&cache.root).expect("remove owned cache");
    }
}

#[test]
fn identical_preview_work_is_coalesced_but_other_keys_and_cancellation_are_independent() {
    let cache = cache("preview-coalesce");
    let (started, start) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let first_cache = cache.clone();
    let first = thread::spawn(move || {
        first_cache.load_or_generate("same".into(), || {
            started.send(()).expect("started");
            released
                .recv_timeout(Duration::from_secs(5))
                .expect("release");
            Ok(png())
        })
    });
    start
        .recv_timeout(Duration::from_secs(5))
        .expect("in flight");
    let token = Cancellation::default();
    let cancelled_cache = cache.cancellable(token.clone());
    let cancelled = thread::spawn(move || {
        cancelled_cache.load_or_generate("same".into(), || panic!("cancelled waiter generated"))
    });
    token.cancel();
    assert!(matches!(
        cancelled.join().expect("waiter"),
        Err(PreviewError::Cancelled)
    ));
    assert!(
        cache.load_or_generate("other".into(), || Ok(png())).is_ok(),
        "unrelated key is not serialized"
    );
    let (waiting, wait) = mpsc::channel();
    let follower_cache = cache.clone();
    let follower = thread::spawn(move || {
        waiting.send(()).expect("waiting");
        follower_cache.load_or_generate("same".into(), || panic!("duplicate generation"))
    });
    wait.recv_timeout(Duration::from_secs(5)).expect("follower");
    release.send(()).expect("complete generation");
    let image = first.join().expect("first").expect("pixels");
    assert_eq!(
        follower.join().expect("follower").expect("shared pixels"),
        image
    );
    assert!(cache.in_flight.0.lock().expect("pending").is_empty());
    assert!(fs::read_dir(&cache.root).expect("cache files").all(|file| {
        file.expect("entry")
            .path()
            .extension()
            .is_some_and(|extension| extension == "png")
    }));
    fs::remove_dir_all(&cache.root).expect("remove owned cache");
}

#[test]
fn failed_or_cancelled_generators_release_the_key_for_a_live_consumer() {
    let cache = cache("preview-retry");
    for cancelled in [false, true] {
        let token = Cancellation::default();
        let producer = cache.cancellable(token.clone());
        let key = format!("retry-{cancelled}");
        let (started, start) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let producer_key = key.clone();
        let first = thread::spawn(move || {
            producer.load_or_generate(producer_key, || {
                started.send(()).expect("start");
                released
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release");
                if cancelled {
                    token.cancel();
                    Ok(png())
                } else {
                    Err(PreviewError::NoFrame)
                }
            })
        });
        start
            .recv_timeout(Duration::from_secs(5))
            .expect("in flight");
        let follower = cache.clone();
        let next = thread::spawn(move || follower.load_or_generate(key, || Ok(png())));
        release.send(()).expect("release");
        assert!(first.join().expect("first").is_err());
        assert!(next.join().expect("next").is_ok());
    }
    fs::remove_dir_all(&cache.root).expect("remove owned cache");
}

#[test]
fn durations_share_successes_bound_entries_and_leave_failures_retryable() {
    let cache = cache("duration-sharing");
    let (started, start) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let first_cache = cache.clone();
    let first = thread::spawn(move || {
        first_cache.duration_with("duration".into(), || {
            started.send(()).expect("start");
            released
                .recv_timeout(Duration::from_secs(5))
                .expect("release");
            Ok(Duration::from_secs(3))
        })
    });
    start
        .recv_timeout(Duration::from_secs(5))
        .expect("probe started");
    let follower_cache = cache.clone();
    let follower = thread::spawn(move || {
        follower_cache.duration_with("duration".into(), || panic!("duplicate probe"))
    });
    release.send(()).expect("release");
    assert_eq!(
        first.join().expect("first").expect("duration"),
        follower.join().expect("follower").expect("shared duration")
    );
    for index in 0..64 {
        cache
            .duration_with(index.to_string(), || Ok(Duration::ZERO))
            .expect("duration");
    }
    assert_eq!(cache.memory.lock().expect("memory").durations.len(), 64);
    assert!(
        !cache
            .memory
            .lock()
            .expect("memory")
            .durations
            .iter()
            .any(|(key, _)| key == "duration")
    );
    assert!(
        cache
            .duration_with("bad".into(), || Err(PreviewError::InvalidDuration))
            .is_err()
    );
    assert!(
        cache
            .duration_with("bad".into(), || Ok(Duration::ZERO))
            .is_ok()
    );
    let token = Cancellation::default();
    token.cancel();
    assert!(matches!(
        cache
            .cancellable(token)
            .duration_with("bad".into(), || panic!("cancelled cache hit")),
        Err(PreviewError::Cancelled)
    ));
    let source = cache.root.join("source.mp4");
    fs::write(&source, b"old").expect("fixture");
    let old = cache_key(&source, "duration-v1").expect("key");
    fs::write(&source, b"replacement").expect("fixture change");
    assert_ne!(cache_key(&source, "duration-v1").expect("changed key"), old);
    fs::remove_dir_all(&cache.root).expect("remove owned cache");
}
