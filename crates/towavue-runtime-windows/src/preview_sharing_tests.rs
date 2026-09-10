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
