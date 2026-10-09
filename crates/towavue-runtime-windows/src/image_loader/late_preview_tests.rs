use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum Completion {
    Deferred,
    Consumed,
    ConsumedAfterThumbnail,
    Replaced,
    SourceChanged,
    OriginalOnly,
}

fn exercise(completion: Completion, adopted: bool) {
    let root = std::env::temp_dir().join(format!(
        "towavue-late-preview-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir(&root).expect("exclusive fixture directory");
    let path = root.join("source");
    std::fs::write(&path, [1]).expect("source identity");
    let image = Arc::new(DecodedImage {
        animation_plays: 0,
        format: "fixture",
        frames: vec![crate::DecodedImageFrame {
            width: 4097,
            height: 64,
            rgba: [17, 37, 59, 255].repeat(4097 * 64),
            delay: Duration::ZERO,
        }],
    });
    let (idle_tx, idle_rx) = mpsc::channel();
    let shared = Arc::new((
        Mutex::new(Mailbox {
            idle_notify: Some(Arc::new(move || {
                let _ = idle_tx.send(());
            })),
            ..Default::default()
        }),
        Condvar::new(),
    ));
    let loader = ImageLoader {
        shared: Arc::clone(&shared),
        prefetch_worker: LatestTask::new("late-preview-test").expect("worker"),
    };
    if adopted {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let original = Arc::clone(&image);
        // Capture no preview cache for this speculative job, so the foreground
        // worker alone creates the late thumbnail after publishing the original.
        loader.prefetch_with_decode(vec![path.clone()], move |_, _, _| {
            started_tx.send(()).expect("started");
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("foreground lookup");
            Ok(Some((*original).clone()))
        });
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("active prefetch");
        let mut first = true;
        shared
            .0
            .lock()
            .expect("mailbox")
            .cache
            .lock()
            .expect("cache")
            .before_lookup = Some(Box::new(move || {
            if std::mem::take(&mut first) {
                release_tx.send(()).expect("release adopted original");
            }
        }));
    }
    let previews = PreviewCache::new(root.join("previews")).expect("preview cache");
    shared.0.lock().expect("mailbox").previews = Some(previews.clone());
    assert!(previews.cached_image(&path).expect("cache").is_none());
    let (published_tx, published_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let notifications = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&notifications);
    let original = Arc::clone(&image);
    let worker = thread::spawn(move || {
        run_worker(
            shared,
            || {
                if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    published_tx.send(()).expect("original publication");
                    continue_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("UI handles original");
                }
            },
            |_, _, _, _| {
                assert!(!adopted, "adopted pixels must not decode again");
                Ok((*original).clone())
            },
        )
    });
    let generation = if matches!(completion, Completion::OriginalOnly) {
        loader.request_originals_with_retained_bytes(vec![path.clone()], 0, &[])
    } else {
        loader.request(vec![path.clone()])
    };
    published_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("original published before thumbnail");
    assert!(previews.cached_image(&path).expect("cache").is_none());
    let original = {
        let mailbox = loader.shared.0.lock().expect("mailbox");
        Arc::clone(
            mailbox.completed.as_ref().expect("original").images[0]
                .1
                .as_ref()
                .expect("decoded original"),
        )
    };
    match completion {
        Completion::Deferred | Completion::ConsumedAfterThumbnail | Completion::OriginalOnly => {
            assert!(matches!(
                loader.take_completed_with_texture_limit(2048),
                Err(4097)
            ));
        }
        Completion::Consumed => {
            assert!(loader.take_completed().is_some());
        }
        Completion::Replaced => {
            loader.clear();
        }
        Completion::SourceChanged => {
            std::fs::write(&path, [1, 2]).expect("change source version");
        }
    }
    continue_tx.send(()).expect("allow thumbnail generation");
    // A speculative completion may have notified idle earlier. The foreground
    // flag remains true through thumbnail publication; wait for that worker too.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        idle_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker idle");
        if loader.is_idle() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "idle deadline");
    }
    if matches!(completion, Completion::ConsumedAfterThumbnail) {
        assert!(
            loader
                .shared
                .0
                .lock()
                .expect("mailbox")
                .preview_ready
                .is_some()
        );
        let completed = loader
            .take_completed_with_texture_limit(8192)
            .expect("device limit fits")
            .expect("original");
        assert!(Arc::ptr_eq(
            &original,
            completed.images[0].1.as_ref().expect("decoded original")
        ));
    }
    let preview = loader.take_preview();
    if matches!(completion, Completion::Deferred) {
        let preview = preview.expect("held original receives its late cached thumbnail");
        assert_eq!(
            (preview.generation, preview.path),
            (generation, path.clone())
        );
        assert_eq!(preview.preview.source_size, (4097, 64));
        assert!(preview.preview.image.width <= 240 && preview.preview.image.height <= 160);
        assert!(
            preview
                .preview
                .image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [17, 37, 59, 255])
        );
        let completed = loader
            .take_completed_with_texture_limit(8192)
            .expect("device limit fits")
            .expect("same original");
        let retained = completed.images[0].1.as_ref().expect("decoded original");
        assert!(Arc::ptr_eq(&original, retained));
        assert_eq!(retained.frames[0].rgba, image.frames[0].rgba);
    } else {
        assert!(preview.is_none(), "no late preview for {completion:?}");
    }
    drop(loader);
    worker.join().expect("worker retirement");
    assert_eq!(
        notifications.load(std::sync::atomic::Ordering::SeqCst),
        if matches!(
            completion,
            Completion::Deferred | Completion::ConsumedAfterThumbnail
        ) {
            2
        } else {
            1
        }
    );
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).expect("remove owned fixture");
}

#[test]
fn held_original_receives_late_thumbnail_without_another_decode() {
    for adopted in [false, true] {
        exercise(Completion::Deferred, adopted);
    }
}

#[test]
fn late_thumbnail_respects_consumption_replacement_source_and_original_only_requests() {
    for completion in [
        Completion::Consumed,
        Completion::ConsumedAfterThumbnail,
        Completion::Replaced,
        Completion::SourceChanged,
        Completion::OriginalOnly,
    ] {
        exercise(completion, true);
    }
}
