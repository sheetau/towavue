use super::*;
use std::sync::mpsc;
use std::thread;

#[test]
#[ignore = "compares fresh native/CLI duration probes; run without concurrent timing work"]
fn native_duration_probe_matches_cli_and_reports_cost() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("run optimized duration comparison");
    }
    let store = cache("native-duration-cost");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1");
    for name in ["h264-aac.mp4", "hevc-aac.mkv", "vp9-opus.webm"] {
        let source = fixtures.join(name);
        let before = fs::read(&source).expect("generated fixture");
        let stamp = fs::metadata(&source)
            .expect("source metadata")
            .modified()
            .expect("mtime");
        let expected = store.probe_duration_cli(&source).expect("CLI duration");
        for native in [false, true, true, false] {
            let mut elapsed = Vec::new();
            for _ in 0..5 {
                let started = std::time::Instant::now();
                let duration = if native {
                    store.probe_duration(&source)
                } else {
                    store.probe_duration_cli(&source)
                }
                .expect("fresh duration probe");
                elapsed.push(started.elapsed());
                assert_eq!(duration, expected, "same format duration");
            }
            elapsed.sort();
            eprintln!(
                "duration probe {name} native={native} median_ms={:.4}",
                elapsed[2].as_secs_f64() * 1000.0
            );
        }
        assert_eq!(
            fs::metadata(&source)
                .expect("metadata")
                .modified()
                .expect("mtime"),
            stamp
        );
        assert!(fs::read(&source).expect("unchanged source") == before);
    }
    fs::remove_dir_all(&store.root).expect("remove owned cache");
    Ok(())
}

#[test]
fn native_duration_preserves_audio_metadata_errors_and_cancellation() {
    let store = cache("native-audio-duration");
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    for (extension, codec) in [("wav", "pcm_s16le"), ("flac", "flac"), ("aac", "aac")] {
        let target = store.root.join(format!("audio.{extension}"));
        let output = hidden_command(
            &tool_path("ffmpeg.exe").expect("fixture encoder"),
            ["-v", "error", "-i"],
        )
        .arg(&source)
        .args(["-vn", "-c:a", codec])
        .arg(&target)
        .output()
        .expect("generate audio");
        assert!(output.status.success(), "audio fixture generation");
        assert_eq!(
            store.duration(&target).expect("native duration"),
            store.probe_duration_cli(&target).expect("CLI duration")
        );
    }
    let malformed = store.root.join("malformed.wav");
    fs::write(&malformed, b"invalid media").expect("malformed fixture");
    for target in [malformed, store.root.join("missing.wav")] {
        assert!(store.probe_duration(&target).is_err());
        assert!(store.probe_duration_cli(&target).is_err());
    }
    let cancellation = Cancellation::default();
    cancellation.cancel();
    assert!(matches!(
        store.cancellable(cancellation).duration(&source),
        Err(PreviewError::Cancelled)
    ));
    fs::remove_dir_all(store.root).expect("owned cache cleanup");
}

#[test]
#[ignore = "uncached filmstrip comparison; use Release without concurrent builds"]
fn native_duration_probe_reduces_uncached_filmstrip_wait() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("run optimized filmstrip comparison");
    }
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    let before = fs::read(&source).expect("generated video");
    let stamp = fs::metadata(&source)
        .expect("metadata")
        .modified()
        .expect("mtime");
    let seed = cache("duration-filmstrip-seed");
    let expected = seed
        .filmstrip(&source, MediaKind::Video)
        .expect("reference thumbnail");
    for native in [false, true, true, false] {
        NATIVE_DURATION_PROBE.set(native);
        let mut elapsed = Vec::new();
        for _ in 0..5 {
            let store = cache("duration-filmstrip-trial");
            let started = std::time::Instant::now();
            let preview = store
                .filmstrip(&source, MediaKind::Video)
                .expect("uncached thumbnail");
            elapsed.push(started.elapsed());
            assert_eq!(preview.duration, expected.duration);
            assert!(
                preview.image == expected.image,
                "same full thumbnail pixels"
            );
            fs::remove_dir_all(store.root).expect("owned trial cleanup");
        }
        elapsed.sort();
        eprintln!(
            "uncached filmstrip native_duration={native} median_ms={:.4}",
            elapsed[2].as_secs_f64() * 1000.0
        );
    }
    NATIVE_DURATION_PROBE.set(true);
    assert!(fs::read(&source).expect("source") == before);
    assert_eq!(
        fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        stamp
    );
    fs::remove_dir_all(seed.root).expect("owned seed cleanup");
    Ok(())
}

#[test]
fn shared_preview_pixels_survive_eviction_and_retire_with_the_last_consumer() {
    let store = cache("shared-pixel-lifetime");
    let expected = [19, 43, 71, 127].repeat(31 * 17);
    let pixels = expected.clone();
    let pointer = pixels.as_ptr();
    let image = PreviewImage {
        width: 31,
        height: 17,
        rgba: pixels.into(),
    };
    let lifetime = Arc::downgrade(&image.rgba);
    store
        .memory
        .lock()
        .expect("memory")
        .insert("retained".into(), image);
    let get = || {
        store
            .load_or_generate("retained".into(), || panic!("cached pixels"))
            .expect("hit")
    };
    let first = get();
    let second = get();
    assert_eq!(
        first.rgba.as_ptr(),
        pointer,
        "original allocation is retained"
    );
    assert!(
        Arc::ptr_eq(&first.rgba, &second.rgba),
        "hits share pixel ownership"
    );
    let mut edited_copy = second.clone();
    Arc::make_mut(&mut edited_copy.rgba)[0] = 0;
    assert!(
        first.rgba.as_slice() == expected,
        "explicit copy-on-write cannot modify other owners"
    );
    drop(edited_copy);
    {
        let mut memory = store.memory.lock().expect("memory");
        for index in 0..64 {
            memory.insert(
                format!("replacement-{index}"),
                PreviewImage {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4].into(),
                },
            );
        }
        assert!(memory.get("retained").is_none(), "normal LRU eviction");
        assert_eq!(memory.bytes, 64 * 4, "only cache-owned entries are charged");
    }
    assert!(
        second.rgba.as_slice() == expected,
        "eviction leaves queued pixels intact"
    );
    drop(first);
    assert!(
        lifetime.upgrade().is_some(),
        "last consumer keeps pixels alive"
    );
    drop(second);
    assert!(
        lifetime.upgrade().is_none(),
        "pixels retire after the last owner"
    );
    fs::remove_dir_all(store.root).expect("owned cache cleanup");
}

