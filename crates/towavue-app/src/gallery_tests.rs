use crate::*;

#[test]
fn gallery_middle_click_adds_unloaded_background_tabs_and_rejects_stale_actions() {
    use crate::audio_export::tests::frame;
    let Some(root) = tests::isolated_test_root(
        "gallery_tests::gallery_middle_click_adds_unloaded_background_tabs_and_rejects_stale_actions",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let mut app = Application::new(None, |_| {}).expect("app");
        let context = fonts::test_context();
        context.enable_accesskit();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        app.ui_context = Some(context.clone());
        let gallery = app.tabs.gallery().expect("Gallery");
        let image = tab_transfer::tests::install(
            &mut app,
            root.join("retained.png"),
            tab_transfer::tests::decoded(false),
        );
        app.push_visual_edit(EditOperation::RotateClockwise);
        let edits = app.edits[&image].clone();
        app.activate_tab(gallery);
        app.gallery_search = "item".into();
        let paths = ["item.png", "item.mp4", "item.wav"].map(|name| root.join(name));
        app.recent_paths = paths.to_vec();
        let size = egui::vec2(960.0, 576.0);
        let generation = app.media_generation;
        let image_generation = app.image_generation;
        for path in &paths {
            for _ in 0..3 {
                frame(&mut app, size, vec![]);
            }
            let output = frame(&mut app, size, vec![]);
            let tree = output.platform_output.accesskit_update.expect("tree");
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.label() == Some(display_name(path).as_str())
                        && node.role() == egui::accesskit::Role::Button
                })
                .expect("card")
                .1
                .bounds()
                .expect("bounds");
            let pos = egui::pos2((bounds.x0 + 20.0) as f32, (bounds.y0 + 20.0) as f32);
            let count = app.tabs.len();
            for pressed in [true, false] {
                frame(
                    &mut app,
                    size,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Middle,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(app.tabs.len(), count + 1, "one new tab at {density}x");
            assert_eq!(app.tabs.active_id(), Some(gallery));
            assert_eq!(app.gallery_search, "item");
            assert!(app.path.is_none() && app.image.is_none() && app.session.is_none());
            assert_eq!(app.media_generation, generation);
            assert_eq!(app.image_generation, image_generation);
            assert_eq!(app.edits[&image], edits);
            assert!(app.retained_images.contains_key(&image));
            let added = app.tabs.tabs().last().expect("background tab").id;
            assert!(!app.edits[&added].is_dirty());
            if MediaKind::from_path(path) != Some(MediaKind::Image) {
                assert!(app.playback_volumes.contains_key(&added));
                app.tabs.activate(added);
                assert_eq!(app.playback_volume(), 0.5);
                app.tabs.activate(gallery);
            }
        }
        let count = app.tabs.len();
        app.handle_ui_action(UiAction::OpenGalleryBackground(paths[0].clone()));
        assert_eq!(
            app.tabs.len(),
            count + 1,
            "middle-click always creates a new tab"
        );
        let count = app.tabs.len();
        for blocked in 0..5 {
            app.palette_open = blocked == 0;
            app.grid_open = blocked == 1;
            if blocked == 2 {
                egui::Popup::open_id(&context, egui::Id::new("gallery-test-menu"));
            }
            if blocked == 3 {
                app.gallery_search = "not-matching".into();
            }
            if blocked == 4 {
                app.activate_tab(image);
            }
            app.handle_ui_action(UiAction::OpenGalleryBackground(paths[0].clone()));
            assert_eq!(app.tabs.len(), count, "stale action blocked: {blocked}");
            app.palette_open = false;
            app.grid_open = false;
            egui::Popup::close_all(&context);
            app.gallery_search = "item".into();
        }
        app.activate_tab(gallery);
        app.recent_paths.clear();
        app.handle_ui_action(UiAction::OpenGalleryBackground(paths[0].clone()));
        assert_eq!(
            app.tabs.len(),
            count,
            "removed history card cannot open a tab"
        );
    }
}

