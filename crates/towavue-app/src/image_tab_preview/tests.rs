use super::*;

type App = Application<fn(AppEvent)>;

fn fixture(root: &Path) -> (App, egui::Context, TabId, Vec<PathBuf>) {
    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
    let context = fonts::test_context();
    context.global_style_mut(chrome::style);
    context.enable_accesskit();
    app.ui_context = Some(context.clone());
    let paths: Vec<_> = ["z.bmp", "a.bmp", "m.bmp", "b.bmp"]
        .map(|name| root.join(name))
        .into();
    for (index, path) in paths.iter().enumerate() {
        write_bitmap(path, 2, 2, [10, 20, 40 + index as u8 * 30, 255]);
    }
    let id = app.tabs.open_new(paths[0].clone(), MediaKind::Image);
    app.path = Some(paths[0].clone());
    app.displayed_tab = Some(id);
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    app.media_generation = 7;
    app.media_sequence = 7;
    let decoded = Arc::new(towavue_runtime_windows::decode_image(&paths[0]).expect("decode"));
    app.image =
        Some(ImagePresentation::from_decoded(&context, &paths[0], decoded).expect("pixels"));
    let mut items: Vec<_> = paths
        .iter()
        .map(|path| towavue_core::FolderMediaItem {
            identity: towavue_core::ShellIdentity::new(vec![]),
            path: path.clone(),
            kind: MediaKind::Image,
        })
        .collect();
    items.insert(
        1,
        towavue_core::FolderMediaItem {
            identity: towavue_core::ShellIdentity::new(vec![]),
            path: root.join("excluded.wav"),
            kind: MediaKind::Audio,
        },
    );
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![]),
        folder_path: root.to_owned(),
        items,
        sort_columns: vec![],
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 11,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    });
    (app, context, id, paths)
}

fn write_bitmap(path: &Path, width: i32, height: i32, bgra: [u8; 4]) {
    let pixels = (width * height) as usize;
    let length = 54 + pixels * 4;
    let mut bytes = vec![0_u8; length];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&(length as u32).to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&width.to_le_bytes());
    bytes[22..26].copy_from_slice(&height.to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&32_u16.to_le_bytes());
    bytes[54..].copy_from_slice(&bgra.repeat(pixels));
    std::fs::write(path, bytes).expect("BMP");
}

fn wait_image(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.image_loading {
        app.finish_image_load();
        assert!(Instant::now() < deadline, "original image completion");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(app.image_error.is_none(), "{:?}", app.image_error);
}

#[test]
fn shell_reordering_keeps_first_spread_loading_navigation_and_background_cards_consistent() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_tab_preview::tests::shell_reordering_keeps_first_spread_loading_navigation_and_background_cards_consistent",
    ) else {
        return;
    };
    for reversed in [false, true] {
        let (mut app, context, id, paths) = fixture(&root);
        for (index, item) in app
            .folder_snapshot
            .as_mut()
            .expect("snapshot")
            .items
            .iter_mut()
            .enumerate()
        {
            item.identity = towavue_core::ShellIdentity::new(vec![index as u8 + 1]);
        }
        let mut reordered = app.folder_snapshot.clone().expect("snapshot");
        reordered.items.reverse();
        let shell_items = reordered.items.clone();
        app.reading_mode = true;
        app.reading_settings.first_page_count = 1;
        app.reading_settings.reversed = reversed;
        let history = app.edits.clone();
        app.apply_folder_snapshot(reordered);
        wait_image(&mut app);
        assert_eq!(app.reading_settings.reversed, reversed);
        assert_eq!(app.path.as_ref(), Some(&paths[0]));
        assert_eq!(app.edits, history);
        assert_eq!(
            app.preview_folder(id, &paths[0]).expect("position").index,
            3
        );
        assert_eq!(
            app.preview_image_path(id, &paths[0], 0),
            Some(paths[3].clone())
        );

        app.dispatch(CommandId::FirstImage);
        wait_image(&mut app);
        assert_eq!(app.path.as_ref(), Some(&paths[3]));
        assert!(
            app.reading_pages.is_empty(),
            "first-spread count starts at the new Shell head"
        );
        app.dispatch(if reversed {
            CommandId::ReadingLeft
        } else {
            CommandId::ReadingRight
        });
        wait_image(&mut app);
        assert_eq!(app.path.as_ref(), Some(&paths[2]));
        assert_eq!(
            app.reading_request_paths(),
            [paths[2].clone(), paths[1].clone()]
        );
        assert_eq!(app.image_prefetch_paths(), Some(vec![paths[0].clone()]));
        assert_eq!(app.reading_pages.len(), 1);
        assert_eq!(
            app.reading_pages[0]
                .as_ref()
                .expect("second page")
                .decoded
                .frames[0]
                .rgba[0],
            70
        );
        assert!(app.status_details().contains(&"2 / 4".to_owned()));
        let first = app.image.as_ref().expect("first page").texture.id();
        let second = app.reading_pages[0]
            .as_ref()
            .expect("second page")
            .texture
            .id();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 500.0),
                )),
                ..Default::default()
            },
            |ui| app.draw_reading_pages(ui),
        );
        let center = |id| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == id => {
                        Some(mesh.calc_bounds().center().x)
                    }
                    _ => None,
                })
                .expect("page pixels drawn")
        };
        assert_eq!(center(first) > center(second), reversed);

        let foreground = app
            .tabs
            .open_new(root.join("foreground.bmp"), MediaKind::Image);
        app.retain_image_tab();
        app.displayed_tab = Some(foreground);
        app.path = Some(root.join("foreground.bmp"));
        let active = (app.path.clone(), app.media_generation, app.image_generation);
        let position = app
            .preview_folder(id, &paths[2])
            .expect("background position");
        assert_eq!(position.index, 1);
        assert_eq!(
            position.reading_paths(),
            Some(vec![paths[2].clone(), paths[1].clone()])
        );
        let target = app
            .preview_image_path(id, &paths[2], 0)
            .expect("reversed first");
        assert_eq!(target, paths[3]);
        app.handle_preview_image_seek(id, position.instance, paths[2].clone(), target);
        assert_eq!(
            (app.path.clone(), app.media_generation, app.image_generation),
            active
        );
        assert_eq!(app.tabs.active_id(), Some(foreground));
        app.activate_tab(id);
        wait_image(&mut app);
        assert_eq!(app.path.as_ref(), Some(&paths[3]));
        assert!(app.reading_pages.is_empty());
        assert_eq!(
            app.folder_snapshot.as_ref().expect("snapshot").items,
            shell_items
        );

        app.dispatch(CommandId::ToggleReadingMode);
        assert!(!app.reading_mode);
        let position = app
            .preview_folder(id, &paths[3])
            .expect("ordinary position");
        assert_eq!(position.index, 0, "leaving reading preserves Shell order");
        assert_eq!(
            app.preview_image_path(id, &paths[3], 0),
            Some(paths[3].clone())
        );
        app.dispatch(CommandId::FirstImage);
        wait_image(&mut app);
        assert_eq!(app.path.as_ref(), Some(&paths[3]));
    }
}