#[test]
#[ignore = "preview cache hit/retained allocation cost; run in Release without concurrent builds"]
fn preview_cache_hits_report_copy_and_contention_cost() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("use Release");
    }
    for (width, height) in [(240, 160), (960, 640)] {
        let store = cache("preview-hit-cost");
        let expected: Vec<Vec<u8>> = (0..6)
            .map(|seed| {
                (0..width * height)
                    .flat_map(|n| [n as u8, seed, 73, (n / 3) as u8])
                    .collect()
            })
            .collect();
        let keys: Vec<_> = (0..6).map(|index| format!("ready-{index}")).collect();
        for (key, rgba) in keys.iter().zip(&expected) {
            store.memory.lock().expect("memory").insert(
                key.clone(),
                PreviewImage {
                    width,
                    height,
                    rgba: rgba.clone().into(),
                },
            );
        }
        for callers in [1, 4] {
            let mut batches = Vec::new();
            let mut requests = Vec::new();
            let mut retained_bytes = None;
            for _ in 0..5 {
                let release = std::sync::Barrier::new(callers + 1);
                let (ready, waiting) = mpsc::channel();
                let (elapsed, outputs) = thread::scope(|scope| {
                    let jobs: Vec<_> = (0..callers)
                        .map(|worker| {
                            let store = &store;
                            let keys = &keys;
                            let release = &release;
                            let ready = ready.clone();
                            scope.spawn(move || {
                                let mut outputs = Vec::with_capacity(64 / callers);
                                ready.send(()).expect("worker ready");
                                release.wait();
                                for n in 0..64 / callers {
                                    let index = (worker * (64 / callers) + n) % keys.len();
                                    let started = std::time::Instant::now();
                                    let image = store
                                        .load_or_generate(keys[index].clone(), || {
                                            panic!("memory hit must not generate")
                                        })
                                        .expect("cache hit");
                                    outputs.push((index, started.elapsed(), image));
                                }
                                outputs
                            })
                        })
                        .collect();
                    for _ in 0..callers {
                        waiting.recv().expect("ready");
                    }
                    let started = std::time::Instant::now();
                    release.wait();
                    let outputs: Vec<_> = jobs
                        .into_iter()
                        .flat_map(|job| job.join().expect("cache worker"))
                        .collect();
                    (started.elapsed(), outputs)
                });
                batches.push(elapsed);
                let mut allocations = std::collections::HashMap::new();
                for (index, elapsed, image) in &outputs {
                    assert_eq!((image.width, image.height), (width, height));
                    assert!(
                        image.rgba.as_slice() == expected[*index],
                        "all returned pixels"
                    );
                    requests.push(*elapsed);
                    allocations.insert(image.rgba.as_ptr() as usize, image.rgba.len());
                }
                let bytes: usize = allocations.values().sum();
                if let Some(previous) = retained_bytes {
                    assert_eq!(bytes, previous);
                }
                retained_bytes = Some(bytes);
            }
            batches.sort();
            requests.sort();
            eprintln!(
                "PREVIEW_HITS width={width} height={height} callers={callers} batch64_median_ms={:.4} request_p95_us={:.3} returned_pixel_bytes={}",
                batches[2].as_secs_f64() * 1000.0,
                requests[(requests.len() * 95).div_ceil(100) - 1].as_secs_f64() * 1_000_000.0,
                retained_bytes.expect("allocation count")
            );
        }
        assert_eq!(
            fs::read_dir(&store.root).expect("cache directory").count(),
            0,
            "memory-only lookup"
        );
        fs::remove_dir_all(store.root).expect("owned cache cleanup");
    }
    Ok(())
}

#[test]
fn native_media_png_transfers_its_canvas_and_matches_persisted_pixels() {
    let image = image::RgbaImage::from_fn(37, 23, |x, y| {
        image::Rgba([x as u8, y as u8, 83, (x * y) as u8])
    });
    let allocation = image.as_raw().as_ptr();
    let (encoded, ready) = ready_preview_png(image).expect("native PNG");
    let ready = ready.expect("original canvas");
    assert_eq!(
        ready.rgba.as_ptr(),
        allocation,
        "transfer without cloning the canvas"
    );
    assert_eq!(ready, decode_png(&encoded).expect("persisted PNG"));
}

#[test]
#[ignore = "native video/sheet/waveform publication cost; use Release without concurrent builds"]
fn native_media_publication_reports_png_round_trip_cost() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("use Release");
    }
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/generated/m1/h264-aac.mp4");
    let source_bytes = fs::read(&source).expect("warm owned fixture");
    let modified = fs::metadata(&source)
        .expect("source")
        .modified()
        .expect("mtime");
    for kind in ["frame", "sheet", "waveform"] {
        let seed = cache("media-publication");
        let duration = seed.duration(&source).expect("duration");
        let layout = VideoSheetLayout::for_position(duration, Duration::ZERO).expect("layout");
        let generate = |store: &PreviewCache| match kind {
            "frame" => store
                .thumbnail(&source, Duration::from_millis(100), 240)
                .expect("frame"),
            "sheet" => store.video_sheet(&source, layout).expect("sheet").image,
            _ => store.waveform(&source, 240, 160).expect("waveform"),
        };
        REDECODE_MEDIA_PNG.set(true);
        let expected = generate(&seed);
        let file = fs::read_dir(&seed.root)
            .expect("seed files")
            .next()
            .expect("one PNG")
            .expect("entry")
            .path();
        let expected_png = fs::read(&file).expect("historical PNG");
        for (batch, ready) in [false, true, true, false].into_iter().enumerate() {
            REDECODE_MEDIA_PNG.set(!ready);
            let mut samples = Vec::new();
            for sample in 0..15 {
                let store = PreviewCache::new(seed.root.join(format!("{batch}-{sample}")))
                    .expect("empty cache");
                let started = std::time::Instant::now();
                let actual = generate(&store);
                samples.push(started.elapsed());
                assert!(
                    actual == expected,
                    "native publication preserves full pixels"
                );
                let file = fs::read_dir(&store.root)
                    .expect("cache files")
                    .next()
                    .expect("PNG")
                    .expect("entry")
                    .path();
                assert!(
                    fs::read(file).expect("cache PNG") == expected_png,
                    "unchanged PNG encoding"
                );
                let fresh = PreviewCache::new(store.root.clone()).expect("fresh memory");
                assert!(generate(&fresh) == expected, "disk reuse preserves pixels");
            }
            samples.sort();
            eprintln!(
                "NATIVE_MEDIA kind={kind} ready={ready} median_ms={:.3}",
                samples[7].as_secs_f64() * 1000.0
            );
        }
        REDECODE_MEDIA_PNG.set(false);
        fs::remove_dir_all(seed.root).expect("remove owned caches");
    }
    assert!(fs::read(&source).expect("source recheck") == source_bytes);
    assert_eq!(
        fs::metadata(&source)
            .expect("source")
            .modified()
            .expect("mtime"),
        modified
    );
    Ok(())
}

