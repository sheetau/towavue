use super::*;
use std::fs;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self::under(&std::env::temp_dir())
    }

    fn under(parent: &Path) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("owned file-operation fixture")
            .as_nanos();
        let path = parent.join(format!(
            "towavue-file-operation-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("owned fixture");
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.0
                .file_name()
                .expect("owned file-operation fixture")
                .to_string_lossy()
                .starts_with("towavue-file-operation-")
        );
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn source(path: &Path) -> FileOperationSource {
    FileOperationSource::capture(path).expect("source snapshot")
}

#[test]
fn rename_move_and_case_only_rename_keep_bytes_and_do_not_replace_destinations() {
    let fixture = Fixture::new();
    let original = fixture.0.join("original.png");
    let renamed = fixture.0.join("renamed.png");
    let destination = fixture.0.join("destination");
    fs::create_dir(&destination).expect("owned file-operation fixture");
    let bytes: Vec<u8> = (0..4096).map(|n| (n % 251) as u8).collect();
    fs::write(&original, &bytes).expect("owned file-operation fixture");
    assert_eq!(
        perform(
            &source(&original),
            FileOperationAction::Rename("renamed.png".into())
        )
        .expect("owned file-operation fixture"),
        FileOperationOutcome::Moved(renamed.clone())
    );
    assert!(!original.exists());
    let upper = fixture.0.join("RENAMED.png");
    assert_eq!(
        perform(
            &source(&renamed),
            FileOperationAction::Rename("RENAMED.png".into())
        )
        .expect("owned file-operation fixture"),
        FileOperationOutcome::Moved(upper.clone())
    );
    assert!(
        fs::read_dir(&fixture.0)
            .expect("owned file-operation fixture")
            .any(|e| e.expect("owned file-operation fixture").file_name() == "RENAMED.png")
    );
    let target = destination.join("RENAMED.png");
    fs::write(&target, b"keep existing destination").expect("owned file-operation fixture");
    assert!(matches!(
        perform(
            &source(&upper),
            FileOperationAction::MoveToFolder(destination.clone())
        ),
        Err(FileOperationError::DestinationExists)
    ));
    assert_eq!(
        fs::read(&target).expect("owned file-operation fixture"),
        b"keep existing destination"
    );
    assert_eq!(
        fs::read(&upper).expect("owned file-operation fixture"),
        bytes
    );
    fs::remove_file(&target).expect("owned file-operation fixture");
    assert_eq!(
        perform(
            &source(&upper),
            FileOperationAction::MoveToFolder(destination)
        )
        .expect("owned file-operation fixture"),
        FileOperationOutcome::Moved(target.clone())
    );
    assert!(!upper.exists());
    assert_eq!(
        fs::read(&target).expect("owned file-operation fixture"),
        bytes
    );
}

#[test]
fn rename_dialog_paths_reject_other_folders_and_accept_equivalent_parents() {
    let fixture = Fixture::new();
    let original = fixture.0.join("source.png");
    let other = fixture.0.join("other");
    fs::create_dir(&other).expect("other folder");
    fs::write(&original, b"owned content").expect("source");
    assert!(matches!(
        perform(
            &source(&original),
            FileOperationAction::RenameToPath(other.join("new.png"))
        ),
        Err(FileOperationError::InvalidName)
    ));
    assert_eq!(fs::read(&original).expect("unchanged"), b"owned content");
    assert!(!other.join("new.png").exists());
    let target = fixture.0.join("new.png");
    let equivalent = other.join("..").join("new.png");
    assert_eq!(
        perform(
            &source(&original),
            FileOperationAction::RenameToPath(equivalent)
        )
        .expect("same parent"),
        FileOperationOutcome::Moved(target.clone())
    );
    assert!(!original.exists());
    assert_eq!(fs::read(target).expect("renamed"), b"owned content");
}

#[test]
fn stale_sources_invalid_names_and_unavailable_folders_preserve_files() {
    let fixture = Fixture::new();
    let original = fixture.0.join("source.png");
    fs::write(&original, b"initial").expect("owned file-operation fixture");
    let stale = source(&original);
    fs::rename(&original, fixture.0.join("old.png")).expect("owned file-operation fixture");
    fs::write(&original, b"replacement").expect("owned file-operation fixture");
    for action in [
        FileOperationAction::Rename("new.png".into()),
        FileOperationAction::MoveToFolder(fixture.0.clone()),
        FileOperationAction::Recycle,
    ] {
        assert!(matches!(
            perform(&stale, action),
            Err(FileOperationError::SourceChanged)
        ));
        assert_eq!(
            fs::read(&original).expect("owned file-operation fixture"),
            b"replacement"
        );
    }
    let current = source(&original);
    for name in [
        "",
        ".",
        "..",
        "../escape.png",
        "nested/file.png",
        "nested\\file.png",
        "c:other.png",
        "NUL",
        "CON.png",
        "COM1.txt",
        "LPT9.txt",
        "trail.",
        "trail ",
        "bad\0.png",
    ] {
        assert!(
            matches!(
                perform(&current, FileOperationAction::Rename(name.into())),
                Err(FileOperationError::InvalidName)
            ),
            "{name:?}"
        );
    }
    assert!(
        perform(
            &current,
            FileOperationAction::MoveToFolder(fixture.0.join("missing"))
        )
        .is_err()
    );
    assert_eq!(
        fs::read(&original).expect("owned file-operation fixture"),
        b"replacement"
    );
    assert!(matches!(
        FileOperationSource::capture(&fixture.0),
        Err(FileOperationError::NotRegularFile)
    ));
    let hard_link = fixture.0.join("hard-link.png");
    fs::hard_link(&original, &hard_link).expect("owned file-operation fixture");
    assert!(matches!(
        perform(
            &current,
            FileOperationAction::Rename("hard-link.png".into())
        ),
        Err(FileOperationError::DestinationExists)
    ));
    assert_eq!(
        fs::read(&hard_link).expect("owned file-operation fixture"),
        b"replacement"
    );
}

#[test]
fn mutation_workers_return_each_accepted_result_once_and_reject_busy_writers() {
    let fixture = Fixture::new();
    let original = fixture.0.join("source.png");
    fs::write(&original, b"worker bytes").expect("owned file-operation fixture");
    let (send, receive) = mpsc::channel();
    inspect_file_operation_source(original.clone(), move |result| {
        send.send(result).expect("owned file-operation fixture")
    })
    .expect("owned file-operation fixture");
    let snapshot = receive
        .recv_timeout(Duration::from_secs(10))
        .expect("owned file-operation fixture")
        .expect("owned file-operation fixture");
    assert_eq!(snapshot.path(), original);
    assert!(matches!(
        receive.recv_timeout(Duration::from_secs(10)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    let writer = OpenOptions::new()
        .write(true)
        .open(&original)
        .expect("owned file-operation fixture");
    assert!(snapshot.verify().is_err());
    drop(writer);
    let (send, receive) = mpsc::channel();
    start_file_operation(
        snapshot,
        FileOperationAction::Rename("worker.png".into()),
        move |result| send.send(result).expect("owned file-operation fixture"),
    )
    .expect("owned file-operation fixture");
    assert_eq!(
        receive
            .recv_timeout(Duration::from_secs(10))
            .expect("owned file-operation fixture")
            .expect("owned file-operation fixture"),
        FileOperationOutcome::Moved(fixture.0.join("worker.png"))
    );
    assert!(matches!(
        receive.recv_timeout(Duration::from_secs(10)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    assert_eq!(
        fs::read(fixture.0.join("worker.png")).expect("owned file-operation fixture"),
        b"worker bytes"
    );
}

#[test]
fn copied_source_is_not_reported_as_moved_and_missing_output_is_not_success() {
    let fixture = Fixture::new();
    let original = fixture.0.join("source.bin");
    let target = fixture.0.join("copy.bin");
    fs::write(&original, b"retained original").expect("owned file-operation fixture");
    fs::write(&target, b"retained original").expect("owned file-operation fixture");
    assert_eq!(
        move_outcome(&original, target.clone(), false).expect("owned file-operation fixture"),
        FileOperationOutcome::CopiedButSourceRetained(target.clone())
    );
    assert_eq!(
        fs::read(&original).expect("owned file-operation fixture"),
        b"retained original"
    );
    fs::remove_file(&target).expect("owned file-operation fixture");
    assert!(matches!(
        move_outcome(&original, target, false),
        Err(FileOperationError::NotCompleted)
    ));
}

#[test]
#[ignore = "recycles one uniquely owned tiny fixture through the native Windows Shell"]
fn native_recycle_removes_only_the_selected_owned_file() {
    let fixture = Fixture::new();
    let selected = fixture.0.join("selected.txt");
    let neighbor = fixture.0.join("neighbor.txt");
    fs::write(&selected, b"towavue native recycle fixture").expect("owned file-operation fixture");
    fs::write(&neighbor, b"keep neighbor").expect("owned file-operation fixture");
    assert_eq!(
        perform(&source(&selected), FileOperationAction::Recycle)
            .expect("owned file-operation fixture"),
        FileOperationOutcome::Recycled
    );
    assert!(!selected.exists());
    assert_eq!(
        fs::read(&neighbor).expect("owned file-operation fixture"),
        b"keep neighbor"
    );
}

#[test]
fn native_recycle_guard_cancels_permanent_delete_before_removal() {
    let fixture = Fixture::new();
    let selected = fixture.0.join("must-survive.txt");
    fs::write(&selected, b"permanent deletion must be rejected").expect("owned source");
    // Deliberately omit recycle flags from the real Shell operation. Its callback
    // must cancel this request, not merely detect disappearance after deletion.
    assert!(
        recycle::run(
            &selected,
            windows::Win32::UI::Shell::FOF_NO_UI | windows::Win32::UI::Shell::FOFX_EARLYFAILURE
        )
        .is_err()
    );
    assert_eq!(
        fs::read(&selected).expect("guard retained source"),
        b"permanent deletion must be rejected"
    );
}

#[test]
#[ignore = "requires TOWAVUE_FILE_MOVE_OTHER_VOLUME naming a writable folder on another volume"]
fn cross_volume_move_preserves_the_file_and_reports_completion() {
    let parent = std::env::var_os("TOWAVUE_FILE_MOVE_OTHER_VOLUME")
        .expect("explicit other-volume test parent");
    let from = Fixture::new();
    let to = Fixture::under(Path::new(&parent));
    let input = from.0.join("cross-volume.bin");
    let probe = to.0.join("probe.bin");
    let bytes = vec![71; 256 * 1024];
    fs::write(&input, &bytes).expect("owned file-operation fixture");
    fs::write(&probe, b"volume probe").expect("owned file-operation fixture");
    let snapshot = source(&input);
    assert_ne!(
        snapshot.stamp.volume,
        source(&probe).stamp.volume,
        "test requires different volumes"
    );
    assert_eq!(
        perform(&snapshot, FileOperationAction::MoveToFolder(to.0.clone()))
            .expect("owned file-operation fixture"),
        FileOperationOutcome::Moved(to.0.join("cross-volume.bin"))
    );
    assert!(!input.exists());
    assert_eq!(
        fs::read(to.0.join("cross-volume.bin")).expect("owned file-operation fixture"),
        bytes
    );
}