#[test]
fn folder_card_navigation_preserves_background_owner_and_reading_activation() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_tab_preview::tests::folder_card_navigation_preserves_background_owner_and_reading_activation",
    ) else {
        return;
    };
    for reading in [false, true] {
        let (mut app, context, id, paths) = fixture(&root);
        app.reading_mode = reading;
        app.reading_settings.page_count = 2;
        app.reading_settings.first_page_count = 2;
        app.reading_settings.reversed = true;
        app.reading_settings.axis = towavue_core::ReadingAxis::Vertical;
        let settings = app.reading_settings;
        let position = app.preview_folder(id, &paths[0]).expect("folder");
        assert_eq!(position.count, paths.len());
        for (index, path) in paths.iter().enumerate() {
            assert_eq!(
                app.preview_image_path(id, &paths[0], index).as_ref(),
                Some(path),
                "Shell order"
            );
        }
        let foreground = app
            .tabs
            .open_new(root.join("foreground.bmp"), MediaKind::Image);
        app.retain_image_tab();
        app.displayed_tab = Some(foreground);
        app.path = Some(root.join("foreground.bmp"));
        app.media_generation = 8;
        app.media_sequence = 8;
        app.image_generation = 123;
        app.image_view.pan = (11.0, -9.0);
        app.prefix_started = Some(Instant::now());
        let active = (
            app.path.clone(),
            app.media_generation,
            app.image_generation,
            app.image_view,
            app.prefix_started,
        );
        app.handle_preview_image_seek(id, position.instance, paths[0].clone(), paths[2].clone());
        assert_eq!(app.tabs.active_id(), Some(foreground));
        assert_eq!(
            (
                app.path.clone(),
                app.media_generation,
                app.image_generation,
                app.image_view,
                app.prefix_started
            ),
            active
        );
        let saved = &app.retained_images[&id];
        assert_eq!(saved.path, paths[2]);
        assert!(saved.resume_loading && saved.image.is_none());
        assert_eq!(saved.reading_mode, reading);
        assert_eq!(saved.reading_settings, settings);
        let new_instance = saved.instance;
        assert_ne!(new_instance, position.instance);
        let new_position = app.preview_folder(id, &paths[2]).expect("new folder");
        assert_eq!(new_position.index, 2);
        if reading {
            assert_eq!(new_position.reading_paths(), Some(paths[2..].to_vec()));
            let pages = new_position.reading_paths().expect("spread");
            app.filmstrip.image_tab_preview(&pages, settings);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                app.filmstrip.finish(&context);
                let preview = app.filmstrip.image_tab_preview(&pages, settings);
                let tab_preview::RetainedPreview::Reading {
                    pages,
                    settings: actual,
                } = preview
                else {
                    panic!("reading preview");
                };
                assert_eq!(actual, settings);
                if pages.iter().all(|page| page.0.is_some()) {
                    break;
                }
                assert!(Instant::now() < deadline, "shared reading previews");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        // Delayed A-to-B results and actions cannot replace the new owner.
        app.handle_preview_image_seek(id, position.instance, paths[0].clone(), paths[1].clone());
        assert_eq!(app.retained_images[&id].path, paths[2]);
        app.activate_tab(id);
        wait_image(&mut app);
        assert_eq!(app.media_generation, new_instance);
        assert_eq!(app.reading_mode, reading);
        assert_eq!(app.reading_settings, settings);
        assert_eq!(
            app.image.as_ref().expect("new original").decoded.frames[0].rgba[0],
            100
        );
        assert_eq!(app.reading_pages.len(), usize::from(reading));
        assert!(app.edits[&id].operations().is_empty());
        // The active path uses normal original loading and retains its old pixels.
        let instance = app.media_generation;
        app.prefix_started = None;
        app.handle_preview_image_seek(id, instance, paths[2].clone(), paths[1].clone());
        assert_eq!(app.path.as_ref(), Some(&paths[1]));
        assert!(app.image_handoff.is_some());
        wait_image(&mut app);
        assert_eq!(
            app.image.as_ref().expect("active original").decoded.frames[0].rgba[0],
            70
        );
        assert_eq!(app.reading_mode, reading);
    }
}

