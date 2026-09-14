use super::*;

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
