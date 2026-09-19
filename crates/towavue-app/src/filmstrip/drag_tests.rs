use super::*;
use crate::*;
use std::time::SystemTime;
use towavue_core::{FolderMediaItem, FolderSnapshotSource, ShellIdentity};

fn snapshot(root: &Path) -> FolderSnapshot {
    FolderSnapshot {
        folder_identity: ShellIdentity::new(vec![]),
        folder_path: root.to_owned(),
        items: ["source.png", "other.png", "third.png"]
            .iter()
            .enumerate()
            .map(|(index, name)| FolderMediaItem {
                identity: ShellIdentity::new(vec![index as u8]),
                path: root.join(name),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: vec![],
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 42,
        captured_at: SystemTime::now(),
    }
}

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn frame(
    strip: &mut Filmstrip,
    context: &Context,
    snapshot: &FolderSnapshot,
    current: &Path,
    enabled: bool,
    input: egui::RawInput,
) -> (egui::FullOutput, Vec<UiAction>) {
    let mut actions = Vec::new();
    let output = context.run_ui(input, |_| {
        strip.show(
            context,
            context.content_rect(),
            Some(snapshot),
            Some(current),
            enabled,
            &mut actions,
        )
    });
    (output, actions)
}

fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(960.0, 576.0),
        )),
        events,
        ..Default::default()
    }
}

fn card(output: &egui::FullOutput, name: &str) -> Rect {
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(name))
        .expect("card")
        .1
        .bounds()
        .expect("bounds");
    Rect::from_min_max(
        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
    )
}

#[test]
fn filmstrip_folder_updates_reveal_relocated_focus_once() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_folder_updates_reveal_relocated_focus_once",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    for density in [1.0, 1.25, 2.0] {
        for discard in [false, true] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            context.enable_accesskit();
            app.filmstrip =
                Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                    .expect("strip");
            let mut snapshot = snapshot(&root);
            for index in 3..100 {
                let mut item = snapshot.items[0].clone();
                item.path = root.join(format!("{index}.png"));
                snapshot.items.push(item);
            }
            let current = snapshot.items[0].path.clone();
            let focused = snapshot.items[1].path.clone();
            app.path = Some(current.clone());
            app.media_kind = Some(MediaKind::Image);
            app.folder_snapshot = Some(snapshot.clone());
            app.filmstrip_open = true;
            let draw = |app: &mut Application<_>, snapshot: &FolderSnapshot, events| {
                let mut actions = Vec::new();
                let output = context.run_ui(input(events), |_| {
                    app.filmstrip.show(
                        &context,
                        context.content_rect(),
                        Some(snapshot),
                        Some(&current),
                        true,
                        &mut actions,
                    );
                    if discard && context.current_pass_index() == 0 {
                        context.request_discard("relocated filmstrip focus");
                    }
                });
                (output, actions)
            };
            for _ in 0..3 {
                draw(&mut app, &snapshot, vec![]);
            }
            let output = draw(&mut app, &snapshot, vec![]).0;
            draw(
                &mut app,
                &snapshot,
                vec![egui::Event::PointerMoved(
                    card(&output, "other.png").center(),
                )],
            );
            for target in [99, 1] {
                let index = snapshot
                    .items
                    .iter()
                    .position(|item| item.path == focused)
                    .expect("focused file");
                let item = snapshot.items.remove(index);
                snapshot.items.insert(target, item);
                snapshot.generation += 1;
                app.apply_folder_snapshot(snapshot.clone());
                for _ in 0..3 {
                    let (output, actions) = draw(&mut app, &snapshot, vec![]);
                    assert!(actions.is_empty());
                    let tree = output.platform_output.accesskit_update.expect("tree");
                    let node = &tree
                        .nodes
                        .iter()
                        .find(|(id, _)| *id == tree.focus)
                        .expect("relocated focused card")
                        .1;
                    assert_eq!(node.label(), Some("other.png"));
                    let bounds = node.bounds().expect("visible focus");
                    assert!(
                        bounds.x0 >= 8.0 && bounds.x1 <= 952.0,
                        "target {target}: {bounds:?}"
                    );
                    assert!(
                        app.filmstrip.card_paths.len() < 12,
                        "keep virtualization bounded"
                    );
                }
                let enter = egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                };
                assert!(
                    draw(&mut app, &snapshot, vec![enter]).1
                        == vec![UiAction::OpenFilmstripMedia(focused.clone(), false)]
                );
            }
            let before = app.filmstrip.scroll_offset;
            draw(
                &mut app,
                &snapshot,
                vec![
                    egui::Event::PointerMoved(egui::pos2(480.0, 30.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(-800.0, 0.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            for _ in 0..12 {
                draw(&mut app, &snapshot, vec![]);
            }
            assert!(
                app.filmstrip.scroll_offset > before + 50.0,
                "manual scrolling must not recenter focus"
            );
            app.apply_folder_snapshot(snapshot.clone());
            draw(&mut app, &snapshot, vec![]);
            assert!(
                app.filmstrip.scroll_offset > before + 50.0,
                "an unchanged refresh must not recenter focus"
            );
        }
    }
}

#[test]
fn filmstrip_tab_navigation_uses_current_order_after_folder_updates() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_tab_navigation_uses_current_order_after_folder_updates",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    for density in [1.0, 1.25, 2.0] {
        for discard in [false, true] {
            for shift in [false, true] {
                for change in ["reorder", "insert", "remove_before", "remove_focus"] {
                    let context = crate::fonts::test_context();
                    context.set_pixels_per_point(density);
                    context.enable_accesskit();
                    app.filmstrip = Filmstrip::new(
                        PreviewCache::new(root.join("cache")).expect("cache"),
                        || {},
                    )
                    .expect("strip");
                    let mut snapshot = snapshot(&root);
                    let current = snapshot.items[0].path.clone();
                    let focused = snapshot.items[1].path.clone();
                    app.path = Some(current.clone());
                    app.media_kind = Some(MediaKind::Image);
                    app.folder_snapshot = Some(snapshot.clone());
                    app.filmstrip_open = true;
                    for _ in 0..3 {
                        frame(
                            &mut app.filmstrip,
                            &context,
                            &snapshot,
                            &current,
                            true,
                            input(vec![]),
                        );
                    }
                    let output = frame(
                        &mut app.filmstrip,
                        &context,
                        &snapshot,
                        &current,
                        true,
                        input(vec![]),
                    )
                    .0;
                    let position = card(&output, "other.png").center();
                    frame(
                        &mut app.filmstrip,
                        &context,
                        &snapshot,
                        &current,
                        true,
                        input(vec![egui::Event::PointerMoved(position)]),
                    );
                    match change {
                        "reorder" => snapshot.items.swap(0, 1),
                        "insert" => {
                            let mut added = snapshot.items[0].clone();
                            added.path = root.join("new.png");
                            snapshot.items.insert(0, added);
                        }
                        "remove_before" => {
                            snapshot.items.remove(0);
                        }
                        "remove_focus" => {
                            snapshot.items.remove(1);
                        }
                        _ => unreachable!(),
                    }
                    snapshot.generation += 1;
                    let texture = context.load_texture(
                        "retained card",
                        egui::ColorImage::filled([1, 1], Color32::WHITE),
                        Default::default(),
                    );
                    let texture_id = texture.id();
                    app.filmstrip
                        .previews
                        .insert(focused.clone(), Ok((texture, None)));
                    app.apply_folder_snapshot(snapshot.clone());
                    assert_eq!(
                        app.filmstrip
                            .previews
                            .get(&focused)
                            .and_then(|preview| preview.as_ref().ok())
                            .map(|(texture, _)| texture.id()),
                        Some(texture_id),
                        "keep ready pixels during same-folder refresh"
                    );
                    let count = snapshot.items.len();
                    let start = snapshot
                        .items
                        .iter()
                        .position(|item| item.path == focused)
                        .or_else(|| snapshot.items.iter().position(|item| item.path == current))
                        .unwrap_or(0);
                    let target = (start + if shift { count - 1 } else { 1 }) % count;
                    let expected = &snapshot.items[target].path;
                    let key = |key, modifiers| egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    };
                    let mut actions = Vec::new();
                    let output = context.run_ui(
                        input(vec![key(
                            egui::Key::Tab,
                            if shift {
                                egui::Modifiers::SHIFT
                            } else {
                                egui::Modifiers::NONE
                            },
                        )]),
                        |_| {
                            app.filmstrip.show(
                                &context,
                                context.content_rect(),
                                Some(&snapshot),
                                Some(&current),
                                true,
                                &mut actions,
                            );
                            if discard && context.current_pass_index() == 0 {
                                context.request_discard("folder-order Tab traversal");
                            }
                        },
                    );
                    assert!(actions.is_empty());
                    if change == "remove_focus" {
                        assert!(
                            !app.filmstrip.previews.contains_key(&focused),
                            "removed cards must release their preview"
                        );
                    }
                    let tree = output.platform_output.accesskit_update.expect("tree");
                    let node = &tree
                        .nodes
                        .iter()
                        .find(|(id, _)| *id == tree.focus)
                        .expect("focused card")
                        .1;
                    assert_eq!(
                        node.label(),
                        Some(display_name(expected).as_str()),
                        "{change}, shift={shift}, discard={discard}, density={density}"
                    );
                    if discard {
                        assert!(output.platform_output.num_completed_passes > 1);
                    }
                    assert!(
                        frame(
                            &mut app.filmstrip,
                            &context,
                            &snapshot,
                            &current,
                            true,
                            input(vec![key(egui::Key::Enter, egui::Modifiers::NONE)])
                        )
                        .1 == vec![UiAction::OpenFilmstripMedia(expected.clone(), false)]
                    );
                }
            }
        }
    }
}

