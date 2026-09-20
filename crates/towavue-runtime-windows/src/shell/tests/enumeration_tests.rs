use super::*;
use std::fs;

// Experimental only. Both controls acquire a fresh view-ordered array and resolve
// every path/identity exactly as capture_view does; no retained Shell item cache.
unsafe fn enumerate(view: &IFolderView2, batched: bool) -> Vec<FolderMediaItem> {
    unsafe {
        let array: IShellItemArray = view
            .Items(SVGIO_ALLVIEW | SVGIO_FLAG_VIEWORDER)
            .expect("view-ordered array");
        let count = array.GetCount().expect("array length");
        let mut items = Vec::new();
        let mut append = |item: IShellItem| {
            let Some(path) = shell_item_path(&item) else {
                return;
            };
            let Some(kind) = MediaKind::from_path(&path) else {
                return;
            };
            let pidl = OwnedPidl(SHGetIDListFromObject(&item).expect("absolute identity"));
            items.push(FolderMediaItem {
                identity: pidl.identity(),
                path,
                kind,
            });
        };
        if batched {
            let enumeration = array.EnumItems().expect("Shell item enumerator");
            let mut total = 0;
            while total < count {
                let mut batch: [Option<IShellItem>; 32] = std::array::from_fn(|_| None);
                let requested = (count - total).min(batch.len() as u32) as usize;
                let mut fetched = 0;
                enumeration
                    .Next(&mut batch[..requested], Some(&mut fetched))
                    .expect("enumerate batch");
                assert!(fetched > 0 && fetched as usize <= requested);
                total += fetched;
                for item in batch.iter_mut().take(fetched as usize) {
                    append(item.take().expect("returned Shell item"));
                }
            }
        } else {
            for index in 0..count {
                append(array.GetItemAt(index).expect("indexed Shell item"));
            }
        }
        items
    }
}

unsafe fn compare(view: &IFolderView2, count: usize, live: bool) {
    // SAFETY: caller retains this view on its initialized STA for the comparison.
    unsafe {
        let expected = enumerate(view, false);
        if !live {
            assert_eq!(expected.len(), count * 3 / 4);
        }
        assert_eq!(enumerate(view, true), expected);
        for batched in [false, true, true, false] {
            let mut samples = Vec::new();
            for _ in 0..15 {
                let started = Instant::now();
                let items = enumerate(view, batched);
                samples.push(started.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(items, expected, "view order, paths, kinds and PIDLs");
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "SHELL_ENUMERATION entries={count} live={live} batched={batched} median_ms={:.3}",
                samples[7]
            );
        }
    }
}

#[test]
#[ignore = "Release read-only enumeration of an existing matching Explorer window"]
fn live_shell_enumeration_reports_batch_cost() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("use Release for timing");
    }
    let folder = std::env::var_os("TOWAVUE_SHELL_REFERENCE_DIR")
        .map(PathBuf::from)
        .ok_or("set TOWAVUE_SHELL_REFERENCE_DIR to an already open reference folder")?;
    thread::spawn(move || {
        let apartment = ShellApartment::new();
        assert!(apartment.0, "Shell STA");
        let folder = canonical_shell_path(&folder).expect("reference folder");
        let pidl = parse_path(&folder).expect("reference PIDL");
        // SAFETY: borrowed live view and all returned interfaces remain on this
        // STA. No window creation/input, sort changes, images or source writes.
        unsafe {
            let (view, _) = matching_live_view(pidl.as_ptr(), None, &|| true)
                .ok_or("no matching live Explorer window; no timing evidence")?;
            let array: IShellItemArray = view
                .Items(SVGIO_ALLVIEW | SVGIO_FLAG_VIEWORDER)
                .expect("view array");
            let count = array.GetCount().expect("view count") as usize;
            drop(array);
            compare(&view, count, true);
        }
        Ok(())
    })
    .join()
    .expect("live Shell comparison worker")
}

