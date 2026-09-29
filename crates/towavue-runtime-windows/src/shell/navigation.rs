use super::*;

/// The destination of a related-folder lookup, before opening or enumerating its media.
#[derive(Debug)]
pub struct FolderNavigationResult {
    pub generation: u64,
    pub target: Option<PathBuf>,
    /// Parent/child search scope for the ordinary empty-folder message when no target exists.
    pub searched_folder: PathBuf,
}

pub(super) fn resolve(
    origin: &Path,
    direction: FolderNavigation,
    generation: u64,
    last_live_window: &mut Option<HWND>,
    current: &impl Fn() -> bool,
    shell_ready: bool,
) -> Option<FolderNavigationResult> {
    let origin = canonical_shell_path(origin).unwrap_or_else(|_| origin.to_owned());
    let searched_folder = match direction {
        FolderNavigation::FirstChild => origin.clone(),
        _ => origin.parent().unwrap_or(&origin).to_owned(),
    };
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
    let mut target = None;
    for folder in listing.unwrap_or_default() {
        if !current() {
            return None;
        }
        if has_media(&folder, current) {
            target = Some(folder);
            break;
        }
    }
    current().then_some(FolderNavigationResult {
        generation,
        target,
        searched_folder,
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

fn has_media(folder: &Path, current: &impl Fn() -> bool) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };
    has_media_entries(entries, current)
}

fn has_media_entries(
    entries: impl IntoIterator<Item = std::io::Result<std::fs::DirEntry>>,
    current: &impl Fn() -> bool,
) -> bool {
    for entry in entries {
        if !current() {
            return false;
        }
        if let Ok(entry) = entry
            && MediaKind::from_path(&entry.path()).is_some()
            && entry
                .file_type()
                .is_ok_and(|kind| kind.is_file() || (kind.is_symlink() && entry.path().is_file()))
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
                        // Most rows may be media files. Use Shell's folder bit before
                        // resolving a filesystem path or issuing a per-path directory query.
                        let folders = windows::Win32::System::SystemServices::SFGAO_FOLDER;
                        if item.GetAttributes(folders).ok()?.0 & folders.0 == 0 {
                            continue;
                        }
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
            && entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() || (kind.is_symlink() && entry.path().is_dir()))
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
    fn ordinary_folder_open_excludes_directories_with_media_extensions() {
        let root = super::super::tests::test_directory("media-directory-names");
        fs::create_dir_all(root.join("directory.jpg")).expect("owned directory");
        let file = root.join("real.png");
        fs::write(&file, b"extension only").expect("owned media");
        let fixture = root.clone();
        thread::spawn(move || {
            let apartment = ShellApartment::new();
            assert!(apartment.0, "owned STA");
            let mut live = None;
            for expected in [1, 0] {
                let deadline = Instant::now() + Duration::from_secs(30);
                let current = || Instant::now() < deadline;
                let native = shell_snapshot(&fixture, 1, &mut live, &current, false)
                    .expect("ordinary folder open");
                let fallback = fallback_snapshot(&fixture, 1, &current).expect("fallback open");
                for result in [native, fallback] {
                    assert_eq!(result.items.len(), expected);
                    if let Some(item) = result.items.first() {
                        assert_eq!(item.path, file);
                    }
                }
                if expected == 1 {
                    fs::remove_file(&file).expect("remove owned media");
                }
            }
        })
        .join()
        .expect("STA test");
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }

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
    fn folder_candidates_stop_at_first_file_and_skip_empty_nested_only_and_missing_paths() {
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
        let first = fs::read_dir(root.join("good"))
            .expect("fixture listing")
            .next()
            .expect("one file");
        let entries = std::iter::once(first).chain(std::iter::from_fn(|| {
            panic!("discovery must stop before reading the next entry");
        }));
        assert!(has_media_entries(entries, &|| true));
        fs::remove_file(&vanished).expect("remove owned media before discovery");
        assert!(!has_media(&root.join("vanished"), &|| true));
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
                if let Some(snapshot) = provider.take_navigation() {
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
            assert_eq!(result.target, Some(root.join(target)));
            assert!(
                provider.take_completed().is_none(),
                "discovery returns no media listing"
            );
        }
        provider.request_navigation(root.join("a"), FolderNavigation::Next);
        provider.request(None);
        let generation = provider.request_navigation(root.join("a"), FolderNavigation::FirstChild);
        let empty = wait(generation);
        assert!(empty.target.is_none(), "no child keeps origin");
        assert_eq!(empty.searched_folder, root.join("a"));
        drop(provider);
        fs::remove_dir_all(root).expect("owned fixture cleanup");
    }
}