#[test]
fn filmstrip_tab_navigation_wraps_without_focusing_background_controls() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_tab_navigation_wraps_without_focusing_background_controls",
    ) else {
        return;
    };
    let snapshot = snapshot(&root);
    let source = snapshot.items[0].path.clone();
    let context = crate::fonts::test_context();
    context.enable_accesskit();
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(context.clone());
    let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
    app.path = Some(source.clone());
    app.media_kind = Some(MediaKind::Image);
    app.displayed_tab = Some(tab);
    app.state = PlaybackState::Paused;
    app.folder_snapshot = Some(snapshot.clone());
    app.dispatch(CommandId::ToggleFilmstrip);
    let discard = std::cell::Cell::new(false);
    let draw = |app: &mut Application<_>, events| {
        let mut actions = Vec::new();
        let output = context.run_ui(input(events), |ui| {
            app.draw_ui(ui, &mut actions);
            if discard.get() && context.current_pass_index() == 0 {
                context.request_discard("Tab navigation commits once across layout passes");
            }
        });
        assert!(
            actions.is_empty(),
            "focus movement must not activate media or controls"
        );
        output.platform_output.accesskit_update.expect("tree")
    };
    for _ in 0..3 {
        draw(&mut app, vec![]);
    }
    for discarded in [false, true] {
        discard.set(discarded);
        for (shift, indices) in [(false, [1, 2, 0, 1, 2, 0]), (true, [2, 1, 0, 2, 1, 0])] {
            for index in indices {
                for pressed in [true, false] {
                    draw(
                        &mut app,
                        vec![egui::Event::Key {
                            key: egui::Key::Tab,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers {
                                shift,
                                ..Default::default()
                            },
                        }],
                    );
                }
                let tree = draw(&mut app, vec![]);
                let focused = tree
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == tree.focus)
                    .expect("focused card");
                assert_eq!(
                    focused.1.label(),
                    Some(display_name(&snapshot.items[index].path).as_str()),
                    "Tab stays in filmstrip and follows folder order; shift={shift}"
                );
                assert_eq!(app.path.as_ref(), Some(&source));
                assert_eq!(app.tabs.active().map(|tab| tab.id), Some(tab));
                assert!(app.filmstrip_open);
            }
        }
    }
    for (shift, index) in [(false, 2), (true, 0)] {
        let events = (0..2)
            .flat_map(|_| [true, false])
            .map(|pressed| egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers {
                    shift,
                    ..Default::default()
                },
            })
            .collect();
        let tree = draw(&mut app, events);
        assert_eq!(
            tree.nodes
                .iter()
                .find(|(id, _)| *id == tree.focus)
                .expect("focused card")
                .1
                .label(),
            Some(display_name(&snapshot.items[index].path).as_str()),
            "batched keys each advance once"
        );
    }
    discard.set(false);
    for (key, index) in [
        (egui::Key::ArrowRight, 1),
        (egui::Key::Tab, 2),
        (egui::Key::ArrowLeft, 1),
        (egui::Key::Tab, 2),
    ] {
        let tree = draw(
            &mut app,
            [true, false]
                .into_iter()
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .collect(),
        );
        assert_eq!(
            tree.nodes
                .iter()
                .find(|(id, _)| *id == tree.focus)
                .expect("focused card")
                .1
                .label(),
            Some(display_name(&snapshot.items[index].path).as_str()),
            "mixed keys use the current card: {key:?}"
        );
    }
    let tree = draw(&mut app, vec![]);
    let hovered = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("other.png"))
        .expect("hover card")
        .1
        .bounds()
        .expect("bounds");
    draw(
        &mut app,
        vec![egui::Event::PointerMoved(egui::pos2(
            ((hovered.x0 + hovered.x1) * 0.5) as f32,
            ((hovered.y0 + hovered.y1) * 0.5) as f32,
        ))],
    );
    let tree = draw(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(
        tree.nodes
            .iter()
            .find(|(id, _)| *id == tree.focus)
            .expect("focused card")
            .1
            .label(),
        Some("third.png"),
        "Tab advances from the pointer-selected card"
    );
}

#[test]
fn filmstrip_open_and_image_handoff_keep_first_frame_geometry() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_open_and_image_handoff_keep_first_frame_geometry",
    ) else {
        return;
    };
    first_frame_trial(&root, |_, _, _| Vec::new());
}

fn first_frame_trial(
    root: &Path,
    mut present: impl FnMut(&Context, &egui::FullOutput, bool) -> Vec<u8>,
) {
    for density in [1.0, 1.25, 2.0] {
        for reading in [false, true] {
            for zoom in [ZoomMode::Fit, ZoomMode::Custom(3.0)] {
                let context = crate::fonts::test_context();
                context.global_style_mut(chrome::style);
                let snapshot = snapshot(root);
                let source = snapshot.items[0].path.clone();
                let mut app = Application::new(None, |_| {}).expect("app");
                app.ui_context = Some(context.clone());
                let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
                app.path = Some(source.clone());
                app.media_kind = Some(MediaKind::Image);
                app.displayed_tab = Some(tab);
                app.state = PlaybackState::Paused;
                app.folder_snapshot = Some(snapshot.clone());
                app.reading_mode = reading;
                app.image_view.zoom = zoom;
                let decoded = Arc::new(towavue_runtime_windows::DecodedImage {
                    format: "test",
                    animation_plays: 0,
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width: 320,
                        height: 480,
                        rgba: [24, 48, 72, 255].repeat(320 * 480),
                        delay: Duration::ZERO,
                    }],
                });
                let image =
                    ImagePresentation::from_decoded(&context, &source, decoded).expect("image");
                let image_id = image.texture.id();
                if reading {
                    app.reading_pages.push(Ok(image.clone()));
                }
                app.image = Some(image);
                let preview = context.load_texture(
                    "owned preview",
                    egui::ColorImage::filled([64, 96], Color32::GRAY),
                    egui::TextureOptions::LINEAR,
                );
                for item in &snapshot.items {
                    app.filmstrip
                        .previews
                        .insert(item.path.clone(), Ok((preview.clone(), None)));
                }
                let mut frame_number = 0;
                let mut draw = |app: &mut Application<_>| {
                    let mut raw = input(vec![]);
                    // Settle pre-existing chrome fades before opening the new overlay.
                    raw.time = Some(
                        frame_number as f64 / 60.0 + if frame_number >= 3 { 1.0 } else { 0.0 },
                    );
                    raw.viewports
                        .get_mut(&egui::ViewportId::ROOT)
                        .expect("viewport")
                        .native_pixels_per_point = Some(density);
                    let output = context.run_ui(raw, |ui| app.draw_ui(ui, &mut Vec::new()));
                    let pixels = present(&context, &output, frame_number == 0);
                    frame_number += 1;
                    (output, pixels)
                };
                let meshes = |output: &egui::FullOutput, id| {
                    output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh) if mesh.texture_id == id => {
                                Some((shape.clip_rect, mesh.vertices.clone()))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                };
                let bars = |output: &egui::FullOutput| {
                    output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Rect(rect)
                                if rect.rect.height() <= 5.0
                                    && rect.rect.width() > 50.0
                                    && rect.rect.top() > 100.0 =>
                            {
                                Some((shape.clip_rect, rect.clone()))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                };
                for _ in 0..3 {
                    draw(&mut app);
                }
                let baseline = meshes(&draw(&mut app).0, image_id);
                assert_eq!(context.pixels_per_point(), density);
                assert_eq!(baseline.len(), if reading { 2 } else { 1 });
                for _ in 0..3 {
                    app.dispatch(CommandId::ToggleFilmstrip);
                    assert!(app.filmstrip_open);
                    let (first, first_pixels) = draw(&mut app);
                    let previews = meshes(&first, preview.id());
                    assert_eq!(
                        previews.len(),
                        3,
                        "first open frame shows every cached preview"
                    );
                    for _ in 0..3 {
                        let (output, pixels) = draw(&mut app);
                        assert_eq!(
                            bars(&output),
                            bars(&first),
                            "scrollbars must not fade in after the filmstrip"
                        );
                        assert_eq!(
                            meshes(&output, preview.id()),
                            previews,
                            "filmstrip must not jump or fade after opening"
                        );
                        assert_eq!(
                            meshes(&output, image_id),
                            baseline,
                            "opening must not move the underlying image"
                        );
                        if pixels != first_pixels {
                            let width = (960.0 * density) as usize;
                            let mut bounds = (usize::MAX, usize::MAX, 0, 0);
                            let mut count = 0;
                            for (index, (before, after)) in first_pixels
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .zip(pixels.as_chunks::<4>().0)
                                .enumerate()
                            {
                                if before != after {
                                    let (x, y) = (index % width, index / width);
                                    bounds = (
                                        bounds.0.min(x),
                                        bounds.1.min(y),
                                        bounds.2.max(x),
                                        bounds.3.max(y),
                                    );
                                    count += 1;
                                }
                            }
                            panic!(
                                "GPU first-frame difference: density={density}, reading={reading}, zoom={zoom:?}, count={count}, bounds={bounds:?}"
                            );
                        }
                    }
                    assert_eq!(meshes(&first, image_id), baseline);
                    app.close_filmstrip();
                    assert_eq!(meshes(&draw(&mut app).0, image_id), baseline);
                    assert!(app.pending_folder.is_some(), "refresh stays unresolved");
                    assert!(app.filmstrip.visible.is_empty(), "background work stops");
                    assert_eq!(
                        app.filmstrip.previews.len(),
                        3,
                        "reopening keeps ready previews"
                    );
                }
                app.image_handoff = app.take_navigation_handoff(MediaKind::Image);
                assert!(app.image_handoff.is_some());
                app.image_loading = true;
                assert_eq!(
                    meshes(&draw(&mut app).0, image_id),
                    baseline,
                    "held image uses the same geometry"
                );
                app.image_loading = false;
                app.image_handoff = None;
                assert_eq!(meshes(&draw(&mut app).0, image_id), baseline);
                for item in &snapshot.items {
                    app.filmstrip
                        .previews
                        .insert(item.path.clone(), Ok((preview.clone(), None)));
                }
                app.filmstrip_open = true;
                assert_eq!(meshes(&draw(&mut app).0, preview.id()).len(), 3);
                app.apply_folder_snapshot(snapshot.clone());
                assert_eq!(
                    app.filmstrip.previews.len(),
                    3,
                    "Shell refresh must retain previews until replacements arrive"
                );
                if reading {
                    assert!(app.image_loading);
                    let held = app.image_handoff.as_ref().expect("held reading spread");
                    assert_eq!(held.image.texture.id(), image_id);
                    assert_eq!(
                        held.reading
                            .as_ref()
                            .expect("reading geometry")
                            .images
                            .len(),
                        1
                    );
                }
                // Hold the asynchronous reload unresolved for the next rendered frame.
                app.image_generation = app.image_loader.request(Vec::new());
                let refreshed = draw(&mut app).0;
                assert_eq!(meshes(&refreshed, image_id), baseline);
                assert_eq!(meshes(&refreshed, preview.id()).len(), 3);
            }
        }
    }
}

#[test]
#[ignore = "requires hardware D3D11; generated image and previews only"]
fn gpu_filmstrip_first_frame_matches_subsequent_presentations() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::gpu_filmstrip_first_frame_matches_subsequent_presentations",
    ) else {
        return;
    };
    struct Trial {
        root: PathBuf,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden owned window");
            let mut renderer = Some(FrameRenderer::new(&window).expect("hardware D3D11"));
            let mut frames = 0;
            first_frame_trial(&self.root, |context, output, reset| {
                if reset {
                    let previous = renderer.take().expect("previous renderer");
                    let device = previous.graphics_device();
                    previous.release_surface();
                    renderer = Some(
                        FrameRenderer::with_graphics_device(&window, device)
                            .expect("fresh texture table"),
                    );
                }
                let renderer = renderer.as_mut().expect("renderer");
                let size = context.viewport_rect().size() * context.pixels_per_point();
                renderer
                    .resize_surface(size.x as u32, size.y as u32)
                    .expect("resize");
                renderer.clear([1.0, 0.0, 1.0, 1.0]).expect("clear");
                renderer.render_ui(context, output.clone()).expect("render");
                let pixels = renderer
                    .verification_surface_rgba()
                    .expect("owned GPU readback");
                assert!(
                    pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|pixel| *pixel == [24, 48, 72, 255] || *pixel == [128, 128, 128, 255]),
                    "source or preview pixels must reach the GPU"
                );
                renderer.present_surface().expect("Present");
                frames += 1;
                pixels
            });
            eprintln!(
                "PASS filmstrip GPU: {frames} frames, 36 compared opens, 108 exact first/subsequent whole-surface comparisons, 12 held refreshes; hidden window, generated images, no physical input"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial { root })
        .expect("trial");
}

