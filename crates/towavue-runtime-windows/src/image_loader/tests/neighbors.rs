use super::*;

#[test]
fn cached_navigation_keeps_the_useful_decode_and_replaces_its_batch_tail() {
    let root =
        std::env::temp_dir().join(format!("towavue-prefetch-neighbor-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("owned directory");
    let paths: Vec<_> = (0..5).map(|i| root.join(format!("{i}.png"))).collect();
    for path in &paths {
        std::fs::write(path, [1]).expect("owned identity");
    }
    let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
    let loader = ImageLoader {
        shared: Arc::clone(&shared),
        prefetch_worker: LatestTask::new("neighbor-reuse-test").expect("worker"),
    };
    let cache = Arc::clone(&shared.0.lock().expect("mailbox").cache);
    *cache.lock().expect("cache") = ImageCache::new(8);
    cache.lock().expect("cache").insert(
        paths[0].clone(),
        ImageStamp::read(&paths[0]).expect("stamp"),
        pixel(7),
    );
    let (ready_tx, ready_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        run_worker(
            shared,
            || {
                let _ = ready_tx.send(());
            },
            |_, _, _, _| panic!("cached foreground must not decode"),
        )
    });
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let (current_tx, current_rx) = mpsc::channel();
    let decode = {
        let calls = Arc::clone(&calls);
        move |path: &Path, budget: usize, current: &dyn Fn() -> bool| {
            let first = {
                let mut calls = calls.lock().expect("calls");
                calls.push((path.to_owned(), budget));
                calls.len() == 1
            };
            if first {
                started_tx.send(()).expect("started");
                release_rx
                    .lock()
                    .expect("gate")
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release");
                current_tx.send(current()).expect("currency");
            }
            Ok(Some((*pixel(42)).clone()))
        }
    };
    loader.verification_trace_path(paths[3].clone(), std::time::Instant::now());
    loader.prefetch_with_decode(vec![paths[1].clone(), paths[2].clone()], decode.clone());
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("neighbor running");
    loader.request_originals_with_retained_bytes(
        vec![paths[0].clone()],
        0,
        &[paths[1].clone(), paths[3].clone()],
    );
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("cache hit must not wait for neighbor");
    let completed = loader.take_completed().expect("foreground result");
    let foreground_pixel = completed.images[0].1.as_ref().expect("cached image").frames[0].rgba[0];
    loader.prefetch_with_decode(vec![paths[1].clone(), paths[2].clone()], decode.clone());
    loader.prefetch_with_decode(
        vec![paths[1].clone(), paths[3].clone(), paths[4].clone()],
        decode,
    );
    release_tx.send(()).expect("resume neighbor");
    let stayed_current = current_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("currency");
    wait_for_prefetch(&loader);
    let decoded_paths = calls.lock().expect("calls").clone();
    let metrics = loader.verification_metrics();
    let trace = loader
        .verification_trace_snapshot()
        .expect("selected neighbor trace");
    assert_eq!(trace.dropped, 0);
    assert_eq!(trace.events.len(), 5);
    assert_eq!(
        trace.events[0].kind,
        TraceKind::Planned {
            position: 1,
            worker_decoding: true
        }
    );
    assert_eq!(
        trace.events[1].kind,
        TraceKind::Considered {
            remaining_bytes: 4,
            preceding_cached_bytes: 4,
            cache_hit: false
        }
    );
    assert_eq!(trace.events[2].kind, TraceKind::PrefetchDecodeStarted);
    assert!(matches!(
        trace.events[3].kind,
        TraceKind::PrefetchReturned {
            outcome: verification::Outcome::Completed,
            ..
        }
    ));
    assert_eq!(trace.events[4].kind, TraceKind::OriginalCached);
    assert!(
        trace
            .events
            .windows(2)
            .all(|events| events[0].elapsed <= events[1].elapsed)
    );
    assert_eq!(cache.lock().expect("cache").bytes, 8);
    drop(loader);
    worker.join().expect("foreground shutdown");
    std::fs::remove_dir_all(&root).expect("remove owned fixtures");
    assert_eq!(foreground_pixel, 7);
    assert!(stayed_current, "the still-useful neighbor was cancelled");
    assert_eq!(
        decoded_paths,
        vec![(paths[1].clone(), 8), (paths[3].clone(), 4)]
    );
    assert_eq!(metrics.prefetch.superseded, 0);
    assert_eq!(metrics.foreground.calls, 0);
}

#[test]
fn retained_neighbors_respect_cancellation_departure_and_changed_sources() {
    for mode in [
        "clear",
        "close",
        "departure",
        "empty-plan",
        "changed-source",
    ] {
        let path = std::env::temp_dir().join(format!(
            "towavue-retained-neighbor-{mode}-{}.png",
            std::process::id()
        ));
        std::fs::write(&path, [1]).expect("owned identity");
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
            prefetch_worker: LatestTask::new("retained-neighbor-cancel").expect("worker"),
        };
        let cache = Arc::clone(&shared.0.lock().expect("mailbox").cache);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (current_tx, current_rx) = mpsc::channel();
        let mut calls = 0;
        loader.prefetch_with_decode(vec![path.clone()], move |_, _, current| {
            calls += 1;
            if calls == 1 {
                started_tx.send(()).expect("started");
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release");
                current_tx.send(current()).expect("currency");
            }
            Ok(Some((*pixel(calls)).clone()))
        });
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("running");
        loader.request_originals_with_retained_bytes(
            vec![PathBuf::from("cached-target.png")],
            0,
            std::slice::from_ref(&path),
        );
        // The decoder is already owned by the retained task, including the new tail.
        loader.prefetch_with_decode(vec![path.clone()], |_, _, _| panic!("duplicate worker"));
        match mode {
            "clear" => {
                loader.clear();
            }
            "departure" => {
                loader.request(vec![PathBuf::from("unrelated.png")]);
            }
            "empty-plan" => loader.prefetch_paths(Vec::new()),
            "changed-source" => std::fs::write(&path, [2, 3]).expect("change identity"),
            "close" => {}
            _ => unreachable!(),
        }
        let loader = if mode == "close" {
            drop(loader);
            None
        } else {
            Some(loader)
        };
        release_tx.send(()).expect("resume");
        let stayed_current = current_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("currency");
        let (mailbox, timeout) = shared
            .1
            .wait_timeout_while(
                shared.0.lock().expect("mailbox"),
                Duration::from_secs(5),
                |mailbox| mailbox.prefetch_tasks != 0,
            )
            .expect("all leases returned");
        assert!(!timeout.timed_out());
        drop(mailbox);
        let cached_pixels: Vec<_> = cache
            .lock()
            .expect("cache")
            .entries
            .iter()
            .map(|entry| entry.2.frames[0].rgba[0])
            .collect();
        drop(loader);
        std::fs::remove_file(&path).expect("remove owned identity");
        assert_eq!(stayed_current, mode == "changed-source", "{mode}");
        assert_eq!(
            cached_pixels,
            if mode == "changed-source" {
                vec![2]
            } else {
                vec![]
            },
            "{mode}"
        );
    }
}
