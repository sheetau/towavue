use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "towavue-view-preferences-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("owned preference fixture")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("owned preference fixture");
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("viewing.conf")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.0.starts_with(std::env::temp_dir())
                && self
                    .0
                    .file_name()
                    .expect("owned preference fixture")
                    .to_string_lossy()
                    .starts_with("towavue-view-preferences-")
        );
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).expect("owned preference fixture");
        }
    }
}

#[test]
fn shutdown_drains_latest_choices_and_defaults_do_not_create_a_file() {
    let fixture = Fixture::new();
    let store = ViewPreferences::open(fixture.path(), |error| panic!("{error}"))
        .expect("owned preference fixture");
    for key in ViewPreference::ALL {
        assert_eq!(store.value(key), key.default_value());
    }
    assert!(!fixture.path().exists());
    for step in 0..200 {
        store
            .remember(ViewPreference::Resize, step % 3)
            .expect("owned preference fixture");
    }
    store
        .remember(ViewPreference::VolumeStep, 10)
        .expect("owned preference fixture");
    drop(store);
    let reopened = ViewPreferences::open(fixture.path(), |error| panic!("{error}"))
        .expect("owned preference fixture");
    assert_eq!(reopened.value(ViewPreference::Resize), 1);
    assert_eq!(reopened.value(ViewPreference::VolumeStep), 10);
    drop(reopened);
}

#[test]
fn concurrent_owners_merge_only_changed_keys_and_reject_delayed_older_values() {
    let fixture = Fixture::new();
    let first = ViewPreferences::open(fixture.path(), |error| panic!("{error}"))
        .expect("owned preference fixture");
    let second = ViewPreferences::open(fixture.path(), |error| panic!("{error}"))
        .expect("owned preference fixture");
    first
        .remember(ViewPreference::Resize, 2)
        .expect("owned preference fixture");
    second
        .remember(ViewPreference::VolumeStep, 5)
        .expect("owned preference fixture");
    drop(second);
    drop(first);
    write(
        &fixture.path(),
        [(
            ViewPreference::Resize,
            Record {
                observed: 1,
                value: 0,
            },
        )]
        .into(),
    )
    .expect("owned preference fixture");
    let reopened = ViewPreferences::open(fixture.path(), |error| panic!("{error}"))
        .expect("owned preference fixture");
    assert_eq!(reopened.value(ViewPreference::Resize), 2);
    assert_eq!(reopened.value(ViewPreference::VolumeStep), 5);
    drop(reopened);
}

#[test]
fn invalid_or_future_files_are_preserved_and_save_failure_is_reported() {
    let fixture = Fixture::new();
    let (send, receive) = std::sync::mpsc::channel();
    let store = ViewPreferences::open(fixture.path(), move |error| {
        send.send(error).expect("owned preference fixture");
    })
    .expect("owned preference fixture");
    assert!(store.remember(ViewPreference::VolumeStep, 3).is_err());
    assert!(store.remember(ViewPreference::ReadingPages, 203).is_err());
    fs::write(fixture.path(), "future preferences").expect("owned preference fixture");
    store
        .remember(ViewPreference::Resize, 1)
        .expect("owned preference fixture");
    drop(store);
    assert_eq!(
        receive
            .recv_timeout(Duration::from_secs(5))
            .expect("owned preference fixture")
            .kind(),
        io::ErrorKind::InvalidData
    );
    for text in [
        "future preferences".to_owned(),
        format!("{HEADER}\nresize 1 3\n"),
        format!("{HEADER}\nresize 1 1\nresize 2 2\n"),
        format!("{HEADER}\nunknown 1 1\n"),
        "x".repeat(4097),
    ] {
        fs::write(fixture.path(), &text).expect("owned preference fixture");
        assert!(ViewPreferences::open(fixture.path(), |_| {}).is_err());
        assert_eq!(
            fs::read_to_string(fixture.path()).expect("owned preference fixture"),
            text
        );
    }
}

#[test]
fn failed_replacement_preserves_prior_settings_and_removes_temporary_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    write(
        &fixture.path(),
        [(
            ViewPreference::Resize,
            Record {
                observed: 1,
                value: 1,
            },
        )]
        .into(),
    )
    .expect("owned preference fixture");
    let original = fs::read(fixture.path()).expect("owned preference fixture");
    let lease = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(fixture.path())
        .expect("owned preference fixture");
    assert!(
        write(
            &fixture.path(),
            [(
                ViewPreference::Resize,
                Record {
                    observed: 2,
                    value: 2
                }
            )]
            .into()
        )
        .is_err()
    );
    assert_eq!(
        fs::read(fixture.path()).expect("owned preference fixture"),
        original
    );
    assert_eq!(
        fs::read_dir(&fixture.0)
            .expect("owned preference fixture")
            .count(),
        2
    );
    drop(lease);
}