#[test]
#[ignore = "requires hardware D3D11; owned files and real preview/image workers"]
fn gpu_filmstrip_refresh_completions_preserve_displayed_pixels() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::gpu_filmstrip_refresh_completions_preserve_displayed_pixels",
    ) else {
        return;
    };
    let mut snapshot = snapshot(&root);
    let mut bitmap = vec![0_u8; 62];
    bitmap[..2].copy_from_slice(b"BM");
    bitmap[2..6].copy_from_slice(&62_u32.to_le_bytes());
    bitmap[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bitmap[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bitmap[18..22].copy_from_slice(&2_u32.to_le_bytes());
    bitmap[22..26].copy_from_slice(&1_u32.to_le_bytes());
    bitmap[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bitmap[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bitmap[34..38].copy_from_slice(&8_u32.to_le_bytes());
    for (index, item) in snapshot.items.iter_mut().enumerate() {
        item.path.set_extension("bmp");
        let red = 48 + index as u8 * 32;
        bitmap[54..].copy_from_slice(&[12, 34, red, 12, 34, red, 0, 0]);
        std::fs::write(&item.path, &bitmap).expect("owned bitmap");
    }
    struct Trial {
        snapshot: FolderSnapshot,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden owned window");
            let mut renderer = FrameRenderer::new(&window).expect("hardware D3D11");
            let mut compared = 0;
            let mut replacements = 0;
            for density in [1.0, 1.25, 2.0] {
                for reading in [false, true] {
                    let device = renderer.graphics_device();
                    renderer.release_surface();
                    renderer = FrameRenderer::with_graphics_device(&window, device)
                        .expect("fresh texture table on the same device");
                    let context = crate::fonts::test_context();
                    context.global_style_mut(chrome::style);
                    let (notify, events) = std::sync::mpsc::channel();
                    let mut app = Application::new(None, move |event| {
                        let _ = notify.send(event);
                    })
                    .expect("app");
                    let source = self.snapshot.items[0].path.clone();
                    app.ui_context = Some(context.clone());
                    app.displayed_tab = Some(app.tabs.open_new(source.clone(), MediaKind::Image));
                    app.path = Some(source);
                    app.media_kind = Some(MediaKind::Image);
                    app.state = PlaybackState::Paused;
                    app.folder_snapshot = Some(self.snapshot.clone());
                    app.reading_mode = reading;
                    app.reading_settings.page_count = 2;
                    app.reading_settings.first_page_count = 2;
                    app.filmstrip_open = true;
                    app.rebuild_reading_pages();
                    let mut frame_number = 0;
                    let media_bounds = std::cell::Cell::new(Rect::NOTHING);
                    let mut draw = |app: &mut Application<_>| {
                        let mut raw = input(vec![]);
                        raw.time = Some(
                            frame_number as f64 / 60.0 + if frame_number >= 3 { 1.0 } else { 0.0 },
                        );
                        raw.viewports
                            .get_mut(&egui::ViewportId::ROOT)
                            .expect("viewport")
                            .native_pixels_per_point = Some(density);
                        let output = context.run_ui(raw, |ui| app.draw_ui(ui, &mut Vec::new()));
                        media_bounds.set(
                            output
                                .shapes
                                .iter()
                                .find_map(|shape| match &shape.shape {
                                    egui::Shape::Rect(rect)
                                        if rect.fill == Color32::from_black_alpha(191) =>
                                    {
                                        Some(rect.rect)
                                    }
                                    _ => None,
                                })
                                .expect("filmstrip covers the media panel"),
                        );
                        renderer
                            .resize_surface((960.0 * density) as u32, (576.0 * density) as u32)
                            .expect("resize");
                        renderer.clear([1.0, 0.0, 1.0, 1.0]).expect("clear");
                        renderer.render_ui(&context, output).expect("render");
                        let pixels = renderer
                            .verification_surface_rgba()
                            .expect("owned readback");
                        renderer.present_surface().expect("Present");
                        frame_number += 1;
                        pixels
                    };
                    draw(&mut app);
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while app.image_loading
                        || app.filmstrip.previews.len() != 3
                        || app
                            .status_file_details
                            .get(app.status_file_source())
                            .is_none()
                    {
                        let event = events
                            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                            .expect("initial image, preview and status-details completion");
                        app.handle_app_event(event);
                        draw(&mut app);
                    }
                    assert!(app.image_error.is_none());
                    assert_eq!(app.reading_pages.len(), usize::from(reading));
                    for _ in 0..6 {
                        draw(&mut app);
                    }
                    let baseline = draw(&mut app);
                    let media = media_bounds.get();
                    assert!(media.is_positive());
                    assert_eq!(
                        baseline.len(),
                        (960.0 * density) as usize * (576.0 * density) as usize * 4
                    );
                    for cycle in 0..3 {
                        let old: Vec<_> = self
                            .snapshot
                            .items
                            .iter()
                            .map(|item| {
                                app.filmstrip.previews[&item.path]
                                    .as_ref()
                                    .expect("preview")
                                    .0
                                    .id()
                            })
                            .collect();
                        app.apply_folder_snapshot(self.snapshot.clone());
                        assert_eq!(app.filmstrip.refreshing.len(), 3);
                        if reading {
                            assert!(app.image_loading && app.image_handoff.is_some());
                        }
                        let deadline = Instant::now() + Duration::from_secs(10);
                        loop {
                            let pixels = draw(&mut app);
                            assert_eq!(media_bounds.get(), media);
                            assert_eq!(pixels.len(), baseline.len());
                            let mut bounds = [usize::MAX, usize::MAX, 0, 0];
                            let different = pixels
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .zip(baseline.as_chunks::<4>().0)
                                .enumerate()
                                .filter(|(index, (a, b))| {
                                    if a == b {
                                        return false;
                                    }
                                    let width = (960.0 * density) as usize;
                                    let (x, y) = (index % width, index / width);
                                    // Reading controls intentionally disable during reload.
                                    // Check the entire media overlay throughout loading, then
                                    // include chrome again once both workers have completed.
                                    if !media.contains(egui::pos2(
                                        (x as f32 + 0.5) / density,
                                        (y as f32 + 0.5) / density,
                                    )) {
                                        return false;
                                    }
                                    bounds[0] = bounds[0].min(x);
                                    bounds[1] = bounds[1].min(y);
                                    bounds[2] = bounds[2].max(x);
                                    bounds[3] = bounds[3].max(y);
                                    true
                                })
                                .count();
                            assert_eq!(
                                different, 0,
                                "refresh pixels: density={density}, reading={reading}, cycle={cycle}, bounds={bounds:?}"
                            );
                            compared += 1;
                            if !app.image_loading && app.filmstrip.refreshing.is_empty() {
                                assert!(
                                    pixels == baseline,
                                    "completed refresh must also restore chrome"
                                );
                                break;
                            }
                            let event = events
                                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                                .expect("refresh workers finish");
                            app.handle_app_event(event);
                        }
                        for (item, old) in self.snapshot.items.iter().zip(old) {
                            assert_ne!(
                                app.filmstrip.previews[&item.path]
                                    .as_ref()
                                    .expect("replacement")
                                    .0
                                    .id(),
                                old,
                                "actual texture replacement"
                            );
                            replacements += 1;
                        }
                        assert!(app.image_handoff.is_none() && app.image_error.is_none());
                    }
                }
            }
            eprintln!(
                "PASS filmstrip refresh: {compared} media-surface comparisons, 18 completed whole-surface comparisons, {replacements} real-worker texture replacements; 18 refreshes, normal/reading at 1/1.25/2x; hidden GPU, owned files, no physical-input claim"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial { snapshot })
        .expect("refresh trial");
}

#[test]
fn filmstrip_highlights_one_target_and_centers_two_line_names_above_the_preview() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_highlights_one_target_and_centers_two_line_names_above_the_preview",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for discard in [false, true] {
            unified_target_trial(&root, density, discard);
        }
    }
}

fn unified_target_trial(root: &Path, density: f32, discard: bool) {
    let context = crate::fonts::test_context();
    context.set_pixels_per_point(density);
    context.enable_accesskit();
    let frame = |strip: &mut Filmstrip,
                 context: &Context,
                 snapshot: &FolderSnapshot,
                 current: &Path,
                 enabled,
                 input| {
        let mut actions = Vec::new();
        let output = context.run_ui(input, |_| {
            strip.show(
                context,
                context.content_rect(),
                Some(snapshot),
                Some(current),
                enabled,
                &mut actions,
            );
            if discard && context.current_pass_index() == 0 {
                context.request_discard("unified filmstrip target survives layout passes");
            }
        });
        if discard {
            assert!(output.platform_output.num_completed_passes > 1);
        }
        (output, actions)
    };
    let mut snapshot = snapshot(root);
    snapshot.items[1].path = root.join(format!("{}.png", "長いファイル名と詳細な説明-".repeat(6)));
    let current = &snapshot.items[0].path;
    let name = display_name(&snapshot.items[1].path);
    let mut strip = Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
        .expect("filmstrip");
    for _ in 0..3 {
        frame(
            &mut strip,
            &context,
            &snapshot,
            current,
            true,
            input(vec![]),
        );
    }
    let output = frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![]),
    )
    .0;
    let rect = card(&output, &name);
    assert_eq!(context.pixels_per_point(), density);
    for _ in 0..2 {
        frame(
            &mut strip,
            &context,
            &snapshot,
            current,
            true,
            input(vec![egui::Event::PointerMoved(rect.center())]),
        );
    }
    let output = frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![]),
    )
    .0;
    let outlines = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if rect.stroke.color == Color32::WHITE && rect.stroke.width == 1.0 =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        outlines(&output),
        [rect.expand(3.0)],
        "hover replaces the current-item outline"
    );
    let enter = || egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let opened = |actions: Vec<UiAction>, expected: &Path| {
        assert_eq!(actions.len(), 1, "activation occurs once");
        assert!(
            matches!(&actions[0], UiAction::OpenFilmstripMedia(path, false) if path == expected)
        );
    };
    opened(
        frame(
            &mut strip,
            &context,
            &snapshot,
            current,
            true,
            input(vec![enter()]),
        )
        .1,
        &snapshot.items[1].path,
    );
    let text = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == name => Some(text),
            _ => None,
        })
        .expect("one filename label");
    assert_eq!(text.galley.rows.len(), 2);
    let bounds = text.galley.rect.translate(text.pos.to_vec2());
    assert!((bounds.bottom() - (rect.top() - 10.0)).abs() < 1.0);
    assert!(bounds.width() > rect.width());
    assert!((bounds.center().x - rect.center().x).abs() < 1.0);
    let third = card(&output, "third.png");
    let third_id = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("third.png"))
        .expect("third card")
        .0;
    let output = frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Focus,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: third_id,
                data: None,
            },
        )]),
    )
    .0;
    assert_eq!(
        outlines(&output),
        [third.expand(3.0)],
        "explicit focus replaces stationary hover"
    );
    opened(
        frame(
            &mut strip,
            &context,
            &snapshot,
            current,
            true,
            input(vec![enter()]),
        )
        .1,
        &snapshot.items[2].path,
    );
    opened(
        frame(
            &mut strip,
            &context,
            &snapshot,
            current,
            true,
            input(vec![
                egui::Event::PointerMoved(rect.center() + egui::vec2(1.0, 0.0)),
                enter(),
            ]),
        )
        .1,
        &snapshot.items[1].path,
    );
    frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![egui::Event::PointerGone]),
    );
    // egui keeps the last interaction position until the frame after PointerGone.
    let output = frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![]),
    )
    .0;
    assert_eq!(
        outlines(&output),
        [rect.expand(3.0)],
        "the unified target remains after the pointer leaves"
    );
    context.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    let _ = context.run_ui(input(vec![]), |_| {});
    let reopened = frame(
        &mut strip,
        &context,
        &snapshot,
        current,
        true,
        input(vec![]),
    )
    .0;
    assert_eq!(
        outlines(&reopened),
        [card(&reopened, "source.png").expand(3.0)],
        "returning filmstrip is immediately opaque with the current item selected"
    );
    // Batch navigation with activation, including wrapping into virtualized cards.
    for (shift, target) in [(false, 1), (false, 2), (false, 0), (true, 2), (true, 1)] {
        let mut raw = input(vec![
            egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    shift,
                    ..Default::default()
                },
            },
            enter(),
        ]);
        raw.screen_rect = Some(Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(320.0, 240.0),
        ));
        let (output, actions) = frame(&mut strip, &context, &snapshot, current, true, raw);
        opened(actions, &snapshot.items[target].path);
        let selected = card(&output, &display_name(&snapshot.items[target].path));
        assert_eq!(outlines(&output), [selected.expand(3.0)]);
        assert!(selected.left() >= 8.0 && selected.right() <= 312.0);
    }
}