// Encoded-only control for cache validation and historical round-trip comparisons.
fn static_thumbnail_png(
    source: &Path,
    byte_limit: usize,
    current: &dyn Fn() -> bool,
) -> Option<Vec<u8>> {
    static_thumbnail_ready(source, byte_limit, current).map(|(bytes, _)| bytes)
}

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
fn generated_thumbnail_transfers_pixels_and_keeps_disk_validation_and_fallback() {
    for stored in [true, false] {
        let root = cache("ready-thumbnail").root;
        let path = root.join("cache");
        if stored {
            fs::create_dir(&path).expect("cache directory");
            fs::write(path.join("ready.png"), b"invalid cached PNG").expect("corrupt cache");
        } else {
            fs::write(&path, b"preserve occupied path").expect("unavailable directory");
        }
        let store = PreviewCache::new(path.clone()).expect("optional storage");
        let small = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(31, 17, |x, y| {
            image::Rgba([x as u8, y as u8, 70, (x * y) as u8])
        }));
        let bytes = thumbnail_png(&small, (4096, 2304), &|| true).expect("cache PNG");
        let pixels = small.into_rgba8().into_raw();
        let pointer = pixels.as_ptr();
        let expected = PreviewImage {
            width: 31,
            height: 17,
            rgba: pixels.clone().into(),
        };
        let result = store
            .load_or_generate_ready("ready".into(), || {
                Ok((
                    bytes.clone(),
                    Some(PreviewImage {
                        width: 31,
                        height: 17,
                        rgba: pixels.into(),
                    }),
                ))
            })
            .expect("generated pixels");
        assert_eq!(
            result.rgba.as_ptr(),
            pointer,
            "do not decode or copy the generated pixels again"
        );
        assert_eq!(result, expected);
        assert_eq!(
            store
                .load_or_generate("ready".into(), || panic!("reuse memory"))
                .expect("memory"),
            expected
        );
        assert_eq!(
            store
                .memory
                .lock()
                .expect("memory")
                .entries
                .front()
                .expect("entry")
                .source_size,
            Some((4096, 2304))
        );
        if stored {
            assert_eq!(
                fs::read(path.join("ready.png")).expect("repaired disk cache"),
                bytes
            );
            let fresh = PreviewCache::new(path).expect("fresh memory");
            assert_eq!(
                fresh
                    .load_or_generate_ready("ready".into(), || panic!("decode disk entry"))
                    .expect("disk reuse"),
                expected
            );
        } else {
            assert_eq!(
                fs::read(path).expect("occupied file"),
                b"preserve occupied path"
            );
        }
        fs::remove_dir_all(root).expect("remove owned fixtures");
    }
}

#[test]
#[ignore = "Release comparison of generated thumbnail encoding, persistence and publication"]
fn generated_thumbnail_reports_png_round_trip_cost() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("use Release for timing");
    }
    let root = cache("ready-thumbnail-cost").root;
    let mut seed = 1_u32;
    let pixels = image::RgbaImage::from_fn(240, 160, |_, _| {
        image::Rgba(std::array::from_fn(|channel| {
            if channel == 3 {
                255
            } else {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            }
        }))
    });
    for (phase, ready) in [false, true, true, false].into_iter().enumerate() {
        let mut samples = Vec::new();
        for sample in 0..15 {
            let store =
                PreviewCache::new(root.join(format!("{phase}-{sample}"))).expect("empty cache");
            let small = image::DynamicImage::ImageRgba8(pixels.clone());
            let start = std::time::Instant::now();
            let result = store
                .load_or_generate_ready("preview".into(), || {
                    let bytes = thumbnail_png(&small, (4096, 2304), &|| true).expect("PNG");
                    let image = ready.then(|| PreviewImage {
                        width: small.width(),
                        height: small.height(),
                        rgba: small.into_rgba8().into_raw().into(),
                    });
                    Ok((bytes, image))
                })
                .expect("publish");
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(result.rgba.as_slice(), pixels.as_raw().as_slice());
            let fresh = PreviewCache::new(store.root.clone()).expect("fresh memory");
            assert_eq!(
                fresh
                    .load_or_generate("preview".into(), || panic!("disk reuse"))
                    .expect("persisted"),
                result
            );
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "THUMBNAIL_PUBLISH ready={ready} median_ms={:.3}",
            samples[7]
        );
    }
    fs::remove_dir_all(root).expect("remove owned caches");
    Ok(())
}

#[test]
fn native_thumbnail_publication_preserves_pixels_geometry_and_disk_reuse() {
    check_native_thumbnail_publication(1);
}

#[test]
#[ignore = "Release comparison of native thumbnail generation, persistence and publication"]
fn native_thumbnail_publication_reports_round_trip_cost() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("use Release for timing");
    }
    check_native_thumbnail_publication(15);
    Ok(())
}

#[test]
fn decoded_cache_png_preserves_owned_and_converted_pixel_layouts() {
    for image in [
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(37, 23, |x, y| {
            image::Rgba([x as u8, y as u8, 97, (x * y) as u8])
        })),
        image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(37, 23, |x, y| {
            image::Rgb([x as u8, y as u8, 97])
        })),
        image::DynamicImage::ImageLumaA16(image::ImageBuffer::from_fn(37, 23, |x, y| {
            image::LumaA([(x * 733) as u16, (y * 2017) as u16])
        })),
    ] {
        let expected = image.to_rgba8();
        let mut encoded = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("generated PNG");
        let actual = decode_png(encoded.get_ref()).expect("cached PNG");
        assert_eq!((actual.width, actual.height), expected.dimensions());
        assert!(
            actual.rgba.as_slice() == *expected.as_raw(),
            "full RGBA pixels, including hidden RGB"
        );
        assert!(decode_png(&encoded.get_ref()[..encoded.get_ref().len() / 2]).is_err());
    }
}

#[test]
#[ignore = "generated cache PNG decode/copy comparison; run in Release without concurrent builds"]
fn owned_cache_png_reports_decode_cost() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("use Release");
    }
    for (width, height) in [(240, 160), (960, 640)] {
        let pixels = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([x as u8, y as u8, (x / 7 + y) as u8, (x ^ y) as u8])
        });
        let mut encoded = std::io::Cursor::new(Vec::new());
        pixels
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("cache PNG");
        for owned in [false, true, true, false] {
            let mut samples = Vec::new();
            for _ in 0..25 {
                let started = std::time::Instant::now();
                let actual = if owned {
                    decode_png(encoded.get_ref()).expect("owned decode")
                } else {
                    let rgba = image::load_from_memory_with_format(
                        encoded.get_ref(),
                        image::ImageFormat::Png,
                    )
                    .expect("historical decode")
                    .to_rgba8();
                    PreviewImage {
                        width: rgba.width(),
                        height: rgba.height(),
                        rgba: rgba.into_raw().into(),
                    }
                };
                samples.push(started.elapsed());
                assert_eq!((actual.width, actual.height), (width, height));
                assert!(
                    actual.rgba.as_slice() == *pixels.as_raw(),
                    "full cached pixels"
                );
            }
            samples.sort();
            eprintln!(
                "CACHE_PNG {width}x{height} owned={owned} median_ms={:.3}",
                samples[12].as_secs_f64() * 1000.0
            );
        }
    }
    Ok(())
}

fn check_native_thumbnail_publication(sample_count: usize) {
    for (width, height) in [(32, 24), (240, 160), (1920, 1080)] {
        let root = cache("native-thumbnail-publication").root;
        let source = root.join("source.png");
        let mut seed = 1_u32;
        let pixels = image::RgbaImage::from_fn(width, height, |_, _| {
            image::Rgba(std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            }))
        });
        pixels.save(&source).expect("owned alpha PNG");
        let expected = image::DynamicImage::ImageRgba8(pixels)
            .resize(240, 160, image::imageops::FilterType::Nearest)
            .into_rgba8();
        let original = fs::read(&source).expect("warm source");
        let modified = fs::metadata(&source)
            .expect("source")
            .modified()
            .expect("mtime");
        let key = cache_key(&source, IMAGE_PREVIEW_VARIANT).expect("key");
        for (phase, ready) in [false, true, true, false].into_iter().enumerate() {
            let mut samples = Vec::new();
            for sample in 0..sample_count {
                let store =
                    PreviewCache::new(root.join(format!("{phase}-{sample}"))).expect("empty cache");
                let started = std::time::Instant::now();
                let image = store
                    .load_or_generate_ready(key.clone(), || {
                        let (bytes, image) =
                            static_thumbnail_ready(&source, STATIC_THUMBNAIL_BYTE_LIMIT, &|| true)
                                .expect("native generation");
                        Ok((bytes, ready.then_some(image)))
                    })
                    .expect("publication");
                samples.push(started.elapsed().as_secs_f64() * 1000.0);
                assert_eq!((image.width, image.height), expected.dimensions());
                assert_eq!(image.rgba.as_ref(), expected.as_raw());
                let fresh = PreviewCache::new(store.root.clone()).expect("fresh memory");
                let saved = fresh
                    .cached_image(&source)
                    .expect("disk read")
                    .expect("entry");
                assert_eq!(saved.source_size, (width, height));
                assert_eq!(saved.image, image);
            }
            if sample_count > 1 {
                samples.sort_by(f64::total_cmp);
                println!(
                    "NATIVE_THUMBNAIL {width}x{height} ready={ready} median_ms={:.3}",
                    samples[samples.len() / 2]
                );
            }
        }
        assert_eq!(fs::read(&source).expect("unchanged source"), original);
        assert_eq!(
            fs::metadata(&source)
                .expect("source")
                .modified()
                .expect("mtime"),
            modified
        );
        fs::remove_dir_all(root).expect("remove owned fixtures");
    }
}