#[test]
fn folder_card_guards_cancel_stale_paths_and_preserve_dirty_sources() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_tab_preview::tests::folder_card_guards_cancel_stale_paths_and_preserve_dirty_sources",
    ) else {
        return;
    };
    let (mut app, _, id, paths) = fixture(&root);
    let instance = app.media_generation;
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    let history = app.edits[&id].clone();
    for blocked in [0, 1, 3, 4] {
        app.palette_open = blocked == 0;
        app.grid_open = blocked == 1;
        app.incoming_tab_pointer = (blocked == 3).then_some(egui::Pos2::ZERO);
        let token = if blocked == 4 { instance + 1 } else { instance };
        app.handle_preview_image_seek(id, token, paths[0].clone(), paths[1].clone());
        assert!(app.pending_guard.is_none());
        assert_eq!(app.path.as_ref(), Some(&paths[0]));
    }
    app.incoming_tab_pointer = None;
    for path in [
        paths[0].clone(),
        root.join("excluded.wav"),
        root.join("not-in-folder.bmp"),
    ] {
        app.handle_preview_image_seek(id, instance, paths[0].clone(), path);
        assert!(app.pending_guard.is_none());
    }
    app.filmstrip_open = true;
    assert!(
        !app.preview_input_blocked(),
        "filmstrip permits card controls"
    );
    app.handle_preview_image_seek(id, instance, paths[0].clone(), paths[1].clone());
    assert!(matches!(
        app.pending_guard,
        Some(GuardedAction::NavigateImageTab(..))
    ));
    app.resolve_guard(GuardDecision::Cancel);
    assert_eq!(app.edits[&id], history);
    assert_eq!(app.path.as_ref(), Some(&paths[0]));
    app.handle_preview_image_seek(id, instance, paths[0].clone(), paths[1].clone());
    app.folder_snapshot
        .as_mut()
        .expect("snapshot")
        .items
        .retain(|item| item.path != paths[1]);
    app.resolve_guard(GuardDecision::Discard);
    assert_eq!(
        app.path.as_ref(),
        Some(&paths[0]),
        "revalidate after confirmation"
    );
    assert_eq!(app.edits[&id], history);
}

