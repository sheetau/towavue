use super::*;
use towavue_core::{FolderMediaItem, FolderSnapshotSource, ShellIdentity};

fn snapshot(root: &Path, paths: &[PathBuf]) -> FolderSnapshot {
    FolderSnapshot {
        folder_identity: ShellIdentity::new(vec![]),
        folder_path: root.to_owned(),
        items: paths
            .iter()
            .map(|path| FolderMediaItem {
                identity: ShellIdentity::new(vec![]),
                path: path.clone(),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: vec![],
        source: FolderSnapshotSource::PersistedShellView,
        generation: 1,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    }
}

fn access(node: egui::accesskit::NodeId, action: egui::accesskit::Action) -> egui::Event {
    egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: node,
        data: None,
    })
}

#[test]
fn playlist_menu_routes_the_listed_file_without_switching_tracks() {
    let Some(root) = crate::tests::isolated_test_root(
        "thumbnail_menu::tests::playlist_menu_routes_the_listed_file_without_switching_tracks",
    ) else {
        return;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = Application::new(None, move |event| {
        if matches!(event, AppEvent::FileOperationSource(..)) {
            tx.send(event).expect("receiver");
        }
    })
    .expect("app");
    let current = root.join("current.wav");
    let target = root.join("target.wav");
    std::fs::write(&current, b"current owned audio source").expect("fixture");
    std::fs::write(&target, b"target owned audio source").expect("fixture");
    let tab = app.tabs.open_new(current.clone(), MediaKind::Audio);
    app.path = Some(current.clone());
    app.media_kind = Some(MediaKind::Audio);
    app.state = PlaybackState::Playing;
    let mut listing = snapshot(&root, &[current.clone(), target.clone()]);
    for item in &mut listing.items {
        item.kind = MediaKind::Audio;
    }
    app.folder_snapshot = Some(listing);
    let owner = Owner {
        tab,
        instance: app.media_generation,
    };
    let intent = Intent {
        owner: Some(owner),
        scope: Scope::Playlist,
        path: target.clone(),
        action: Action::Copy,
    };
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    let copied = context.run_ui(Default::default(), |_| {
        app.handle_thumbnail_menu(intent.clone())
    });
    assert!(copied.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == &target.display().to_string())));
    for action in [
        Action::Open,
        Action::Window,
        Action::Tab,
        Action::RemoveHistory,
    ] {
        app.handle_thumbnail_menu(Intent {
            action,
            ..intent.clone()
        });
        assert_eq!(app.tabs.active_id(), Some(tab));
        assert_eq!(app.tabs.tabs().len(), 1);
        assert_eq!(app.path.as_ref(), Some(&current));
    }
    for kind in [
        file_operations::Kind::Rename,
        file_operations::Kind::Move,
        file_operations::Kind::Delete,
    ] {
        app.handle_thumbnail_menu(Intent {
            action: Action::File(kind),
            ..intent.clone()
        });
        let pending = app
            .file_operations
            .pending
            .as_ref()
            .expect("shared native preparation");
        assert_eq!(pending.path, target);
        assert_eq!(pending.origin_path, current);
        assert_eq!(pending.tab, tab);
        assert_eq!(pending.kind, kind);
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5))
                .expect("preparation complete"),
            AppEvent::FileOperationSource(_, Ok(_))
        ));
        app.file_operations.pending = None;
        assert_eq!(app.state, PlaybackState::Playing);
        assert_eq!(app.tabs.active_id(), Some(tab));
        assert_eq!(app.path.as_ref(), Some(&current));
    }
    for invalid in 0..4 {
        let mut candidate = intent.clone();
        candidate.action = Action::File(file_operations::Kind::Delete);
        match invalid {
            0 => {
                candidate.owner = Some(Owner {
                    instance: owner.instance + 1,
                    ..owner
                })
            }
            1 => candidate.path = root.join("unlisted.wav"),
            2 => app.filmstrip_open = true,
            _ => {
                app.filmstrip_open = false;
                app.folder_snapshot = None;
            }
        }
        app.handle_thumbnail_menu(candidate);
        assert!(app.file_operations.pending.is_none());
    }
    assert!(
        current.is_file() && target.is_file(),
        "preparation alone never mutates files"
    );
}