#[test]
fn filmstrip_allows_logo_menu_for_pointer_keyboard_and_accessibility() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_allows_logo_menu_for_pointer_keyboard_and_accessibility",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.enable_accesskit();
        context.global_style_mut(crate::chrome::style);
        let mut app = Application::new(None, |_| {}).expect("app");
        let current = root.join("source.png");
        app.ui_context = Some(context.clone());
        app.tabs.open_new(current.clone(), MediaKind::Image);
        app.path = Some(current);
        app.media_kind = Some(MediaKind::Image);
        app.state = PlaybackState::Paused;
        app.folder_snapshot = Some(snapshot(&root));
        let render = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(input(events), |ui| app.draw_ui(ui, &mut actions));
            assert!(actions.is_empty(), "opening a menu dispatches no command");
            output
        };
        for open in [true, false] {
            app.filmstrip_open = open;
            for _ in 0..3 {
                render(&mut app, vec![]);
            }
            let output = render(&mut app, vec![]);
            let tree = output.platform_output.accesskit_update.expect("tree");
            let (id, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("towavue menu"))
                .expect("logo");
            let action = |action| {
                egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                    action,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: *id,
                    data: None,
                })
            };
            let key = |key| egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            };
            let logo = egui::pos2(20.0, 18.0);
            render(&mut app, vec![egui::Event::PointerMoved(logo)]);
            render(&mut app, vec![pointer(logo, true)]);
            render(&mut app, vec![pointer(logo, false)]);
            assert!(egui::Popup::is_any_open(&context), "pointer");
            render(&mut app, vec![key(egui::Key::Escape)]);
            render(&mut app, vec![action(egui::accesskit::Action::Click)]);
            assert!(egui::Popup::is_any_open(&context), "accessible activation");
            render(&mut app, vec![key(egui::Key::Escape)]);
            render(&mut app, vec![action(egui::accesskit::Action::Focus)]);
            render(&mut app, vec![key(egui::Key::Enter)]);
            assert!(egui::Popup::is_any_open(&context), "keyboard activation");
            render(&mut app, vec![key(egui::Key::Escape)]);
            assert!(!node.is_disabled(), "accessible availability");
            assert_eq!(
                app.filmstrip_open, open,
                "menu input does not close filmstrip"
            );
        }
    }
}

#[test]
fn filmstrip_disables_underlying_seek_hover_and_input_until_closed() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_disables_underlying_seek_hover_and_input_until_closed",
    ) else {
        return;
    };
    let context = crate::fonts::test_context();
    context.enable_accesskit();
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(context.clone());
    app.folder_snapshot = Some(snapshot(&root));
    app.path = Some(root.join("source.png"));
    app.media_kind = Some(MediaKind::Image);
    let render = |app: &mut Application<_>, events| {
        let mut actions = Vec::new();
        let output = context.run_ui(input(events), |_| {
            app.draw_seek_bar(
                &context,
                Rect::from_min_max(egui::pos2(0.0, 550.0), egui::pos2(960.0, 576.0)),
                None,
                &mut actions,
            );
        });
        (output, actions)
    };
    let position = egui::pos2(900.0, 550.0);
    for open in [false, true, false] {
        app.filmstrip_open = open;
        for _ in 0..3 {
            render(&mut app, vec![egui::Event::PointerMoved(position)]);
        }
        let (output, _) = render(&mut app, vec![]);
        assert_eq!(
            output.platform_output.cursor_icon == egui::CursorIcon::PointingHand,
            !open,
            "seek hover cursor, filmstrip={open}"
        );
        assert_eq!(
            output
                .shapes
                .iter()
                .any(|shape| matches!(shape.shape, egui::Shape::Circle(_))),
            !open,
            "seek hover thumb, filmstrip={open}"
        );
        let (id, node) = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree")
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Image position"))
            .expect("seek slider");
        assert_eq!(node.is_disabled(), open);
        let (_, actions) = render(
            &mut app,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::SetValue,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: *id,
                    data: Some(egui::accesskit::ActionData::NumericValue(3.0)),
                },
            )],
        );
        assert_eq!(
            actions.is_empty(),
            open,
            "accessible seek, filmstrip={open}"
        );
        render(&mut app, vec![pointer(position, true)]);
        let (_, actions) = render(&mut app, vec![pointer(position, false)]);
        assert_eq!(actions.is_empty(), open, "pointer seek, filmstrip={open}");
    }
}

#[test]
fn filmstrip_media_bounds_own_dimming_wheel_and_scrollbar_without_dragging_cards() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_media_bounds_own_dimming_wheel_and_scrollbar_without_dragging_cards",
    ) else {
        return;
    };
    let context = crate::fonts::test_context();
    context.global_style_mut(crate::chrome::style);
    context.enable_accesskit();
    let snapshot = snapshot(&root);
    let current = &snapshot.items[1].path;
    let mut strip = Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
        .expect("filmstrip");
    let media = Rect::from_min_max(egui::pos2(0.0, 40.0), egui::pos2(960.0, 536.0));
    let mut time = 0.0;
    let mut render = |strip: &mut Filmstrip, enabled, events| {
        let mut actions = Vec::new();
        let output = context.run_ui(
            egui::RawInput {
                time: Some(time),
                ..input(events)
            },
            |_| {
                strip.show(
                    &context,
                    media,
                    Some(&snapshot),
                    Some(current),
                    enabled,
                    &mut actions,
                )
            },
        );
        time += 0.1;
        assert!(
            actions.is_empty(),
            "scrolling must not open or detach media"
        );
        output
    };
    for _ in 0..3 {
        render(&mut strip, true, vec![]);
    }
    let output = render(&mut strip, true, vec![]);
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.role() == egui::accesskit::Role::ScrollBar)
        .expect("scrollbar")
        .1
        .bounds()
        .expect("bounds");
    assert!(bounds.x0 >= f64::from(media.left() + 8.0));
    assert!(bounds.x1 <= f64::from(media.right() - 8.0));
    assert!(bounds.y1 <= f64::from(media.bottom() - 8.0));
    assert!((bounds.height() - 5.0).abs() < 0.01);
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(191) && rect.rect == media
    )), "75% dim stays inside the media panel");
    assert!(
        output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.rect.top() > media.bottom() - 20.0
                && rect.fill.a() > 0
                && rect.rect.bottom() <= media.bottom() - 8.0
                && rect.rect.width() > 20.0 && rect.rect.height() <= 5.0
        )),
        "visible scrollbar is inset from status/resize edges"
    );
    for (point, enabled, moves) in [
        (egui::pos2(480.0, 80.0), true, true),
        (egui::pos2(480.0, 500.0), true, true),
        (egui::pos2(2.0, 280.0), true, true),
        (egui::pos2(958.0, 280.0), true, true),
        (egui::pos2(480.0, 42.0), true, true),
        (egui::pos2(480.0, 534.0), true, true),
        (egui::pos2(480.0, 20.0), true, false),
        (egui::pos2(480.0, 555.0), true, false),
        (egui::pos2(480.0, 80.0), false, false),
        (egui::pos2(480.0, 534.0), false, false),
    ] {
        strip.focus = None;
        for _ in 0..3 {
            render(&mut strip, enabled, vec![egui::Event::PointerMoved(point)]);
        }
        let before = strip.scroll_offset;
        render(
            &mut strip,
            enabled,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -64.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        for _ in 0..20 {
            render(&mut strip, enabled, vec![]);
        }
        assert_eq!(
            strip.scroll_offset > before + 1.0,
            moves,
            "{point:?}, enabled={enabled}"
        );
        assert_eq!(strip.focus.as_deref(), Some(current.as_path()));
    }
    strip.focus = None;
    let grab = egui::pos2(480.0, media.bottom() - 10.0);
    for _ in 0..3 {
        render(&mut strip, true, vec![egui::Event::PointerMoved(grab)]);
    }
    let before = strip.scroll_offset;
    render(&mut strip, true, vec![pointer(grab, true)]);
    let moved = grab + egui::vec2(100.0, 0.0);
    render(&mut strip, true, vec![egui::Event::PointerMoved(moved)]);
    render(&mut strip, true, vec![pointer(moved, false)]);
    assert!(
        strip.scroll_offset > before + 1.0,
        "thumb drag scrolls instead of dragging a card"
    );
    for gutter in [
        egui::pos2(480.0, media.bottom() - 1.0),
        egui::pos2(media.left() + 1.0, media.bottom() - 10.0),
        egui::pos2(media.right() - 1.0, media.bottom() - 10.0),
    ] {
        strip.focus = None;
        for _ in 0..3 {
            render(&mut strip, true, vec![egui::Event::PointerMoved(gutter)]);
        }
        let before = strip.scroll_offset;
        render(&mut strip, true, vec![pointer(gutter, true)]);
        let end = gutter + egui::vec2(100.0, 0.0);
        render(&mut strip, true, vec![egui::Event::PointerMoved(end)]);
        render(&mut strip, true, vec![pointer(end, false)]);
        assert!(
            (strip.scroll_offset - (before - 100.0).max(0.0)).abs() < 1.0,
            "gutters swipe the content without dragging the bar"
        );
    }
    for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
        let context = crate::fonts::test_context();
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context.clone());
        app.tabs.open_new(current.clone(), kind);
        app.path = Some(current.clone());
        app.media_kind = Some(kind);
        app.folder_snapshot = Some(snapshot.clone());
        app.filmstrip_open = true;
        for fullscreen in [false, true] {
            app.fullscreen = fullscreen;
            for _ in 0..3 {
                let output = context.run_ui(input(vec![]), |ui| {
                    app.draw_ui(ui, &mut Vec::new());
                });
                let bounds = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(191) => {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .expect("filmstrip dim");
                assert_eq!(bounds.left(), 0.0);
                assert_eq!(bounds.right(), 960.0);
                if fullscreen {
                    assert_eq!(bounds.top(), 0.0, "no reserved tab area in fullscreen");
                } else {
                    assert!(bounds.top() >= 24.0, "tab bar remains outside dim");
                }
                if fullscreen {
                    assert_eq!(bounds.bottom(), 576.0);
                } else {
                    assert!(
                        bounds.bottom() < 556.0,
                        "status/timeline remain outside dim"
                    );
                }
            }
        }
    }
}