#[test]
fn folder_card_pointer_numeric_and_cancel_paths_use_image_indices() {
    for (density, reading) in [1.0, 1.25, 2.0].into_iter().flat_map(|density| {
        [
            None,
            Some(ReadingSettings::default()),
            Some(ReadingSettings {
                reversed: true,
                ..ReadingSettings::default()
            }),
        ]
        .map(move |reading| (density, reading))
    }) {
        let reversed = reading.is_some_and(|settings| settings.reversed);
        let context = fonts::test_context();
        context.set_pixels_per_point(density);
        context.enable_accesskit();
        let thumbnail = egui::Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(240.0, 80.0));
        let folder = FolderPosition {
            instance: 1,
            count: 5,
            pages: None,
            index: 1,
            revision: 2,
            reading,
        };
        let frame = |events, instance, revision, count, enabled, focused| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    events,
                    focused,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 400.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        let position = FolderPosition {
                            instance,
                            revision,
                            count,
                            pages: None,
                            ..folder
                        };
                        actions.extend(position.show(ui, thumbnail));
                        if context.current_pass_index() == 0 {
                            context.request_discard("seek action once");
                        }
                    });
                },
            );
            (output, actions)
        };
        frame(vec![], 1, 2, 5, true, true);
        let (output, _) = frame(vec![], 1, 2, 5, true, true);
        let fill = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.fill == chrome::FOREGROUND => Some(rect.rect),
                _ => None,
            })
            .expect("folder progress fill");
        let expected_left = thumbnail.left()
            + if reversed {
                thumbnail.width() * 0.75
            } else {
                0.0
            };
        assert!((fill.left() - expected_left).abs() < 0.01);
        assert!((fill.width() - thumbnail.width() * 0.25).abs() < 0.01);
        let tree = output.platform_output.accesskit_update.expect("tree");
        let (node_id, node) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Preview image position"))
            .expect("image position");
        assert_eq!(node.numeric_value(), Some(2.0));
        assert_eq!(node.min_numeric_value(), Some(1.0));
        assert_eq!(node.max_numeric_value(), Some(5.0));
        let bounds = node.bounds().expect("bounds");
        let rect = egui::Rect::from_min_max(
            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
        );
        let point = rect.center();
        let button = |pos, pressed, button| egui::Event::PointerButton {
            pos,
            pressed,
            button,
            modifiers: egui::Modifiers::NONE,
        };
        for button_kind in [
            egui::PointerButton::Secondary,
            egui::PointerButton::Middle,
            egui::PointerButton::Primary,
        ] {
            frame(vec![egui::Event::PointerMoved(point)], 1, 2, 5, true, true);
            assert!(
                frame(vec![button(point, true, button_kind)], 1, 2, 5, true, true)
                    .1
                    .is_empty()
            );
            let actions = frame(vec![button(point, false, button_kind)], 1, 2, 5, true, true).1;
            assert_eq!(
                actions,
                if button_kind == egui::PointerButton::Primary {
                    vec![preview_transport::Action::ImageSeek(2)]
                } else {
                    vec![]
                }
            );
        }
        for (instance, revision, count, enabled, focused, escape) in [
            (1, 2, 5, true, true, false),
            (2, 2, 5, true, true, false),
            (1, 3, 5, true, true, false),
            (1, 2, 1, true, true, false),
            (1, 2, 5, false, true, false),
            (1, 2, 5, true, false, false),
            (1, 2, 5, true, true, true),
        ] {
            frame(vec![egui::Event::PointerMoved(point)], 1, 2, 5, true, true);
            frame(
                vec![button(point, true, egui::PointerButton::Primary)],
                1,
                2,
                5,
                true,
                true,
            );
            let outside = rect.right_bottom() + egui::vec2(100.0, 100.0);
            let mut events = vec![egui::Event::PointerMoved(outside)];
            if escape {
                events.push(egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            if !focused {
                events.push(egui::Event::WindowFocused(false));
            }
            frame(events, instance, revision, count, enabled, focused);
            let actions = frame(
                vec![button(outside, false, egui::PointerButton::Primary)],
                instance,
                revision,
                count,
                enabled,
                focused,
            )
            .1;
            assert_eq!(
                actions,
                if instance == 1 && revision == 2 && count == 5 && enabled && focused && !escape {
                    vec![preview_transport::Action::ImageSeek(if reversed {
                        0
                    } else {
                        4
                    })]
                } else {
                    vec![]
                }
            );
            timeline_input::cancel(&context);
        }
        for (value, expected) in [
            (1.0, Some(0)),
            (2.0, None),
            (4.0, Some(3)),
            (100.0, Some(4)),
        ] {
            let event = egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: *node_id,
                data: Some(egui::accesskit::ActionData::NumericValue(value)),
            });
            assert_eq!(
                frame(vec![event], 1, 2, 5, true, true).1,
                expected
                    .map(preview_transport::Action::ImageSeek)
                    .into_iter()
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn image_tab_card_clicks_keep_card_open_for_animation_reading_and_background() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_tab_preview::tests::image_tab_card_clicks_keep_card_open_for_animation_reading_and_background",
    ) else {
        return;
    };
    for (density, grow) in [1.0, 1.25, 2.0]
        .into_iter()
        .flat_map(|density| [false, true].map(move |grow| (density, grow)))
    {
        for reading in [false, true] {
            for background in [false, true] {
                let (mut app, context, id, paths) = fixture(&root);
                context.set_pixels_per_point(density);
                let (source_size, destination_size) = if grow {
                    ((200, 20), (120, 160))
                } else {
                    ((120, 160), (200, 20))
                };
                write_bitmap(&paths[0], source_size.0, source_size.1, [10, 20, 40, 255]);
                write_bitmap(
                    &paths[3],
                    destination_size.0,
                    destination_size.1,
                    [10, 20, 130, 255],
                );
                let mut animated =
                    towavue_runtime_windows::decode_image(&paths[0]).expect("source");
                animated.frames.push(animated.frames[0].clone());
                for frame in &mut animated.frames {
                    frame.delay = Duration::from_millis(20);
                }
                app.image = Some(
                    ImagePresentation::from_decoded(&context, &paths[0], Arc::new(animated))
                        .expect("animation"),
                );
                app.reading_mode = reading;
                app.reading_settings.page_count = 2;
                app.reading_settings.first_page_count = 2;
                if reading {
                    app.reading_pages.push(Ok(ImagePresentation::from_decoded(
                        &context,
                        &paths[1],
                        Arc::new(towavue_runtime_windows::decode_image(&paths[1]).expect("page")),
                    )
                    .expect("texture")));
                }
                app.tabs.close_gallery(app.tabs.gallery().expect("Gallery"));
                if background {
                    app.tabs
                        .open_new(root.join("foreground.bmp"), MediaKind::Image);
                    app.retain_image_tab();
                    app.path = Some(root.join("foreground.bmp"));
                    app.displayed_tab = app.tabs.active_id();
                    app.media_generation = 8;
                    app.media_sequence = 8;
                }
                let foreground = app.tabs.active_id();
                let mut time = 0.0;
                let mut frame = |app: &mut App, events| {
                    time += 0.05;
                    let mut actions = Vec::new();
                    let output = context.run_ui(
                        egui::RawInput {
                            time: Some(time),
                            focused: true,
                            events,
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(960.0, 576.0),
                            )),
                            ..Default::default()
                        },
                        |ui| app.draw_top_bar(ui, &mut actions),
                    );
                    let mut accepted = Vec::new();
                    for action in actions {
                        if !accepted.contains(&action) {
                            app.handle_ui_action(action.clone());
                            accepted.push(action);
                        }
                    }
                    (output, accepted)
                };
                for _ in 0..5 {
                    frame(&mut app, vec![]);
                }
                let hover = egui::pos2(100.0, 15.0);
                for _ in 0..3 {
                    frame(&mut app, vec![egui::Event::PointerMoved(hover)]);
                }
                let (output, _) = frame(&mut app, vec![]);
                let tree = output.platform_output.accesskit_update.expect("tree");
                if !background {
                    assert!(
                        !tree
                            .nodes
                            .iter()
                            .any(|(_, node)| node.label() == Some("Preview image position")),
                        "active tab has no image-seek card"
                    );
                    continue;
                }
                let node = &tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("Preview image position"))
                    .expect("folder seek in actual tab card")
                    .1;
                assert_eq!(node.numeric_value(), Some(1.0));
                assert_eq!(
                    node.max_numeric_value(),
                    Some(4.0),
                    "animation length is not folder position"
                );
                let bounds = node.bounds().expect("seek bounds");
                let point =
                    egui::pos2(bounds.x1 as f32 - 1.0, (bounds.y0 + bounds.y1) as f32 * 0.5);
                frame(&mut app, vec![egui::Event::PointerMoved(point)]);
                let button = |pressed| egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                };
                assert!(frame(&mut app, vec![button(true)]).1.is_empty());
                let (_, actions) = frame(&mut app, vec![button(false)]);
                assert!(
                    matches!(actions.as_slice(), [UiAction::PreviewImageSeek(tab, _, source, path)] if *tab == id && source == &paths[0] && path == &paths[3]),
                    "one image navigation action"
                );
                assert_eq!(app.tabs.active_id(), foreground);
                assert_eq!(
                    app.tabs
                        .tabs()
                        .iter()
                        .find(|tab| tab.id == id)
                        .expect("image tab")
                        .target
                        .current_path(),
                    paths[3]
                );
                let (output, actions) = frame(&mut app, vec![]);
                assert!(actions.is_empty());
                let tree = output.platform_output.accesskit_update.expect("tree");
                let seek = &tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some("Preview image position"))
                    .expect("card stays open after navigation")
                    .1;
                assert_eq!(
                    seek.bounds(),
                    Some(bounds),
                    "pending destination retains the operated seek position"
                );
                if !background {
                    wait_image(&mut app);
                    let decoded = &app
                        .image
                        .as_ref()
                        .expect("destination original")
                        .decoded
                        .frames[0];
                    assert_eq!(
                        (decoded.width, decoded.height),
                        (destination_size.0 as u32, destination_size.1 as u32)
                    );
                    for _ in 0..3 {
                        let (output, actions) = frame(&mut app, vec![]);
                        assert!(actions.is_empty());
                        let tree = output
                            .platform_output
                            .accesskit_update
                            .expect("loaded tree");
                        let seek = &tree
                            .nodes
                            .iter()
                            .find(|(_, node)| node.label() == Some("Preview image position"))
                            .expect("card stays open when the destination finishes loading")
                            .1;
                        assert_eq!(seek.numeric_value(), Some(4.0));
                        assert_eq!(
                            seek.bounds(),
                            Some(bounds),
                            "destination aspect ratio must not move the operated seek bar"
                        );
                    }
                    frame(
                        &mut app,
                        vec![egui::Event::PointerMoved(egui::pos2(5.0, 500.0))],
                    );
                    for _ in 0..3 {
                        frame(&mut app, vec![egui::Event::PointerMoved(hover)]);
                    }
                    let (output, _) = frame(&mut app, vec![]);
                    let tree = output
                        .platform_output
                        .accesskit_update
                        .expect("reopened tree");
                    let reopened = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| node.label() == Some("Preview image position"))
                        .expect("reopened seek")
                        .1
                        .bounds()
                        .expect("reopened bounds");
                    assert!(
                        (reopened.y0 - bounds.y0).abs() > 20.0,
                        "reopening restores natural destination geometry"
                    );
                }
            }
        }
    }
}

