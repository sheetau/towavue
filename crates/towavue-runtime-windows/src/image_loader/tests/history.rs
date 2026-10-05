use super::*;

#[test]
fn distant_return_reuses_recent_foreground_after_speculative_pressure() {
    let root = std::env::temp_dir().join(format!("towavue-image-history-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("owned directory");
    let paths: Vec<_> = (0..12).map(|i| root.join(format!("{i}.png"))).collect();
    for path in &paths {
        std::fs::write(path, [1]).expect("source identity");
    }
    let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
    let cache = Arc::clone(&shared.0.lock().expect("mailbox").cache);
    *cache.lock().expect("cache") = ImageCache::new(16);
    let loader = ImageLoader {
        shared: Arc::clone(&shared),
        prefetch_worker: LatestTask::new("history-prefetch-test").expect("worker"),
    };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&calls);
    let (ready_tx, ready_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        run_worker(
            shared,
            || {
                let _ = ready_tx.send(());
            },
            move |path, _, _, _| {
                observed.lock().expect("calls").push(path.to_owned());
                Ok((*pixel(42)).clone())
            },
        )
    });
    let speculative = Arc::new(Mutex::new(Vec::new()));
    for (step, selected) in [3, 9, 3].into_iter().enumerate() {
        loader.request_originals_with_retained_bytes(vec![paths[selected].clone()], 0, &[]);
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("foreground ready");
        let completed = loader.take_completed().expect("foreground result");
        assert_eq!(
            completed.images[0].1.as_ref().expect("pixels").frames[0].rgba[0],
            42
        );
        if step == 2 {
            break;
        }
        let neighbors = if selected == 3 {
            // The current cached page must not consume its reservation twice.
            &paths[3..8]
        } else {
            // A protected opposite neighbor appears after the nearest cold pages.
            &paths[0..4]
        };
        let speculative = Arc::clone(&speculative);
        loader.prefetch_with_decode(neighbors.to_vec(), move |path, _, _| {
            speculative
                .lock()
                .expect("speculative calls")
                .push(path.to_owned());
            Ok(Some((*pixel(7)).clone()))
        });
        wait_for_prefetch(&loader);
        let cache = cache.lock().expect("cache");
        assert!(cache.bytes <= 16 && cache.entries.len() <= CACHE_ENTRY_LIMIT);
    }
    let decoded = calls.lock().expect("calls").clone();
    let prefetched = speculative.lock().expect("speculative calls").clone();
    drop(loader);
    worker.join().expect("foreground shutdown");
    std::fs::remove_dir_all(&root).expect("remove owned fixtures");
    assert_eq!(
        decoded,
        vec![paths[3].clone(), paths[9].clone()],
        "return must reuse the original after distant speculative work"
    );
    assert_eq!(
        prefetched,
        [4, 5, 6, 0, 1].map(|index| paths[index].clone()),
        "reserve history before decoding so nearer neighbors keep their priority"
    );
}

#[test]
fn history_preference_expires_and_rejects_changed_versions() {
    let stamp = ImageStamp {
        identity: None,
        bytes: 1,
        modified: SystemTime::UNIX_EPOCH,
    };
    let mut cache = ImageCache::new(16);
    let held = pixel(42);
    cache.insert_foreground("a".into(), stamp, Arc::clone(&held));
    cache.insert_foreground("b".into(), stamp, pixel(2));
    for index in 0..10 {
        cache.insert_prefetched(index.to_string().into(), stamp, pixel(index));
        assert!(cache.bytes <= 16 && cache.entries.len() <= CACHE_ENTRY_LIMIT);
    }
    assert!(Arc::ptr_eq(
        &held,
        &cache
            .get(Path::new("a"), Some(stamp), 16)
            .expect("recent hit")
            .expect("budget")
    ));
    cache.insert_foreground("c".into(), stamp, pixel(3));
    for index in 10..20 {
        cache.insert_prefetched(index.to_string().into(), stamp, pixel(index));
    }
    assert!(
        cache.get(Path::new("a"), Some(stamp), 16).is_none(),
        "old history must be evictable"
    );
    assert_eq!(
        Arc::strong_count(&held),
        1,
        "history metadata retains no pixels"
    );
    let changed = ImageStamp { bytes: 2, ..stamp };
    assert!(cache.get(Path::new("b"), Some(changed), 16).is_none());
    cache.insert_prefetched("b".into(), changed, pixel(99));
    assert_eq!(
        cache.prefetch_allocation().0,
        12,
        "new source version does not inherit old preference"
    );
    assert!(
        cache.prefetch_allocation().1 == vec![(PathBuf::from("c"), stamp)],
        "a reservation identifies the exact cached source version"
    );
    drop(cache);
    assert_eq!(held.frames[0].rgba, vec![42; 4]);
}

#[test]
fn oversized_history_leaves_capacity_for_speculation_and_admission() {
    let stamp = ImageStamp {
        identity: None,
        bytes: 1,
        modified: SystemTime::UNIX_EPOCH,
    };
    let mut cache = ImageCache::new(16);
    let mut animation = (*pixel(1)).clone();
    animation.frames.push(animation.frames[0].clone());
    cache.insert_foreground("animation".into(), stamp, Arc::new(animation.clone()));
    assert_eq!(
        cache.prefetch_allocation().0,
        16,
        "all animation frames count toward preference eligibility"
    );
    for index in 0..4 {
        cache.insert_prefetched(index.to_string().into(), stamp, pixel(index));
    }
    assert!(cache.get(Path::new("animation"), Some(stamp), 16).is_none());
    cache.insert_foreground("a".into(), stamp, pixel(42));
    cache.insert_foreground("b".into(), stamp, pixel(42));
    animation.frames.extend(animation.frames.clone());
    cache.insert_foreground("full-budget".into(), stamp, Arc::new(animation));
    assert_eq!(
        cache.entries.len(),
        1,
        "foreground admission can evict preferred history when necessary"
    );
    assert_eq!(cache.bytes, 16);
}