#[test]
fn filmstrip_clicks_dismiss_and_background_open_preserves_the_current_edit() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_clicks_dismiss_and_background_open_preserves_the_current_edit",
    ) else {
        return;
    };
    let snapshot = snapshot(&root);
    for item in &snapshot.items {
        std::fs::write(&item.path, b"owned path fixture").expect("fixture");
    }
    for (button, target_index) in [
        (egui::PointerButton::Primary, 0),
        (egui::PointerButton::Middle, 1),
        (egui::PointerButton::Middle, 0),
        (egui::PointerButton::Primary, 1),
    ] {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        let source = &snapshot.items[0].path;
        let target = &snapshot.items[target_index].path;
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context.clone());
        let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
        app.path = Some(source.clone());
        app.media_kind = Some(MediaKind::Image);
        app.displayed_tab = Some(tab);
        app.folder_snapshot = Some(snapshot.clone());
        app.filmstrip_open = true;
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::RotateClockwise, MediaKind::Image);
        let history = app.edits[&tab].clone();
        let generation = app.media_generation;
        for _ in 0..3 {
            frame(
                &mut app.filmstrip,
                &context,
                &snapshot,
                source,
                true,
                input(vec![]),
            );
        }
        let output = frame(
            &mut app.filmstrip,
            &context,
            &snapshot,
            source,
            true,
            input(vec![]),
        )
        .0;
        let point = card(&output, &display_name(target)).center();
        let offset = app.filmstrip.scroll_offset;
        let focus = app.filmstrip.focus.clone();
        let visible = app.filmstrip.visible.clone();
        let return_focus = Some((generation, egui::Id::new("filmstrip-origin")));
        app.filmstrip_return_focus = return_focus;
        for pressed in [true, false] {
            let (_, actions) = frame(
                &mut app.filmstrip,
                &context,
                &snapshot,
                source,
                true,
                input(vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]),
            );
            for action in actions {
                app.handle_ui_action(action);
            }
        }
        assert_eq!(
            app.filmstrip_open,
            button == egui::PointerButton::Middle,
            "primary clicks dismiss; background opening keeps filmstrip open"
        );
        assert_eq!(app.tabs.active().expect("current tab").id, tab);
        assert_eq!(app.path.as_ref(), Some(source));
        assert_eq!(app.media_generation, generation);
        assert_eq!(app.edits[&tab], history);
        if button == egui::PointerButton::Middle {
            assert_eq!(app.filmstrip.scroll_offset, offset);
            assert_eq!(app.filmstrip.focus, focus);
            assert_eq!(app.filmstrip.visible, visible);
            assert_eq!(app.filmstrip_return_focus, return_focus);
            assert_eq!(app.tabs.tabs().len(), 2);
            let added = &app.tabs.tabs()[1];
            assert_eq!(
                added.target.current_path().expect("file-backed tab"),
                target
            );
            assert!(!app.edits[&added.id].is_dirty());
            assert!(app.pending_guard.is_none());
            assert!(
                !app.image_loading,
                "background opening does not decode or disturb active media"
            );
            for _ in 0..3 {
                let output = context.run_ui(input(vec![]), |ui| {
                    app.draw_ui(ui, &mut Vec::new());
                });
                assert!(app.filmstrip_open);
                assert!(
                    output.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(191)
                    )),
                    "filmstrip remains drawn after background opening"
                );
                assert_eq!(app.tabs.active().expect("current tab").id, tab);
                assert_eq!(app.filmstrip.scroll_offset, offset);
                assert_eq!(app.edits[&tab], history);
            }
        } else {
            assert_eq!(app.tabs.tabs().len(), 1);
            assert_eq!(app.pending_guard.is_some(), target_index != 0);
            if target_index != 0 {
                app.resolve_guard(GuardDecision::Cancel);
                assert_eq!(app.path.as_ref(), Some(source));
                assert_eq!(app.edits[&tab], history);
            }
        }
    }
}

#[test]
fn filmstrip_empty_space_keeps_open_and_respects_disabled_loading_and_drag() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_empty_space_keeps_open_and_respects_disabled_loading_and_drag",
    ) else {
        return;
    };
    let snapshot = snapshot(&root);
    let media = Rect::from_min_max(egui::pos2(0.0, 40.0), egui::pos2(960.0, 536.0));
    for (loading, enabled, outside, drag) in [
        (false, true, false, false),
        (true, true, false, false),
        (false, false, false, false),
        (true, false, false, false),
        (false, true, true, false),
        (false, true, false, true),
    ] {
        let context = crate::fonts::test_context();
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                .expect("filmstrip");
        let mut render = |events| {
            let mut actions = Vec::new();
            let _ = context.run_ui(input(events), |_| {
                strip.show(
                    &context,
                    media,
                    (!loading).then_some(&snapshot),
                    Some(&snapshot.items[0].path),
                    enabled,
                    &mut actions,
                )
            });
            actions
        };
        let point = egui::pos2(480.0, if outside { 20.0 } else { 80.0 });
        for _ in 0..3 {
            render(vec![egui::Event::PointerMoved(point)]);
        }
        assert!(render(vec![pointer(point, true)]).is_empty());
        let release = if drag {
            point + egui::vec2(100.0, 0.0)
        } else {
            point
        };
        if drag {
            assert!(render(vec![egui::Event::PointerMoved(release)]).is_empty());
        }
        let actions = render(vec![pointer(release, false)]);
        assert!(
            actions.is_empty(),
            "empty-space clicks and drags never dismiss or open media"
        );
    }
}

#[test]
fn filmstrip_drag_copies_the_owned_path_once_without_a_floating_preview() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_drag_copies_the_owned_path_once_without_a_floating_preview",
    ) else {
        return;
    };
    for batched in [false, true] {
        let context = crate::fonts::test_context();
        context.global_style_mut(|style| {
            crate::chrome::style(style);
            style.animation_time = 0.0;
        });
        context.enable_accesskit();
        let snapshot = snapshot(&root);
        let source = &snapshot.items[0].path;
        let target = &snapshot.items[1].path;
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                .expect("strip");
        for _ in 0..3 {
            frame(&mut strip, &context, &snapshot, source, true, input(vec![]));
        }
        let texture = context.load_texture(
            "drag fixture",
            egui::ColorImage::filled([4, 2], Color32::GREEN),
            egui::TextureOptions::LINEAR,
        );
        strip
            .previews
            .insert(target.clone(), Ok((texture.clone(), None)));
        let output = frame(&mut strip, &context, &snapshot, source, true, input(vec![])).0;
        let rect = card(&output, "other.png");
        assert!(
            output.shapes.iter().any(|shape| matches!(
                &shape.shape, egui::Shape::Rect(shape)
                    if shape.rect == rect && shape.fill == crate::chrome::BORDER
            )),
            "card uses the shared grayscale surface"
        );
        let origin = rect.center();
        let moved = origin + egui::vec2(12.0, -120.0);
        let outside = egui::pos2(-20.0, 100.0);
        let generation = strip.generation;
        if !batched {
            frame(
                &mut strip,
                &context,
                &snapshot,
                source,
                true,
                input(vec![
                    egui::Event::PointerMoved(origin),
                    pointer(origin, true),
                ]),
            );
            let (output, actions) = frame(
                &mut strip,
                &context,
                &snapshot,
                source,
                true,
                input(vec![egui::Event::PointerMoved(moved)]),
            );
            assert!(actions.is_empty());
            let floating = Rect::from_min_size(moved - (origin - rect.min), rect.size());
            assert!(
                !output
                    .shapes
                    .iter()
                    .any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect)
                        if rect.rect == floating && rect.fill == crate::chrome::BORDER)),
                "no floating card"
            );
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture.id() && mesh.calc_bounds().center() == floating.center())), "no preview mesh follows the pointer");
            assert_eq!(
                strip.active_drag(&context, Some(source)),
                Some((target.as_path(), snapshot.generation, moved))
            );
            frame(
                &mut strip,
                &context,
                &snapshot,
                source,
                true,
                input(vec![egui::Event::PointerGone]),
            );
            for _ in 0..2 {
                frame(&mut strip, &context, &snapshot, source, true, input(vec![]));
            }
        }
        let mut events = if batched {
            vec![egui::Event::PointerMoved(origin), pointer(origin, true)]
        } else {
            vec![]
        };
        events.extend([egui::Event::PointerMoved(outside), pointer(outside, false)]);
        let (_, actions) = frame(&mut strip, &context, &snapshot, source, true, input(events));
        assert!(
            actions
                == vec![UiAction::OpenWindow(
                    target.clone(),
                    snapshot.generation,
                    outside,
                    origin - rect.min,
                )]
        );
        assert!(
            frame(&mut strip, &context, &snapshot, source, true, input(vec![]))
                .1
                .is_empty()
        );
        assert_eq!(
            strip.generation, generation,
            "drag does not request another preview decode"
        );
    }
}

#[test]
fn filmstrip_window_origin_preserves_the_card_grab_across_sizes_and_densities() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_window_origin_preserves_the_card_grab_across_sizes_and_densities",
    ) else {
        return;
    };
    for width in [480.0, 960.0, 1440.0] {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::fonts::test_context();
            context.enable_accesskit();
            context.set_pixels_per_point(density);
            let snapshot = snapshot(&root);
            let source = &snapshot.items[0].path;
            let target = &snapshot.items[1].path;
            let mut strip =
                Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                    .expect("strip");
            let raw = |events| egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 576.0),
                )),
                ..input(events)
            };
            for _ in 0..3 {
                frame(&mut strip, &context, &snapshot, source, true, raw(vec![]));
            }
            let output = frame(&mut strip, &context, &snapshot, source, true, raw(vec![])).0;
            let rect = card(&output, "other.png");
            let offset = egui::vec2(24.0, 20.0);
            let origin = rect.min + offset;
            let outside = egui::pos2(width + 120.0, 110.0);
            frame(
                &mut strip,
                &context,
                &snapshot,
                source,
                true,
                raw(vec![pointer(origin, true)]),
            );
            let (_, actions) = frame(
                &mut strip,
                &context,
                &snapshot,
                source,
                true,
                raw(vec![
                    egui::Event::PointerMoved(outside),
                    pointer(outside, false),
                ]),
            );
            assert!(
                actions
                    == vec![UiAction::OpenWindow(
                        target.clone(),
                        snapshot.generation,
                        outside,
                        offset,
                    )],
                "width {width}, density {density}"
            );
            assert!(
                frame(&mut strip, &context, &snapshot, source, true, raw(vec![]))
                    .1
                    .is_empty()
            );
        }
    }
}

