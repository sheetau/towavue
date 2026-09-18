use super::*;
use crate::{FileOperationError, FileOperationSource};
use std::fs::{self, File, FileTimes};
use std::time::Instant;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "towavue-image-source-version-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&path).expect("exclusive fixture");
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for name in ["source", "replacement", "old"] {
            let _ = fs::remove_file(self.0.join(name));
        }
        let _ = fs::remove_dir(&self.0);
    }
}
fn load(loader: &ImageLoader, path: &Path) -> LoadedImages {
    let generation = loader.request(vec![path.to_owned()]);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = loader.take_completed() {
            assert_eq!(result.generation, generation);
            return result;
        }
        assert!(Instant::now() < deadline, "source-version load deadline");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn source_version_survives_cache_hits_and_rejects_same_length_and_mtime_replacements() {
    let fixture = Fixture::new();
    let path = fixture.0.join("source");
    fs::write(&path, [17]).expect("original");
    let modified = fs::metadata(&path)
        .expect("metadata")
        .modified()
        .expect("mtime");
    let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
    let loader = ImageLoader {
        shared: Arc::clone(&shared),
        prefetch_worker: LatestTask::new("source-version-prefetch").expect("worker"),
    };
    let worker = thread::spawn(move || {
        run_worker(
            shared,
            || {},
            |path, _, _, _| Ok((*pixel(fs::read(path).expect("owned input")[0])).clone()),
        )
    });
    let first = load(&loader, &path);
    let source = first.source.expect("loaded version");
    source.verify().expect("unchanged input");
    let pixels = first.images[0].1.as_ref().expect("first pixels");
    let cached = load(&loader, &path);
    assert_eq!(cached.source.as_ref(), Some(&source));
    assert!(Arc::ptr_eq(
        pixels,
        cached.images[0].1.as_ref().expect("cached pixels")
    ));
    let replacement = fixture.0.join("replacement");
    fs::write(&replacement, [93]).expect("replacement");
    File::options()
        .write(true)
        .open(&replacement)
        .expect("metadata handle")
        .set_times(FileTimes::new().set_modified(modified))
        .expect("same mtime");
    fs::rename(&path, fixture.0.join("old")).expect("retain old file ID");
    fs::rename(&replacement, &path).expect("replace logical input");
    assert_eq!(
        fs::metadata(&path)
            .expect("new metadata")
            .modified()
            .expect("mtime"),
        modified
    );
    assert!(matches!(
        source.verify(),
        Err(FileOperationError::SourceChanged)
    ));
    source
        .with_path(fixture.0.join("old"))
        .verify()
        .expect("logical rename retains the old identity");
    assert_eq!(
        source
            .after_move(&fixture.0.join("old"))
            .expect("verified moved version")
            .path(),
        fixture.0.join("old")
    );
    assert!(
        matches!(
            source.after_move(&path),
            Err(FileOperationError::SourceChanged)
        ),
        "an unrelated same-size replacement is not the moved original"
    );
    let changed = load(&loader, &path);
    assert_ne!(changed.source.as_ref(), Some(&source));
    assert_eq!(
        changed.source,
        Some(FileOperationSource::capture(&path).expect("new version"))
    );
    let actual = changed.images[0].1.as_ref().expect("new pixels");
    assert_eq!(actual.frames[0].rgba, [93; 4]);
    assert!(
        !Arc::ptr_eq(pixels, actual),
        "old pixels cannot acquire the new file identity"
    );
    drop(loader);
    worker.join().expect("worker shutdown");
}

#[test]
fn source_version_is_unavailable_when_the_file_changes_during_decode() {
    let fixture = Fixture::new();
    let path = fixture.0.join("source");
    fs::write(&path, [17]).expect("original");
    let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
    let loader = ImageLoader {
        shared: Arc::clone(&shared),
        prefetch_worker: LatestTask::new("source-version-mutation").expect("worker"),
    };
    let worker = thread::spawn(move || {
        run_worker(
            shared,
            || {},
            |path, _, _, _| {
                fs::write(path, [93, 94]).expect("controlled during-decode mutation");
                Ok((*pixel(17)).clone())
            },
        )
    });
    let result = load(&loader, &path);
    assert!(
        result.images[0].1.is_ok(),
        "viewing remains independent of save authorization"
    );
    assert!(
        result.source.is_none(),
        "never bind old pixels to the newly observed file"
    );
    assert!(
        loader
            .shared
            .0
            .lock()
            .expect("mailbox")
            .cache
            .lock()
            .expect("cache")
            .entries
            .is_empty()
    );
    drop(loader);
    worker.join().expect("worker shutdown");
}
