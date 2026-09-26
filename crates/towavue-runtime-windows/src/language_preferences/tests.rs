use super::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "towavue-language-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("fixture");
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("language.conf")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root = self.0.canonicalize().expect("owned root");
        assert!(root.starts_with(std::env::temp_dir().canonicalize().expect("temp")));
        assert!(
            root.file_name()
                .expect("name")
                .to_string_lossy()
                .starts_with("towavue-language-")
        );
        if !std::thread::panicking() {
            fs::remove_dir_all(root).expect("cleanup");
        }
    }
}

#[test]
fn language_preferences_publish_before_completion_and_drain_on_shutdown() {
    let fixture = Fixture::new();
    let (send, receive) = mpsc::channel();
    let store = LanguagePreferences::open(fixture.path(), move |result| {
        send.send(result).expect("result");
    })
    .expect("open");
    assert_eq!(store.initial(), Language::English);
    assert!(!fixture.path().exists());
    for language in [Language::Japanese, Language::English, Language::Japanese] {
        store.remember(language).expect("queue");
        assert_eq!(
            receive
                .recv_timeout(Duration::from_secs(5))
                .expect("completed")
                .expect("saved language"),
            language
        );
        assert_eq!(read(&fixture.path()).expect("published"), Some(language));
    }
    store.remember(Language::English).expect("final choice");
    drop(store);
    assert_eq!(
        read(&fixture.path()).expect("drained"),
        Some(Language::English)
    );
    assert_eq!(
        fs::read_dir(&fixture.0).expect("files").count(),
        2,
        "no partial temporary file"
    );
}

#[test]
fn language_preferences_preserve_unknown_corrupt_and_externally_changed_content() {
    let fixture = Fixture::new();
    for content in [
        b"future v2".to_vec(),
        format!("{HEADER}\nfr\n").into_bytes(),
        format!("{HEADER}\nja\nextra\n").into_bytes(),
        vec![b'x'; 129],
        vec![0xff],
    ] {
        fs::write(fixture.path(), &content).expect("fixture");
        assert!(LanguagePreferences::open(fixture.path(), |_| {}).is_err());
        assert!(write(&fixture.path(), Language::English).is_err());
        assert_eq!(fs::read(fixture.path()).expect("intact"), content);
    }
    fs::remove_file(fixture.path()).expect("reset");
    let (send, receive) = mpsc::channel();
    let store = LanguagePreferences::open(fixture.path(), move |result| {
        send.send(result).expect("result");
    })
    .expect("open");
    fs::write(fixture.path(), b"external replacement").expect("external write");
    store.remember(Language::Japanese).expect("queue");
    let error = receive
        .recv_timeout(Duration::from_secs(5))
        .expect("failure")
        .expect_err("unknown content refused");
    assert_eq!(error.to_string(), "Invalid language preference");
    assert_eq!(
        error.message(Language::Japanese),
        "表示言語の設定が不正です"
    );
    assert!(
        matches!(error, LanguagePreferenceError::Io(ref io) if io.kind() == io::ErrorKind::InvalidData)
    );
    assert_eq!(
        fs::read(fixture.path()).expect("intact"),
        b"external replacement"
    );
}

#[test]
fn language_preferences_report_locked_storage_and_remain_retryable() {
    let fixture = Fixture::new();
    write(&fixture.path(), Language::English).expect("initial");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.path().with_extension("lock"))
        .expect("lock file");
    lock.lock().expect("competing writer");
    let (send, receive) = mpsc::channel();
    let store = LanguagePreferences::open(fixture.path(), move |result| {
        send.send(result).expect("result");
    })
    .expect("open");
    store.remember(Language::Japanese).expect("queue");
    assert!(
        receive
            .recv_timeout(Duration::from_secs(5))
            .expect("bounded failure")
            .is_err()
    );
    assert_eq!(
        read(&fixture.path()).expect("unchanged"),
        Some(Language::English)
    );
    drop(lock);
    store.remember(Language::Japanese).expect("retry");
    assert_eq!(
        receive
            .recv_timeout(Duration::from_secs(5))
            .expect("saved")
            .expect("retry succeeds"),
        Language::Japanese
    );
}

#[test]
fn language_queue_refusal_keeps_would_block_and_native_text() {
    let (sender, _receiver) = mpsc::sync_channel(0);
    let store = LanguagePreferences {
        initial: Language::English,
        sender: Some(sender),
        worker: None,
    };
    let error = store
        .remember(Language::Japanese)
        .expect_err("no waiting receiver");
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(error.to_string(), "Language preference writer is busy");
    assert_eq!(
        LanguagePreferenceError::io_message(&error, Language::Japanese),
        "表示言語の設定を保存中です"
    );
    let external = io::Error::other("Invalid language preference");
    assert_eq!(
        LanguagePreferenceError::io_message(&external, Language::Japanese),
        "Invalid language preference"
    );
}