#[test]
fn disk_pruning_preserves_under_limit_files_and_removes_oldest_when_over() {
    let cache = cache("prune-bounds");
    let size = CACHE_LIMIT_BYTES / 2;
    for index in 0..3 {
        let path = cache.root.join(format!("{index}.png"));
        let file = fs::File::create(&path).expect("owned cache entry");
        file.set_len(size).expect("logical cache length");
        file.set_times(
            fs::FileTimes::new()
                .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(100 + index)),
        )
        .expect("ordered modification times");
        drop(file);
        cache.prune().expect("prune");
        assert_eq!(
            fs::read_dir(&cache.root).expect("entries").count(),
            (index + 1).min(2) as usize
        );
        assert!(path.exists(), "newest entry is retained");
    }
    assert!(!cache.root.join("0.png").exists());
    for index in 1..3 {
        let metadata =
            fs::metadata(cache.root.join(format!("{index}.png"))).expect("retained entry");
        assert_eq!(metadata.len(), size);
        assert_eq!(
            metadata.modified().expect("modified"),
            SystemTime::UNIX_EPOCH + Duration::from_secs(100 + index)
        );
    }
    cache.prune().expect("exact-limit no-op");
    assert_eq!(fs::read_dir(&cache.root).expect("entries").count(), 2);
    fs::remove_dir_all(&cache.root).expect("remove owned cache");
}

#[test]
#[ignore = "creates 8192 owned small PNG cache entries; warm filesystem timing"]
fn populated_preview_cache_reports_pruning_cost() {
    // Historical control: enumerate the same metadata and sort even below the limit.
    fn historical(cache: &PreviewCache) {
        let mut entries: Vec<_> = fs::read_dir(&cache.root)
            .expect("cache directory")
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                metadata.is_file().then(|| {
                    (
                        entry.path(),
                        metadata.len(),
                        metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    )
                })
            })
            .collect();
        let mut total: u64 = entries.iter().map(|(_, size, _)| size).sum();
        entries.sort_by_key(|(_, _, modified)| *modified);
        for (path, size, _) in entries {
            if total <= CACHE_LIMIT_BYTES {
                break;
            }
            if fs::remove_file(path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }
    let cache = cache("prune-cost");
    let bytes = png();
    let mut prepared = 0_u32;
    for count in [512_u32, 8192] {
        for index in prepared..count {
            // Hash-like names keep directory order separate from creation/age order.
            let name = index.wrapping_mul(2_654_435_761);
            fs::write(cache.root.join(format!("{name:08x}.png")), &bytes).expect("owned PNG");
        }
        prepared = count;
        assert!(u64::from(count) * bytes.len() as u64 <= CACHE_LIMIT_BYTES);
        for old in [true, false, false, true] {
            let mut timings = Vec::new();
            for _ in 0..9 {
                let start = std::time::Instant::now();
                if old {
                    historical(&cache);
                } else {
                    cache.prune().expect("current pruning");
                }
                timings.push(start.elapsed());
            }
            timings.sort();
            eprintln!(
                "prune entries={count} historical={old} median_ms={:.3}",
                timings[4].as_secs_f64() * 1000.0
            );
        }
        let entries: Vec<_> = fs::read_dir(&cache.root)
            .expect("retained entries")
            .map(|entry| entry.expect("entry"))
            .collect();
        assert_eq!(entries.len(), count as usize);
        for entry in entries {
            assert_eq!(fs::read(entry.path()).expect("unchanged PNG"), bytes);
        }
    }
    fs::remove_dir_all(&cache.root).expect("remove owned benchmark cache");
}

fn animation_fixture(root: &Path, extension: &str, size: (u32, u32), poster: bool) -> PathBuf {
    let path = root.join(format!("animation.{extension}"));
    let frames: Vec<_> = [0, 1]
        .into_iter()
        .map(|index| {
            image::RgbaImage::from_fn(size.0, size.1, |x, y| {
                image::Rgba([
                    (x % 251) as u8,
                    (y % 253) as u8,
                    if index == 0 { 200 } else { 30 },
                    if x < size.0 / 4 { 0 } else { 255 },
                ])
            })
        })
        .collect();
    if extension == "gif" {
        let mut encoder =
            image::codecs::gif::GifEncoder::new(fs::File::create(&path).expect("GIF"));
        encoder
            .encode_frames(frames.into_iter().map(|frame| {
                image::Frame::from_parts(frame, 0, 0, image::Delay::from_numer_denom_ms(500, 1))
            }))
            .expect("GIF frames");
    } else if extension == "webp" {
        let input = animation_fixture(root, "apng", size, false);
        let result = hidden_command(
            &tool_path("ffmpeg.exe").expect("fixed FFmpeg"),
            ["-v", "error", "-i"],
        )
        .arg(input)
        .args(["-c:v", "libwebp_anim", "-lossless", "1", "-threads", "1"])
        .arg(&path)
        .output()
        .expect("WebP fixture");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    } else {
        let mut encoder = png::Encoder::new(fs::File::create(&path).expect("APNG"), size.0, size.1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 0).expect("animation");
        encoder.set_sep_def_img(poster).expect("poster flag");
        let mut writer = encoder.write_header().expect("APNG header");
        if poster {
            writer
                .write_image_data(
                    image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([0, 255, 0, 255]))
                        .as_raw(),
                )
                .expect("poster");
        }
        for frame in frames {
            writer.set_frame_delay(1, 2).expect("delay");
            writer.write_image_data(frame.as_raw()).expect("APNG frame");
        }
        writer.finish().expect("APNG finish");
    }
    path
}

