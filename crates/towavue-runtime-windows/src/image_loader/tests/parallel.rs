use super::*;

#[test]
fn paired_prefetch_preserves_mixed_animation_spreads_under_a_shared_budget() {
    const FRAME_BYTES: usize = 8 * 6 * 4;
    let root = std::env::temp_dir().join(format!("towavue-paired-spreads-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("owned fixtures");
    let paths: Vec<_> = (0..8)
        .map(|page| root.join(format!("{page}.png")))
        .collect();
    let mut references = Vec::new();
    for (page, path) in paths.iter().enumerate() {
        let animated = page % 2 == 1;
        let mut encoder = png::Encoder::new(std::fs::File::create(path).expect("fixture"), 8, 6);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if animated {
            encoder.set_animated(3, 2).expect("animation");
        }
        let mut writer = encoder.write_header().expect("header");
        for frame in 0..if animated { 3 } else { 1 } {
            if animated {
                writer.set_frame_delay(frame + 1, 100).expect("delay");
                writer
                    .set_blend_op(png::BlendOp::Source)
                    .expect("source frame");
            }
            writer
                .write_image_data(&[page as u8 * 17, frame as u8 * 43, 81, 255].repeat(8 * 6))
                .expect("pixels");
        }
        writer.finish().expect("complete PNG");
        references.push(crate::decode_image(path).expect("full reference"));
    }
    let stamps: Vec<_> = paths.iter().map(|path| ImageStamp::read(path)).collect();
    let previews = PreviewCache::new(root.join("cache")).expect("preview cache");
    let (ready_tx, ready_rx) = mpsc::channel();
    let (idle_tx, idle_rx) = mpsc::channel();
    let loader = ImageLoader::with_idle_notify(
        previews.clone(),
        move || {
            let _ = ready_tx.send(());
        },
        move || {
            let _ = idle_tx.send(());
        },
    )
    .expect("normal loader");
    let cache = {
        let mailbox = loader.shared.0.lock().expect("mailbox");
        assert!(
            mailbox.parallel_decode.is_some(),
            "normal construction must enable paired prefetch"
        );
        Arc::clone(&mailbox.cache)
    };
    *cache.lock().expect("cache") = ImageCache::new(4 * FRAME_BYTES);
    for spread in [[0, 1], [3, 2], [0, 1], [4, 5], [7, 6], [3, 2]] {
        let wanted: Vec<_> = spread.iter().map(|&page| paths[page].clone()).collect();
        loader.prefetch_paths(wanted.clone());
        wait_for_prefetch(&loader);
        let static_page = *spread
            .iter()
            .find(|&&page| page % 2 == 0)
            .expect("static page");
        let static_owner = cache
            .lock()
            .expect("cache")
            .entries
            .iter()
            .find(|entry| entry.0 == paths[static_page])
            .map(|entry| Arc::clone(&entry.2))
            .expect("prefetched static original");
        let animation_page = *spread
            .iter()
            .find(|&&page| page % 2 == 1)
            .expect("animation page");
        assert!(
            previews
                .cached_image(&paths[animation_page])
                .expect("preview lookup")
                .is_some()
        );
        let generation =
            loader.request_with_retained_bytes(wanted.clone(), IMAGE_BYTE_LIMIT - 4 * FRAME_BYTES);
        let mut images = Vec::new();
        while images.len() < wanted.len() {
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("spread page");
            if let Some(completed) = loader.take_completed() {
                assert_eq!(completed.generation, generation);
                assert_eq!(completed.first_index, images.len());
                images.extend(completed.images);
            }
        }
        let mut bytes = 0;
        for ((path, image), page) in images.iter().zip(spread) {
            assert_eq!(path, &paths[page]);
            let image = image.as_ref().expect("complete page");
            assert!(
                image.as_ref() == &references[page],
                "full frames, timing and plays must match"
            );
            bytes += image.retained_bytes();
            if page == static_page {
                assert!(
                    Arc::ptr_eq(image, &static_owner),
                    "reuse the prefetched original without copying"
                );
            }
        }
        assert_eq!(bytes, 4 * FRAME_BYTES);
        assert!(cache.lock().expect("cache").bytes <= 4 * FRAME_BYTES);
    }
    while !loader.is_idle() {
        idle_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("all image work finished");
    }
    drop(loader);
    assert!(
        paths
            .iter()
            .zip(stamps)
            .all(|(path, stamp)| ImageStamp::read(path) == stamp)
    );
    std::fs::remove_dir_all(root).expect("remove owned fixtures");
}

#[test]
fn paired_prefetch_preserves_budget_adoption_cancellation_and_source_identity() {
    paired_prefetch_contract(image::ImageFormat::Png);
}

#[test]
fn paired_jpeg_prefetch_preserves_budget_adoption_cancellation_and_source_identity() {
    paired_prefetch_contract(image::ImageFormat::Jpeg);
}

fn paired_prefetch_contract(format: image::ImageFormat) {
    for mode in [
        "adopt_first",
        "retain_first_tail",
        "adopt",
        "after_first",
        "spread",
        "replace_tail",
        "cancel",
        "change_source",
        "clear",
        "close",
    ] {
        let root = std::env::temp_dir().join(format!(
            "towavue-prefetch-pair-{}-{mode}-{format:?}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("owned fixture directory");
        let paths: Vec<_> = (0..3)
            .map(|index| root.join(format!("{index}.{}", format.extensions_str()[0])))
            .collect();
        for path in &paths {
            image::RgbImage::from_pixel(1, 1, image::Rgb([42; 3]))
                .save_with_format(path, format)
                .expect("owned image");
        }
        assert_eq!(
            super::super::parallel::pair_budget(&paths[0], &paths[1], 7),
            None
        );
        assert_eq!(
            super::super::parallel::pair_budget(&paths[0], &paths[1], 8),
            Some(4)
        );
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
            prefetch_worker: LatestTask::new("paired-prefetch-test").expect("worker"),
        };
        loader
            .verification_set_parallel_prefetch(true)
            .expect("unused loader");
        let cache = Arc::clone(&shared.0.lock().expect("mailbox").cache);
        *cache.lock().expect("cache") = ImageCache::new(8);
        let (ahead_tx, ahead_rx) = mpsc::channel();
        let (currency_tx, currency_rx) = mpsc::channel();
        let (ahead_release_tx, ahead_release_rx) = mpsc::channel();
        let ahead_release_rx = Mutex::new(ahead_release_rx);
        let expected_path = paths[1].clone();
        shared.0.lock().expect("mailbox").parallel_decode =
            Some(Arc::new(move |path, budget, current| {
                assert_eq!(path, expected_path);
                ahead_tx.send(budget).expect("ahead started");
                ahead_release_rx
                    .lock()
                    .expect("gate")
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release ahead");
                currency_tx.send(current()).expect("ahead currency");
                Ok(Some((*pixel(42)).clone()))
            }));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (published_release_tx, published_release_rx) = mpsc::channel();
        let foreground_calls = Arc::new(Mutex::new(Vec::new()));
        let worker_calls = Arc::clone(&foreground_calls);
        let worker_shared = Arc::clone(&shared);
        let worker = thread::spawn(move || {
            let first_notification = std::cell::Cell::new(true);
            run_worker(
                worker_shared,
                || {
                    let _ = ready_tx.send(());
                    if mode == "after_first" && first_notification.replace(false) {
                        published_release_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("finish first publication");
                    }
                },
                |path, _, _, _| {
                    worker_calls.lock().expect("calls").push(path.to_owned());
                    Ok((*pixel(99)).clone())
                },
            )
        });
        let (primary_tx, primary_rx) = mpsc::channel();
        let (primary_release_tx, primary_release_rx) = mpsc::channel();
        let primary_calls = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::clone(&primary_calls);
        let first = paths[0].clone();
        loader.prefetch_with_decode(paths.clone(), move |path, budget, _| {
            calls.lock().expect("calls").push(path.to_owned());
            if path == first {
                primary_tx.send(budget).expect("primary started");
                primary_release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release primary");
            }
            if budget < 4 {
                return Err(ImageDecodeError::TooLarge);
            }
            Ok(Some((*pixel(42)).clone()))
        });
        assert_eq!(
            primary_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("primary decode"),
            4
        );
        assert_eq!(
            ahead_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("parallel decode"),
            4
        );
        if mode == "after_first" {
            loader.request_originals_with_retained_bytes(vec![paths[0].clone()], 0, &paths[1..2]);
            primary_release_tx.send(()).expect("publish first original");
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("first publication");
            let first = loader.take_completed().expect("first original");
            assert_eq!(first.images[0].0, paths[0]);
            assert_eq!(
                first.images[0].1.as_ref().expect("first pixels").frames[0].rgba,
                vec![42; 4]
            );
            // The foreground publisher is still gated in notify while the helper
            // remains in flight. Reassign ownership before either can finish.
            loader.request(vec![paths[1].clone()]);
            ahead_release_tx.send(()).expect("complete adopted helper");
            let current = currency_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("adopted currency");
            wait_for_prefetch(&loader);
            published_release_tx
                .send(())
                .expect("release foreground publisher");
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("second publication");
            let second = loader.take_completed().expect("second original");
            assert!(current);
            assert_eq!(second.images[0].0, paths[1]);
            assert_eq!(
                second.images[0].1.as_ref().expect("second pixels").frames[0].rgba,
                vec![42; 4]
            );
            assert!(foreground_calls.lock().expect("calls").is_empty());
            assert_eq!(
                *primary_calls.lock().expect("calls"),
                vec![paths[0].clone()]
            );
            assert!(cache.lock().expect("cache").bytes <= 8);
            drop(loader);
            worker.join().expect("foreground stopped");
            std::fs::remove_dir_all(root).expect("remove owned fixtures");
            continue;
        }
        if matches!(mode, "clear" | "close") {
            let mut closing_loader = Some(loader);
            if mode == "clear" {
                closing_loader.as_ref().expect("loader").clear();
            } else {
                drop(closing_loader.take());
            }
            // Both decoders are still gated: returning above proves that the UI
            // call did not join either decoder. Idle must wait for their release.
            assert_eq!(shared.0.lock().expect("mailbox").prefetch_tasks, 1);
            ahead_release_tx.send(()).expect("release cancelled helper");
            let current = currency_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("cancelled helper currency");
            primary_release_tx
                .send(())
                .expect("release cancelled primary");
            let (mailbox, timeout) = shared
                .1
                .wait_timeout_while(
                    shared.0.lock().expect("mailbox"),
                    Duration::from_secs(5),
                    |mailbox| mailbox.prefetch_tasks != 0,
                )
                .expect("retired pair");
            assert!(!timeout.timed_out());
            assert!(!current);
            assert!(mailbox.completed.is_none());
            assert!(mailbox.preview_ready.is_none());
            drop(mailbox);
            assert!(cache.lock().expect("cache").entries.is_empty());
            drop(closing_loader);
            worker.join().expect("foreground stopped");
            std::fs::remove_dir_all(root).expect("remove owned fixtures");
            continue;
        }
        let target = paths[match mode {
            "cancel" => 2,
            "adopt_first" | "retain_first_tail" => 0,
            _ => 1,
        }]
        .clone();
        let targets = if mode == "spread" {
            paths[..2].to_vec()
        } else {
            vec![target]
        };
        loader.request(targets.clone());
        assert!(loader.verification_set_parallel_prefetch(false).is_err());
        if mode == "replace_tail" {
            loader.prefetch_with_decode(paths[1..].to_vec(), |_, _, _| {
                panic!("replacement must retain the running pair")
            });
        }
        if mode == "retain_first_tail" {
            loader.prefetch_with_decode(vec![paths[0].clone(), paths[2].clone()], |_, _, _| {
                panic!("replacement must retain the primary decoder")
            });
        }
        if mode == "change_source" {
            let mut bytes = std::fs::read(&paths[1]).expect("owned source");
            bytes.push(0);
            std::fs::write(&paths[1], bytes).expect("change owned source stamp");
        }
        ahead_release_tx.send(()).expect("resume ahead");
        let ahead_current = currency_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("ahead currency before primary completes");
        primary_release_tx.send(()).expect("resume primary");
        let mut images = Vec::new();
        while images.len() < targets.len() {
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("foreground completion");
            if let Some(completed) = loader.take_completed() {
                assert_eq!(completed.first_index, images.len());
                images.extend(completed.images);
            }
        }
        let expected = if matches!(mode, "cancel" | "change_source") {
            99
        } else {
            42
        };
        for ((path, image), target) in images.iter().zip(&targets) {
            assert_eq!(path, target);
            assert_eq!(
                image.as_ref().expect("original").frames[0].rgba,
                vec![expected; 4]
            );
        }
        wait_for_prefetch(&loader);
        assert_eq!(
            ahead_current,
            !matches!(mode, "cancel" | "adopt_first" | "retain_first_tail"),
            "only a wanted lookahead may remain current; mode={mode}"
        );
        assert_eq!(
            foreground_calls.lock().expect("calls").len(),
            usize::from(expected == 99)
        );
        assert!(
            !primary_calls.lock().expect("calls").contains(&paths[1]),
            "second image must not decode twice"
        );
        let cache = cache.lock().expect("cache");
        assert!(cache.bytes <= 8);
        assert_eq!(
            cache.entries.iter().any(|entry| entry.0 == paths[2]),
            matches!(mode, "cancel" | "replace_tail" | "retain_first_tail")
        );
        if matches!(mode, "adopt_first" | "retain_first_tail") {
            assert!(
                !cache.entries.iter().any(|entry| entry.0 == paths[1]),
                "unwanted lookahead must not be published"
            );
        }
        if mode == "cancel" {
            assert!(
                !cache
                    .entries
                    .iter()
                    .any(|entry| entry.0 == paths[0] || entry.0 == paths[1])
            );
        }
        drop(cache);
        drop(loader);
        worker.join().expect("foreground stopped");
        std::fs::remove_dir_all(root).expect("remove owned fixtures");
    }
}