#[test]
fn gallery_month_rail_tracks_filtered_cards_and_navigates_without_opening_media() {
    use crate::audio_export::tests::frame;
    use egui::accesskit::{Action, ActionData, ActionRequest, TreeId};
    let Some(root) = tests::isolated_test_root(
        "gallery_tests::gallery_month_rail_tracks_filtered_cards_and_navigates_without_opening_media",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let mut app = Application::new(None, |_| {}).expect("app");
        let context = fonts::test_context();
        context.enable_accesskit();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        context.global_style_mut(|style| style.animation_time = 0.0);
        app.ui_context = Some(context.clone());
        for index in 0..40 {
            let (name, date) = match index / 10 {
                0 => ("september", Some((2026, 9))),
                1 => ("july", Some((2026, 7))),
                2 => ("december", Some((2024, 12))),
                _ => ("legacy", None),
            };
            let path = root.join(format!("{name}-{index:02}.png"));
            if let Some(date) = date {
                app.recent_months.insert(path.clone(), date);
            }
            app.recent_paths.push(path);
        }
        let size = egui::vec2(480.0, 300.0);
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        let output = frame(&mut app, size, vec![]);
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree");
        let node = |label| crate::video_rotation::tests::node(tree, label);
        for label in [
            "September 2026",
            "July 2026",
            "December 2024",
            "Date unknown",
        ] {
            node(label);
        }
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, n)| n.label() == Some("August 2026"))
        );
        let search = node("Search Gallery");
        let december = node("December 2024");
        let september = node("September 2026");
        let july = node("July 2026");
        let card_top = |output: &egui::FullOutput, name: &str| {
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("tree")
                .nodes
                .iter()
                .find(|(_, n)| n.label() == Some(name))
                .expect("card")
                .1
                .bounds()
                .expect("bounds")
                .y0
        };
        let first_row_top = card_top(&output, "september-00.png");
        let activate = |target| {
            egui::Event::AccessKitActionRequest(ActionRequest {
                action: Action::Click,
                target_tree: TreeId::ROOT,
                target_node: target,
                data: None,
            })
        };
        frame(&mut app, size, vec![activate(december)]);
        let output = frame(&mut app, size, vec![]);
        assert!(app.path.is_none(), "rail navigation does not open a card");
        let selected_top = card_top(&output, "december-20.png");
        assert!(
            (selected_top - first_row_top).abs() <= 1.0 / f64::from(density),
            "selected month aligns its first row at {density}x: {selected_top} vs {first_row_top}"
        );
        app.palette_open = true;
        frame(&mut app, size, vec![activate(september)]);
        app.palette_open = false;
        let output = frame(&mut app, size, vec![]);
        assert_eq!(
            card_top(&output, "december-20.png"),
            selected_top,
            "covered month controls cannot scroll the Gallery"
        );
        frame(
            &mut app,
            size,
            vec![egui::Event::AccessKitActionRequest(ActionRequest {
                action: Action::Focus,
                target_tree: TreeId::ROOT,
                target_node: july,
                data: None,
            })],
        );
        frame(
            &mut app,
            size,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let output = frame(&mut app, size, vec![]);
        assert!(
            (card_top(&output, "july-10.png") - first_row_top).abs() <= 1.0 / f64::from(density),
            "keyboard activation uses the same month target"
        );
        let set = |value: &str| {
            egui::Event::AccessKitActionRequest(ActionRequest {
                action: Action::SetValue,
                target_tree: TreeId::ROOT,
                target_node: search,
                data: Some(ActionData::Value(value.into())),
            })
        };
        let output = frame(&mut app, size, vec![set("july"), activate(december)]);
        let filtered = output
            .platform_output
            .accesskit_update
            .expect("filtered tree");
        assert!(
            filtered
                .nodes
                .iter()
                .any(|(_, n)| n.label() == Some("July 2026"))
        );
        for absent in ["September 2026", "December 2024", "Date unknown"] {
            assert!(
                !filtered
                    .nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some(absent))
            );
        }
        assert!(
            app.path.is_none(),
            "stale month activation cannot open media"
        );
        let output = frame(&mut app, size, vec![set("not-present")]);
        assert!(
            !output
                .platform_output
                .accesskit_update
                .expect("empty tree")
                .nodes
                .iter()
                .any(|(_, n)| n.label() == Some("July 2026"))
        );
        // Reflow can put several populated months in the same row; every month
        // remains separately addressable, without duplicate IDs or invented dates.
        app.recent_paths = [0, 10, 20, 30]
            .map(|index| app.recent_paths[index].clone())
            .into();
        frame(&mut app, size, vec![set("")]);
        for width in [240.0, 960.0] {
            let output = frame(&mut app, egui::vec2(width, 576.0), vec![]);
            let tree = output
                .platform_output
                .accesskit_update
                .expect("reflow tree");
            for label in [
                "September 2026",
                "July 2026",
                "December 2024",
                "Date unknown",
            ] {
                assert_eq!(
                    tree.nodes
                        .iter()
                        .filter(|(_, n)| n.label() == Some(label))
                        .count(),
                    1
                );
            }
        }
    }
}

