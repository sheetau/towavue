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