#[test]
fn filmstrip_window_request_preserves_the_original_tab_and_handles_failure_and_stale_actions() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_window_request_preserves_the_original_tab_and_handles_failure_and_stale_actions",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let snapshot = snapshot(&root);
    let source = snapshot.items[0].path.clone();
    let target = snapshot.items[1].path.clone();
    let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
    app.path = Some(source.clone());
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    app.export_paths.insert(tab, root.join("saved.png"));
    app.folder_snapshot = Some(snapshot);
    app.filmstrip_open = true;
    let tabs = app.tabs.clone();
    let edits = app.edits.clone();
    let exports = app.export_paths.clone();
    let generation = app.media_generation;
    app.open_filmstrip_window(target.clone(), 41, |_| panic!("stale launch"));
    app.open_filmstrip_window(root.join("absent.png"), 42, |_| panic!("foreign path"));
    app.palette_open = true;
    app.open_filmstrip_window(target.clone(), 42, |_| panic!("covered launch"));
    app.palette_open = false;
    app.open_filmstrip_window(target.clone(), 42, |path| {
        assert_eq!(path, target);
        Err(std::io::Error::other("injected start failure"))
    });
    assert!(app.filmstrip_open);
    assert!(
        app.status_message
            .as_ref()
            .expect("diagnostic")
            .0
            .contains("injected start failure")
    );
    app.open_filmstrip_window(target.clone(), 42, |path| {
        assert_eq!(path, target);
        Ok(())
    });
    assert!(!app.filmstrip_open);
    app.open_filmstrip_window(target, 42, |_| panic!("duplicate launch"));
    assert_eq!(app.tabs, tabs);
    assert_eq!(app.edits, edits);
    assert_eq!(app.export_paths, exports);
    assert_eq!(app.path, Some(source));
    assert_eq!(app.state, PlaybackState::Paused);
    assert_eq!(app.media_generation, generation);
    assert!(app.pending_guard.is_none());
    let executable = root.join("app with spaces.exe");
    let path = root.join("日本語 & 'quoted' source.png");
    let command = crate::new_window_command(&executable, &path);
    assert_eq!(command.get_program(), executable.as_os_str());
    assert_eq!(command.get_args().collect::<Vec<_>>(), [path.as_os_str()]);
}

#[test]
fn filmstrip_drag_cancels_on_context_changes_without_replaying_the_release() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_drag_cancels_on_context_changes_without_replaying_the_release",
    ) else {
        return;
    };
    for case in 0..12 {
        let context = crate::fonts::test_context();
        context.enable_accesskit();
        let mut snapshot = snapshot(&root);
        let source = snapshot.items[0].path.clone();
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                .expect("strip");
        for _ in 0..3 {
            frame(
                &mut strip,
                &context,
                &snapshot,
                &source,
                true,
                input(vec![]),
            );
        }
        let output = frame(
            &mut strip,
            &context,
            &snapshot,
            &source,
            true,
            input(vec![]),
        )
        .0;
        let origin = card(&output, "other.png").center();
        let moved = origin + egui::vec2(12.0, -120.0);
        frame(
            &mut strip,
            &context,
            &snapshot,
            &source,
            true,
            input(vec![
                egui::Event::PointerMoved(origin),
                pointer(origin, true),
            ]),
        );
        frame(
            &mut strip,
            &context,
            &snapshot,
            &source,
            true,
            input(vec![egui::Event::PointerMoved(moved)]),
        );
        let mut raw = input(vec![]);
        let mut enabled = true;
        let mut current = source.clone();
        match case {
            0 => raw.events.push(pointer(moved, false)),
            1 => raw.events.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }),
            2 => raw.focused = false,
            3 => enabled = false,
            4 => snapshot.generation += 1,
            5 => current = snapshot.items[2].path.clone(),
            6 => {
                raw.screen_rect = Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 400.0),
                ))
            }
            7 => context.set_pixels_per_point(2.0),
            8 => strip.clear(),
            9 => {
                let _ = context.run_ui(input(vec![]), |_| {});
            }
            10 => strip.clear_previews(),
            11 => {
                assert!(strip.cancel_native_drag(&context));
                assert!(
                    strip.active_drag(&context, Some(&source)).is_none(),
                    "native cancellation clears feedback before redraw"
                );
            }
            _ => unreachable!(),
        }
        assert!(
            frame(&mut strip, &context, &snapshot, &current, enabled, raw)
                .1
                .is_empty(),
            "cancel {case}"
        );
        let outside = egui::pos2(-20.0, 100.0);
        assert!(
            frame(
                &mut strip,
                &context,
                &snapshot,
                &current,
                true,
                input(vec![
                    egui::Event::PointerMoved(outside),
                    pointer(outside, false)
                ])
            )
            .1
            .is_empty(),
            "late release {case}"
        );
    }
}

pub(crate) fn hardware_drag_cancel<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
) {
    use crate::video_rotation::tests::frame as native_frame;
    let original_snapshot = app.folder_snapshot.clone();
    let original_open = app.filmstrip_open;
    let original_view = app.filmstrip.take_view();
    let path = app.path.clone().expect("source");
    let tabs = app.tabs.clone();
    let edits = app.edits.clone();
    let transport = app.state;
    let generation = app.media_generation;
    let mut snapshot = snapshot(path.parent().expect("folder"));
    snapshot.items[0].path = path;
    let context = app.ui_context.clone().expect("context");
    let texture = context.load_texture(
        "native filmstrip drag",
        egui::ColorImage::filled([16, 8], Color32::GREEN),
        egui::TextureOptions::LINEAR,
    );
    for item in &snapshot.items {
        app.filmstrip
            .previews
            .insert(item.path.clone(), Ok((texture.clone(), None)));
    }
    app.folder_snapshot = Some(snapshot);
    app.filmstrip_open = true;
    for _ in 0..3 {
        native_frame(app, vec![]);
    }
    let tree = native_frame(app, vec![]);
    let bounds = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("other.png"))
        .expect("native card")
        .1
        .bounds()
        .expect("bounds");
    let origin = egui::pos2(
        (bounds.x0 + bounds.x1) as f32 * 0.5,
        (bounds.y0 + bounds.y1) as f32 * 0.5,
    ) / context.pixels_per_point();
    let moved = origin + egui::vec2(-20.0, -100.0);
    native_frame(
        app,
        vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
    );
    native_frame(app, vec![egui::Event::PointerMoved(moved)]);
    native_frame(
        app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    native_frame(app, vec![pointer(moved, false)]);
    app.folder_snapshot = original_snapshot;
    app.filmstrip_open = original_open;
    app.filmstrip.restore_view(original_view);
    native_frame(app, vec![]);
    assert_eq!(app.tabs, tabs);
    assert_eq!(app.edits, edits);
    assert_eq!(app.state, transport);
    assert_eq!(app.media_generation, generation);
    assert_eq!(
        app.session
            .as_ref()
            .expect("session")
            .metrics()
            .cpu_transfer_count,
        0
    );
    eprintln!(
        "PASS hardware filmstrip drag: stationary preview draw/cancel; unchanged tabs/history/transport/generation and CPU transfers 0; no child launched"
    );
}

#[test]
fn filmstrip_blank_swipe_tracks_pointer_with_velocity_sensitive_brakeable_momentum() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_blank_swipe_tracks_pointer_with_velocity_sensitive_brakeable_momentum",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for discard in [false, true] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            let mut snapshot = snapshot(&root);
            for index in 3..40 {
                let mut item = snapshot.items[0].clone();
                item.path = root.join(format!("{index}.png"));
                snapshot.items.push(item);
            }
            let current = &snapshot.items[10].path;
            let mut strip =
                Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                    .expect("strip");
            let mut time = 0.0;
            let mut render = |strip: &mut Filmstrip, events: Vec<egui::Event>, enabled| {
                time += 1.0 / 60.0;
                let mut raw = input(events);
                raw.time = Some(time);
                let mut actions = Vec::new();
                let _ = context.run_ui(raw, |_| {
                    strip.show(
                        &context,
                        context.content_rect(),
                        Some(&snapshot),
                        Some(current),
                        enabled,
                        &mut actions,
                    );
                    if discard && context.current_pass_index() == 0 {
                        context.request_discard("swipe multipass control");
                    }
                });
                assert!(
                    actions.is_empty(),
                    "swipe never opens media or closes the overlay"
                );
                assert!(strip.active_drag(&context, Some(current)).is_none());
            };
            let origin = egui::pos2(680.0, 100.0);
            for _ in 0..4 {
                render(&mut strip, vec![egui::Event::PointerMoved(origin)], true);
            }
            for interrupt in ["settle", "press", "wheel", "focus", "disabled"] {
                let before = strip.scroll_offset;
                render(&mut strip, vec![pointer(origin, true)], true);
                for step in 1..=3 {
                    render(
                        &mut strip,
                        vec![egui::Event::PointerMoved(
                            origin - egui::vec2(40.0 * step as f32, 0.0),
                        )],
                        true,
                    );
                    assert!(
                        (strip.scroll_offset - before - 40.0 * step as f32).abs() < 1.0,
                        "pointer distance is applied once at density {density}, discard={discard}"
                    );
                }
                let end = origin - egui::vec2(120.0, 0.0);
                render(&mut strip, vec![pointer(end, false)], true);
                let released = strip.scroll_offset;
                render(&mut strip, vec![], true);
                assert!(strip.scroll_offset > released, "a fast release coasts");
                match interrupt {
                    "press" => render(&mut strip, vec![pointer(end, true)], true),
                    "wheel" => render(
                        &mut strip,
                        vec![egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::Vec2::ZERO,
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        true,
                    ),
                    "focus" => render(&mut strip, vec![egui::Event::WindowFocused(false)], true),
                    "disabled" => render(&mut strip, vec![], false),
                    _ => {}
                }
                let stopped = strip.scroll_offset;
                for _ in 0..90 {
                    render(&mut strip, vec![], interrupt != "disabled");
                }
                assert!(
                    strip.scroll_offset <= released + 2_000.1,
                    "bounded travel after release"
                );
                if interrupt != "settle" {
                    assert_eq!(strip.scroll_offset, stopped, "{interrupt} brakes momentum");
                } else {
                    assert!(
                        strip.scroll_offset > released + 150.0,
                        "a fast flick crosses multiple thumbnails"
                    );
                }
                let settled = strip.scroll_offset;
                for _ in 0..5 {
                    render(&mut strip, vec![], true);
                }
                assert_eq!(strip.scroll_offset, settled, "no idle drift");
                render(
                    &mut strip,
                    vec![
                        pointer(end, false),
                        egui::Event::WindowFocused(true),
                        egui::Event::PointerMoved(origin),
                    ],
                    true,
                );
            }
            let mut previous_tail = 0.0;
            for speed in [60.0, 600.0, 2_400.0] {
                strip.scroll_offset = 1_000.0;
                for _ in 0..12 {
                    render(&mut strip, vec![egui::Event::PointerMoved(origin)], true);
                }
                let before = strip.scroll_offset;
                render(&mut strip, vec![pointer(origin, true)], true);
                let mut end = origin;
                for step in 1..=6 {
                    end = origin - egui::vec2(speed * step as f32 / 60.0, 0.0);
                    render(&mut strip, vec![egui::Event::PointerMoved(end)], true);
                }
                render(&mut strip, vec![pointer(end, false)], true);
                let released = strip.scroll_offset;
                assert!(
                    (released - before - speed / 10.0).abs() < 1.0,
                    "slow and fast drags both track the pointer exactly"
                );
                for _ in 0..90 {
                    render(&mut strip, vec![], true);
                }
                let tail = strip.scroll_offset - released;
                if speed == 60.0 {
                    assert_eq!(tail, 0.0, "slow dragging has no release drift");
                } else {
                    assert!(tail > previous_tail + 10.0, "faster flicks travel farther");
                }
                previous_tail = tail;
            }
            render(
                &mut strip,
                vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
                true,
            );
            let slow_end = origin - egui::vec2(40.0, 0.0);
            render(&mut strip, vec![egui::Event::PointerMoved(slow_end)], true);
            for _ in 0..12 {
                render(&mut strip, vec![], true);
            }
            render(&mut strip, vec![pointer(slow_end, false)], true);
            let stopped = strip.scroll_offset;
            for _ in 0..20 {
                render(&mut strip, vec![], true);
            }
            assert_eq!(
                strip.scroll_offset, stopped,
                "pausing before release stops exactly under the pointer"
            );
            let before = strip.scroll_offset;
            let end = origin - egui::vec2(120.0, 0.0);
            render(
                &mut strip,
                vec![
                    egui::Event::PointerMoved(egui::pos2(-10_000.0, origin.y)),
                    egui::Event::PointerMoved(origin),
                    pointer(origin, true),
                    egui::Event::PointerMoved(end),
                    pointer(end, false),
                ],
                true,
            );
            assert!(
                (strip.scroll_offset - before - 120.0).abs() < 1.0,
                "a complete batched swipe moves once, including discarded passes"
            );
        }
    }
}