#[test]
fn gallery_retains_dirty_image_state_and_preserves_close_guards() {
    let Some(root) = tests::isolated_test_root(
        "gallery_tests::gallery_retains_dirty_image_state_and_preserves_close_guards",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(fonts::test_context());
    let gallery = app.tabs.gallery().expect("default Gallery");
    app.gallery_search = "retained search".into();
    let image = tab_transfer::tests::install(
        &mut app,
        root.join("image.png"),
        tab_transfer::tests::decoded(false),
    );
    app.push_visual_edit(EditOperation::RotateClockwise);
    app.image_view.zoom = ZoomMode::Custom(3.0);
    app.image_view.pan = (20.0, 10.0);
    app.filmstrip_open = true;
    let pixels = Arc::clone(&app.image.as_ref().expect("image").decoded);
    let texture = app.image.as_ref().expect("image").texture.id();
    let view = app.image_view;
    let edits = app.edits[&image].clone();
    app.process_shortcut("Ctrl+T".parse().expect("Gallery shortcut"));
    assert_eq!(app.tabs.active_id(), Some(gallery));
    assert_eq!(app.gallery_search, "retained search");
    assert!(app.path.is_none() && app.image.is_none() && !app.filmstrip_open);
    assert_eq!(app.edits[&image], edits);
    assert!(app.retained_images.contains_key(&image));
    assert_eq!(app.tabs.len(), 2);
    let generation = app.media_generation;
    app.dispatch(CommandId::OpenGallery);
    assert_eq!(app.tabs.len(), 2, "reuse Gallery");
    assert_eq!(
        app.media_generation, generation,
        "already-active Gallery is unchanged"
    );
    app.activate_tab(image);
    assert_eq!(app.image_view, view);
    assert_eq!(app.image.as_ref().expect("restored").texture.id(), texture);
    assert!(Arc::ptr_eq(
        &pixels,
        &app.image.as_ref().expect("restored").decoded
    ));
    assert!(app.filmstrip_open);
    app.activate_tab(gallery);
    app.dispatch_tab_command(gallery, CommandId::CloseOtherTabs);
    assert!(
        app.pending_guard.is_some(),
        "Gallery cannot discard a dirty media tab"
    );
    assert_eq!(app.tabs.active_id(), Some(image));
    app.resolve_guard(GuardDecision::Cancel);
    assert_eq!(app.edits[&image], edits);
    app.activate_tab(gallery);
    app.dispatch(CommandId::CloseTab);
    assert!(app.tabs.gallery().is_none());
    assert!(app.gallery_search.is_empty());
    assert_eq!(app.tabs.active_id(), Some(image));
    app.dispatch(CommandId::OpenGallery);
    assert_ne!(app.tabs.gallery(), Some(gallery));
    app.dispatch_tab_command(
        app.tabs.gallery().expect("new Gallery"),
        CommandId::CloseOtherTabs,
    );
    app.resolve_guard(GuardDecision::Discard);
    assert!(app.tabs.tabs().is_empty());
    app.dispatch(CommandId::CloseTab);
    assert_eq!(app.tabs.len(), 1, "last Gallery cannot close");
    assert!(!app.exit_requested);
}

#[test]
fn gallery_search_filters_immediately_rejects_hidden_cards_and_respects_modal_input() {
    let Some(root) = tests::isolated_test_root(
        "gallery_tests::gallery_search_filters_immediately_rejects_hidden_cards_and_respects_modal_input",
    ) else {
        return;
    };
    use crate::audio_export::tests::frame;
    use egui::accesskit::{Action, ActionData, ActionRequest, TreeId};
    for density in [1.0, 1.25, 2.0] {
        let mut app = Application::new(None, |_| {}).expect("app");
        let context = fonts::test_context();
        context.enable_accesskit();
        context.global_style_mut(chrome::style);
        context.set_pixels_per_point(density);
        app.ui_context = Some(context);
        app.recent_paths = ["alpha.png", "日本 Japan.png", "video.mp4"]
            .map(|name| root.join(name))
            .into();
        let size = egui::vec2(960.0, 576.0);
        frame(&mut app, size, vec![]);
        let output = frame(&mut app, size, vec![]);
        let tree = output.platform_output.accesskit_update.expect("tree");
        let search = crate::video_rotation::tests::node(&tree, "Search Gallery");
        let hidden = crate::video_rotation::tests::node(&tree, "alpha.png");
        let set = |text: &str| {
            egui::Event::AccessKitActionRequest(ActionRequest {
                action: Action::SetValue,
                target_tree: TreeId::ROOT,
                target_node: search,
                data: Some(ActionData::Value(text.into())),
            })
        };
        let output = frame(
            &mut app,
            size,
            vec![
                set("jApAn 日本"),
                egui::Event::AccessKitActionRequest(ActionRequest {
                    action: Action::Click,
                    target_tree: TreeId::ROOT,
                    target_node: hidden,
                    data: None,
                }),
            ],
        );
        assert_eq!(app.gallery_search, "jApAn 日本");
        assert!(
            app.path.is_none(),
            "removed card cannot handle a stale click in the query frame"
        );
        let tree = output
            .platform_output
            .accesskit_update
            .expect("filtered tree");
        assert!(
            tree.nodes
                .iter()
                .any(|(_, n)| n.label() == Some("日本 Japan.png"))
        );
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, n)| n.label() == Some("alpha.png"))
        );
        let no_match = frame(&mut app, size, vec![set("missing-result")]);
        let text = |output: &egui::FullOutput, value: &str| {
            output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == value)
        })
        };
        assert!(text(&no_match, "No matching files."));
        assert!(!text(
            &no_match,
            "Drop media files or a folder here to begin."
        ));
        app.palette_open = true;
        frame(&mut app, size, vec![set("must not replace")]);
        assert_eq!(app.gallery_search, "missing-result");
        app.palette_open = false;
        app.recent_paths.clear();
        let empty = frame(&mut app, size, vec![]);
        assert!(text(&empty, "Drop media files or a folder here to begin."));
        assert!(!text(&empty, "No matching files."));
    }
}

