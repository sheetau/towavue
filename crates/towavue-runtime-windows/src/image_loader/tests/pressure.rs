use super::*;

#[test]
#[ignore = "generated large PNG/APNG spreads exercise the production memory budget"]
fn large_reading_spreads_preserve_pixels_and_cache_hits_under_byte_pressure() {
    let root = std::env::temp_dir().join(format!("towavue-large-spreads-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("owned fixture directory");
    let mut sources = Vec::new();
    for page in 0..12_u8 {
        let animated = page % 3 == 1;
        let (width, height, frames) = if animated {
            (512, 512, 128)
        } else {
            (4096, 2304, 1)
        };
        let path = root.join(format!("{page}.png"));
        let mut encoder = png::Encoder::new(
            std::fs::File::create(&path).expect("fixture"),
            width,
            height,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        if animated {
            encoder.set_animated(frames, 3).expect("animation");
        }
        let mut writer = encoder.write_header().expect("PNG header");
        let mut expected = Vec::new();
        for frame in 0..frames {
            let delay = if animated {
                writer
                    .set_blend_op(png::BlendOp::Source)
                    .expect("source frame");
                writer
                    .set_frame_delay((frame % 3 + 1) as u16, 100)
                    .expect("frame delay");
                Duration::from_millis(u64::from(frame % 3 + 1) * 10)
            } else {
                Duration::ZERO
            };
            let pixels = [page * 17, frame as u8, 255 - page * 17, 255]
                .repeat(width as usize * height as usize);
            expected.push((crc32fast::hash(&pixels), delay));
            writer.write_image_data(&pixels).expect("PNG frame");
        }
        writer.finish().expect("PNG tail");
        let stamp = ImageStamp::read(&path).expect("source stamp");
        sources.push((path, width, height, expected, stamp));
    }
    let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
    let cache = Arc::clone(&shared.0.lock().expect("mailbox").cache);
    assert_eq!(cache.lock().expect("cache").byte_limit, 384 * 1024 * 1024);
    let loader = ImageLoader {
        shared: Arc::clone(&shared),
        prefetch_worker: LatestTask::new("large-spread-prefetch").expect("prefetch worker"),
    };
    let (ready_tx, ready_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        run_worker(
            shared,
            || {
                let _ = ready_tx.send(());
            },
            decode_image_with_preview,
        )
    });
    let mut displayed = Vec::new();
    let mut owners = Vec::new();
    let mut expected_decodes = 0;
    let mut partial_hits = 0;
    let mut total_pages = 0;
    for spread in [0, 1, 0, 1, 2, 3, 2, 1, 0, 3, 2, 1]
        .into_iter()
        .cycle()
        .take(36)
    {
        let selected = &sources[spread * 3..spread * 3 + 3];
        let paths: Vec<_> = selected.iter().map(|source| source.0.clone()).collect();
        // Weak references observe cache identity without protecting entries from eviction.
        let hits: Vec<_> = {
            let cache = cache.lock().expect("cache");
            paths
                .iter()
                .map(|path| {
                    cache
                        .entries
                        .iter()
                        .find(|entry| &entry.0 == path)
                        .map(|entry| Arc::downgrade(&entry.2))
                })
                .collect()
        };
        let hit_count = hits.iter().filter(|hit| hit.is_some()).count();
        expected_decodes += 3 - hit_count;
        partial_hits += usize::from(hit_count > 0 && hit_count < 3);
        let generation = loader.request_originals_with_retained_bytes(paths, 0, &[]);
        let mut loaded = Vec::new();
        while loaded.len() < 3 {
            ready_rx
                .recv_timeout(Duration::from_secs(60))
                .expect("page ready");
            if let Some(result) = loader.take_completed() {
                assert_eq!(result.generation, generation);
                assert_eq!(result.first_index, loaded.len());
                loaded.extend(
                    result
                        .images
                        .into_iter()
                        .map(|(path, image)| (path, image.expect("original"))),
                );
            }
        }
        for (index, ((path, image), source)) in loaded.iter().zip(selected).enumerate() {
            assert_eq!(path, &source.0);
            assert_eq!(image.dimensions(), (source.1, source.2));
            assert_eq!(image.frames.len(), source.3.len());
            assert_eq!(
                image.retained_bytes(),
                source.1 as usize * source.2 as usize * 4 * source.3.len()
            );
            if image.is_animated() {
                assert_eq!(image.animation_plays, 3);
            }
            for (frame, (digest, delay)) in image.frames.iter().zip(&source.3) {
                assert_eq!(crc32fast::hash(&frame.rgba), *digest, "full-frame digest");
                assert_eq!(frame.delay, *delay);
            }
            let weak = Arc::downgrade(image);
            if let Some(hit) = &hits[index] {
                assert!(std::sync::Weak::ptr_eq(hit, &weak));
            }
            owners.push(weak);
        }
        total_pages += loaded.len();
        displayed = loaded;
        let cache = cache.lock().expect("cache");
        assert!(cache.bytes <= CACHE_BYTE_LIMIT && cache.entries.len() <= CACHE_ENTRY_LIMIT);
        assert_eq!(
            cache.bytes,
            cache
                .entries
                .iter()
                .map(|entry| entry.2.retained_bytes())
                .sum::<usize>()
        );
        assert_eq!(
            loader.verification_metrics().foreground.calls,
            expected_decodes as u64
        );
    }
    // Each spread is 200 MiB: two cannot fit in 384 MiB. The cold first
    // traversal has three partial hits; the two repeated traversals have four each.
    assert_eq!(partial_hits, 11);
    assert_eq!(expected_decodes, 86);
    eprintln!(
        "LARGE_SPREADS pages={total_pages} partial_requests={partial_hits} decodes={expected_decodes} cache_mib={}",
        CACHE_BYTE_LIMIT / 1024 / 1024
    );
    drop(displayed);
    drop(loader);
    worker.join().expect("worker shutdown");
    drop(cache);
    assert!(
        owners.iter().all(|owner| owner.upgrade().is_none()),
        "close releases all decoded originals"
    );
    for (path, _, _, _, stamp) in sources {
        assert!(
            ImageStamp::read(&path) == Some(stamp),
            "source remains unchanged"
        );
        std::fs::remove_file(path).expect("remove owned source");
    }
    std::fs::remove_dir(root).expect("remove empty fixture directory");
}

#[test]
fn partial_spread_cache_survives_earlier_page_insertion() {
    let root = std::env::temp_dir().join(format!("towavue-spread-pressure-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("owned fixture directory");
    for animated in [false, true] {
        for changed in [false, true] {
            let paths: Vec<_> = (0..4)
                .map(|index| {
                    let path = root.join(format!("{animated}-{changed}-{index}.png"));
                    std::fs::write(&path, [index]).expect("source identity");
                    path
                })
                .collect();
            let make_image = |value| {
                let mut image = (*pixel(value)).clone();
                if animated {
                    image.frames.push(image.frames[0].clone());
                    image.frames[1].rgba = vec![value + 1; 4];
                    image.animation_plays = 2;
                }
                Arc::new(image)
            };
            let originals: Vec<_> = (0..4).map(make_image).collect();
            let budget = originals[0].retained_bytes() * 3;
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let cache = Arc::clone(&shared.0.lock().expect("mailbox").cache);
            {
                let mut cache = cache.lock().expect("cache");
                *cache = ImageCache::new(budget);
                for index in 1..4 {
                    cache.insert(
                        paths[index].clone(),
                        ImageStamp::read(&paths[index]).expect("stamp"),
                        Arc::clone(&originals[index]),
                    );
                }
            }
            let loader = ImageLoader {
                shared: Arc::clone(&shared),
                prefetch_worker: LatestTask::new("spread-pressure").expect("prefetch worker"),
            };
            let (ready_tx, ready_rx) = mpsc::channel();
            let (decoded_tx, decoded_rx) = mpsc::channel();
            let sources = paths.clone();
            let worker = thread::spawn(move || {
                run_worker(
                    shared,
                    || {
                        let _ = ready_tx.send(());
                    },
                    |path, _, _, _| {
                        let index = sources
                            .iter()
                            .position(|source| source == path)
                            .expect("source");
                        decoded_tx.send(index).expect("decode observation");
                        if index == 0 && changed {
                            std::fs::write(&sources[1], [9, 9]).expect("change future cached page");
                        }
                        let value = std::fs::read(path).expect("source value")[0];
                        let mut image = (*pixel(value)).clone();
                        if animated {
                            image.frames.push(image.frames[0].clone());
                            image.frames[1].rgba = vec![value + 1; 4];
                            image.animation_plays = 2;
                        }
                        Ok(image)
                    },
                );
            });
            let generation = loader.request(paths[..3].to_vec());
            let mut loaded = Vec::new();
            while loaded.len() < 3 {
                ready_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("page ready");
                if let Some(result) = loader.take_completed() {
                    assert_eq!(result.generation, generation);
                    assert_eq!(result.first_index, loaded.len());
                    loaded.extend(
                        result
                            .images
                            .into_iter()
                            .map(|(_, image)| image.expect("page")),
                    );
                }
            }
            drop(loader);
            worker.join().expect("worker shutdown");
            assert_eq!(
                decoded_rx.try_iter().collect::<Vec<_>>(),
                if changed { vec![0, 1] } else { vec![0] }
            );
            assert!(
                Arc::ptr_eq(&loaded[2], &originals[2]),
                "reuse the later page without copying"
            );
            if changed {
                assert_eq!(loaded[1].frames[0].rgba, vec![9; 4]);
                assert!(!Arc::ptr_eq(&loaded[1], &originals[1]));
            } else {
                assert!(Arc::ptr_eq(&loaded[1], &originals[1]));
            }
            assert!(cache.lock().expect("cache").bytes <= budget);
            drop(loaded);
            drop(cache);
            assert!(originals.iter().all(|image| Arc::strong_count(image) == 1));
            for path in paths {
                std::fs::remove_file(path).expect("remove owned source");
            }
        }
    }
    std::fs::remove_dir(root).expect("remove empty fixture directory");
}

#[test]
fn borrowed_spread_pages_obey_budget_replacement_and_close() {
    let root = std::env::temp_dir().join(format!("towavue-spread-lifetime-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("owned fixture directory");
    for mode in ["budget", "replace", "close"] {
        let paths: Vec<_> = (0..3)
            .map(|index| {
                let path = root.join(format!("{mode}-{index}.png"));
                std::fs::write(&path, [index]).expect("source identity");
                path
            })
            .collect();
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let cache = Arc::clone(&shared.0.lock().expect("mailbox").cache);
        let borrowed = pixel(42);
        cache.lock().expect("cache").insert(
            paths[1].clone(),
            ImageStamp::read(&paths[1]).expect("stamp"),
            Arc::clone(&borrowed),
        );
        let mut loader = Some(ImageLoader {
            shared: Arc::clone(&shared),
            prefetch_worker: LatestTask::new("spread-lifetime").expect("prefetch worker"),
        });
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let first = paths[0].clone();
        let tail = paths[1].clone();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    let _ = ready_tx.send(());
                },
                |path, _, _, _| {
                    assert_ne!(path, tail, "cached tail must not enter the decoder");
                    if path == first {
                        started_tx.send(()).expect("first decode started");
                        release_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("release decode");
                    }
                    Ok((*pixel(7)).clone())
                },
            )
        });
        let generation = loader
            .as_ref()
            .expect("loader")
            .request_with_retained_bytes(
                paths[..2].to_vec(),
                if mode == "budget" {
                    IMAGE_BYTE_LIMIT - 4
                } else {
                    0
                },
            );
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("active request");
        assert_eq!(
            Arc::strong_count(&borrowed),
            3,
            "request borrows cache pixels"
        );
        {
            let mut cache = cache.lock().expect("cache");
            cache.entries.clear();
            cache.bytes = 0;
        }
        assert_eq!(
            Arc::strong_count(&borrowed),
            2,
            "request survives cache eviction"
        );
        let expected_generation = if mode == "replace" {
            loader
                .as_ref()
                .expect("loader")
                .request(vec![paths[2].clone()])
        } else {
            generation
        };
        if mode == "close" {
            drop(loader.take());
        }
        release_tx.send(()).expect("finish old decoder");
        if let Some(loader) = &loader {
            let mut results = Vec::new();
            let count = if mode == "budget" { 2 } else { 1 };
            while results.len() < count {
                ready_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("current result");
                if let Some(result) = loader.take_completed() {
                    assert_eq!(result.generation, expected_generation);
                    results.extend(result.images.into_iter().map(|(_, image)| image));
                }
            }
            assert!(results[0].is_ok());
            if mode == "budget" {
                assert!(matches!(results[1], Err(ImageDecodeError::TooLarge)));
            }
        }
        drop(loader);
        worker.join().expect("worker shutdown");
        if mode == "close" {
            assert!(
                ready_rx.try_recv().is_err(),
                "closed request cannot publish"
            );
        }
        assert_eq!(
            Arc::strong_count(&borrowed),
            1,
            "request releases borrowed pages"
        );
        for path in paths {
            std::fs::remove_file(path).expect("remove owned source");
        }
    }
    std::fs::remove_dir(root).expect("remove empty fixture directory");
}