#[test]
#[ignore = "Release hidden-Shell enumeration comparison; creates owned empty files"]
fn hidden_shell_enumeration_reports_batch_cost() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("use Release for timing");
    }
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "towavue-shell-enumeration-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("owned fixture root");
    for count in [32, 1024] {
        let folder = root.join(count.to_string());
        fs::create_dir(&folder).expect("fixture folder");
        for index in (0..count).rev() {
            let extension = ["png", "jpg", "mp4", "txt"][index % 4];
            fs::write(folder.join(format!("item-{index}.{extension}")), [])
                .expect("owned empty file");
        }
        thread::spawn(move || {
            let apartment = ShellApartment::new();
            assert!(apartment.0, "Shell STA");
            // SAFETY: this thread owns the apartment, hidden browser, view, arrays
            // and batch interfaces. They all drop before apartment teardown.
            unsafe {
                let folder = canonical_shell_path(&folder).expect("canonical folder");
                let pidl = parse_path(&folder).expect("folder PIDL");
                let browser = HiddenExplorerBrowser::new().expect("hidden Shell browser");
                browser
                    .browser
                    .BrowseToIDList(pidl.as_ptr(), SBSP_ABSOLUTE)
                    .expect("open fixture");
                let deadline = Instant::now() + Duration::from_secs(10);
                let view = loop {
                    pump_messages();
                    if let Ok(view) = browser.browser.GetCurrentView::<IFolderView2>()
                        && view_matches(&view, pidl.as_ptr())
                        && let Ok(array) =
                            view.Items::<IShellItemArray>(SVGIO_ALLVIEW | SVGIO_FLAG_VIEWORDER)
                        && array.GetCount().expect("view count") == count as u32
                    {
                        break view;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "hidden view population timed out"
                    );
                    thread::sleep(Duration::from_millis(10));
                };
                compare(&view, count, false);
            }
        })
        .join()
        .expect("Shell comparison worker");
    }
    fs::remove_dir_all(root).expect("remove owned fixtures");
    Ok(())
}

#[test]
#[ignore = "read-only first/reused Shell snapshots of an explicitly selected reference folder"]
fn reference_first_requests_preserve_shell_order() -> Result<(), String> {
    let folder = std::env::var_os("TOWAVUE_SHELL_REFERENCE_DIR")
        .map(PathBuf::from)
        .ok_or("set TOWAVUE_SHELL_REFERENCE_DIR to the reference folder")?;
    let expected_first = std::env::var_os("TOWAVUE_SHELL_EXPECTED_FIRST");
    let hidden = std::env::var_os("TOWAVUE_SHELL_FORCE_HIDDEN").is_some();
    let mut baseline: Option<FolderSnapshot> = if std::env::var_os("TOWAVUE_SHELL_COMPARE_LIVE")
        .is_some()
    {
        let mut provider = FolderOrderProvider::new().map_err(|error| error.to_string())?;
        let snapshot = provider
            .snapshot(&folder)
            .map_err(|error| error.to_string())?;
        if snapshot.source != FolderSnapshotSource::LiveExplorerView {
            return Err("the comparison requires an already open matching Explorer view".into());
        }
        println!("FIRST_SHELL live_baseline_count={}", snapshot.items.len());
        Some(snapshot)
    } else {
        None
    };
    for provider_index in 0..4 {
        let mut provider = FolderOrderProvider::new().map_err(|error| error.to_string())?;
        for request in 0..2 {
            let started = Instant::now();
            let snapshot = if hidden {
                let folder = folder.clone();
                thread::spawn(move || first_hidden_snapshot(&folder))
                    .join()
                    .map_err(|_| "hidden snapshot worker panicked")??
            } else {
                provider
                    .snapshot(&folder)
                    .map_err(|error| error.to_string())?
            };
            let expected_index = expected_first.as_ref().and_then(|name| {
                snapshot
                    .items
                    .iter()
                    .position(|item| item.path.file_name() == Some(name.as_os_str()))
            });
            let changed = baseline.as_ref().is_some_and(|baseline| {
                baseline.sort_columns != snapshot.sort_columns
                    || !baseline
                        .items
                        .iter()
                        .map(|item| &item.path)
                        .eq(snapshot.items.iter().map(|item| &item.path))
            });
            println!(
                "FIRST_SHELL provider={provider_index} request={request} source={:?} count={} columns={:?} expected_first_index={expected_index:?} changed={changed} elapsed_ms={:.3}",
                snapshot.source,
                snapshot.items.len(),
                snapshot.sort_columns,
                started.elapsed().as_secs_f64() * 1000.0
            );
            if expected_first.is_some() && expected_index != Some(0) {
                return Err(
                    "the supplied first item is not first in the captured Shell view".into(),
                );
            }
            if changed {
                return Err("the first/reused Shell snapshots differ; inspect source and sort metadata before attributing the change".into());
            }
            if baseline.is_none() {
                baseline = Some(snapshot);
            }
        }
    }
    Ok(())
}