#[test]
fn animated_thumbnails_use_the_first_composited_frame_with_bounded_native_decoding() {
    for (extension, poster) in [
        ("gif", false),
        ("apng", false),
        ("png", true),
        ("webp", false),
    ] {
        let root = cache("native-animation-thumbnail").root;
        let source = animation_fixture(&root, extension, (320, 200), poster);
        let reference = crate::decode_image(&source).expect("original animation");
        assert_eq!(reference.frames.len(), 2, "{extension}");
        let first = &reference.frames[0];
        let expected = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(first.width, first.height, first.rgba.clone())
                .expect("first frame"),
        )
        .resize(240, 160, image::imageops::FilterType::Nearest)
        .into_rgba8();
        let (bytes, ready) = static_thumbnail_ready(&source, 320 * 200 * 4, &|| true)
            .expect("native thumbnail without external FFmpeg fallback");
        assert_eq!(thumbnail_source_size(&bytes), Some((320, 200)));
        let preview = decode_png(&bytes).expect("thumbnail PNG");
        assert_eq!(ready, preview, "published pixels equal persisted pixels");
        assert_eq!((preview.width, preview.height), expected.dimensions());
        assert_eq!(
            preview.rgba.as_ref(),
            expected.as_raw(),
            "first frame, alpha and sampling: {extension}"
        );
        assert!(
            preview
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 0)
        );
        assert!(static_thumbnail_png(&source, 1, &|| true).is_none());
        assert!(static_thumbnail_png(&source, STATIC_THUMBNAIL_BYTE_LIMIT, &|| false).is_none());
        let previews = PreviewCache::new(root.join("cache")).expect("cache");
        assert_eq!(
            previews
                .filmstrip(&source, MediaKind::Image)
                .expect("card")
                .image,
            preview
        );
        assert_eq!(
            PreviewCache::new(root.join("cache"))
                .expect("reopened cache")
                .filmstrip(&source, MediaKind::Image)
                .expect("persisted card")
                .image,
            preview
        );
        fs::write(&source, b"broken animation").expect("corrupt owned source");
        assert!(static_thumbnail_png(&source, STATIC_THUMBNAIL_BYTE_LIMIT, &|| true).is_none());
        fs::remove_dir_all(root).expect("remove owned animation and cache");
    }
}

#[test]
#[ignore = "Release comparison; generates owned GIF/APNG/WebP fixtures and runs the former helper path"]
fn animation_thumbnail_native_generation_reports_helper_comparison() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("run this measurement with --release");
    }
    for (extension, size) in [
        ("gif", (1920, 1080)),
        ("apng", (4000, 2400)),
        ("webp", (1920, 1080)),
    ] {
        let root = cache("animation-thumbnail-measurement").root;
        let source = animation_fixture(&root, extension, size, false);
        let reference = crate::decode_image(&source).expect("reference animation");
        let frame = &reference.frames[0];
        let expected = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
                .expect("reference pixels"),
        )
        .resize(240, 160, image::imageops::FilterType::Nearest)
        .into_rgba8();
        drop(reference);
        let baseline = || {
            frame_preview(
                &source,
                Duration::ZERO,
                "scale=240:160:force_original_aspect_ratio=decrease:reset_sar=1",
                None,
            )
        };
        let supported = match baseline() {
            Ok(bytes) => {
                let image = decode_png(&bytes).expect("helper PNG");
                assert_eq!((image.width, image.height), expected.dimensions());
                true
            }
            Err(error) => {
                println!("ANIMATION_THUMBNAIL {extension} helper unavailable: {error}");
                false
            }
        };
        // Warm filesystem, uncached generation, same-binary ABBA. Helper includes process startup;
        // neither path includes disk-cache persistence, GPU upload, UI or the initial fixture work.
        for native in [false, true, true, false] {
            if !native && !supported {
                continue;
            }
            let mut samples = Vec::new();
            for _ in 0..3 {
                let start = std::time::Instant::now();
                let bytes = if native {
                    static_thumbnail_png(&source, STATIC_THUMBNAIL_BYTE_LIMIT, &|| true)
                        .expect("native thumbnail")
                } else {
                    baseline().expect("helper thumbnail")
                };
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
                let image = decode_png(&bytes).expect("thumbnail pixels");
                assert_eq!((image.width, image.height), expected.dimensions());
                if native {
                    assert_eq!(image.rgba.as_ref(), expected.as_raw());
                }
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "ANIMATION_THUMBNAIL {extension} {}x{} native={native} median_ms={:.3}",
                size.0, size.1, samples[1]
            );
        }
        fs::remove_dir_all(root).expect("remove owned measurement fixtures");
    }
    Ok(())
}

#[test]
fn unavailable_disk_cache_still_coalesces_and_reuses_generated_pixels() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let root = cache("preview-memory-without-disk").root;
    let occupied = root.join("occupied");
    fs::write(&occupied, b"preserve occupied path").expect("owned collision");
    let cache = PreviewCache::new(occupied.clone()).expect("optional disk storage");
    let bytes = png();
    let expected = decode_png(&bytes).expect("expected pixels");
    let generated = AtomicUsize::new(0);
    let ready = std::sync::Barrier::new(4);
    thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    ready.wait();
                    cache
                        .clone()
                        .load_or_generate("shared".into(), || {
                            generated.fetch_add(1, Ordering::Relaxed);
                            Ok(bytes.clone())
                        })
                        .expect("preview without disk")
                })
            })
            .collect();
        for worker in workers {
            assert_eq!(worker.join().expect("worker"), expected);
        }
    });
    assert_eq!(
        generated.load(Ordering::Relaxed),
        1,
        "one decode across simultaneous callers even if persistence fails"
    );
    assert_eq!(
        cache
            .load_or_generate("shared".into(), || panic!("reuse memory"))
            .expect("memory hit"),
        expected
    );
    let cancellation = Cancellation::default();
    cancellation.cancel();
    assert!(matches!(
        cache
            .cancellable(cancellation)
            .load_or_generate("shared".into(), || panic!("cancelled")),
        Err(PreviewError::Cancelled)
    ));
    assert_eq!(
        fs::read(&occupied).expect("occupied path"),
        b"preserve occupied path"
    );
    fs::remove_file(occupied).expect("remove owned collision");
    fs::remove_dir(root).expect("remove owned directory");
}

#[test]
#[ignore = "Release measurement; set TOWAVUE_PREVIEW_BENCH_IMAGE to a large PNG"]
fn large_image_preview_reuse_without_disk_reports_repeated_request_cost() {
    let source =
        PathBuf::from(std::env::var_os("TOWAVUE_PREVIEW_BENCH_IMAGE").expect("large PNG path"));
    let dimensions = image::image_dimensions(&source).expect("source dimensions");
    assert!(dimensions.0.max(dimensions.1) >= 4000);
    let root = cache("preview-no-disk-measurement").root;
    let occupied = root.join("occupied");
    fs::write(&occupied, b"preserve cache collision").expect("owned collision");
    let mut expected = None;
    // Same-binary ABBA comparison; clearing memory models the previous failure path.
    // Each batch includes first generation and three requests for the same image.
    for retain in [false, true, true, false] {
        let cache = PreviewCache::new(occupied.clone()).expect("optional disk cache");
        let mut samples = Vec::new();
        for _ in 0..3 {
            *cache.memory.lock().expect("memory") = PreviewMemory::default();
            let started = std::time::Instant::now();
            for _ in 0..4 {
                if !retain {
                    *cache.memory.lock().expect("memory") = PreviewMemory::default();
                }
                let image = cache
                    .filmstrip(&source, MediaKind::Image)
                    .expect("preview")
                    .image;
                if let Some(expected) = &expected {
                    assert_eq!(&image, expected);
                } else {
                    expected = Some(image);
                }
            }
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "preview-no-disk source={}x{} retain_memory={retain} requests=4 batch_median_ms={:.3}",
            dimensions.0, dimensions.1, samples[1]
        );
    }
    assert_eq!(
        fs::read(&occupied).expect("collision"),
        b"preserve cache collision"
    );
    fs::remove_file(occupied).expect("remove owned collision");
    fs::remove_dir(root).expect("remove owned measurement directory");
}

