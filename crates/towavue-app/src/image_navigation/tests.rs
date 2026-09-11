use super::*;

#[test]
fn adjacent_navigation_preserves_mixed_shell_order_without_copying_the_folder() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::tests::adjacent_navigation_preserves_mixed_shell_order_without_copying_the_folder",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let items: Vec<_> = (0..50_000)
        .map(|index| {
            let kind = if index % 3 == 1 {
                MediaKind::Video
            } else {
                MediaKind::Image
            };
            towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![]),
                path: root.join(format!(
                    "{}-{index}.{}",
                    "long-owned-name-".repeat(8),
                    if kind == MediaKind::Image {
                        "png"
                    } else {
                        "mp4"
                    }
                )),
                kind,
            }
        })
        .collect();
    let tab = app.tabs.open_new(items[0].path.clone(), MediaKind::Image);
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    let history = app.edits.clone();
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![]),
        folder_path: root.clone(),
        items,
        sort_columns: vec![],
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 1,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    });
    let mut elapsed = Duration::ZERO;
    for current in [0, 25_000, 49_999] {
        let snapshot = app.folder_snapshot.as_ref().expect("snapshot");
        let path = snapshot.items[current].path.clone();
        let kind = snapshot.items[current].kind;
        app.path = Some(path.clone());
        app.media_kind = Some(kind);
        app.tabs
            .active_mut()
            .expect("tab")
            .target
            .set_current_path(path.clone(), kind);
        for same_kind in [false, true] {
            let eligible: Vec<_> = app
                .folder_snapshot
                .as_ref()
                .expect("snapshot")
                .items
                .iter()
                .filter(|item| !same_kind || item.kind == kind)
                .map(|item| item.path.clone())
                .collect();
            let index = eligible
                .iter()
                .position(|candidate| candidate == &path)
                .expect("current");
            for forward in [false, true] {
                let target = if forward {
                    (index + 1) % eligible.len()
                } else {
                    (index + eligible.len() - 1) % eligible.len()
                };
                for _ in 0..10 {
                    let started = Instant::now();
                    app.navigate(forward, same_kind);
                    elapsed += started.elapsed();
                    assert!(
                        matches!(&app.pending_guard, Some(GuardedAction::Navigate(actual)) if actual == &eligible[target])
                    );
                    app.pending_guard = None;
                    assert_eq!(app.path.as_ref(), Some(&path));
                    assert_eq!(app.edits, history);
                }
            }
        }
    }
    eprintln!(
        "adjacent navigation: 120 guarded requests over 50000 mixed long paths in {elapsed:?}"
    );
    app.folder_snapshot
        .as_mut()
        .expect("snapshot")
        .items
        .truncate(1);
    app.path = Some(
        app.folder_snapshot.as_ref().expect("snapshot").items[0]
            .path
            .clone(),
    );
    app.media_kind = Some(MediaKind::Image);
    for same_kind in [false, true] {
        for forward in [false, true] {
            app.navigate(forward, same_kind);
            assert!(app.pending_guard.is_none());
            assert_eq!(app.edits, history);
        }
    }
}

