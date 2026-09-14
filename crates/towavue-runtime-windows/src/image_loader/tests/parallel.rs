use super::*;

#[test]
fn paired_prefetch_preserves_budget_adoption_cancellation_and_source_identity() {
    for mode in ["adopt", "replace_tail", "cancel", "change_source"] {
        let root = std::env::temp_dir().join(format!(
            "towavue-prefetch-pair-{}-{mode}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("owned fixture directory");
        let paths: Vec<_> = (0..3)
            .map(|index| root.join(format!("{index}.png")))
            .collect();
        for path in &paths {
            image::RgbaImage::from_pixel(1, 1, image::Rgba([42; 4]))
                .save(path)
                .expect("owned PNG");
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
        let (ahead_release_tx, ahead_release_rx) = mpsc::channel();
        let ahead_release_rx = Mutex::new(ahead_release_rx);
        let expected_path = paths[1].clone();
        shared.0.lock().expect("mailbox").parallel_decode =
            Some(Arc::new(move |path, budget, _| {
                assert_eq!(path, expected_path);
                ahead_tx.send(budget).expect("ahead started");
                ahead_release_rx
                    .lock()
                    .expect("gate")
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release ahead");
                Ok(Some((*pixel(42)).clone()))
            }));
        let (ready_tx, ready_rx) = mpsc::channel();
        let foreground_calls = Arc::new(Mutex::new(Vec::new()));
        let worker_calls = Arc::clone(&foreground_calls);
        let worker_shared = Arc::clone(&shared);
        let worker = thread::spawn(move || {
            run_worker(
                worker_shared,
                || {
                    let _ = ready_tx.send(());
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
        let target = paths[if mode == "cancel" { 2 } else { 1 }].clone();
        loader.request(vec![target.clone()]);
        assert!(loader.verification_set_parallel_prefetch(false).is_err());
        if mode == "replace_tail" {
            loader.prefetch_with_decode(paths[1..].to_vec(), |_, _, _| {
                panic!("replacement must retain the running pair")
            });
        }
        if mode == "change_source" {
            let mut bytes = std::fs::read(&paths[1]).expect("owned source");
            bytes.push(0);
            std::fs::write(&paths[1], bytes).expect("change owned source stamp");
        }
        primary_release_tx.send(()).expect("resume primary");
        ahead_release_tx.send(()).expect("resume ahead");
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("foreground completion");
        let completed = loader.take_completed().expect("foreground result");
        assert_eq!(completed.images[0].0, target);
        let expected = if matches!(mode, "cancel" | "change_source") {
            99
        } else {
            42
        };
        assert_eq!(
            completed.images[0].1.as_ref().expect("original").frames[0].rgba,
            vec![expected; 4]
        );
        wait_for_prefetch(&loader);
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
            matches!(mode, "cancel" | "replace_tail")
        );
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