#[test]
fn image_card_export_and_background_dirty_guards_keep_source_ownership() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_tab_preview::tests::image_card_export_and_background_dirty_guards_keep_source_ownership",
    ) else {
        return;
    };
    for background in [false, true] {
        let (mut app, _, id, paths) = fixture(&root);
        let original: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(path).expect("source bytes"))
            .collect();
        let instance = app.media_generation;
        // Retain an unpublished Save as slot until its event is handled. The
        // source-version guard or dropping the candidate prevents publication.
        assert!(app.start_test_save_as(root.join("unpublished.png"), None));
        if background {
            app.tabs
                .open_new(root.join("foreground.bmp"), MediaKind::Image);
            app.retain_image_tab();
            app.path = Some(root.join("foreground.bmp"));
            app.displayed_tab = app.tabs.active_id();
            app.media_generation = 8;
            app.media_sequence = 8;
        }
        let foreground = app.tabs.active_id();
        app.handle_preview_image_seek(id, instance, paths[0].clone(), paths[1].clone());
        assert_eq!(app.tabs.active_id(), foreground);
        assert_eq!(
            app.tabs
                .tabs()
                .iter()
                .find(|tab| tab.id == id)
                .expect("tab")
                .target
                .current_path(),
            paths[0]
        );
        assert!(app.pending_guard.is_none());
        assert!(app.active_export.is_some());
        assert!(
            app.status_message
                .as_ref()
                .expect("busy notice")
                .0
                .contains("Export is in progress")
        );
        app.active_export.take();
        app.edits
            .entry(id)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let history = app.edits[&id].clone();
        app.handle_preview_image_seek(id, instance, paths[0].clone(), paths[1].clone());
        assert_eq!(
            app.tabs.active_id(),
            Some(id),
            "show the actual dirty owner for confirmation"
        );
        assert_eq!(app.path.as_ref(), Some(&paths[0]));
        assert!(matches!(
            app.pending_guard,
            Some(GuardedAction::NavigateImageTab(..))
        ));
        app.resolve_guard(GuardDecision::Cancel);
        assert_eq!(app.edits[&id], history);
        app.handle_preview_image_seek(id, instance, paths[0].clone(), paths[1].clone());
        app.resolve_guard(GuardDecision::Discard);
        assert_eq!(app.path.as_ref(), Some(&paths[1]));
        wait_image(&mut app);
        assert!(app.edits[&id].operations().is_empty());
        for (path, bytes) in paths.iter().zip(original) {
            assert_eq!(std::fs::read(path).expect("unchanged source"), bytes);
        }
    }
}

