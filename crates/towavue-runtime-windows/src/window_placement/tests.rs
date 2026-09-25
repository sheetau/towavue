use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "towavue-window-placement-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("owned placement fixture")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("owned placement fixture");
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("placement.conf")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root = self.0.canonicalize().expect("owned placement fixture");
        assert!(
            root.starts_with(
                std::env::temp_dir()
                    .canonicalize()
                    .expect("owned placement fixture")
            )
        );
        assert!(
            root.file_name()
                .expect("owned placement fixture")
                .to_string_lossy()
                .starts_with("towavue-window-placement-")
        );
        if !std::thread::panicking() {
            fs::remove_dir_all(root).expect("owned placement fixture");
        }
    }
}
fn sample() -> SavedWindowPlacement {
    SavedWindowPlacement {
        bounds: [-1200, 25, -400, 625],
        dpi: 144,
        maximized: false,
    }
}

#[test]
fn placement_shutdown_publishes_only_the_last_accepted_snapshot() {
    let fixture = Fixture::new();
    drop(WindowPlacementPreferences::open(fixture.path()).expect("owned placement fixture"));
    assert!(!fixture.path().exists(), "no accepted close means no write");
    let mut store =
        WindowPlacementPreferences::open(fixture.path()).expect("owned placement fixture");
    assert_eq!(store.initial(), None);
    store.remember(sample());
    let latest = SavedWindowPlacement {
        maximized: true,
        ..sample()
    };
    store.remember(latest);
    assert!(
        !fixture.path().exists(),
        "no I/O during input or individual closure"
    );
    drop(store);
    assert_eq!(
        WindowPlacementPreferences::open(fixture.path())
            .expect("owned placement fixture")
            .initial(),
        Some(latest)
    );
    let mut store =
        WindowPlacementPreferences::open(fixture.path()).expect("owned placement fixture");
    store.remember(sample());
    drop(store);
    assert_eq!(
        read(&fixture.path()).expect("owned placement fixture"),
        Some(sample())
    );
    assert_eq!(
        fs::read_dir(&fixture.0)
            .expect("owned placement fixture")
            .count(),
        2,
        "no partial publication"
    );
}

#[test]
fn placement_preserves_invalid_future_oversized_and_replaced_records() {
    let fixture = Fixture::new();
    for content in [
        b"future v2".to_vec(),
        vec![b'x'; 257],
        vec![0xff],
        format!("{HEADER}\n0\n0\n0\n500\n96\nnormal\n").into_bytes(),
        format!("{HEADER}\n-2147483648\n0\n2147483647\n500\n96\nnormal\n").into_bytes(),
        format!("{HEADER}\n0\n0\n800\n500\n0\nnormal\n").into_bytes(),
        format!("{HEADER}\n0\n0\n800\n500\n96\nminimized\n").into_bytes(),
        format!("{HEADER}\n0\n0\n800\n500\n96\nnormal\nextra\n").into_bytes(),
    ] {
        fs::write(fixture.path(), &content).expect("owned placement fixture");
        assert!(WindowPlacementPreferences::open(fixture.path()).is_err());
        assert!(write(&fixture.path(), sample()).is_err());
        assert_eq!(
            fs::read(fixture.path()).expect("owned placement fixture"),
            content
        );
    }
    fs::remove_file(fixture.path()).expect("owned placement fixture");
    let mut store =
        WindowPlacementPreferences::open(fixture.path()).expect("owned placement fixture");
    store.remember(sample());
    fs::write(fixture.path(), b"future v2").expect("owned placement fixture");
    drop(store);
    assert_eq!(
        fs::read(fixture.path()).expect("owned placement fixture"),
        b"future v2"
    );
}

#[test]
fn placement_busy_lock_does_not_wait_or_replace_the_previous_record() {
    let fixture = Fixture::new();
    write(&fixture.path(), sample()).expect("owned placement fixture");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.path().with_extension("lock"))
        .expect("owned placement fixture");
    lock.lock().expect("owned placement fixture");
    let mut store =
        WindowPlacementPreferences::open(fixture.path()).expect("owned placement fixture");
    store.remember(SavedWindowPlacement {
        maximized: true,
        ..sample()
    });
    let start = std::time::Instant::now();
    drop(store);
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(
        read(&fixture.path()).expect("owned placement fixture"),
        Some(sample())
    );
}
