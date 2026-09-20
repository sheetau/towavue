use super::*;

#[test]
fn hidden_views_read_saved_shell_sort_without_overwriting_it() {
    let root = test_directory("saved-sort");
    fs::create_dir(&root).expect("owned fixture");
    create_timed_file(&root.join("item10.jpg"), 100, 1, 4);
    create_timed_file(&root.join("item2.mp3"), 200, 2, 3);
    create_timed_file(&root.join("alpha.png"), 100, 3, 2);
    create_timed_file(&root.join("beta.mp4"), 300, 4, 1);
    let fixture = root.clone();
    thread::spawn(move || {
        let apartment = ShellApartment::new();
        assert!(apartment.0, "owned STA");
        let folder = canonical_shell_path(&fixture).expect("fixture path");
        let pidl = parse_path(&folder).expect("fixture PIDL");
        // SAFETY: all native objects stay on this STA. Only this unique generated
        // folder's view state is written; no reference or user folder is changed.
        unsafe {
            for columns in [
                vec![native_column(DATE_CREATED, SORT_DESCENDING)],
                vec![native_column(NAME, SORT_DESCENDING)],
                vec![native_column(NAME, SORT_ASCENDING)],
                vec![
                    native_column(SIZE, SORT_ASCENDING),
                    native_column(NAME, SORT_DESCENDING),
                ],
            ] {
                let expected = save_fixture_sort(&folder, &pidl, &columns, 4);
                for generation in 1..=2 {
                    let deadline = Instant::now() + Duration::from_secs(30);
                    let actual =
                        hidden_snapshot(&folder, &pidl, generation, &|| Instant::now() < deadline)
                            .expect("fresh read-only snapshot");
                    assert_eq!(actual.source, FolderSnapshotSource::PersistedShellView);
                    assert_eq!(actual.sort_columns, expected.sort_columns);
                    assert!(
                        actual
                            .items
                            .iter()
                            .map(|item| (&item.path, item.kind))
                            .eq(expected.items.iter().map(|item| (&item.path, item.kind)))
                    );
                }
                // Mutating the observer's transient order must not replace the
                // persisted order when its hidden browser is destroyed.
                {
                    let browser = HiddenExplorerBrowser::new().expect("observer");
                    assert_ne!(
                        browser.browser.GetOptions().expect("observer options").0
                            & EBO_NOPERSISTVIEWSTATE.0,
                        0
                    );
                    let enumeration = HiddenEnumeration::new(&browser.browser).expect("events");
                    browser
                        .browser
                        .BrowseToIDList(pidl.as_ptr(), SBSP_ABSOLUTE)
                        .expect("browse");
                    let deadline = Instant::now() + Duration::from_secs(30);
                    enumeration
                        .wait(&|| Instant::now() < deadline)
                        .expect("ready");
                    let view = browser
                        .browser
                        .GetCurrentView::<IFolderView2>()
                        .expect("view");
                    set_sort_and_capture(
                        &view,
                        &folder,
                        &pidl,
                        &[native_column(DATE_MODIFIED, SORT_ASCENDING)],
                    );
                }
                let deadline = Instant::now() + Duration::from_secs(30);
                let retained = hidden_snapshot(&folder, &pidl, 3, &|| Instant::now() < deadline)
                    .expect("retained persisted order");
                assert_eq!(retained.sort_columns, expected.sort_columns);
                assert!(
                    retained
                        .items
                        .iter()
                        .map(|item| &item.path)
                        .eq(expected.items.iter().map(|item| &item.path))
                );
            }
        }
    })
    .join()
    .expect("fixture worker");
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

// SAFETY: use only for an owned generated folder on its initialized STA. This
// deliberately persists that fixture's view settings, never a user folder's.
pub(in crate::shell) unsafe fn save_name_order_fixture(
    folder: &Path,
    pidl: &OwnedPidl,
    count: usize,
) -> FolderSnapshot {
    unsafe { save_fixture_sort(folder, pidl, &[native_column(NAME, SORT_ASCENDING)], count) }
}

unsafe fn save_fixture_sort(
    folder: &Path,
    pidl: &OwnedPidl,
    columns: &[SORTCOLUMN],
    count: usize,
) -> FolderSnapshot {
    unsafe {
        let browser = FixtureWriter::new();
        browser
            .browser
            .SetPropertyBag(w!("Shell"))
            .expect("Explorer state");
        browser
            .browser
            .SetOptions(EBO_NOBORDER | EBO_NOTRAVELLOG)
            .expect("enable persistence for owned fixture only");
        let enumeration = HiddenEnumeration::new(&browser.browser).expect("events");
        browser
            .browser
            .BrowseToIDList(pidl.as_ptr(), SBSP_ABSOLUTE)
            .expect("browse");
        let deadline = Instant::now() + Duration::from_secs(30);
        enumeration
            .wait(&|| Instant::now() < deadline)
            .expect("ready");
        let view = browser
            .browser
            .GetCurrentView::<IFolderView2>()
            .expect("view");
        view.SetCurrentFolderFlags(FWF_NOBROWSERVIEWSTATE.0 as u32, 0)
            .expect("enable view persistence for fixture writer");
        let snapshot = set_sort_and_capture_count(&view, folder, pidl, columns, count);
        view.cast::<IShellView>()
            .expect("Shell view")
            .SaveViewState()
            .expect("save");
        snapshot
    }
}

// Deliberately independent of the read-only production browser's teardown.
// The fixture must actually persist settings before testing observer reads.
struct FixtureWriter {
    browser: IExplorerBrowser,
    host: HWND,
}

impl FixtureWriter {
    unsafe fn new() -> Self {
        unsafe {
            let host = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE::default(),
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .expect("fixture writer host");
            let browser: IExplorerBrowser = CoCreateInstance(&ExplorerBrowser, None, CLSCTX_ALL)
                .expect("fixture writer browser");
            let writer = Self { browser, host };
            writer
                .browser
                .Initialize(
                    host,
                    &RECT {
                        left: 0,
                        top: 0,
                        right: 1,
                        bottom: 1,
                    },
                    None,
                )
                .expect("initialize fixture writer");
            writer
        }
    }
}

impl Drop for FixtureWriter {
    fn drop(&mut self) {
        // SAFETY: the test creates and drops this writer on one STA. Only the
        // uniquely generated fixture's view state is persisted during Destroy.
        unsafe {
            let _ = self.browser.Destroy();
            let _ = DestroyWindow(self.host);
        }
    }
}

#[test]
fn refreshed_views_use_current_files_and_live_sort_group_without_persisting() {
    let root = test_directory("publication-order");
    fs::create_dir(&root).expect("owned fixture");
    for (i, name) in ["a.png", "b.jpg", "c.mp3", "d.mp4"].iter().enumerate() {
        create_timed_file(&root.join(name), 100, i as u64 + 1, i as u64 + 1);
    }
    let fixture = root.clone();
    thread::spawn(move || {
        let apartment = ShellApartment::new();
        assert!(apartment.0);
        let folder = canonical_shell_path(&fixture).expect("folder");
        let pidl = parse_path(&folder).expect("PIDL");
        let deadline = Instant::now() + Duration::from_secs(30);
        let current = || Instant::now() < deadline;
        // SAFETY: all Shell interfaces stay on this STA. Only the generated
        // fixture's view state is initially saved; later live changes are transient.
        unsafe {
            let browser = FixtureWriter::new();
            browser.browser.SetPropertyBag(w!("Shell")).expect("bag");
            browser
                .browser
                .SetOptions(EBO_NOBORDER | EBO_NOTRAVELLOG)
                .expect("options");
            let enumeration = HiddenEnumeration::new(&browser.browser).expect("events");
            browser
                .browser
                .BrowseToIDList(pidl.as_ptr(), SBSP_ABSOLUTE)
                .expect("browse");
            enumeration.wait(&current).expect("ready");
            let view = browser
                .browser
                .GetCurrentView::<IFolderView2>()
                .expect("view");
            view.SetCurrentFolderFlags(FWF_NOBROWSERVIEWSTATE.0 as u32, 0)
                .expect("fixture persistence");
            let before = set_sort_and_capture(
                &view,
                &folder,
                &pidl,
                &[native_column(DATE_CREATED, SORT_DESCENDING)],
            );
            view.cast::<IShellView>()
                .expect("view")
                .SaveViewState()
                .expect("save fixture order");
            view.SetCurrentFolderFlags(
                FWF_NOBROWSERVIEWSTATE.0 as u32,
                FWF_NOBROWSERVIEWSTATE.0 as u32,
            )
            .expect("protect persisted settings");
            let names = |s: &FolderSnapshot| {
                s.items
                    .iter()
                    .map(|i| {
                        i.path
                            .file_name()
                            .expect("name")
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(names(&before), ["d.mp4", "c.mp3", "b.jpg", "a.png"]);
            let staging = folder.join("staging");
            fs::create_dir(&staging).expect("staging");
            for (phase, name) in ["a.png", "new.png"].iter().enumerate() {
                create_timed_file(&staging.join("output.png"), 200, phase as u64 + 10, 10);
                fs::rename(staging.join("output.png"), folder.join(name))
                    .expect("atomic publication");
                let order = refresh::ViewOrder::read(&view).expect("live settings");
                let fresh = hidden_snapshot_with_order(&folder, &pidl, 2, &current, Some(&order))
                    .expect("fresh live settings");
                let expected = if phase == 0 {
                    vec!["a.png", "d.mp4", "c.mp3", "b.jpg"]
                } else {
                    vec!["new.png", "a.png", "d.mp4", "c.mp3", "b.jpg"]
                };
                assert_eq!(
                    names(&fresh),
                    expected,
                    "new native enumeration must not inherit stale item properties/order"
                );
                assert_eq!(fresh.sort_columns, before.sort_columns);
                assert_eq!(fresh.source, FolderSnapshotSource::LiveExplorerView);
                eprintln!("PASS publication phase={phase}: {:?}", names(&fresh));
            }
            // Live unsaved settings take precedence, including grouping; the
            // observer must not fall back to the saved creation-date order.
            let columns = [native_column(NAME, SORT_DESCENDING)];
            view.SetGroupBy(&TYPE, true).expect("live grouping");
            view.SetSortColumns(&columns).expect("live sort");
            let order = refresh::ViewOrder::read(&view).expect("live settings");
            let grouped = hidden_snapshot_with_order(&folder, &pidl, 3, &current, Some(&order))
                .expect("fresh grouped order");
            assert_eq!(
                grouped.sort_columns,
                columns
                    .into_iter()
                    .map(sort_column)
                    .collect::<Option<Vec<_>>>()
                    .expect("columns")
            );
            loop {
                assert!(current(), "live settings settle");
                pump_messages();
                let live = capture_view(
                    &view,
                    &folder,
                    &pidl,
                    FolderSnapshotSource::LiveExplorerView,
                    4,
                    &current,
                )
                .expect("live capture");
                if names(&live) == names(&grouped) {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let after = refresh::ViewOrder::read(&view).expect("live settings preserved");
            assert_eq!(after.group, order.group);
            assert_eq!(after.ascending, order.ascending);
            let saved =
                hidden_snapshot(&folder, &pidl, 5, &current).expect("saved settings retained");
            assert_eq!(saved.sort_columns, before.sort_columns);
            assert_eq!(
                names(&saved),
                ["new.png", "a.png", "d.mp4", "c.mp3", "b.jpg"]
            );
            assert!(
                hidden_snapshot_with_order(&folder, &pidl, 6, &|| false, Some(&order)).is_none()
            );
            eprintln!(
                "PASS live grouping: {:?}; saved order and cancellation preserved",
                names(&grouped)
            );
        }
    })
    .join()
    .expect("owned STA control");
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
#[ignore = "exports an owned temporary sibling of an explicitly selected reference image"]
fn reference_exports_and_source_save_refresh_creation_order() {
    let source = PathBuf::from(
        std::env::var_os("TOWAVUE_SHELL_REFERENCE_SOURCE").expect("set reference image"),
    );
    let source = canonical_shell_path(&source).expect("reference file");
    let folder = source.parent().expect("reference folder");
    let original = fs::read(&source).expect("reference bytes remain unchanged");
    let original_time = fs::metadata(&source)
        .expect("metadata")
        .modified()
        .expect("modified");
    let mut provider = FolderOrderProvider::new().expect("provider");
    let before = provider.snapshot(folder).expect("baseline");
    assert_eq!(
        before.sort_columns,
        vec![sort_column(native_column(DATE_CREATED, SORT_DESCENDING)).expect("created sort")]
    );
    let target = folder.join(format!(
        ".towavue-order-{}-{}.png",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let reservation = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .expect("reserve owned output");
    drop(reservation);
    struct OwnedOutput(PathBuf);
    impl Drop for OwnedOutput {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            notify_published_file(&self.0);
        }
    }
    let output = OwnedOutput(target.clone());
    let request = crate::ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: MediaKind::Image,
        operations: vec![towavue_core::EditOperation::FlipHorizontal],
        hardware_encode: false,
    };
    for phase in 0..3 {
        if phase == 1 {
            let prepared = crate::prepare_source_save(
                crate::FileOperationSource::capture(&target).expect("owned source"),
                crate::ExportRequest {
                    source: target.clone(),
                    ..request.clone()
                },
                crate::ExportOptions::default(),
                &std::sync::atomic::AtomicBool::new(false),
                &|_| {},
                &|_| {},
            )
            .expect("prepare actual Source Save");
            let (send, receive) = mpsc::channel();
            crate::commit_source_save(prepared, move |result| {
                send.send(result).ok();
            })
            .expect("publication");
            let saved = receive
                .recv_timeout(Duration::from_secs(30))
                .expect("completion")
                .expect("saved");
            drop(saved);
        } else {
            let (send, receive) = mpsc::channel();
            let job = crate::ExportJob::start(request.clone(), move |event| {
                if let crate::ExportEvent::Finished(result) = event {
                    send.send(result).ok();
                }
            })
            .expect("actual export worker");
            receive
                .recv_timeout(Duration::from_secs(30))
                .expect("completion")
                .expect("exported");
            drop(job);
        }
        let old = provider.snapshot(folder).expect("ordinary live capture");
        let old_index = old.items.iter().position(|i| i.path == target);
        let started = Instant::now();
        let generation = provider.request_refreshed(folder.to_owned());
        let refreshed = loop {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "refresh deadline"
            );
            if let Some(snapshot) = provider.take_completed() {
                break snapshot;
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(refreshed.generation, generation);
        assert_eq!(refreshed.sort_columns, before.sort_columns);
        assert_eq!(refreshed.items.len(), before.items.len() + 1);
        assert_eq!(refreshed.items.first().expect("first").path, target);
        let created: Vec<_> = refreshed
            .items
            .iter()
            .map(|i| {
                fs::metadata(&i.path)
                    .expect("current item")
                    .created()
                    .expect("created time")
            })
            .collect();
        let inversions = created
            .windows(2)
            .enumerate()
            .filter(|(_, times)| times[0] < times[1])
            .collect::<Vec<_>>();
        let baseline_paths = before.items.iter().map(|i| &i.path).collect::<Vec<_>>();
        let retained_paths = refreshed
            .items
            .iter()
            .filter(|i| i.path != target)
            .map(|i| &i.path)
            .collect::<Vec<_>>();
        eprintln!(
            "REFERENCE baseline_order_preserved={} raw_creation_inversions={} examples={:?}",
            retained_paths == baseline_paths,
            inversions.len(),
            inversions
                .iter()
                .take(5)
                .map(|(index, times)| (
                    index,
                    times[1].duration_since(times[0]).expect("inverted gap")
                ))
                .collect::<Vec<_>>()
        );
        assert!(
            retained_paths == baseline_paths,
            "unmodified entries retain the existing native Shell sequence"
        );
        eprintln!(
            "PASS reference publication phase={phase} source={:?} count={} old_index={old_index:?} refreshed_index=0 elapsed_ms={:.3}",
            refreshed.source,
            refreshed.items.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    drop(output);
    assert!(!target.exists());
    assert_eq!(fs::read(&source).expect("reference preserved"), original);
    assert_eq!(
        fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("modified"),
        original_time
    );
    eprintln!("PASS reference source bytes/mtime unchanged; owned sibling removed");
}
