use super::*;

pub(super) fn resolve(
    origin: &Path,
    direction: FolderNavigation,
    generation: u64,
    last_live_window: &mut Option<HWND>,
    current: &impl Fn() -> bool,
    shell_ready: bool,
) -> Option<FolderSnapshot> {
    let origin = canonical_shell_path(origin).unwrap_or_else(|_| origin.to_owned());
    let listing = match direction {
        FolderNavigation::Parent => origin.parent().map(|path| vec![path.to_owned()]),
        FolderNavigation::FirstChild => {
            directories(&origin, last_live_window, current, shell_ready)
        }
        FolderNavigation::Previous | FolderNavigation::Next => origin.parent().and_then(|parent| {
            directories(parent, last_live_window, current, shell_ready)
                .map(|paths| siblings(&paths, &origin, direction == FolderNavigation::Next))
        }),
    };
    choose(listing.unwrap_or_default(), generation, current, |path| {
        shell_ready
            .then(|| shell_snapshot(path, generation, last_live_window, current, false))
            .flatten()
            .or_else(|| fallback_snapshot(path, generation, current))
    })
    .or_else(|| {
        current().then(|| FolderSnapshot {
            folder_identity: ShellIdentity::new(Vec::new()),
            folder_path: origin,
            items: Vec::new(),
            sort_columns: Vec::new(),
            source: FolderSnapshotSource::NaturalNameFallback,
            generation,
            captured_at: SystemTime::now(),
        })
    })
}

fn siblings(paths: &[PathBuf], origin: &Path, forward: bool) -> Vec<PathBuf> {
    let Some(index) = paths.iter().position(|path| path == origin) else {
        return Vec::new();
    };
    (1..paths.len())
        .map(|step| {
            let target = if forward {
                (index + step) % paths.len()
            } else {
                (index + paths.len() - step) % paths.len()
            };
            paths[target].clone()
        })
        .collect()
}

fn choose(
    candidates: Vec<PathBuf>,
    generation: u64,
    current: &impl Fn() -> bool,
    mut snapshot: impl FnMut(&Path) -> Option<FolderSnapshot>,
) -> Option<FolderSnapshot> {
    for folder in candidates {
        if !current() {
            return None;
        }
        if !has_media(&folder, current) {
            continue;
        }
        if let Some(mut result) = snapshot(&folder) {
            // Enumeration may race deletion or a folder named like a media file.
            // Keep only direct files that still exist before handing off to the app.
            result.items.retain(|item| current() && item.path.is_file());
            if !current() {
                return None;
            }
            if !result.items.is_empty() {
                result.generation = generation;
                return Some(result);
            }
        }
    }
    None
}

fn has_media(folder: &Path, current: &impl Fn() -> bool) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };
    for entry in entries {
        if !current() {
            return false;
        }
        if let Ok(entry) = entry
            && MediaKind::from_path(&entry.path()).is_some()
            && entry.path().is_file()
        {
            return true;
        }
    }
    false
}