#[test]
fn filmstrip_command_overlays_reveal_edits_and_preserve_media_history() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_tab_preview::tests::filmstrip_command_overlays_reveal_edits_and_preserve_media_history",
    ) else {
        return;
    };
    let (mut app, _, id, paths) = fixture(&root);
    app.filmstrip_open = true;
    app.dispatch(CommandId::ToggleCommandPalette);
    assert!(app.palette_open && app.filmstrip_open);
    app.dispatch(CommandId::RotateClockwise);
    assert!(!app.palette_open && !app.filmstrip_open);
    assert_eq!(
        app.edits[&id].operations(),
        &[EditOperation::RotateClockwise]
    );
    app.filmstrip_open = true;
    app.dispatch(CommandId::Undo);
    assert!(!app.filmstrip_open);
    assert!(app.edits[&id].operations().is_empty());
    assert_eq!(app.path.as_ref(), Some(&paths[0]));
    assert_eq!(app.tabs.active_id(), Some(id));
}

#[test]
fn unopened_image_card_prepares_order_and_navigates_without_loading_or_activating() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_tab_preview::tests::unopened_image_card_prepares_order_and_navigates_without_loading_or_activating",
    ) else {
        return;
    };
    let (mut app, context, active, paths) = fixture(&root);
    let instance = app.media_generation;
    let loader = app.image_generation;
    let pixels = Arc::clone(&app.image.as_ref().expect("active pixels").decoded);
    let id = app.tabs.open_new(paths[1].clone(), MediaKind::Image);
    app.tabs.activate(active);
    assert!(app.preview_folder(id, &paths[1]).is_none());
    context.global_style_mut(|style| {
        style.interaction.tooltip_delay = 0.0;
        style.interaction.show_tooltips_only_when_still = false;
    });
    let frame = |app: &mut App, events| {
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 576.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let mut actions = Vec::new();
                app.draw_ui(ui, &mut actions);
                assert!(
                    actions.is_empty(),
                    "hover prepares metadata without a media action"
                );
            },
        )
    };
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    let output = frame(&mut app, vec![]);
    let point = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "a.bmp" => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("background tab label");
    for _ in 0..4 {
        frame(&mut app, vec![egui::Event::PointerMoved(point)]);
    }

    let position = app
        .preview_folder(id, &paths[1])
        .expect("prepared folder position");
    assert_eq!((position.index, position.count), (1, 4));
    assert!(app.retained_images[&id].image.is_none());
    assert!(app.retained_images[&id].resume_loading);
    app.handle_preview_image_seek(id, position.instance, paths[1].clone(), paths[3].clone());
    assert_eq!(
        app.tabs
            .get_mut(id)
            .expect("background tab")
            .target
            .current_path(),
        paths[3]
    );
    let changed = app.preview_folder(id, &paths[3]).expect("new card");
    assert_ne!(changed.instance, position.instance);
    assert_eq!(changed.index, 3);
    app.handle_preview_image_seek(id, position.instance, paths[1].clone(), paths[2].clone());
    assert_eq!(
        app.tabs
            .get_mut(id)
            .expect("background tab")
            .target
            .current_path(),
        paths[3]
    );
    assert_eq!(app.tabs.active_id(), Some(active));
    assert_eq!(app.media_generation, instance);
    assert_eq!(app.image_generation, loader);
    assert!(app.session.is_none());
    assert!(Arc::ptr_eq(
        &app.image.as_ref().expect("unchanged foreground").decoded,
        &pixels
    ));
    // Hovering must not freeze the defaults a fresh tab would inherit later.
    app.reading_mode = true;
    app.reading_settings.page_count = 2;
    app.activate_tab(id);
    wait_image(&mut app);
    assert!(app.reading_mode);
    assert_eq!(app.reading_settings.page_count, 2);
    assert_eq!(app.path.as_ref(), Some(&paths[3]));
    assert_eq!(
        app.image
            .as_ref()
            .expect("selected original")
            .decoded
            .frames[0]
            .rgba[0],
        130
    );
}

