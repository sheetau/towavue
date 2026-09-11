use super::*;

#[test]
fn clear_releases_originals_off_the_ui_thread_and_survives_immediate_reopen() {
    for prefetch in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "towavue-clear-images-{}-{prefetch}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("owned fixtures");
        let old_path = root.join("old.png");
        let next_path = root.join("next.png");
        std::fs::write(&old_path, [1]).expect("old fixture");
        std::fs::write(&next_path, [2]).expect("next fixture");
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let loader = ImageLoader {
            shared: Arc::clone(&shared),
            prefetch_worker: LatestTask::new("image-release-test").expect("prefetch worker"),
        };
        let cache = Arc::clone(&shared.0.lock().expect("mailbox").cache);
        let seeded = pixel(3);
        let weak = Arc::downgrade(&seeded);
        cache.lock().expect("cache").insert(
            root.join("seed.png"),
            ImageStamp::read(&old_path).expect("stamp"),
            seeded,
        );
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let (ready_tx, ready_rx) = mpsc::channel();
        let blocked = move || {
            started_tx.send(()).expect("started");
            release_rx
                .lock()
                .expect("release receiver")
                .recv()
                .expect("release");
        };
        let blocked = Arc::new(blocked);
        let worker_blocked = Arc::clone(&blocked);
        let worker_old = old_path.clone();
        let worker = thread::spawn(move || {
            run_worker(
                shared,
                || {
                    ready_tx.send(()).expect("ready");
                },
                |path, _, _, _| {
                    if path == worker_old {
                        worker_blocked();
                    }
                    Ok((*pixel(7)).clone())
                },
            )
        });
        if prefetch {
            loader.prefetch_with_decode(vec![old_path.clone()], move |_, _, _| {
                blocked();
                Ok(Some((*pixel(8)).clone()))
            });
        } else {
            loader.request(vec![old_path.clone()]);
        }
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("decode started");
        // An expensive eviction or a stuck codec must not make the UI wait for this lock.
        thread::scope(|scope| {
            let held = cache.lock().expect("held cache");
            let (done_tx, done_rx) = mpsc::channel();
            let loader_ref = &loader;
            scope.spawn(move || {
                done_tx.send(loader_ref.clear()).expect("clear returned");
            });
            let result = done_rx.recv_timeout(Duration::from_secs(5));
            drop(held);
            result.expect("clear must not wait for the cache lock");
        });
        let next_generation = loader.request(vec![next_path.clone()]);
        release_tx.send(()).expect("finish obsolete decode");
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("new original");
        let result = loader.take_completed().expect("new result");
        assert_eq!(result.generation, next_generation);
        assert_eq!(result.images[0].0, next_path);
        assert!(
            weak.upgrade().is_none(),
            "reopen must not erase the clear request"
        );
        if prefetch {
            wait_for_prefetch(&loader);
        }
        {
            let cache = cache.lock().expect("cache");
            assert_eq!(cache.entries.len(), 1);
            assert_eq!(cache.entries[0].0, next_path);
        }
        drop(result);
        loader.clear();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let cache = cache.lock().expect("cache");
            if cache.entries.is_empty() {
                assert_eq!(cache.bytes, 0);
                break;
            }
            drop(cache);
            assert!(
                std::time::Instant::now() < deadline,
                "idle clear was not processed"
            );
            thread::sleep(Duration::from_millis(1));
        }
        assert!(loader.take_completed().is_none());
        drop(loader);
        worker.join().expect("worker closed");
        std::fs::remove_file(old_path).expect("remove owned fixture");
        std::fs::remove_file(next_path).expect("remove owned fixture");
        std::fs::remove_dir(root).expect("remove empty fixture directory");
    }
}