#[test]
fn thumbnail_context_menus_keep_exact_target_and_expose_only_their_scope_actions() {
    let Some(root) = crate::tests::isolated_test_root(
        "thumbnail_menu::tests::thumbnail_context_menus_keep_exact_target_and_expose_only_their_scope_actions",
    ) else {
        return;
    };
    let paths = [root.join("current.png"), root.join("target.png")];
    let mut tabs = TabSet::default();
    let owner = Owner {
        tab: tabs.open_new(paths[0].clone(), MediaKind::Image),
        instance: 7,
    };
    for scope in [Scope::Gallery, Scope::Filmstrip, Scope::Playlist] {
        let paths = if scope == Scope::Playlist {
            [root.join("current.wav"), root.join("target.wav")]
        } else {
            paths.clone()
        };
        let mut listing = snapshot(&root, &paths);
        if scope == Scope::Playlist {
            for item in &mut listing.items {
                item.kind = MediaKind::Audio;
            }
        }
        let context = fonts::test_context();
        context.enable_accesskit();
        context.global_style_mut(chrome::style);
        let mut strip = filmstrip::Filmstrip::new(
            towavue_runtime_windows::PreviewCache::new(root.join(format!("{scope:?}")))
                .expect("cache"),
            || {},
        )
        .expect("strip");
        let mut playlist = playlist::Playlist::default();
        let mut frame = |events, active_owner| {
            strip.menu_owner = Some(active_owner);
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(660.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| match scope {
                    Scope::Gallery => {
                        strip.show_recent(ui, &paths, 1, true, &mut actions);
                    }
                    Scope::Playlist => {
                        assert!(
                            playlist
                                .show_with_menu(
                                    ui,
                                    Some(&listing),
                                    Some(&paths[0]),
                                    true,
                                    (Some(active_owner), &mut actions)
                                )
                                .is_none(),
                            "context menu must not activate a row"
                        );
                    }
                    Scope::Filmstrip => strip.show(
                        ui.ctx(),
                        ui.max_rect(),
                        Some(&listing),
                        Some(&paths[0]),
                        true,
                        &mut actions,
                    ),
                },
            );
            (
                actions,
                output.platform_output.accesskit_update.expect("tree"),
            )
        };
        for _ in 0..3 {
            frame(vec![], owner);
        }
        let mut choices = vec![
            ("Open", Action::Open),
            ("Open in new window", Action::Window),
            ("Open in new tab", Action::Tab),
            ("Copy file path", Action::Copy),
            ("Reveal in File Explorer", Action::Reveal),
        ];
        if scope == Scope::Playlist {
            choices.drain(..3);
        }
        if scope == Scope::Gallery {
            choices.push(("Remove from history", Action::RemoveHistory));
        } else {
            choices.extend([
                ("Rename file…", Action::File(file_operations::Kind::Rename)),
                ("Move file…", Action::File(file_operations::Kind::Move)),
                ("Delete file…", Action::File(file_operations::Kind::Delete)),
            ]);
        }
        for (index, (label, expected)) in choices.into_iter().enumerate() {
            let (_, tree) = frame(vec![], owner);
            let (target, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.label()
                        == Some(if scope == Scope::Playlist {
                            "2. target.wav"
                        } else {
                            "target.png"
                        })
                })
                .expect("target thumbnail");
            let target = *target;
            if index == 0 {
                let b = node.bounds().expect("thumbnail bounds");
                let pos = egui::pos2(((b.x0 + b.x1) * 0.5) as f32, ((b.y0 + b.y1) * 0.5) as f32);
                for pressed in [true, false] {
                    assert!(
                        frame(
                            vec![
                                egui::Event::PointerMoved(pos),
                                egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Secondary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE
                                }
                            ],
                            owner
                        )
                        .0
                        .is_empty()
                    );
                }
            } else {
                assert!(
                    frame(
                        vec![access(target, egui::accesskit::Action::ShowContextMenu)],
                        owner
                    )
                    .0
                    .is_empty()
                );
            }
            let (_, tree) = frame(vec![], owner);
            assert_eq!(
                tree.nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some("Remove from history")),
                scope == Scope::Gallery
            );
            assert_eq!(
                tree.nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some("Delete file…")),
                scope != Scope::Gallery
            );
            assert_eq!(
                tree.nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some("Open")),
                scope != Scope::Playlist
            );
            let action = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .expect("menu action")
                .0;
            let chosen = frame(vec![access(action, egui::accesskit::Action::Click)], owner).0;
            assert!(
                chosen
                    == [UiAction::ThumbnailMenu(Intent {
                        owner: Some(owner),
                        scope,
                        path: paths[1].clone(),
                        action: expected
                    })],
                "one exact target action: {scope:?} {label}"
            );
            assert!(!egui::Popup::is_any_open(&context));
        }
        let (_, tree) = frame(vec![], owner);
        let target = tree
            .nodes
            .iter()
            .find(|(_, n)| {
                n.label()
                    == Some(if scope == Scope::Playlist {
                        "2. target.wav"
                    } else {
                        "target.png"
                    })
            })
            .expect("target")
            .0;
        frame(vec![access(target, egui::accesskit::Action::Focus)], owner);
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        frame(vec![key(egui::Key::F10, egui::Modifiers::SHIFT)], owner);
        assert!(egui::Popup::is_any_open(&context));
        let (_, initial_menu) = frame(vec![], owner);
        assert!(
            initial_menu
                .nodes
                .iter()
                .any(|(id, node)| *id == initial_menu.focus
                    && node.label()
                        == Some(if scope == Scope::Playlist {
                            "Copy file path"
                        } else {
                            "Open"
                        })),
            "keyboard menu initially selects Open"
        );
        frame(
            vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
            owner,
        );
        let (_, menu_tree) = frame(vec![], owner);
        assert!(
            menu_tree
                .nodes
                .iter()
                .any(|(id, node)| *id == menu_tree.focus
                    && node.label()
                        == Some(if scope == Scope::Playlist {
                            "Reveal in File Explorer"
                        } else {
                            "Open in new window"
                        })),
            "ArrowDown advances exactly one menu action"
        );
        frame(vec![key(egui::Key::Escape, egui::Modifiers::NONE)], owner);
        let (_, restored) = frame(vec![], owner);
        assert!(!egui::Popup::is_any_open(&context));
        assert_eq!(restored.focus, target, "Escape restores keyboard origin");
        frame(
            vec![access(target, egui::accesskit::Action::ShowContextMenu)],
            owner,
        );
        assert!(egui::Popup::is_any_open(&context));
        assert!(
            frame(
                vec![],
                Owner {
                    instance: 8,
                    ..owner
                }
            )
            .0
            .is_empty()
        );
        assert!(
            !egui::Popup::is_any_open(&context),
            "changed owner cancels its menu"
        );
    }
}