#[test]
fn gallery_logo_middle_click_and_tab_drag_use_the_shared_actions() {
    let Some(root) = tests::isolated_test_root(
        "gallery_tests::gallery_logo_middle_click_and_tab_drag_use_the_shared_actions",
    ) else {
        return;
    };
    use crate::audio_export::tests::frame;
    let mut app = Application::new(None, |_| {}).expect("app");
    let context = fonts::test_context();
    context.enable_accesskit();
    context.global_style_mut(chrome::style);
    app.ui_context = Some(context.clone());
    let image = tab_transfer::tests::install(
        &mut app,
        root.join("image.png"),
        tab_transfer::tests::decoded(false),
    );
    app.close_tab_unchecked(app.tabs.gallery().expect("Gallery"));
    let size = egui::vec2(960.0, 576.0);
    let settle = |app: &mut Application<_>| {
        frame(app, size, vec![]);
        frame(app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("tree")
    };
    let tree = settle(&mut app);
    let bounds = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("towavue menu"))
        .expect("menu")
        .1
        .bounds()
        .expect("bounds");
    let point = egui::pos2(
        ((bounds.x0 + bounds.x1) / 2.0) as f32,
        ((bounds.y0 + bounds.y1) / 2.0) as f32,
    );
    let pointer = |pos, button, pressed| egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(
        &mut app,
        size,
        vec![
            egui::Event::PointerMoved(point),
            pointer(point, egui::PointerButton::Middle, true),
            pointer(point, egui::PointerButton::Middle, false),
        ],
    );
    let gallery = app.tabs.gallery().expect("middle click opens Gallery");
    assert_eq!(app.tabs.active_id(), Some(gallery));
    assert_eq!(app.tabs.tab_ids().collect::<Vec<_>>(), [image, gallery]);
    assert!(!egui::Popup::is_any_open(&context));
    settle(&mut app);
    let start = tab_drag::tests::label_center(&app, gallery);
    let end = tab_drag::tests::drop_point(&context, 0);
    frame(
        &mut app,
        size,
        vec![
            egui::Event::PointerMoved(start),
            pointer(start, egui::PointerButton::Primary, true),
        ],
    );
    frame(&mut app, size, vec![egui::Event::PointerMoved(end)]);
    assert_eq!(
        app.tabs.tab_ids().collect::<Vec<_>>(),
        [image, gallery],
        "hold does not reorder"
    );
    frame(
        &mut app,
        size,
        vec![pointer(end, egui::PointerButton::Primary, false)],
    );
    assert_eq!(app.tabs.tab_ids().collect::<Vec<_>>(), [gallery, image]);
    assert_eq!(app.tabs.active_id(), Some(gallery));
    app.process_shortcut("Ctrl+Tab".parse().expect("next tab"));
    assert_eq!(app.tabs.active_id(), Some(image));
    app.process_shortcut("Ctrl+Shift+Tab".parse().expect("previous tab"));
    assert_eq!(app.tabs.active_id(), Some(gallery));
}