#[test]
fn jumps_count_only_shell_ordered_images_clamp_and_preserve_reading_and_dirty_history() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::tests::jumps_count_only_shell_ordered_images_clamp_and_preserve_reading_and_dirty_history",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let names = [
        "z.png", "d.png", "y.png", "a.png", "x.png", "b.png", "q.png", "p.png", "s.png", "j.png",
        "h.png", "e.png",
    ];
    let tab = app.tabs.open_new(root.join(names[0]), MediaKind::Image);
    app.media_kind = Some(MediaKind::Image);
    app.shortcuts = shortcuts::defaults();
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    let history = app.edits.clone();
    let mut items: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(index, name)| towavue_core::FolderMediaItem {
            identity: towavue_core::ShellIdentity::new(vec![index as u8]),
            path: root.join(name),
            kind: MediaKind::Image,
        })
        .collect();
    items.insert(
        3,
        towavue_core::FolderMediaItem {
            identity: towavue_core::ShellIdentity::new(vec![99]),
            path: root.join("movie.mp4"),
            kind: MediaKind::Video,
        },
    );
    app.folder_snapshot = Some(towavue_core::FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![0]),
        folder_path: root.clone(),
        items,
        sort_columns: vec![],
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 1,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    });
    for reading in [false, true] {
        app.reading_mode = reading;
        app.reading_settings.first_page_count = 1;
        let settings = app.reading_settings;
        for current in [0, 5, 11] {
            let path = root.join(names[current]);
            app.path = Some(path.clone());
            app.tabs
                .active_mut()
                .expect("tab")
                .target
                .set_current_path(path.clone(), MediaKind::Image);
            for count in 1..=10 {
                for forward in [false, true] {
                    let command = format!(
                        "jump_images_{}_{count}",
                        if forward { "forward" } else { "backward" }
                    )
                    .parse::<CommandId>()
                    .expect("jump command");
                    let target = if forward {
                        (current + count).min(names.len() - 1)
                    } else {
                        current.saturating_sub(count)
                    };
                    let generation = app.media_generation;
                    app.dispatch(command);
                    if target == current {
                        assert!(app.pending_guard.is_none());
                    } else {
                        assert!(
                            matches!(&app.pending_guard,Some(GuardedAction::Navigate(target_path)) if *target_path==root.join(names[target])),
                            "{command:?} at {current}"
                        );
                        app.resolve_guard(GuardDecision::Cancel);
                    }
                    assert_eq!(app.path.as_ref(), Some(&path));
                    assert_eq!(app.media_generation, generation);
                    assert_eq!(app.edits, history);
                    assert_eq!(app.reading_settings, settings);
                }
            }
        }
    }
    app.path = Some(root.join(names[0]));
    app.tabs
        .active_mut()
        .expect("tab")
        .target
        .set_current_path(root.join(names[0]), MediaKind::Image);
    for (key, expected) in [
        ("PageDown", 1),
        ("Space", 1),
        ("D", 1),
        ("PageUp", 11),
        ("Backspace", 11),
        ("A", 11),
    ] {
        app.process_shortcut(key.parse().expect("alias"));
        assert!(
            matches!(&app.pending_guard,Some(GuardedAction::Navigate(path)) if *path==root.join(names[expected])),
            "reading alias {key}"
        );
        app.resolve_guard(GuardDecision::Cancel);
    }
    for (key, expected) in [("Ctrl+Space", 5), ("Ctrl+Right", 1), ("Ctrl+0", 10)] {
        app.process_shortcut(key.parse().expect("jump key"));
        assert!(
            matches!(&app.pending_guard,Some(GuardedAction::Navigate(path)) if *path==root.join(names[expected]))
        );
        app.resolve_guard(GuardDecision::Cancel);
    }
}

#[test]
fn jumps_load_actual_images_keep_the_tab_and_preserve_reading_spreads() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::tests::jumps_load_actual_images_keep_the_tab_and_preserve_reading_spreads",
    ) else {
        return;
    };
    let mut bitmap = vec![0_u8; 58];
    bitmap[..2].copy_from_slice(b"BM");
    bitmap[2..6].copy_from_slice(&58_u32.to_le_bytes());
    bitmap[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bitmap[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bitmap[18..22].copy_from_slice(&1_u32.to_le_bytes());
    bitmap[22..26].copy_from_slice(&1_u32.to_le_bytes());
    bitmap[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bitmap[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bitmap[34..38].copy_from_slice(&4_u32.to_le_bytes());
    let paths: Vec<_> = (0..12)
        .map(|index| root.join(format!("{:02}.bmp", 12 - index)))
        .collect();
    for (index, path) in paths.iter().enumerate() {
        bitmap[54..].copy_from_slice(&[index as u8, 30, 90, 0]);
        std::fs::write(path, &bitmap).expect("owned bitmap");
    }
    let (notify, events) = std::sync::mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = notify.send(event);
    })
    .expect("app");
    app.shortcuts = shortcuts::defaults();
    app.ui_context = Some(fonts::test_context());
    let tab = app.tabs.open_new(paths[0].clone(), MediaKind::Image);
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![0]),
        folder_path: root.clone(),
        items: paths
            .iter()
            .enumerate()
            .map(|(index, path)| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![index as u8]),
                path: path.clone(),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: vec![],
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 1,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    });
    let wait = |app: &mut Application<_>| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.image_loading {
            let event = events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("image completion");
            if matches!(event, AppEvent::ImagesReady) {
                app.finish_image_load();
            }
        }
    };
    for reading in [false, true] {
        app.reading_mode = reading;
        app.reading_settings.first_page_count = 1;
        app.tabs
            .active_mut()
            .expect("tab")
            .target
            .set_current_path(paths[0].clone(), MediaKind::Image);
        app.load_path(paths[0].clone(), MediaKind::Image);
        wait(&mut app);
        let settings = app.reading_settings;
        for (key, index) in [
            ("Ctrl+5", 5),
            ("Ctrl+0", 11),
            ("Ctrl+0", 11),
            ("Ctrl+Shift+3", 8),
            ("Ctrl+Backspace", 3),
            ("Ctrl+Shift+0", 0),
            ("Ctrl+Shift+0", 0),
        ] {
            let unchanged = app.path.as_ref() == Some(&paths[index]);
            let generation = app.image_generation;
            app.process_shortcut(key.parse().expect("jump key"));
            wait(&mut app);
            assert_eq!(
                app.path.as_ref(),
                Some(&paths[index]),
                "{key}, reading={reading}"
            );
            assert_eq!(app.tabs.active().expect("tab").id, tab);
            assert_eq!(
                app.tabs.active().expect("tab").target.current_path(),
                &paths[index]
            );
            assert_eq!(app.reading_settings, settings);
            assert_eq!(
                app.image.as_ref().expect("loaded image").decoded.frames[0].rgba,
                [90, 30, index as u8, 255]
            );
            assert!(app.pending_guard.is_none());
            assert!(app.edits.values().all(|history| !history.is_dirty()));
            if unchanged {
                assert_eq!(app.image_generation, generation, "endpoint does not reload");
            }
            if reading {
                assert_eq!(
                    app.reading_pages.len() + 1,
                    settings.spread(index, paths.len()).len()
                );
                assert!(app.reading_pages.iter().all(Result::is_ok));
            }
        }
    }
}