#[test]
fn avif_thumbnail_revision_leaves_other_format_cache_keys_unchanged() {
    let cache = cache("avif-thumbnail-revision");
    for extension in ["avif", "AVIF", "png", "jpg"] {
        let source = cache.root.join(format!("source.{extension}"));
        fs::write(&source, png()).expect("fixture");
        let metadata = source.metadata().expect("fixture");
        let mut old = DefaultHasher::new();
        source
            .canonicalize()
            .expect("fixture")
            .to_string_lossy()
            .to_lowercase()
            .hash(&mut old);
        metadata.len().hash(&mut old);
        metadata
            .modified()
            .expect("fixture")
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("fixture")
            .as_nanos()
            .hash(&mut old);
        let prior_avif = old.clone();
        IMAGE_PREVIEW_VARIANT.hash(&mut old);
        let old = format!("{:016x}", old.finish());
        let current = cache_key(&source, IMAGE_PREVIEW_VARIANT).expect("current key");
        if extension.eq_ignore_ascii_case("avif") {
            for version in [
                "filmstrip-avif-v1",
                "filmstrip-avif-v2",
                "filmstrip-avif-v3",
            ] {
                let mut prior = prior_avif.clone();
                version.hash(&mut prior);
                assert_ne!(current, format!("{:016x}", prior.finish()));
            }
        }
        assert_eq!(current == old, !extension.eq_ignore_ascii_case("avif"));
    }
    fs::remove_dir_all(&cache.root).expect("owned fixture cleanup");
}

#[test]
fn cached_filmstrip_uses_memory_only_and_preserves_duration_and_source_identity() {
    let cache = cache("filmstrip-memory-priority");
    let image = decode_png(&png()).expect("fixture pixels");
    for (kind, variant) in [
        (MediaKind::Image, IMAGE_PREVIEW_VARIANT),
        (MediaKind::Video, "filmstrip-video-v4"),
        (MediaKind::Audio, "waveform-v3-240-160"),
    ] {
        let source = cache.root.join(format!("{kind:?}"));
        fs::write(&source, b"not decodable media").expect("source identity only");
        let key = cache_key(&source, variant).expect("key");
        fs::write(cache.root.join(format!("{key}.png")), png()).expect("disk cache");
        assert!(
            cache
                .cached_filmstrip(&source, kind)
                .expect("no disk lookup")
                .is_none()
        );
        cache
            .memory
            .lock()
            .expect("memory")
            .insert(key, image.clone());
        let duration = if kind == MediaKind::Image {
            None
        } else {
            assert!(
                cache
                    .cached_filmstrip(&source, kind)
                    .expect("no media probe")
                    .is_none()
            );
            cache.memory.lock().expect("memory").durations.push_back((
                cache_key(&source, "duration-v1").expect("duration key"),
                Duration::from_secs(7),
            ));
            Some(Duration::from_secs(7))
        };
        let cached = cache
            .clone()
            .cached_filmstrip(&source, kind)
            .expect("shared memory lookup")
            .expect("cached card");
        assert_eq!(cached.image, image);
        assert_eq!(cached.duration, duration);
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert!(matches!(
            cache
                .cancellable(cancellation)
                .cached_filmstrip(&source, kind),
            Err(PreviewError::Cancelled)
        ));
        fs::write(&source, b"changed source identity and length").expect("replace fixture");
        assert!(
            cache
                .cached_filmstrip(&source, kind)
                .expect("changed key")
                .is_none()
        );
    }
    fs::remove_dir_all(cache.root).expect("remove owned fixtures");
}

#[test]
fn persisted_thumbnail_dimensions_reject_invalid_metadata_and_keep_legacy_pixels() {
    let cache = cache("thumbnail-dimension-validation");
    let source = cache.root.join("source.png");
    image::RgbImage::from_pixel(480, 320, image::Rgb([24, 48, 96]))
        .save(&source)
        .expect("owned PNG");
    let valid = static_thumbnail_png(&source, STATIC_THUMBNAIL_BYTE_LIMIT, &|| true)
        .expect("encoded thumbnail");
    assert_eq!(thumbnail_source_size(&valid), Some((480, 320)));
    let mut memory = PreviewMemory::default();
    memory.insert_encoded("known".into(), decode_png(&valid).expect("pixels"), &valid);
    memory.insert_encoded(
        "known".into(),
        decode_png(&png()).expect("legacy pixels"),
        &png(),
    );
    assert_eq!(
        memory.entries[0].source_size,
        Some((480, 320)),
        "a late legacy cache read must not erase known dimensions for the same source key"
    );
    for end in 0..53 {
        assert_eq!(thumbnail_source_size(&valid[..end]), None);
    }
    let key = cache_key(&source, IMAGE_PREVIEW_VARIANT).expect("key");
    let disk = cache.root.join(format!("{key}.png"));
    for mode in [
        "legacy",
        "checksum",
        "zero",
        "oversize",
        "overflow",
        "preview-size",
        "length",
        "pixels",
        "file-size",
    ] {
        let mut bytes = valid.clone();
        match mode {
            "legacy" => {
                bytes.drain(33..53);
            }
            "checksum" => bytes[49] ^= 1,
            "zero" => bytes[41..45].copy_from_slice(&0_u32.to_be_bytes()),
            "oversize" => bytes[41..49]
                .copy_from_slice(&[32768_u32.to_be_bytes(), 32768_u32.to_be_bytes()].concat()),
            "overflow" => bytes[41..49].fill(255),
            "preview-size" => bytes[16..20].copy_from_slice(&241_u32.to_be_bytes()),
            "length" => bytes[36] = 7,
            "pixels" => bytes.truncate(53),
            "file-size" => bytes.resize(IMAGE_PREVIEW_FILE_LIMIT + 1, 0),
            _ => unreachable!(),
        }
        if matches!(mode, "zero" | "oversize" | "overflow") {
            let crc = crc32fast::hash(&bytes[37..49]);
            bytes[49..53].copy_from_slice(&crc.to_be_bytes());
        }
        fs::write(&disk, &bytes).expect("owned invalid/legacy cache");
        let fresh = PreviewCache::new(cache.root.clone()).expect("fresh memory");
        assert!(
            fresh
                .cached_image(&source)
                .expect("optional invalid cache")
                .is_none(),
            "{mode}"
        );
        assert_eq!(fs::read(&disk).expect("cache retained"), bytes);
        if mode == "legacy" {
            assert_eq!(
                fresh
                    .filmstrip(&source, MediaKind::Image)
                    .expect("legacy thumbnail remains usable")
                    .image,
                decode_png(&valid).expect("pixels")
            );
            assert!(
                fresh
                    .cached_image(&source)
                    .expect("unknown legacy dimensions")
                    .is_none()
            );
        }
    }
    fs::write(&disk, &valid).expect("restore owned cache");
    let token = Cancellation::default();
    token.cancel();
    assert!(matches!(
        cache.cancellable(token).cached_image(&source),
        Err(PreviewError::Cancelled)
    ));
    fs::write(&source, b"changed source identity").expect("replace owned source");
    assert!(cache.cached_image(&source).expect("new key").is_none());
    fs::remove_dir_all(&cache.root).expect("remove owned fixtures");
}