fn directories(
    folder: &Path,
    last_live_window: &mut Option<HWND>,
    current: &impl Fn() -> bool,
    shell_ready: bool,
) -> Option<Vec<PathBuf>> {
    if shell_ready
        && let Some(paths) = shell_listing(
            folder,
            last_live_window,
            current,
            false,
            &|view, _, _, _| {
                // SAFETY: Shell invokes this callback synchronously on the owning STA.
                // Interfaces never leave it; only owned filesystem paths are returned.
                unsafe {
                    let array: IShellItemArray =
                        view.Items(SVGIO_ALLVIEW | SVGIO_FLAG_VIEWORDER).ok()?;
                    let mut paths = Vec::new();
                    for index in 0..array.GetCount().ok()? {
                        if !current() {
                            return None;
                        }
                        let item = array.GetItemAt(index).ok()?;
                        if let Some(path) = shell_item_path(&item)
                            && path.is_dir()
                        {
                            paths.push(path);
                        }
                    }
                    Some(paths)
                }
            },
        )
    {
        return Some(paths);
    }
    if !current() {
        return None;
    }
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(folder).ok()? {
        if !current() {
            return None;
        }
        if let Ok(entry) = entry
            && entry.path().is_dir()
        {
            paths.push(entry.path());
        }
    }
    paths.sort_by(|left, right| natural_path_cmp(left, right));
    Some(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sibling_candidates_follow_input_order_once_and_wrap_in_both_directions() {
        let paths: Vec<_> = ["folder10", "folder2", "folder1"].map(PathBuf::from).into();
        assert_eq!(siblings(&paths, &paths[2], true), paths[..2]);
        assert_eq!(
            siblings(&paths, &paths[0], false),
            vec![paths[2].clone(), paths[1].clone()]
        );
        assert!(siblings(&paths[..1], &paths[0], true).is_empty());
        assert!(siblings(&[], Path::new("missing"), true).is_empty());
        assert!(siblings(&paths, Path::new("removed"), false).is_empty());
    }

    #[test]
    fn folder_candidates_skip_empty_nested_only_and_vanished_media_without_decoding() {
        let root = super::super::tests::test_directory("related-folders");
        for name in ["empty", "nested/child", "named.jpg", "good", "vanished"] {
            fs::create_dir_all(root.join(name)).expect("owned folder");
        }
        fs::write(root.join("nested/child/deeper.jpg"), b"extension only")
            .expect("folder navigation fixture");
        fs::write(root.join("empty/text.txt"), b"unsupported").expect("folder navigation fixture");
        fs::create_dir(root.join("named.jpg/another.jpg")).expect("folder navigation fixture");
        fs::write(root.join("good/test.MP3"), b"extension only")
            .expect("folder navigation fixture");
        let vanished = root.join("vanished/test.png");
        fs::write(&vanished, b"extension only").expect("folder navigation fixture");
        assert!(!has_media(&root.join("empty"), &|| true));
        assert!(!has_media(&root.join("nested"), &|| true));
        assert!(!has_media(&root.join("named.jpg"), &|| true));
        assert!(!has_media(&root.join("absent"), &|| true));
        assert!(!has_media(&root.join("good"), &|| false));
        let mut visited = Vec::new();
        let result = choose(
            ["empty", "nested", "named.jpg", "vanished", "good"]
                .map(|p| root.join(p))
                .into(),
            71,
            &|| true,
            |folder| {
                visited.push(folder.to_owned());
                let result = fallback_snapshot(folder, 1, &|| true);
                if folder == root.join("vanished") {
                    fs::remove_file(&vanished).expect("folder navigation fixture");
                }
                result
            },
        )
        .expect("folder navigation fixture");
        assert_eq!(result.folder_path, root.join("good"));
        assert_eq!(result.generation, 71);
        assert_eq!(visited, [root.join("vanished"), root.join("good")]);
        assert!(
            choose(vec![root.join("good")], 1, &|| false, |_| panic!(
                "cancelled"
            ))
            .is_none()
        );
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

    #[test]
    fn native_related_folder_requests_complete_and_invalidate_superseded_results() {
        let root = super::super::tests::test_directory("native-related-folders");
        for name in ["a", "b", "empty"] {
            fs::create_dir_all(root.join(name)).expect("folder navigation fixture");
        }
        for path in ["root.png", "a/first.jpg", "b/last.mp4"] {
            fs::write(root.join(path), b"extension only").expect("folder navigation fixture");
        }
        let (sent, ready) = mpsc::channel();
        let provider = FolderOrderProvider::with_notify(move || {
            let _ = sent.send(());
        })
        .expect("folder navigation fixture");
        let wait = |generation| {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if let Some(snapshot) = provider.take_completed() {
                    assert_eq!(snapshot.generation, generation);
                    return snapshot;
                }
                assert!(Instant::now() < deadline, "native folder request");
                let _ = ready.recv_timeout(Duration::from_millis(10));
            }
        };
        for (origin, direction, target) in [
            ("b", FolderNavigation::Next, "a"),
            ("a", FolderNavigation::Previous, "b"),
            ("b", FolderNavigation::Parent, ""),
            ("", FolderNavigation::FirstChild, "a"),
        ] {
            let generation = provider.request_navigation(root.join(origin), direction);
            let result = wait(generation);
            assert_eq!(result.folder_path, root.join(target));
            assert_eq!(result.items.len(), 1);
            assert_ne!(result.source, FolderSnapshotSource::NaturalNameFallback);
        }
        provider.request_navigation(root.join("a"), FolderNavigation::Next);
        provider.request(None);
        let generation = provider.request_navigation(root.join("a"), FolderNavigation::FirstChild);
        assert!(wait(generation).items.is_empty(), "no child keeps origin");
        drop(provider);
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }
}