#[test]
fn thumbnail_menu_dispatch_preserves_gallery_and_rejects_stale_or_hidden_targets() {
    let Some(root) = crate::tests::isolated_test_root(
        "thumbnail_menu::tests::thumbnail_menu_dispatch_preserves_gallery_and_rejects_stale_or_hidden_targets",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let path = root.join("target.png");
    std::fs::write(&path, b"owned menu dispatch fixture").expect("owned file");
    let gallery = app.tabs.gallery().expect("gallery");
    app.tabs.activate(gallery);
    app.recent_paths = vec![path.clone()];
    let owner = Owner {
        tab: gallery,
        instance: app.media_generation,
    };
    let intent = Intent {
        owner: Some(owner),
        scope: Scope::Gallery,
        path: path.clone(),
        action: Action::Tab,
    };
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    let copied = context.run_ui(Default::default(), |_| {
        app.handle_thumbnail_menu(Intent {
            action: Action::Copy,
            ..intent.clone()
        });
    });
    assert!(
        copied
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command,
        egui::OutputCommand::CopyText(text) if text == &path.display().to_string()))
    );
    app.handle_thumbnail_menu(Intent {
        action: Action::Window,
        ..intent.clone()
    });
    assert_eq!(
        app.pending_window_launches.as_slice(),
        std::slice::from_ref(&path)
    );
    app.pending_window_launches.clear();
    app.handle_thumbnail_menu(intent.clone());
    assert_eq!(app.tabs.active_id(), Some(gallery));
    assert_eq!(
        app.tabs.tabs().len(),
        1,
        "one registered background media tab"
    );
    assert!(app.path.is_none() && !app.image_loading);
    let count = app.tabs.tabs().len();
    app.handle_thumbnail_menu(Intent {
        owner: Some(Owner {
            instance: owner.instance + 1,
            ..owner
        }),
        ..intent.clone()
    });
    app.gallery_search = "not-a-match".into();
    app.handle_thumbnail_menu(intent.clone());
    assert_eq!(app.tabs.tabs().len(), count);
    app.gallery_search.clear();
    app.handle_thumbnail_menu(Intent {
        action: Action::RemoveHistory,
        ..intent.clone()
    });
    assert!(!app.recent_paths.contains(&path));
    assert_eq!(
        app.tabs.tabs().len(),
        count,
        "history removal does not close registered media"
    );
    assert!(path.is_file(), "removing history never deletes the file");
    app.recent_paths.push(path.clone());
    let background = app.tabs.tabs()[0].id;
    app.handle_thumbnail_menu(Intent {
        action: Action::Open,
        ..intent
    });
    assert!(app.tabs.gallery().is_none());
    assert_eq!(app.path.as_ref(), Some(&path));
    assert_ne!(
        app.tabs.active_id(),
        Some(background),
        "Open creates a fresh Gallery replacement"
    );
    assert_eq!(app.tabs.tabs().len(), 2);
}