#[test]
fn persisted_thumbnail_geometry_uses_all_exif_orientations() {
    use image::ImageEncoder;
    let cache = cache("thumbnail-oriented-dimensions");
    let pixels = image::RgbaImage::from_fn(48, 32, |x, y| {
        image::Rgba([x as u8 * 4, y as u8 * 6, 91, 127])
    });
    for tag in 1..=8 {
        let source = cache.root.join(format!("oriented-{tag}.png"));
        let mut exif = *b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0";
        exif[18] = tag;
        let file = fs::File::create(&source).expect("owned PNG");
        let mut encoder = image::codecs::png::PngEncoder::new(file);
        encoder.set_exif_metadata(exif.to_vec()).expect("EXIF");
        encoder
            .write_image(pixels.as_raw(), 48, 32, image::ExtendedColorType::Rgba8)
            .expect("oriented source");
        let thumbnail = cache
            .filmstrip(&source, MediaKind::Image)
            .expect("thumbnail");
        let fresh = PreviewCache::new(cache.root.clone()).expect("fresh memory");
        let preview = fresh
            .cached_image(&source)
            .expect("lookup")
            .expect("persisted geometry");
        let mut expected = image::DynamicImage::ImageRgba8(pixels.clone());
        expected
            .apply_orientation(image::metadata::Orientation::from_exif(tag).expect("orientation"));
        assert_eq!(
            preview.source_size,
            (expected.width(), expected.height()),
            "orientation={tag}"
        );
        let small = expected
            .resize(240, 160, image::imageops::FilterType::Nearest)
            .into_rgba8();
        assert_eq!(preview.image, thumbnail.image);
        assert_eq!(preview.image.rgba.as_slice(), small.into_raw());
    }
    fs::remove_dir_all(&cache.root).expect("remove owned fixtures");
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
        // JPEG cards use reduced decoding when possible, not the full decoder's
        // nearest-neighbor samples checked above. Other formats keep those pixels.
        let direct = if extension == "jpg" {
            crate::image::jpeg_thumbnail(&path, required, &|| true)
                .expect("reduced JPEG")
                .expect("half-size thumbnail")
                .image
        } else {
            direct
        };
        assert!(
            cache
                .filmstrip(&path, MediaKind::Image)
                .expect("integrated thumbnail")
                .image
                == direct,
            "integrated thumbnail must use the format's prepared pixels"
        );
        let cached = cache
            .cached_filmstrip(&path, MediaKind::Image)
            .expect("memory lookup")
            .expect("warm image");
        assert_eq!(cached.image, direct);
        assert_eq!(cached.duration, None);
        let fresh = PreviewCache::new(cache.root.clone()).expect("fresh memory");
        for consumer in [&cache, &fresh] {
            let preview = consumer
                .cached_image(&path)
                .expect("lookup")
                .expect("direct thumbnail retains source dimensions for first display");
            assert_eq!(preview.source_size, (480, 320));
            assert_eq!(preview.image, direct);
        }
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
    let first = crate::decode_image(&gif)
        .expect("original GIF")
        .frames
        .remove(0);
    let preview = decode_png(
        &static_thumbnail_png(&gif, STATIC_THUMBNAIL_BYTE_LIMIT, &|| true)
            .expect("native first-frame thumbnail"),
    )
    .expect("GIF thumbnail PNG");
    assert_eq!((preview.width, preview.height), (240, 120));
    assert!(
        preview
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == first.rgba[..4])
    );
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
            if extension == "jpg" { 2 } else { 1 },
            "persist JPEG; keep cheap BMP sampling memory-only"
        );
        let reopened = PreviewCache::new(cache.root.clone()).expect("new session cache");
        let persisted = reopened.cached_image(&path).expect("disk lookup");
        if extension == "jpg" {
            let persisted = persisted.expect("reuse fast JPEG without decoding the original");
            assert_eq!(persisted.source_size, shared.source_size);
            assert_eq!(persisted.image, first.image);
        } else {
            assert!(
                persisted.is_none(),
                "BMP does not create a slower disk cache"
            );
        }
        let foreground =
            PreviewCache::new(cache.root.join("foreground")).expect("foreground cache");
        assert_eq!(
            foreground
                .prepare_image_preview(&path, crate::image::IMAGE_BYTE_LIMIT, &|| true)
                .expect("speculative first preview")
                .image,
            first.image
        );
        assert_eq!(
            fs::read_dir(&foreground.root)
                .expect("foreground directory")
                .count(),
            0,
            "foreground speculation must not encode or persist a thumbnail"
        );
        let occupied = cache.root.join("occupied");
        fs::write(&occupied, b"preserve occupied path").expect("owned collision");
        let blocked = PreviewCache::new(occupied.clone()).expect("optional storage");
        assert_eq!(
            blocked
                .filmstrip(&path, MediaKind::Image)
                .expect("memory fallback")
                .image,
            first.image
        );
        assert_eq!(
            fs::read(&occupied).expect("unchanged collision"),
            b"preserve occupied path"
        );
        fs::remove_file(&occupied).expect("remove owned collision");
        fs::create_dir(&occupied).expect("make storage available");
        assert_eq!(
            blocked
                .filmstrip(&path, MediaKind::Image)
                .expect("memory reuse")
                .image,
            first.image
        );
        assert_eq!(
            fs::read_dir(&occupied)
                .expect("available directory")
                .count(),
            0,
            "memory hits do not retry failed persistence"
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
fn medium_jpeg_thumbnail_persists_without_starting_a_speculative_preview() {
    let root = cache("medium-jpeg-thumbnail").root;
    let source = root.join("medium.jpg");
    image::RgbImage::from_fn(1920, 1080, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 253) as u8, ((x ^ y) % 255) as u8])
    })
    .save(&source)
    .expect("owned JPEG");
    let store = PreviewCache::new(root.join("cache")).expect("cache");
    assert!(
        store
            .prepare_image_preview(&source, crate::image::IMAGE_BYTE_LIMIT, &|| true)
            .is_none()
    );
    let expected = crate::image::jpeg_thumbnail(&source, crate::image::IMAGE_BYTE_LIMIT, &|| true)
        .expect("reduced decoder")
        .expect("medium JPEG");
    let generated = store
        .filmstrip(&source, MediaKind::Image)
        .expect("thumbnail");
    assert_eq!(generated.image, expected.image);
    let fresh = PreviewCache::new(store.root.clone()).expect("fresh cache");
    let saved = fresh
        .cached_image(&source)
        .expect("disk read")
        .expect("saved thumbnail");
    assert_eq!(saved.source_size, expected.source_size);
    assert_eq!(saved.image, expected.image);
    assert_eq!(
        fresh
            .filmstrip(&source, MediaKind::Image)
            .expect("reuse")
            .image,
        generated.image
    );
    fs::remove_dir_all(root).expect("owned source and caches");
}