#[test]
fn filmstrip_repeated_swipes_accumulate_and_pointer_departure_keeps_momentum() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_repeated_swipes_accumulate_and_pointer_departure_keeps_momentum",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for discard in [false, true] {
            let context = crate::fonts::test_context();
            context.set_pixels_per_point(density);
            let mut snapshot = snapshot(&root);
            for index in 3..2_000 {
                let mut item = snapshot.items[0].clone();
                item.path = root.join(format!("{index}.png"));
                snapshot.items.push(item);
            }
            let current = &snapshot.items[20].path;
            let mut strip =
                Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                    .expect("strip");
            let mut time = 0.0;
            let mut render = |strip: &mut Filmstrip, events: Vec<egui::Event>| {
                time += 1.0 / 60.0;
                let mut raw = input(events);
                raw.time = Some(time);
                let mut actions = Vec::new();
                let _ = context.run_ui(raw, |_| {
                    strip.show(
                        &context,
                        context.content_rect(),
                        Some(&snapshot),
                        Some(current),
                        true,
                        &mut actions,
                    );
                    if discard && context.current_pass_index() == 0 {
                        context.request_discard("repeated swipe multipass control");
                    }
                });
                assert!(actions.is_empty(), "background swipes never activate media");
                assert!(strip.active_drag(&context, Some(current)).is_none());
            };
            let origin = egui::pos2(680.0, 100.0);
            for _ in 0..4 {
                render(&mut strip, vec![egui::Event::PointerMoved(origin)]);
            }
            let flick = |strip: &mut Filmstrip,
                         render: &mut dyn FnMut(&mut Filmstrip, Vec<egui::Event>),
                         direction: f32| {
                render(
                    strip,
                    vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
                );
                let before = strip.scroll_offset;
                let mut end = origin;
                for step in 1..=3 {
                    end = origin - egui::vec2(direction * 40.0 * step as f32, 0.0);
                    render(strip, vec![egui::Event::PointerMoved(end)]);
                    assert!(
                        (strip.scroll_offset - before - direction * 40.0 * step as f32).abs() < 1.0,
                        "even an accumulated swipe tracks the pointer exactly"
                    );
                }
                render(strip, vec![pointer(end, false)]);
                let released = strip.scroll_offset;
                render(strip, vec![]);
                strip.scroll_offset - released
            };
            let first = flick(&mut strip, &mut render, 1.0);
            let second = flick(&mut strip, &mut render, 1.0);
            let third = flick(&mut strip, &mut render, 1.0);
            assert!(first > 0.0);
            assert!(
                second > first * 1.25,
                "second flick adds residual speed: {first}, {second}"
            );
            assert!(
                third > second * 1.15,
                "third flick keeps accumulating: {second}, {third}"
            );
            eprintln!(
                "SWIPE density={density} discard={discard}: release-frame travel {first:.3} -> {second:.3} -> {third:.3} points"
            );
            let reverse = flick(&mut strip, &mut render, -1.0);
            assert!(
                reverse < 0.0 && (-reverse - first).abs() < 1.0,
                "reversal starts at its own speed"
            );
            let mut latest = 0.0;
            for _ in 0..30 {
                latest = flick(&mut strip, &mut render, 1.0);
                assert!(latest <= 201.0, "repeated flick speed stays bounded");
            }
            assert!(
                latest > first * 3.0,
                "rapid flicks exceed the old single-flick ceiling"
            );

            // A click still stops immediately; waiting before a later swipe must
            // discard the saved speed even while the button remains down.
            render(
                &mut strip,
                vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
            );
            let stopped = strip.scroll_offset;
            for _ in 0..30 {
                render(&mut strip, vec![]);
            }
            assert_eq!(strip.scroll_offset, stopped);
            render(&mut strip, vec![pointer(origin, false)]);
            let fresh = flick(&mut strip, &mut render, 1.0);
            assert!(
                (fresh - first).abs() < 1.0,
                "a held click discards accumulated speed"
            );

            // Leaving after release must keep the existing coast. Coming back
            // without a new gesture neither resets its clock nor jumps the offset.
            let released = strip.scroll_offset;
            render(&mut strip, vec![egui::Event::PointerGone]);
            assert!(strip.scroll_offset > released);
            let outside = strip.scroll_offset;
            for _ in 0..3 {
                render(&mut strip, vec![]);
            }
            assert!(strip.scroll_offset > outside);
            let outside = strip.scroll_offset;
            render(&mut strip, vec![egui::Event::PointerMoved(origin)]);
            assert!(strip.scroll_offset > outside && strip.scroll_offset < outside + first + 1.0);

            for final_move in [false, true] {
                // Start a separate drag; its last reported movement is still
                // applied when PointerGone is batched into the same input frame.
                strip.cancel_drag();
                for _ in 0..12 {
                    render(&mut strip, vec![egui::Event::PointerMoved(origin)]);
                }
                render(&mut strip, vec![pointer(origin, true)]);
                for step in 1..=3 {
                    render(
                        &mut strip,
                        vec![egui::Event::PointerMoved(
                            origin - egui::vec2(40.0 * step as f32, 0.0),
                        )],
                    );
                }
                let before = strip.scroll_offset;
                let mut events = Vec::new();
                if final_move {
                    events.push(egui::Event::PointerMoved(origin - egui::vec2(160.0, 0.0)));
                }
                events.push(egui::Event::PointerGone);
                render(&mut strip, events);
                assert!(
                    (strip.scroll_offset - before - if final_move { 40.0 } else { 0.0 }).abs()
                        < 1.0
                );
                let departed = strip.scroll_offset;
                for _ in 0..3 {
                    render(&mut strip, vec![]);
                }
                assert!(
                    strip.scroll_offset > departed,
                    "leaving during a held drag starts a coast"
                );
                let before = strip.scroll_offset;
                render(
                    &mut strip,
                    vec![
                        egui::Event::PointerMoved(egui::pos2(20.0, 100.0)),
                        pointer(egui::pos2(20.0, 100.0), false),
                    ],
                );
                assert!(
                    strip.scroll_offset >= before && strip.scroll_offset < before + 67.0,
                    "reentry release stays within one single-flick frame: before={before}, after={}, first_tail={first}, final_move={final_move}",
                    strip.scroll_offset
                );
                for _ in 0..90 {
                    render(&mut strip, vec![]);
                }
                let settled = strip.scroll_offset;
                for _ in 0..5 {
                    render(&mut strip, vec![]);
                }
                assert_eq!(
                    strip.scroll_offset, settled,
                    "departure momentum stops without idle drift"
                );
            }
        }
    }
}

#[test]
fn filmstrip_card_drag_enters_transfer_only_after_leaving_the_band() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_card_drag_enters_transfer_only_after_leaving_the_band",
    ) else {
        return;
    };
    // Render the real incoming layout as the app does. Without it, an internal
    // release would be rejected independently of the thumbnail-band guard.
    let frame = |strip: &mut Filmstrip,
                 context: &Context,
                 snapshot: &FolderSnapshot,
                 current: &Path,
                 enabled: bool,
                 input: egui::RawInput| {
        let mut actions = Vec::new();
        let output = context.run_ui(input, |ui| {
            crate::tab_drag::incoming(
                ui,
                vec![],
                vec![],
                Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 32.0)),
                None,
            );
            strip.show(
                context,
                context.content_rect(),
                Some(snapshot),
                Some(current),
                enabled,
                &mut actions,
            );
        });
        (output, actions)
    };
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.enable_accesskit();
        let snapshot = snapshot(&root);
        let current = &snapshot.items[0].path;
        let mut strip =
            Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                .expect("strip");
        for _ in 0..4 {
            frame(
                &mut strip,
                &context,
                &snapshot,
                current,
                true,
                input(vec![]),
            );
        }
        let output = frame(
            &mut strip,
            &context,
            &snapshot,
            current,
            true,
            input(vec![]),
        )
        .0;
        let rect = card(&output, "source.png");
        let origin = rect.center();
        let inside = origin + egui::vec2(100.0, 0.0);
        let outside = egui::pos2(inside.x, rect.top() - 40.0);
        let before = strip.scroll_offset;
        let actions = frame(
            &mut strip,
            &context,
            &snapshot,
            current,
            true,
            input(vec![
                egui::Event::PointerMoved(origin),
                pointer(origin, true),
                egui::Event::PointerMoved(inside),
                pointer(inside, false),
            ]),
        )
        .1;
        assert!(actions.is_empty());
        assert_eq!(
            strip.scroll_offset, before,
            "a batched card drag cannot become a background swipe"
        );
        for (leave, return_inside, batched) in [
            (false, false, false),
            (true, false, false),
            (true, true, false),
            (true, true, true),
        ] {
            frame(
                &mut strip,
                &context,
                &snapshot,
                current,
                true,
                input(vec![
                    egui::Event::PointerMoved(origin),
                    pointer(origin, true),
                ]),
            );
            frame(
                &mut strip,
                &context,
                &snapshot,
                current,
                true,
                input(vec![egui::Event::PointerMoved(inside)]),
            );
            assert!(
                strip.active_drag(&context, Some(current)).is_none(),
                "horizontal movement inside the band is not a transfer"
            );
            assert_eq!(strip.scroll_offset, before, "card drags do not swipe");
            let end = if leave { outside } else { inside };
            frame(
                &mut strip,
                &context,
                &snapshot,
                current,
                true,
                input(vec![egui::Event::PointerMoved(end)]),
            );
            assert_eq!(strip.active_drag(&context, Some(current)).is_some(), leave);
            if return_inside && !batched {
                frame(
                    &mut strip,
                    &context,
                    &snapshot,
                    current,
                    true,
                    input(vec![egui::Event::PointerMoved(inside)]),
                );
                assert!(
                    strip.active_drag(&context, Some(current)).is_none(),
                    "returning to the band removes live transfer feedback"
                );
            }
            let release = if return_inside { inside } else { end };
            let events = if return_inside && batched {
                vec![egui::Event::PointerMoved(inside), pointer(inside, false)]
            } else {
                vec![pointer(release, false)]
            };
            let actions = frame(
                &mut strip,
                &context,
                &snapshot,
                current,
                true,
                input(events),
            )
            .1;
            if leave && !return_inside {
                assert!(
                    matches!(actions.as_slice(), [UiAction::OpenWindow(path, generation, point, _)]
                    if path == current && *generation == snapshot.generation && *point == outside)
                );
            } else {
                assert!(
                    actions.is_empty(),
                    "release inside the band never opens or navigates"
                );
            }
            assert!(strip.active_drag(&context, Some(current)).is_none());
        }
    }
}

