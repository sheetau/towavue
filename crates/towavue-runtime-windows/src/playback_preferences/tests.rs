use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "towavue-volume-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos(),
        ));
        fs::create_dir(&root).expect("owned fixture");
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("volume.conf")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.0.is_absolute() && self.0.starts_with(std::env::temp_dir()));
        assert!(
            self.0
                .file_name()
                .expect("name")
                .to_string_lossy()
                .starts_with("towavue-volume-")
        );
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn preference_worker_coalesces_adjustments_and_flushes_mute_restore_on_shutdown() {
    let fixture = Fixture::new();
    let store =
        PlaybackVolumePreferences::open(fixture.path(), |error| panic!("{error}")).expect("open");
    assert_eq!(store.initial(), (0.5, 0.5));
    assert!(
        !fixture.path().exists(),
        "opening does not manufacture a preference"
    );
    for step in 1..=200 {
        let level = step as f32 / 100.0;
        store.remember(level, level).expect("adjust");
    }
    store.remember(0.0, 1.234567).expect("muted");
    drop(store);
    let store =
        PlaybackVolumePreferences::open(fixture.path(), |error| panic!("{error}")).expect("reopen");
    assert_eq!(store.initial(), (0.0, 1.234567));
    store.remember(1.234567, 1.234567).expect("unmute");
    drop(store);
    let store =
        PlaybackVolumePreferences::open(fixture.path(), |error| panic!("{error}")).expect("reopen");
    assert_eq!(store.initial(), (1.234567, 1.234567));
}

#[test]
fn preferences_reject_invalid_levels_and_preserve_unknown_or_changed_records() {
    let fixture = Fixture::new();
    let (send, receive) = std::sync::mpsc::channel();
    let store = PlaybackVolumePreferences::open(fixture.path(), move |error| {
        send.send(error).expect("error receiver");
    })
    .expect("open");
    for (level, unmuted) in [
        (f32::NAN, 0.5),
        (0.0, f32::INFINITY),
        (-0.1, 0.5),
        (2.1, 2.1),
        (0.0, 0.0),
        (0.5, 0.7),
    ] {
        assert!(store.remember(level, unmuted).is_err());
    }
    assert!(!fixture.path().exists());
    fs::write(fixture.path(), b"future preference version").expect("external change");
    store.remember(0.8, 0.8).expect("enqueue");
    drop(store);
    assert!(
        !receive
            .recv_timeout(Duration::from_secs(5))
            .expect("reported failure")
            .is_empty()
    );
    assert_eq!(
        fs::read(fixture.path()).expect("preserved"),
        b"future preference version"
    );
    for text in [
        "future preference version".to_owned(),
        format!("{HEADER}\n1\nNaN\n0.5\n"),
        format!("{HEADER}\n1\n0\n0\n"),
        format!("{HEADER}\n1\n0.5\n0.5\nextra\n"),
        "x".repeat(257),
    ] {
        fs::write(fixture.path(), &text).expect("malformed fixture");
        assert!(PlaybackVolumePreferences::open(fixture.path(), |_| {}).is_err());
        assert_eq!(fs::read_to_string(fixture.path()).expect("preserved"), text);
    }
}

#[test]
fn delayed_older_volume_write_never_replaces_a_newer_observation() {
    let fixture = Fixture::new();
    write(
        &fixture.path(),
        Record {
            observed: 20,
            level: 1.5,
            unmuted: 1.5,
        },
    )
    .expect("new");
    let expected = fs::read(fixture.path()).expect("new bytes");
    write(
        &fixture.path(),
        Record {
            observed: 10,
            level: 0.0,
            unmuted: 0.7,
        },
    )
    .expect("old");
    assert_eq!(fs::read(fixture.path()).expect("retained"), expected);
}

#[test]
fn failed_atomic_replacement_keeps_previous_volume_and_cleans_its_candidate() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    write(
        &fixture.path(),
        Record {
            observed: 1,
            level: 0.3,
            unmuted: 0.3,
        },
    )
    .expect("initial");
    let original = fs::read(fixture.path()).expect("initial bytes");
    let lease = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(fixture.path())
        .expect("deny replacement");
    assert!(
        write(
            &fixture.path(),
            Record {
                observed: 2,
                level: 0.7,
                unmuted: 0.7
            }
        )
        .is_err()
    );
    assert_eq!(fs::read(fixture.path()).expect("original bytes"), original);
    assert_eq!(
        fs::read_dir(&fixture.0).expect("directory").count(),
        2,
        "only preference and lock"
    );
    drop(lease);
    write(
        &fixture.path(),
        Record {
            observed: 3,
            level: 0.7,
            unmuted: 0.7,
        },
    )
    .expect("retry");
    assert_eq!(
        read(&fixture.path()).expect("read").expect("record").level,
        0.7
    );
}
