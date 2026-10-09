use super::*;
use std::time::{Duration, Instant};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "towavue-startup-completion-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir(&root).expect("exclusive fixture");
        for (name, value) in [("small", 1), ("large", 2), ("bad", 3)] {
            std::fs::write(root.join(name), [value]).expect("source identity");
        }
        Self(root)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for name in ["small", "large", "bad"] {
            let _ = std::fs::remove_file(self.path(name));
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn worker() -> (ImageLoader, thread::JoinHandle<()>) {
    let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
    let loader = ImageLoader {
        shared: Arc::clone(&shared),
        prefetch_worker: LatestTask::new("startup-completion-prefetch").expect("worker"),
    };
    let worker = thread::spawn(move || {
        run_worker(
            shared,
            || {},
            |path, _, _, _| {
                let value = std::fs::read(path).expect("source")[0];
                if value == 3 {
                    return Err(ImageDecodeError::TooLarge);
                }
                // The first animation frame fits. A later frame requires the device limit.
                let widths = if value == 2 { vec![2, 4097] } else { vec![2] };
                Ok(DecodedImage {
                    animation_plays: 0,
                    format: "fixture",
                    frames: widths
                        .into_iter()
                        .map(|width| crate::DecodedImageFrame {
                            width,
                            height: 1,
                            rgba: vec![value; width as usize * 4],
                            delay: Duration::from_secs(1),
                        })
                        .collect(),
                })
            },
        )
    });
    (loader, worker)
}
fn wait(loader: &ImageLoader) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !loader.is_idle() {
        assert!(Instant::now() < deadline, "worker completion deadline");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn held_batch_preserves_all_frames_errors_source_and_chunk_identity() {
    let fixture = Fixture::new();
    let (loader, thread) = worker();
    let paths = vec![
        fixture.path("small"),
        fixture.path("large"),
        fixture.path("bad"),
    ];
    let generation = loader.request(paths.clone());
    wait(&loader);
    let (original, source) = {
        let mailbox = loader.shared.0.lock().expect("mailbox");
        let result = mailbox.completed.as_ref().expect("completed batch");
        (
            Arc::clone(result.images[1].1.as_ref().expect("animation")),
            result.source.clone(),
        )
    };
    assert!(source.is_some(), "first original version is retained");
    for _ in 0..3 {
        assert!(matches!(
            loader.take_completed_with_texture_limit(2048),
            Err(4097)
        ));
    }
    let result = loader
        .take_completed_with_texture_limit(4097)
        .unwrap_or_else(|_| panic!("sufficient limit"))
        .expect("batch");
    assert_eq!(
        (result.generation, result.first_index, result.total),
        (generation, 0, 3)
    );
    assert_eq!(result.source, source);
    assert_eq!(
        result
            .images
            .iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>(),
        paths.iter().collect::<Vec<_>>()
    );
    assert!(Arc::ptr_eq(
        result.images[1].1.as_ref().expect("same original"),
        &original
    ));
    assert!(matches!(
        result.images[2].1,
        Err(ImageDecodeError::TooLarge)
    ));
    assert!(loader.take_completed().is_none());
    drop(loader);
    thread.join().expect("worker retirement");
}

#[test]
fn navigation_clear_and_drop_retire_blocked_completions() {
    let fixture = Fixture::new();
    let (loader, thread) = worker();
    let first = loader.request(vec![fixture.path("large")]);
    wait(&loader);
    assert!(matches!(
        loader.take_completed_with_texture_limit(2048),
        Err(4097)
    ));
    let second = loader.request(vec![fixture.path("small")]);
    wait(&loader);
    let result = loader
        .take_completed_with_texture_limit(2)
        .unwrap_or_else(|_| panic!("small fits"))
        .expect("new request");
    assert_ne!(first, second);
    assert_eq!(result.generation, second);
    assert_eq!(result.images[0].0, fixture.path("small"));
    loader.request(vec![fixture.path("large")]);
    wait(&loader);
    let weak = {
        let mailbox = loader.shared.0.lock().expect("mailbox");
        Arc::downgrade(
            mailbox.completed.as_ref().expect("result").images[0]
                .1
                .as_ref()
                .expect("original"),
        )
    };
    assert!(matches!(
        loader.take_completed_with_texture_limit(2048),
        Err(4097)
    ));
    loader.clear();
    assert!(loader.take_completed().is_none());
    // Clear retires the cache asynchronously; is_idle covers decode/prefetch,
    // not destruction of cached pixels on the owner thread.
    let deadline = Instant::now() + Duration::from_secs(5);
    while weak.upgrade().is_some() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert!(
        weak.upgrade().is_none(),
        "clear releases held completion and cached original"
    );
    loader.request(vec![fixture.path("large")]);
    wait(&loader);
    let weak = {
        let mailbox = loader.shared.0.lock().expect("mailbox");
        Arc::downgrade(
            mailbox.completed.as_ref().expect("result").images[0]
                .1
                .as_ref()
                .expect("original"),
        )
    };
    drop(loader);
    thread.join().expect("worker retirement");
    assert!(
        weak.upgrade().is_none(),
        "drop releases the last retained original"
    );
}
