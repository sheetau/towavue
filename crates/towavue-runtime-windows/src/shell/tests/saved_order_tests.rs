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
                let expected = {
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
                    let snapshot = set_sort_and_capture(&view, &folder, &pidl, &columns);
                    view.cast::<IShellView>()
                        .expect("Shell view")
                        .SaveViewState()
                        .expect("save");
                    snapshot
                };
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