#[test]
#[ignore = "Release comparison of medium-JPEG full and reduced thumbnail generation"]
fn medium_jpeg_thumbnail_reports_full_decode_comparison() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("run this measurement with --release");
    }
    for (width, height) in [(503, 317), (1001, 701), (1920, 1080)] {
        let root = cache("medium-jpeg-measurement").root;
        let source = root.join("source.jpg");
        image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 253) as u8, ((x ^ y) % 255) as u8])
        })
        .save(&source)
        .expect("fixture");
        let original = fs::read(&source).expect("warm source");
        for (phase, reduced) in [false, true, true, false].into_iter().enumerate() {
            let mut samples = Vec::new();
            for sample in 0..5 {
                let store =
                    PreviewCache::new(root.join(format!("{phase}-{sample}"))).expect("empty cache");
                let started = std::time::Instant::now();
                let image = if reduced {
                    store
                        .filmstrip(&source, MediaKind::Image)
                        .expect("reduced thumbnail")
                        .image
                } else {
                    store
                        .load_or_generate(
                            cache_key(&source, IMAGE_PREVIEW_VARIANT).expect("key"),
                            || {
                                Ok(static_thumbnail_png(
                                    &source,
                                    STATIC_THUMBNAIL_BYTE_LIMIT,
                                    &|| true,
                                )
                                .expect("full decode and resize"))
                            },
                        )
                        .expect("full thumbnail")
                };
                samples.push(started.elapsed().as_secs_f64() * 1000.0);
                assert!(image.width <= 240 && image.height <= 160);
                let fresh = PreviewCache::new(store.root.clone()).expect("fresh cache");
                let saved = fresh
                    .cached_image(&source)
                    .expect("readback")
                    .expect("geometry");
                assert_eq!(saved.source_size, (width, height));
                assert_eq!(saved.image, image);
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "MEDIUM_JPEG {width}x{height} reduced={reduced} median_ms={:.3}",
                samples[2]
            );
        }
        assert_eq!(fs::read(&source).expect("unchanged source"), original);
        fs::remove_dir_all(root).expect("owned fixtures");
    }
    Ok(())
}

#[test]
fn persisted_fast_preview_keeps_large_source_geometry_without_a_full_canvas() {
    let root = cache("large-fast-preview-geometry").root;
    let cache = PreviewCache::new(root.join("cache")).expect("cache separate from source");
    let path = root.join("large.jpg");
    let (width, height) = (8000_u32, 6000_u32);
    image::GrayImage::new(width, height)
        .save(&path)
        .expect("owned grayscale JPEG");
    let preview = cache
        .filmstrip(&path, MediaKind::Image)
        .expect("reduced JPEG")
        .image;
    assert_eq!((preview.width, preview.height), (213, 160));
    let reopened = PreviewCache::new(cache.root.clone()).expect("fresh memory");
    let saved = reopened
        .cached_image(&path)
        .expect("disk lookup")
        .expect("persisted fast preview");
    assert_eq!(saved.source_size, (width, height));
    assert_eq!(saved.image, preview);
    fs::remove_dir_all(root).expect("remove owned JPEG and cache");
}

#[test]
#[ignore = "Release comparison of fast-preview generation, persistence and fresh-memory disk reuse"]
fn fast_preview_persistence_reports_generation_and_reopen_cost() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("run this measurement with --release");
    }
    for extension in ["jpg", "bmp"] {
        let root = cache("fast-preview-persistence-measurement").root;
        let source = root.join(format!("source.{extension}"));
        image::RgbImage::from_fn(6000, 4000, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 253) as u8, ((x ^ y) % 255) as u8])
        })
        .save(&source)
        .expect("owned large source");
        let expected =
            crate::image::first_image_preview(&source, crate::image::IMAGE_BYTE_LIMIT, &|| true)
                .expect("fast decoder")
                .expect("supported format")
                .image;
        // Warm source files; separate empty caches on each sample. The former path is the
        // same non-persisting preparation method; new-session reuse has fresh cache memory.
        for (phase, card_request) in [false, true, true, false].into_iter().enumerate() {
            let mut generation = Vec::new();
            let mut reopen = Vec::new();
            for sample in 0..3 {
                let store =
                    PreviewCache::new(root.join(format!("{phase}-{sample}"))).expect("empty cache");
                let start = std::time::Instant::now();
                let image = if card_request {
                    store
                        .filmstrip(&source, MediaKind::Image)
                        .expect("card preview")
                        .image
                } else {
                    store
                        .prepare_image_preview(&source, crate::image::IMAGE_BYTE_LIMIT, &|| true)
                        .expect("former fast preview")
                        .image
                };
                generation.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(image, expected);
                if card_request {
                    let fresh = PreviewCache::new(store.root.clone()).expect("new session memory");
                    let start = std::time::Instant::now();
                    let image = fresh
                        .filmstrip(&source, MediaKind::Image)
                        .expect("new-session preview");
                    reopen.push(start.elapsed().as_secs_f64() * 1000.0);
                    assert_eq!(image.image, expected);
                }
            }
            generation.sort_by(f64::total_cmp);
            reopen.sort_by(f64::total_cmp);
            println!(
                "FAST_PREVIEW_PERSIST {extension} 6000x4000 card_request={card_request} generation_ms={:.3} reopen_ms={:?}",
                generation[1],
                reopen.get(1)
            );
        }
        fs::remove_dir_all(root).expect("remove owned source and caches");
    }
    Ok(())
}

#[test]
fn static_first_preview_reuses_pixels_without_waiting_for_another_generator() {
    for extension in ["jpg", "bmp", "gif"] {
        let prepare: fn(
            &PreviewCache,
            &Path,
            usize,
            &dyn Fn() -> bool,
        ) -> Option<CachedImagePreview> = if extension == "gif" {
            PreviewCache::prepare_animation_preview
        } else {
            PreviewCache::prepare_image_preview
        };
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
            sent.send(prepare(
                &worker_cache,
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
        let preview = prepare(&cache, &path, crate::image::IMAGE_BYTE_LIMIT, &|| true)
            .expect("generate after release");
        let held = cache.claim_generation(&key).expect("hold generation again");
        assert_eq!(
            prepare(&cache, &path, crate::image::IMAGE_BYTE_LIMIT, &|| true)
                .expect("warm reuse")
                .image,
            preview.image
        );
        assert!(prepare(&cache, &path, crate::image::IMAGE_BYTE_LIMIT, &|| false).is_none());
        drop(held);
        fs::write(&path, b"changed input").expect("replace owned source");
        assert!(prepare(&cache, &path, crate::image::IMAGE_BYTE_LIMIT, &|| true).is_none());
        assert!(cache.in_flight.0.lock().expect("pending").is_empty());
        fs::remove_dir_all(&cache.root).expect("remove owned cache");
    }
}

#[test]
fn speculative_preview_drops_changed_cancelled_or_failed_results_and_releases_its_lease() {
    for mode in ["changed", "cancelled", "failed"] {
        let cache = cache("preview-publication");
        let path = cache.root.join("source.gif");
        fs::write(&path, b"source identity").expect("owned source");
        let current = std::cell::Cell::new(true);
        let result = cache.prepare_preview(&path, &|| current.get(), || {
            match mode {
                "changed" => fs::write(&path, b"new identity").expect("change owned source"),
                "cancelled" => current.set(false),
                "failed" => return None,
                _ => unreachable!(),
            }
            Some(CachedImagePreview {
                source_size: (1, 1),
                image: preview_pixels(1, 1, &[12, 34, 56, 255]),
            })
        });
        assert!(result.is_none(), "{mode}");
        assert!(cache.memory.lock().expect("memory").entries.is_empty());
        assert!(cache.in_flight.0.lock().expect("lease").is_empty());
        fs::remove_dir_all(&cache.root).expect("remove owned fixtures");
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