#[test]
fn unopened_image_folder_preparation_is_latest_only_and_rejects_closed_owners() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_tab_preview::tests::unopened_image_folder_preparation_is_latest_only_and_rejects_closed_owners",
    ) else {
        return;
    };
    let (mut app, _, active, _) = fixture(&root);
    let mut ids = Vec::new();
    let mut paths = Vec::new();
    for folder in ["first", "second"] {
        let directory = root.join(folder);
        std::fs::create_dir(&directory).expect("owned folder");
        let path = directory.join("a.bmp");
        write_bitmap(&path, 2, 2, [10, 20, 30, 255]);
        write_bitmap(&directory.join("b.bmp"), 2, 2, [40, 50, 60, 255]);
        let id = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.tabs.activate(active);
        app.prepare_image_tab(Some(id));
        ids.push(id);
        paths.push(path);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.preview_folder(ids[1], &paths[1]).is_none() {
        app.finish_image_tab_preparation();
        assert!(
            Instant::now() < deadline,
            "background Shell order completion"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        app.preview_folder(ids[1], &paths[1]).expect("folder").count,
        2
    );
    assert!(
        app.retained_images[&ids[0]].folder_snapshot.is_none(),
        "superseded completion is not applied"
    );
    assert_eq!(app.tabs.active_id(), Some(active));
    assert!(
        app.retained_images
            .values()
            .all(|saved| saved.image.is_none())
    );
    app.prepare_image_tab(Some(ids[0]));
    app.close_tab_unchecked(ids[0]);
    app.finish_image_tab_preparation();
    assert!(!app.retained_images.contains_key(&ids[0]));
    assert!(app.image_tab_preparation.pending.is_none());
    app.prepare_image_tab(None);
    assert!(app.image_tab_preparation.pending.is_none());
}

#[test]
fn reading_leading_focus_survives_regrouping_background_restore_and_shell_changes() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_tab_preview::tests::reading_leading_focus_survives_regrouping_background_restore_and_shell_changes",
    ) else {
        return;
    };

    for reversed in [false, true] {
        for axis in [ReadingAxis::Horizontal, ReadingAxis::Vertical] {
            for (initial_count, source_index, count) in [(2, 5, 5), (3, 10, 10)] {
                let (mut app, context, id, _) = fixture(&root);
                let paths: Vec<_> = (0..12)
                    .map(|i| root.join(format!("focus-{i}.bmp")))
                    .collect();
                for (i, path) in paths.iter().enumerate() {
                    write_bitmap(path, 2 + i as i32, 3, [10, 20, 40 + i as u8, 255]);
                }
                let snapshot = app.folder_snapshot.as_mut().expect("folder");
                snapshot.items = paths
                    .iter()
                    .enumerate()
                    .map(|(i, path)| towavue_core::FolderMediaItem {
                        path: path.clone(),
                        kind: MediaKind::Image,
                        identity: towavue_core::ShellIdentity::new(vec![i as u8 + 1]),
                    })
                    .collect();
                let source = paths[source_index].clone();
                app.path = Some(source.clone());
                app.tabs
                    .get_mut(id)
                    .expect("tab")
                    .target
                    .set_current_path(source.clone(), MediaKind::Image);
                app.image = None;
                app.reading_mode = false;
                app.set_reading_layout(
                    true,
                    ReadingSettings {
                        page_count: initial_count,
                        first_page_count: initial_count,
                        axis,
                        reversed,
                    },
                );
                wait_image(&mut app);
                let focus_index = source_index - 1;
                assert_eq!(app.reading_focus_path(), Some(&paths[focus_index]));
                let mut edits = EditHistory::default();
                assert!(edits.push(EditOperation::FlipHorizontal, MediaKind::Image));
                assert!(edits.undo());
                app.edits.insert(id, edits);
                let history = app.edits.clone();
                app.set_reading_layout(
                    true,
                    ReadingSettings {
                        page_count: count,
                        first_page_count: count,
                        ..app.reading_settings
                    },
                );
                wait_image(&mut app);
                assert_eq!(app.path.as_ref(), Some(&source));
                assert_eq!(app.edits, history);
                assert_eq!(app.reading_focus_path(), Some(&paths[focus_index]));
                assert_eq!(
                    app.reading_request_paths(),
                    std::iter::once(source.clone())
                        .chain(paths[..count].iter().cloned())
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    app.reading_pages.len(),
                    count,
                    "all visible originals load even with an extra retained source"
                );
                let source_texture = app.image.as_ref().expect("source retained").texture.id();
                let output = context.run_ui(Default::default(), |ui| app.draw_reading_pages(ui));
                let meshes: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) => Some(mesh),
                        _ => None,
                    })
                    .collect();
                assert_eq!(meshes.len(), count);
                assert!(meshes.iter().all(|mesh| mesh.texture_id != source_texture));
                assert_eq!(
                    app.image_copy_request().expect("focused copy").image.frames[0].rgba[0],
                    40 + focus_index as u8
                );
                let position = app.preview_folder(id, &source).expect("position");
                assert_eq!(position.index, focus_index);
                assert_eq!(position.reading_paths(), Some(paths[..count].to_vec()));
                assert!(
                    app.status_details()
                        .contains(&format!("{} / 12", focus_index + 1))
                );
                assert_eq!(
                    app.image_prefetch_paths(),
                    Some(paths[count..(count * 2).min(paths.len())].to_vec())
                );
                let foreground = app.tabs.open_new(root.join("other.bmp"), MediaKind::Image);
                app.retain_image_tab();
                app.displayed_tab = Some(foreground);
                app.path = Some(root.join("other.bmp"));
                let preview = app
                    .retained_tab_preview(id, &source)
                    .expect("retained preview");
                let tab_preview::RetainedPreview::Reading { pages, .. } = preview else {
                    panic!("reading card")
                };
                assert_eq!(pages.len(), count);
                assert!(pages.iter().all(|page| {
                    page.0
                        .as_ref()
                        .is_some_and(|texture| texture.id() != source_texture)
                }));
                assert_eq!(
                    app.preview_folder(id, &source).expect("background").index,
                    focus_index
                );
                assert!(
                    app.valid_preview_image_destination(
                        id,
                        app.retained_images[&id].instance,
                        &source,
                        &source
                    ),
                    "a hidden source remains a valid seek destination"
                );
                let saved = app.retained_images.remove(&id).expect("saved");
                app.tabs.activate(id);
                app.displayed_tab = Some(id);
                app.path = Some(source.clone());
                app.restore_image_tab(saved);
                assert_eq!(app.reading_focus_path(), Some(&paths[focus_index]));
                // Rename the focus while retaining its Shell identity, then remove it.
                let mut snapshot = app.folder_snapshot.clone().expect("folder");
                let renamed = root.join("renamed-focus.bmp");
                std::fs::copy(&paths[focus_index], &renamed).expect("rename fixture");
                snapshot
                    .items
                    .iter_mut()
                    .find(|item| item.path == paths[focus_index])
                    .expect("focus")
                    .path = renamed.clone();
                snapshot.generation += 1;
                app.apply_folder_snapshot(snapshot.clone());
                wait_image(&mut app);
                assert_eq!(app.reading_focus_path(), Some(&renamed));
                snapshot.items.retain(|item| item.path != renamed);
                snapshot.generation += 1;
                app.apply_folder_snapshot(snapshot);
                wait_image(&mut app);
                assert_ne!(app.reading_focus_path(), Some(&renamed));
                app.set_reading_layout(false, app.reading_settings);
                assert_eq!(app.path.as_ref(), Some(&source));
                assert_eq!(app.edits, history);
                assert!(app.reading_focus.is_none());
                assert_eq!(
                    app.image
                        .as_ref()
                        .expect("original single view")
                        .decoded
                        .frames[0]
                        .rgba[0],
                    40 + source_index as u8
                );
                // Recreate the original grouping after the Shell removal control.
                let snapshot = app.folder_snapshot.as_mut().expect("folder");
                snapshot.items = paths
                    .iter()
                    .enumerate()
                    .map(|(i, path)| towavue_core::FolderMediaItem {
                        path: path.clone(),
                        kind: MediaKind::Image,
                        identity: towavue_core::ShellIdentity::new(vec![i as u8 + 1]),
                    })
                    .collect();
                app.set_reading_layout(
                    true,
                    ReadingSettings {
                        page_count: initial_count,
                        first_page_count: initial_count,
                        ..app.reading_settings
                    },
                );
                wait_image(&mut app);
                app.set_reading_layout(
                    true,
                    ReadingSettings {
                        page_count: count,
                        first_page_count: count,
                        ..app.reading_settings
                    },
                );
                wait_image(&mut app);
                app.dispatch(CommandId::NextImage);
                wait_image(&mut app);
                assert_eq!(
                    app.path.as_ref(),
                    Some(&paths[count]),
                    "advance from the focused spread, even back to the retained source"
                );
                app.dispatch(CommandId::PreviousImage);
                wait_image(&mut app);
                assert_eq!(app.path.as_ref(), Some(&paths[0]));
            }
        }
    }
}