fn first_hidden_snapshot(folder: &Path) -> Result<FolderSnapshot, String> {
    let apartment = ShellApartment::new();
    if !apartment.0 {
        return Err("could not initialize reference STA".into());
    }
    let folder = canonical_shell_path(folder).map_err(|error| error.to_string())?;
    let pidl = parse_path(&folder).ok_or("could not parse reference folder")?;
    if std::env::var_os("TOWAVUE_SHELL_LEGACY_CAPTURE").is_none() {
        // SAFETY: this worker owns the initialized apartment and PIDL.
        let started = Instant::now();
        return unsafe {
            hidden_snapshot(&folder, &pidl, 1, &|| {
                started.elapsed() < Duration::from_secs(30)
            })
        }
        .ok_or("hidden reference snapshot failed".into());
    }
    // SAFETY: the probe owns this STA, PIDL, browser and view. No live Explorer
    // window, view state, source media or settings are modified. Drop all native
    // objects before the apartment, as in the production hidden-view path.
    unsafe {
        let browser = HiddenExplorerBrowser::new().ok_or("hidden browser unavailable")?;
        browser
            .browser
            .BrowseToIDList(pidl.as_ptr(), SBSP_ABSOLUTE)
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            pump_messages();
            if let Ok(view) = browser.browser.GetCurrentView::<IFolderView2>()
                && view_matches(&view, pidl.as_ptr())
                && let Some(snapshot) = capture_view(
                    &view,
                    &folder,
                    &pidl,
                    FolderSnapshotSource::PersistedShellView,
                    1,
                    &|| true,
                )
            {
                return Ok(snapshot);
            }
            if Instant::now() >= deadline {
                return Err("hidden reference view timed out".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn hidden_snapshots_wait_for_complete_mixed_and_empty_folders() {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("clock after Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "towavue-shell-ready-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("owned fixture root");
    for count in [0, 4, 4096] {
        let folder = root.join(count.to_string());
        fs::create_dir(&folder).expect("owned folder");
        let mut expected = std::collections::BTreeSet::new();
        for index in (0..count).rev() {
            let extension = ["png", "jpg", "mp4", "wav"][index % 4];
            let name = format!("item-{index}.{extension}");
            fs::write(folder.join(&name), []).expect("owned media placeholder");
            expected.insert(name);
        }
        if count != 0 {
            fs::write(folder.join("excluded.txt"), []).expect("unsupported item");
        }
        thread::spawn(move || {
            if count == 0 {
                // Preserve the established empty-folder fallback: Shell may not
                // expose an IShellItemArray for an empty view. A labelled empty
                // result is safe; it must not leave the request waiting forever.
                let mut provider = FolderOrderProvider::new().expect("empty provider");
                let snapshot = provider.snapshot(&folder).expect("empty folder result");
                assert!(snapshot.items.is_empty());
                return;
            }
            let apartment = ShellApartment::new();
            assert!(apartment.0);
            let folder = canonical_shell_path(&folder).expect("canonical folder");
            let pidl = parse_path(&folder).expect("PIDL");
            // Shell PIDLs are opaque: equivalent items may have different bytes.
            // Compare their canonical meaning while retaining exact path/kind order.
            // https://learn.microsoft.com/windows/win32/api/shlobj_core/nf-shlobj_core-ilisequal
            let desktop =
                unsafe { windows::Win32::UI::Shell::SHGetDesktopFolder().expect("desktop folder") };
            let same_identity = |left: &ShellIdentity, right: &ShellIdentity| {
                // SAFETY: both byte arrays are complete absolute PIDLs copied by
                // capture_view. They remain alive, and this desktop belongs to this STA.
                let result = unsafe {
                    desktop.CompareIDs(
                        windows::Win32::Foundation::LPARAM(
                            windows::Win32::UI::Shell::SHCIDS_CANONICALONLY as isize,
                        ),
                        left.as_bytes().as_ptr().cast(),
                        right.as_bytes().as_ptr().cast(),
                    )
                };
                result.ok().expect("canonical Shell identity comparison");
                result.0 as u16 == 0
            };
            // A new folder inherits the runner's template and can change its
            // default order on first use. Establish an explicit persisted order
            // on this owned fixture before comparing fresh observer snapshots.
            let expected_order =
                unsafe { tests::saved_order_tests::save_name_order_fixture(&folder, &pidl, count) };
            assert_eq!(expected_order.items.len(), count);
            let mut baseline = Some(expected_order);
            for generation in 1..=3 {
                eprintln!("HIDDEN_COMPLETE count={count} generation={generation}");
                let started = Instant::now();
                // SAFETY: all native objects live and drop on this fresh STA.
                let snapshot = unsafe {
                    hidden_snapshot(&folder, &pidl, generation, &|| {
                        started.elapsed() < Duration::from_secs(30)
                    })
                }
                .expect("complete hidden snapshot");
                assert_eq!(snapshot.source, FolderSnapshotSource::PersistedShellView);
                assert_eq!(snapshot.generation, generation);
                assert_eq!(snapshot.items.len(), count);
                let actual = snapshot
                    .items
                    .iter()
                    .map(|item| {
                        item.path
                            .file_name()
                            .expect("fixture file name")
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect::<std::collections::BTreeSet<_>>();
                assert_eq!(actual, expected);
                if let Some(baseline) = &baseline {
                    assert_eq!(snapshot.sort_columns, baseline.sort_columns);
                    for (index, (item, previous)) in
                        snapshot.items.iter().zip(&baseline.items).enumerate()
                    {
                        assert_eq!(
                            (&item.path, item.kind),
                            (&previous.path, previous.kind),
                            "fresh hidden view order/kind at index {index}"
                        );
                        assert!(
                            same_identity(&item.identity, &previous.identity),
                            "fresh hidden view canonical identity at index {index}"
                        );
                    }
                }
                assert!(
                    !same_identity(&snapshot.items[0].identity, &snapshot.items[1].identity),
                    "distinct fixture items must not compare equal"
                );
                baseline = Some(snapshot);
            }
            // Cancellation must work even if the view is already ready, and a
            // cancelled wait must not poison a subsequent wait on that view.
            unsafe {
                let browser = HiddenExplorerBrowser::new().expect("owned browser");
                let enumeration =
                    HiddenEnumeration::new(&browser.browser).expect("subscribe before navigation");
                browser
                    .browser
                    .BrowseToIDList(pidl.as_ptr(), SBSP_ABSOLUTE)
                    .expect("browse");
                let view = browser
                    .browser
                    .GetCurrentView::<IFolderView2>()
                    .expect("view");
                let checks = std::cell::Cell::new(0);
                assert!(
                    enumeration
                        .wait(&|| {
                            checks.set(checks.get() + 1);
                            checks.get() < 2
                        })
                        .is_none()
                );
                assert_eq!(checks.get(), 2);
                let started = Instant::now();
                assert!(
                    enumeration
                        .wait(&|| started.elapsed() < Duration::from_secs(30))
                        .is_some()
                );
                let snapshot = capture_view(
                    &view,
                    &folder,
                    &pidl,
                    FolderSnapshotSource::PersistedShellView,
                    4,
                    &|| true,
                )
                .expect("ready after cancelled wait");
                assert_eq!(snapshot.items.len(), count);
            }
        })
        .join()
        .expect("Shell worker");
    }
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}

#[test]
fn browser_enumeration_sink_does_not_advertise_agility() {
    let events: IExplorerBrowserEvents = EnumerationEvents {
        state: Arc::new(AtomicU8::new(0)),
        view: std::rc::Rc::new(std::cell::RefCell::new(None)),
    }
    .into();
    assert!(
        events
            .cast::<windows::Win32::System::Com::IAgileObject>()
            .is_err(),
        "native connection-point ownership must remain on the creating STA"
    );
}

#[test]
#[ignore = "read-only hidden-view order settling for an explicitly selected reference folder"]
fn reference_hidden_order_reports_settling() -> Result<(), String> {
    let folder = std::env::var_os("TOWAVUE_SHELL_REFERENCE_DIR")
        .map(PathBuf::from)
        .ok_or("set TOWAVUE_SHELL_REFERENCE_DIR")?;
    let expected = std::env::var_os("TOWAVUE_SHELL_EXPECTED_FIRST");
    thread::spawn(move || {
        let apartment = ShellApartment::new();
        assert!(apartment.0, "reference STA");
        let folder = canonical_shell_path(&folder).map_err(|error| error.to_string())?;
        let pidl = parse_path(&folder).ok_or("reference PIDL")?;
        // SAFETY: all browser/view/PIDL values stay on this STA; persistence is
        // disabled and only enumeration/sort metadata are read from the reference.
        unsafe {
            let browser = HiddenExplorerBrowser::new().ok_or("hidden browser")?;
            let enumeration = HiddenEnumeration::new(&browser.browser).ok_or("enumeration sink")?;
            browser.browser.BrowseToIDList(pidl.as_ptr(), SBSP_ABSOLUTE)
                .map_err(|error| error.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(30);
            enumeration.wait(&|| Instant::now() < deadline).ok_or("enumeration failed")?;
            let view = browser.browser.GetCurrentView::<IFolderView2>()
                .map_err(|error| error.to_string())?;
            let started = Instant::now();
            let mut previous: Option<Vec<PathBuf>> = None;
            for delay in [0, 20, 100, 500, 2000] {
                while started.elapsed() < Duration::from_millis(delay) {
                    pump_messages();
                    thread::sleep(Duration::from_millis(2));
                }
                pump_messages();
                let snapshot = capture_view(&view, &folder, &pidl,
                    FolderSnapshotSource::PersistedShellView, 1, &|| true).ok_or("capture")?;
                let paths: Vec<_> = snapshot.items.iter().map(|item| item.path.clone()).collect();
                let first: Vec<_> = snapshot.items.iter().take(6).map(|item| item.path.file_name()).collect();
                let expected_index = expected.as_ref().and_then(|name| snapshot.items.iter()
                    .position(|item| item.path.file_name() == Some(name.as_os_str())));
                println!("SHELL_SETTLING delay_ms={delay} count={} columns={:?} expected_index={expected_index:?} changed={} first={first:?}",
                    paths.len(), snapshot.sort_columns, previous.as_ref().is_some_and(|old| *old != paths));
                previous = Some(paths);
            }
        }
        Ok(())
    }).join().map_err(|_| "reference thread panicked")?
}