#[test]
fn filmstrip_file_mutations_keep_the_open_view_through_pending_and_changed_order() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::drag_tests::filmstrip_file_mutations_keep_the_open_view_through_pending_and_changed_order",
    ) else {
        return;
    };
    for (trial, (density, active_source)) in [(1.0, false), (1.25, true), (2.0, false), (2.0, true)]
        .into_iter()
        .enumerate()
    {
        let folder = root.join(format!("trial-{trial}"));
        let destination = folder.join("destination");
        std::fs::create_dir_all(&destination).expect("owned folders");
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.enable_accesskit();
        let mut listing = snapshot(&folder);
        listing.items.clear();
        for index in 0..50 {
            let path = folder.join(format!("{index:02}.bmp"));
            crate::tab_transfer::tests::bitmap(&path);
            listing.items.push(FolderMediaItem {
                identity: ShellIdentity::new(vec![index]),
                path,
                kind: MediaKind::Image,
            });
        }
        let current = listing.items[10].path.clone();
        let source = listing.items[if active_source { 10 } else { 9 }]
            .path
            .clone();
        let renamed = folder.join("renamed.bmp");
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context.clone());
        let id = app.tabs.open_new(current.clone(), MediaKind::Image);
        app.displayed_tab = Some(id);
        app.path = Some(current.clone());
        app.media_kind = Some(MediaKind::Image);
        app.state = PlaybackState::Paused;
        app.source_versions.insert(
            id,
            Some(towavue_runtime_windows::FileOperationSource::capture(&current).expect("version")),
        );
        app.folder_snapshot = Some(listing.clone());
        app.filmstrip_open = true;
        let draw = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(input(events), |_| {
                app.draw_filmstrip(&context, context.content_rect(), &mut actions);
                if context.current_pass_index() == 0 {
                    context.request_discard("mutation layout control");
                }
            });
            assert!(actions.is_empty());
            output
        };
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        let output = draw(&mut app, vec![]);
        draw(
            &mut app,
            vec![egui::Event::PointerMoved(
                card(&output, &display_name(&source)).center(),
            )],
        );
        let offset = app.filmstrip.scroll_offset;
        assert!(offset > 500.0, "fixture has a nontrivial viewport");
        let original =
            towavue_runtime_windows::FileOperationSource::capture(&source).expect("source");
        app.quiesce_file_relocation(&source);
        std::fs::rename(&source, &renamed).expect("owned rename");
        let completed = crate::file_operations::Completed {
            versions: Some(Box::new(crate::file_operations::RelocatedVersions {
                original,
                current: Some(
                    towavue_runtime_windows::FileOperationSource::capture(&renamed)
                        .expect("renamed"),
                ),
            })),
            outcome: towavue_runtime_windows::FileOperationOutcome::Moved(renamed.clone()),
            resume: None,
            recycle: None,
            preference_warning: None,
        };
        app.finish_file_relocation(&source, Some(&completed));
        assert!(
            app.folder_snapshot.is_some(),
            "keep listing during replacement request"
        );
        assert!(app.pending_folder.is_some());
        draw(&mut app, vec![]);
        assert!(
            (app.filmstrip.scroll_offset - offset).abs() < 1.0,
            "pending rename retains offset"
        );
        let index = listing
            .items
            .iter()
            .position(|item| item.path == source)
            .expect("source row");
        let mut item = listing.items.remove(index);
        item.path = renamed.clone();
        listing.items.push(item);
        listing.generation += 1;
        app.folder_order.request(None);
        app.pending_folder = None;
        app.apply_folder_snapshot(listing.clone());
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        assert!(
            (app.filmstrip.scroll_offset - offset).abs() < 1.0,
            "renamed card does not pull the viewport to its new index"
        );
        assert!(app.filmstrip_open);
        assert!(app.filmstrip_target(&renamed).is_some());
        assert!(app.filmstrip_target(&source).is_none());
        assert_eq!(
            app.path.as_ref(),
            Some(if active_source { &renamed } else { &current })
        );

        let moved = destination.join("renamed.bmp");
        let original =
            towavue_runtime_windows::FileOperationSource::capture(&renamed).expect("source");
        app.quiesce_file_relocation(&renamed);
        std::fs::rename(&renamed, &moved).expect("owned move");
        app.finish_file_relocation(
            &renamed,
            Some(&crate::file_operations::Completed {
                versions: Some(Box::new(crate::file_operations::RelocatedVersions {
                    original,
                    current: Some(
                        towavue_runtime_windows::FileOperationSource::capture(&moved)
                            .expect("moved"),
                    ),
                })),
                outcome: towavue_runtime_windows::FileOperationOutcome::Moved(moved.clone()),
                resume: None,
                recycle: None,
                preference_warning: None,
            }),
        );
        draw(&mut app, vec![]);
        assert!((app.filmstrip.scroll_offset - offset).abs() < 1.0);
        listing.items.retain(|item| item.path != renamed);
        if active_source {
            listing.folder_path = destination.clone();
            for item in &mut listing.items {
                item.path = destination.join(item.path.file_name().expect("name"));
                crate::tab_transfer::tests::bitmap(&item.path);
            }
            listing.items.insert(
                0,
                FolderMediaItem {
                    identity: ShellIdentity::new(vec![100]),
                    path: moved.clone(),
                    kind: MediaKind::Image,
                },
            );
        }
        listing.generation += 1;
        app.folder_order.request(None);
        app.pending_folder = None;
        app.apply_folder_snapshot(listing.clone());
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        assert!(
            (app.filmstrip.scroll_offset - offset).abs() < 1.0,
            "move keeps the viewport, including a new current folder"
        );
        assert!(app.filmstrip_open);
        assert_eq!(
            app.path.as_ref(),
            Some(if active_source { &moved } else { &current })
        );
        assert_eq!(app.tabs.active_id(), Some(id));
        assert!(app.filmstrip_target(&renamed).is_none());
        assert_eq!(app.filmstrip_target(&moved).is_some(), active_source);

        let removed = if active_source {
            moved.clone()
        } else {
            listing
                .items
                .iter()
                .find(|item| Some(&item.path) != app.path.as_ref())
                .expect("inactive file")
                .path
                .clone()
        };
        let expected =
            towavue_runtime_windows::FileOperationSource::capture(&removed).expect("delete source");
        app.quiesce_source_save(&removed);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !app.source_readers_idle() {
            assert!(Instant::now() < deadline, "preview readers drained");
            std::thread::sleep(Duration::from_millis(2));
        }
        let retained = active_source.then(|| {
            towavue_runtime_windows::RetainedSource::capture(&expected).expect("held original")
        });
        let before = listing.clone();
        listing.items.retain(|item| item.path != removed);
        listing.generation += 1;
        std::fs::remove_file(&removed).expect("owned deletion");
        app.finish_file_recycling(
            &expected,
            &towavue_runtime_windows::FileRecycleReport {
                retained_source: retained,
                before,
                after: Some(listing.clone()),
            },
        );
        app.thaw_source_save(&removed);
        app.folder_order.request(None);
        app.pending_folder = None;
        app.apply_folder_snapshot(listing.clone());
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        assert!(app.filmstrip_open);
        assert!(
            (app.filmstrip.scroll_offset - offset).abs() < 1.0,
            "deleted card must not reset the viewport"
        );
        assert_eq!(app.current_source_deleted(), active_source);
        assert_eq!(app.filmstrip_target(&removed).is_some(), active_source);
        assert!(
            app.folder_snapshot
                .as_ref()
                .expect("disk listing")
                .items
                .iter()
                .all(|item| item.path != removed)
        );
        if active_source {
            let deleted = &app.deleted_sources[&id];
            let first = app.filmstrip.deleted_snapshot(deleted, Some(&listing));
            let repeated = app.filmstrip.deleted_snapshot(deleted, Some(&listing));
            assert!(
                std::sync::Arc::ptr_eq(&first, &repeated),
                "idle paints reuse the merged held view"
            );
            listing.generation += 1;
            listing.items.reverse();
            let refreshed = app.filmstrip.deleted_snapshot(deleted, Some(&listing));
            assert!(!std::sync::Arc::ptr_eq(&first, &refreshed));
            assert_eq!(
                refreshed
                    .items
                    .iter()
                    .filter(|item| item.path == removed)
                    .count(),
                1
            );
            let real: Vec<_> = refreshed
                .items
                .iter()
                .filter(|item| item.path != removed)
                .map(|item| &item.path)
                .collect();
            assert_eq!(
                real,
                listing
                    .items
                    .iter()
                    .map(|item| &item.path)
                    .collect::<Vec<_>>(),
                "held view follows the newest real order"
            );
        }
        app.filmstrip
            .preserve_after_file_operation(Some(&context), &removed, None);
        assert!(app.filmstrip.preserve_refresh_view);
        let view = app.filmstrip.take_view();
        assert!(!app.filmstrip.preserve_refresh_view);
        assert!(app.filmstrip.held_deleted.is_none() && app.filmstrip.held_snapshot.is_none());
        app.filmstrip.restore_view(view);
        assert!(
            (app.filmstrip.scroll_offset - offset).abs() < 1.0,
            "only the settled view survives tab transfer"
        );
        assert!(!app.filmstrip.preserve_refresh_view);
    }
}
