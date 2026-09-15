use crate::*;

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