#[test]
fn gallery_tab_is_accessible_and_last_close_stays_visible_but_disabled() {
    let Some(root) = tests::isolated_test_root(
        "gallery_tests::gallery_tab_is_accessible_and_last_close_stays_visible_but_disabled",
    ) else {
        return;
    };
    use crate::audio_export::tests::frame;
    for density in [1.0, 1.25, 2.0] {
        let mut app = Application::new(None, |_| {}).expect("app");
        let context = fonts::test_context();
        context.enable_accesskit();
        context.global_style_mut(chrome::style);
        context.set_pixels_per_point(density);
        app.ui_context = Some(context);
        let size = egui::vec2(640.0, 480.0);
        let settle = |app: &mut Application<_>| {
            frame(app, size, vec![]);
            frame(app, size, vec![])
                .platform_output
                .accesskit_update
                .expect("tree")
        };
        let tree = settle(&mut app);
        let close = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Close tab: Gallery"))
            .expect("visible close button");
        assert!(close.1.is_disabled());
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Gallery tab"))
        );
        let gallery = app.tabs.gallery().expect("Gallery");
        tab_transfer::tests::install(
            &mut app,
            root.join("image.png"),
            tab_transfer::tests::decoded(false),
        );
        let tree = settle(&mut app);
        let close = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Close tab: Gallery"))
            .expect("close button");
        assert!(!close.1.is_disabled());
        let gallery_node = crate::video_rotation::tests::node(&tree, "Gallery tab");
        frame(
            &mut app,
            size,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Click,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: gallery_node,
                    data: None,
                },
            )],
        );
        assert_eq!(app.tabs.active_id(), Some(gallery));
    }
}
