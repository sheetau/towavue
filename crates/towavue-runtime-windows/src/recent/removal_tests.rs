use super::*;

fn fixture(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "towavue-recent-removal-{name}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("owned directory");
    root
}

fn entry(path: &Path, kind: RecentKind, stamp: u64) -> RecentEntry {
    RecentEntry {
        path: path.to_path_buf(),
        kind,
        opened_at: Some(stamp),
    }
}

#[test]
fn legacy_kinds_stay_read_only_until_folder_removal_migrates_atomically() {
    let root = fixture("migration");
    let source = root.join("source.png");
    fs::write(&source, b"original source").expect("owned source");
    let path = root.join("recent-files.txt");
    let text = format!(
        "{KIND_HEADER}\nF\t1\t{}\nD\t2\t{}\n",
        source.display(),
        root.display()
    );
    fs::write(&path, &text).expect("v3 fixture");
    let initial = read_history(&path).expect("legacy read");
    assert!(initial.hidden_folders.is_empty());
    assert_eq!(fs::read_to_string(&path).expect("unchanged"), text);
    let mut pending = Pending::default();
    pending.remove(root.clone(), RecentKind::Folder);
    let removed = update_pending(&path, &pending, false).expect("remove folder");
    assert_eq!(removed.entries, [entry(&source, RecentKind::File, 1)]);
    assert_eq!(removed.hidden_folders, std::slice::from_ref(&root));
    assert!(fs::read_to_string(&path).expect("v4").starts_with(HEADER));
    assert_eq!(
        read_history(&path).expect("reopen").hidden_folders,
        std::slice::from_ref(&root)
    );
    assert_eq!(
        fs::read(&source).expect("retained source"),
        b"original source"
    );
    let mut revisit = Pending::default();
    revisit.record(entry(&source, RecentKind::File, 3));
    assert!(
        update_pending(&path, &revisit, false)
            .expect("revisit")
            .hidden_folders
            .is_empty()
    );
    fs::remove_dir_all(root).expect("owned cleanup");
}

#[test]
fn coalesced_open_remove_and_folder_effects_keep_request_order() {
    let root = fixture("ordering");
    let path = root.join("recent-files.txt");
    let a = root.join("a.png");
    let b = root.join("b.png");
    let mut pending = Pending::default();
    pending.record(entry(&a, RecentKind::File, 1));
    pending.record(entry(&root, RecentKind::Folder, 2));
    pending.remove(root.clone(), RecentKind::Folder);
    let hidden = update_pending(&path, &pending, false).expect("record then remove folder");
    assert_eq!(hidden.entries, [entry(&a, RecentKind::File, 1)]);
    assert_eq!(hidden.hidden_folders, std::slice::from_ref(&root));
    let mut incoming = Pending::default();
    incoming.record(entry(&b, RecentKind::File, 3));
    incoming.remove(b.clone(), RecentKind::File);
    pending.merge(incoming);
    let restored =
        update_pending(&path, &pending, false).expect("preserve visit effect after file removal");
    assert!(restored.hidden_folders.is_empty());
    assert_eq!(restored.entries, [entry(&a, RecentKind::File, 1)]);
    let mut later = Pending::default();
    later.remove(a.clone(), RecentKind::File);
    later.record(entry(&a, RecentKind::File, 4));
    assert_eq!(
        update_pending(&path, &later, false)
            .expect("reopen after removal")
            .entries,
        [entry(&a, RecentKind::File, 4)]
    );
    later.remove(a, RecentKind::File);
    assert!(
        update_pending(&path, &later, false)
            .expect("last removal")
            .entries
            .is_empty()
    );
    fs::remove_dir_all(root).expect("owned cleanup");
}

#[test]
fn workers_suppress_stale_removal_results_and_refresh_other_window_changes() {
    let root = fixture("workers");
    let path = root.join("recent-files.txt");
    let source = root.join("source.png");
    fs::write(&source, b"source unchanged").expect("source");
    update(&path, &[entry(&source, RecentKind::File, 1)], false).expect("seed");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
        .expect("lock");
    lock.lock().expect("hold initial read");
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = RecentFiles::new(path.clone(), move || {
        let _ = sender.send(());
    })
    .expect("worker");
    worker.remove(root.clone(), RecentKind::Folder);
    drop(lock);
    let await_update = |expected_hidden: bool, expected_files: usize| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            receiver
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("notification");
            if let Some(result) = worker.take_completed() {
                assert!(result.error.is_none());
                if result.entries.len() == expected_files
                    && result.hidden_folders.contains(&root) == expected_hidden
                {
                    return result;
                }
                panic!("stale history snapshot was published");
            }
        }
    };
    await_update(true, 1);
    let other = RecentFiles::new(path.clone(), || {}).expect("other window");
    other.record(source.clone());
    other.record_command(towavue_core::CommandId::OpenFile);
    drop(other);
    worker.refresh();
    let refreshed = await_update(false, 1);
    assert_eq!(
        refreshed.commands.expect("refreshed MRU"),
        [towavue_core::CommandId::OpenFile]
    );
    worker.remove(source.clone(), RecentKind::File);
    await_update(false, 0);
    drop(worker);
    assert!(read(&path).expect("persisted removal").is_empty());
    assert_eq!(
        fs::read(source).expect("source unchanged"),
        b"source unchanged"
    );
    fs::remove_dir_all(root).expect("owned cleanup");
}

#[test]
fn clear_removes_folder_exclusions_and_malformed_exclusions_preserve_store() {
    let root = fixture("clear");
    let path = root.join("recent-files.txt");
    let child = root.join("child.png");
    let mut pending = Pending::default();
    pending.record(entry(&child, RecentKind::File, 1));
    pending.remove(root.clone(), RecentKind::Folder);
    update_pending(&path, &pending, false).expect("hidden parent");
    let cleared = update_pending(&path, &Pending::default(), true).expect("clear all file history");
    assert!(cleared.entries.is_empty() && cleared.hidden_folders.is_empty());
    let invalid = format!("{HEADER}\nH\trelative-folder\n");
    fs::write(&path, &invalid).expect("malformed fixture");
    assert!(update_pending(&path, &pending, false).is_err());
    assert_eq!(fs::read_to_string(&path).expect("preserved bytes"), invalid);
    fs::remove_dir_all(root).expect("owned cleanup");
}