#[test]
fn named_navigation_keys_and_shifted_number_row_preserve_custom_symbols_and_context() {
    let mut app = Application::new(None, |_| {}).expect("app");
    app.media_kind = Some(MediaKind::Image);
    app.shortcuts = shortcuts::defaults();
    for (named, key) in [
        (NamedKey::PageUp, Key::PageUp),
        (NamedKey::PageDown, Key::PageDown),
        (NamedKey::Backspace, Key::Backspace),
    ] {
        assert_eq!(
            app.key_stroke_for(
                &WinitKey::Named(named),
                PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified)
            )
            .expect("key")
            .key,
            key
        );
    }
    app.reading_mode = true;
    let settings = app.reading_settings;
    app.process_shortcut("L".parse().expect("reading axis"));
    assert_ne!(app.reading_settings.axis, settings.axis);
    app.process_shortcut("R".parse().expect("reading axis"));
    assert_eq!(app.reading_settings, settings);
    app.process_shortcut("V".parse().expect("reading order"));
    assert_ne!(app.reading_settings.reversed, settings.reversed);
    app.process_shortcut("H".parse().expect("reading order"));
    assert_eq!(app.reading_settings, settings);
    assert!(app.edits.is_empty());
    app.reading_mode = false;
    app.modifiers = ModifiersState::CONTROL | ModifiersState::SHIFT;
    use winit::keyboard::KeyCode::*;
    for (code, symbol, digit) in [
        (Digit1, "!", 1),
        (Digit2, "@", 2),
        (Digit3, "#", 3),
        (Digit4, "$", 4),
        (Digit5, "%", 5),
        (Digit6, "^", 6),
        (Digit7, "&", 7),
        (Digit8, "*", 8),
        (Digit9, "(", 9),
        (Digit0, ")", 0),
    ] {
        assert_eq!(
            app.key_stroke_for(&WinitKey::Character(symbol.into()), PhysicalKey::Code(code))
                .expect("shifted number")
                .to_string(),
            format!("Ctrl+Shift+{digit}")
        );
    }
    let physical = PhysicalKey::Code(winit::keyboard::KeyCode::Digit1);
    let logical = WinitKey::Character("!".into());
    assert_eq!(
        app.key_stroke_for(&logical, physical)
            .expect("number")
            .to_string(),
        "Ctrl+Shift+1"
    );
    app.shortcuts.set(
        CommandId::ToggleFilmstrip,
        "Ctrl+Shift+! F".parse().expect("custom symbol prefix"),
    );
    assert_eq!(
        app.key_stroke_for(&logical, physical)
            .expect("custom")
            .to_string(),
        "Ctrl+Shift+!"
    );
    app.shortcuts = shortcuts::defaults();
    app.media_kind = Some(MediaKind::Video);
    assert_eq!(
        app.key_stroke_for(&logical, physical)
            .expect("video")
            .to_string(),
        "Ctrl+Shift+!"
    );
    for text in [
        "PageUp",
        "PgUp",
        "PageDown",
        "PgDn",
        "Backspace",
        "Ctrl+Backspace",
    ] {
        let parsed = text.parse::<towavue_core::KeySequence>().expect("parse");
        assert_eq!(
            parsed
                .to_string()
                .parse::<towavue_core::KeySequence>()
                .expect("round trip"),
            parsed
        );
    }
}
