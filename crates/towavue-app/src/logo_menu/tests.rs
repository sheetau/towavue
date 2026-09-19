use super::*;
use crate::audio_export::tests::frame;
use crate::video_rotation::tests::{access, node};

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn popup_bounds(output: &egui::FullOutput) -> Vec<egui::Rect> {
    fn collect(shape: &egui::Shape, bounds: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, bounds);
                }
            }
            egui::Shape::Rect(rect)
                if rect.fill == chrome::FLOATING_BACKGROUND
                    && rect.stroke.color == chrome::BORDER =>
            {
                bounds.push(rect.rect);
            }
            _ => {}
        }
    }
    let mut bounds = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut bounds);
    }
    bounds.sort_by(|a, b| a.left().total_cmp(&b.left()));
    bounds
}

fn direction_labels(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if ["File", "Edit", "View"].contains(&text.galley.text()) => {
                Some(text.galley.text().to_owned())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn drag_direction_label_is_immediate_clear_of_the_button_and_input_transparent() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::drag_direction_label_is_immediate_clear_of_the_button_and_input_transparent",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let (mut app, origin) = setup(&root);
        let context = app.ui_context.clone().expect("context");
        context.set_pixels_per_point(density);
        context.global_style_mut(|style| style.interaction.tooltip_delay = 1_000.0);
        let size = egui::vec2(640.0, 480.0);
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        let output = frame(
            &mut app,
            size,
            vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
        );
        assert!(direction_labels(&output).is_empty());
        let state = context
            .data(|data| data.get_temp::<State>(state_id()))
            .expect("gesture");
        let owner = state.drag.expect("owned press").id;
        let button = context.read_response(owner).expect("logo response").rect;
        let output = frame(
            &mut app,
            size,
            vec![egui::Event::PointerMoved(origin + egui::vec2(4.0, 0.0))],
        );
        assert!(
            direction_labels(&output).is_empty(),
            "below the direction threshold"
        );
        for (delta, section) in [
            (egui::vec2(24.0, -12.0), Section::File),
            (egui::vec2(24.0, 24.0), Section::Edit),
            (egui::vec2(-12.0, 24.0), Section::View),
        ] {
            let output = frame(
                &mut app,
                size,
                vec![egui::Event::PointerMoved(origin + delta)],
            );
            assert_eq!(
                direction_labels(&output),
                [section.title()],
                "first direction frame, no tooltip timer"
            );
            assert!(!egui::Popup::is_any_open(&context));
            let bounds = popup_bounds(&output);
            assert_eq!(bounds.len(), 1);
            assert!(
                !bounds[0].intersects(button),
                "label must not cover the menu button"
            );
            assert!(context.content_rect().contains_rect(bounds[0]));
            assert_ne!(
                context.layer_id_at(bounds[0].center()),
                Some(egui::LayerId::new(
                    egui::Order::Tooltip,
                    owner.with("direction-label")
                ))
            );
            let tree = output
                .platform_output
                .accesskit_update
                .expect("accessibility");
            let node = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("towavue menu"))
                .expect("logo")
                .1
                .clone();
            assert_eq!(
                node.description(),
                Some(format!("Release to open the {} menu", section.title()).as_str())
            );
        }
        for point in [origin, origin - egui::vec2(9.0, 9.0)] {
            let output = frame(&mut app, size, vec![egui::Event::PointerMoved(point)]);
            assert!(direction_labels(&output).is_empty());
        }
        let target = origin + egui::vec2(24.0, 24.0);
        let output = frame(&mut app, size, vec![egui::Event::PointerMoved(target)]);
        assert_eq!(direction_labels(&output), ["Edit"]);
        frame(&mut app, size, vec![pointer(target, false)]);
        let output = frame(&mut app, size, vec![]);
        assert!(
            direction_labels(&output).is_empty(),
            "release removes the transient label"
        );
        assert!(
            egui::Popup::is_any_open(&context),
            "label must not consume the menu-opening release"
        );
    }
}

fn setup(root: &Path) -> (Application<fn(AppEvent)>, egui::Pos2) {
    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
    let context = fonts::test_context();
    context.enable_accesskit();
    context.global_style_mut(chrome::style);
    app.ui_context = Some(context);
    let source = root.join("source.png");
    app.tabs.open_new(source.clone(), MediaKind::Image);
    app.path = Some(source);
    app.media_kind = Some(MediaKind::Image);
    for _ in 0..3 {
        frame(&mut app, egui::vec2(640.0, 480.0), vec![]);
    }
    let tree = frame(&mut app, egui::vec2(640.0, 480.0), vec![])
        .platform_output
        .accesskit_update
        .expect("tree");
    let bounds = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("towavue menu"))
        .expect("logo")
        .1
        .bounds()
        .expect("bounds");
    (
        app,
        egui::pos2(
            (bounds.x0 + bounds.x1) as f32 * 0.5,
            (bounds.y0 + bounds.y1) as f32 * 0.5,
        ),
    )
}

#[test]
fn logo_menu_sizes_follow_content_and_pointer_opening_does_not_focus_the_first_item() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::logo_menu_sizes_follow_content_and_pointer_opening_does_not_focus_the_first_item",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for height in [300.0, 720.0] {
            check_menu_geometry(&root, density, egui::vec2(960.0, height));
        }
    }
}

fn check_menu_geometry(root: &Path, density: f32, size: egui::Vec2) {
    let (mut app, origin) = setup(root);
    let context = app.ui_context.as_ref().expect("context");
    context.set_pixels_per_point(density);
    context.global_style_mut(|style| style.animation_time = 0.0);
    let mut measured = Vec::new();
    let mut popup_sizes = Vec::new();
    let mut first_focused = Vec::new();
    for (delta, title) in [
        (egui::Vec2::ZERO, "File"),
        (egui::vec2(24.0, -12.0), "Open file "),
        (egui::vec2(24.0, 24.0), "Undo "),
        (egui::vec2(-12.0, 24.0), "Toggle fullscreen "),
        (egui::Vec2::ZERO, "File"),
    ] {
        for _ in 0..4 {
            frame(&mut app, size, vec![]);
        }
        let target = origin + delta;
        frame(
            &mut app,
            size,
            vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
        );
        frame(
            &mut app,
            size,
            vec![egui::Event::PointerMoved(target), pointer(target, false)],
        );
        for _ in 0..4 {
            frame(&mut app, size, vec![]);
        }
        let output = frame(&mut app, size, vec![]);
        let bounds = popup_bounds(&output);
        assert_eq!(bounds.len(), 1, "one menu frame: {bounds:?}");
        popup_sizes.push(bounds[0].size());
        let tree = output.platform_output.accesskit_update.expect("menu tree");
        let (id, node) = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label().is_some_and(|label| label.starts_with(title)))
            .expect("first item");
        let bounds = node.bounds().expect("item bounds");
        measured.push((title, bounds.width(), bounds.height()));
        first_focused.push(tree.focus == *id);
        if title == "Open file " {
            let focused = frame(&mut app, size, vec![key(egui::Key::ArrowDown)])
                .platform_output
                .accesskit_update
                .expect("keyboard entry");
            assert_eq!(focused.focus, *id, "first arrow selects the first command");
            let second = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.label()
                        .is_some_and(|label| label.starts_with("Open folder "))
                })
                .expect("second command")
                .1
                .bounds()
                .expect("bounds");
            let output = frame(
                &mut app,
                size,
                vec![egui::Event::PointerMoved(egui::pos2(
                    ((second.x0 + second.x1) * 0.5) as f32,
                    ((second.y0 + second.y1) * 0.5) as f32,
                ))],
            );
            assert_eq!(
                output
                    .platform_output
                    .accesskit_update
                    .expect("pointer return")
                    .focus,
                *id,
                "hovering another command retains the keyboard highlight until a click"
            );
        }
        frame(&mut app, size, vec![key(egui::Key::Escape)]);
        frame(&mut app, size, vec![key(egui::Key::Escape)]);
    }
    assert!(
        measured[0].1 < 160.0 && measured[4].1 < 160.0,
        "compact root menu: {measured:?}"
    );
    assert!(
        (measured[0].1 - measured[4].1).abs() < 1.0,
        "root width must not inherit direct menu dimensions"
    );
    assert_eq!(
        popup_sizes[0], popup_sizes[4],
        "root menu restores its size"
    );
    assert!(
        measured[1..4]
            .iter()
            .all(|(_, width, height)| *width < 560.0 && *height < 30.0),
        "natural command rows: {measured:?}"
    );
    assert_eq!(
        first_focused, [false; 5],
        "pointer opening must not pin a keyboard highlight"
    );
    for (steps, direct) in [(0, 1), (1, 2), (2, 3)] {
        for _ in 0..4 {
            frame(&mut app, size, vec![]);
        }
        frame(
            &mut app,
            size,
            vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
        );
        frame(&mut app, size, vec![pointer(origin, false)]);
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        for _ in 0..=steps {
            frame(&mut app, size, vec![key(egui::Key::ArrowDown)]);
        }
        frame(
            &mut app,
            size,
            vec![
                egui::Event::PointerMoved(origin),
                key(egui::Key::ArrowRight),
            ],
        );
        for _ in 0..4 {
            frame(&mut app, size, vec![]);
        }
        let output = frame(&mut app, size, vec![]);
        let bounds = popup_bounds(&output);
        assert_eq!(bounds.len(), 2, "root and nested menu frames: {bounds:?}");
        assert!(
            (bounds[1].size() - popup_sizes[direct]).abs().max_elem() < 1.0,
            "direct and nested popup sizes must match: {:?} vs {:?}",
            popup_sizes[direct],
            bounds[1].size()
        );
        let tree = output
            .platform_output
            .accesskit_update
            .expect("nested menu");
        let (title, width, height) = measured[direct];
        let bounds = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label().is_some_and(|label| label.starts_with(title)))
            .expect("same nested command")
            .1
            .bounds()
            .expect("bounds");
        assert!(
            (bounds.width() - width).abs() < 1.0 && (bounds.height() - height).abs() < 1.0,
            "direct and nested {title} must share geometry: {bounds:?} vs {width}x{height}"
        );
        frame(&mut app, size, vec![key(egui::Key::Escape)]);
        frame(&mut app, size, vec![key(egui::Key::Escape)]);
    }
}

#[test]
fn logo_direction_threshold_and_sector_boundaries_match_the_three_arrows() {
    for (delta, expected) in [
        (egui::vec2(7.99, 0.0), None),
        (egui::vec2(8.0, 0.0), Some(Section::File)),
        (egui::vec2(0.0, 8.0), Some(Section::View)),
        (egui::vec2(0.0, -8.0), Some(Section::File)),
        (egui::vec2(-8.0, 0.0), Some(Section::View)),
        (egui::vec2(-8.0, -8.0), None),
        (egui::vec2(10.0, -5.0), Some(Section::File)),
        (egui::vec2(-5.0, 10.0), Some(Section::View)),
    ] {
        assert_eq!(direction(delta), expected, "{delta:?}");
    }
    for (angle, expected) in [
        (0.0_f32, Some(Section::File)),
        (112.49, Some(Section::File)),
        (112.51, Some(Section::Edit)),
        (157.49, Some(Section::Edit)),
        (157.51, Some(Section::View)),
        (269.99, Some(Section::View)),
        (270.01, None),
        (359.99, None),
    ] {
        let angle = angle.to_radians();
        assert_eq!(
            direction(egui::vec2(angle.sin(), -angle.cos()) * 30.0),
            expected
        );
    }
}

#[test]
fn logo_drag_opens_each_existing_submenu_without_dispatch_and_keeps_keyboard_navigation() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::logo_drag_opens_each_existing_submenu_without_dispatch_and_keeps_keyboard_navigation",
    ) else {
        return;
    };
    for (delta, expected) in [
        (egui::vec2(24.0, -12.0), "Open file "),
        (egui::vec2(24.0, 24.0), "Undo"),
        (egui::vec2(-12.0, 24.0), "Toggle fullscreen"),
    ] {
        for batched in [false, true] {
            let (mut app, origin) = setup(&root);
            let size = egui::vec2(640.0, 480.0);
            let history = app.edits.clone();
            let target = origin + delta;
            if batched {
                frame(
                    &mut app,
                    size,
                    vec![
                        pointer(origin, true),
                        egui::Event::PointerMoved(target),
                        pointer(target, false),
                    ],
                );
            } else {
                frame(
                    &mut app,
                    size,
                    vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
                );
                frame(&mut app, size, vec![egui::Event::PointerMoved(target)]);
                assert!(
                    !egui::Popup::is_any_open(app.ui_context.as_ref().expect("context")),
                    "holding never opens a menu"
                );
                frame(&mut app, size, vec![pointer(target, false)]);
            }
            for _ in 0..3 {
                frame(&mut app, size, vec![]);
            }
            let tree = frame(&mut app, size, vec![])
                .platform_output
                .accesskit_update
                .expect("tree");
            assert!(
                !tree.nodes.iter().any(|(_, node)| matches!(
                    node.label(),
                    Some("File" | "Edit" | "View" | "Help")
                )),
                "direct menu must not show its parent categories"
            );
            let first = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.label()
                        .is_some_and(|label| label.starts_with(expected))
                })
                .expect("direct first item")
                .1
                .bounds()
                .expect("item bounds");
            assert!(
                first.x0 < f64::from(origin.x) && first.y0 < 45.0,
                "direct content stays below the logo: {first:?}"
            );
            assert!(
                tree.nodes.iter().any(|(_, node)| node
                    .label()
                    .is_some_and(|label| label.starts_with(expected))),
                "missing category content {expected}; labels={:?}",
                tree.nodes
                    .iter()
                    .filter_map(|(_, node)| node.label())
                    .collect::<Vec<_>>()
            );
            assert!(
                app.pending_dialog.is_none() && app.edits == history,
                "gesture only opens menu"
            );
            frame(&mut app, size, vec![key(egui::Key::ArrowLeft)]);
            frame(&mut app, size, vec![key(egui::Key::ArrowRight)]);
            let tree = frame(&mut app, size, vec![])
                .platform_output
                .accesskit_update
                .expect("keyboard reopened submenu");
            assert!(
                tree.nodes.iter().any(|(_, node)| {
                    node.label()
                        .is_some_and(|label| label.starts_with(expected))
                }),
                "keyboard return: {expected}, batched={batched}; focus={:?}, labels={:?}",
                tree.focus,
                tree.nodes
                    .iter()
                    .filter_map(|(_, node)| node.label())
                    .collect::<Vec<_>>()
            );
            frame(&mut app, size, vec![key(egui::Key::Escape)]);
            frame(&mut app, size, vec![key(egui::Key::Escape)]);
            assert!(!egui::Popup::is_any_open(
                app.ui_context.as_ref().expect("context")
            ));
        }
    }
}

#[test]
fn logo_keeps_its_owned_press_when_batched_motion_hits_loaded_media_or_a_tab() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::logo_keeps_its_owned_press_when_batched_motion_hits_loaded_media_or_a_tab",
    ) else {
        return;
    };
    for (target, expected) in [
        (egui::pos2(50.0, 9.0), "Open file "),
        (egui::pos2(45.0, 46.0), "Undo "),
        (egui::pos2(9.0, 50.0), "Toggle fullscreen "),
    ] {
        let (mut app, origin) = setup(&root);
        let context = app.ui_context.clone().expect("context");
        app.image = Some(
            ImagePresentation::from_decoded(
                &context,
                app.path.as_ref().expect("path"),
                crate::tab_transfer::tests::decoded(false),
            )
            .expect("image"),
        );
        let size = egui::vec2(960.0, 576.0);
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        frame(&mut app, size, vec![egui::Event::PointerMoved(origin)]);
        frame(
            &mut app,
            size,
            vec![pointer(origin, true), egui::Event::PointerMoved(target)],
        );
        for _ in 0..10 {
            frame(&mut app, size, vec![]);
        }
        assert!(
            context
                .data(|data| data.get_temp::<State>(state_id()))
                .expect("state")
                .drag
                .is_some(),
            "owned drag disappeared over {target:?}"
        );
        frame(&mut app, size, vec![pointer(target, false)]);
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        let tree = frame(&mut app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("tree");
        assert!(
            tree.nodes.iter().any(|(_, node)| node
                .label()
                .is_some_and(|label| label.starts_with(expected))),
            "missing {expected}"
        );
        assert!(
            app.image_view.selection.is_none()
                && app.view_drag.is_none()
                && app.pending_dialog.is_none()
        );
        assert!(context.dragged_id().is_none());
        frame(&mut app, size, vec![key(egui::Key::Escape)]);
        frame(
            &mut app,
            size,
            vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
        );
        frame(&mut app, size, vec![pointer(origin, false)]);
        assert!(
            egui::Popup::is_any_open(&context),
            "ordinary separated click still opens root"
        );
        assert!(context.dragged_id().is_none());
    }
}

#[test]
fn logo_pointer_exit_obeys_release_order_in_batched_native_events() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::logo_pointer_exit_obeys_release_order_in_batched_native_events",
    ) else {
        return;
    };
    for batched_press in [false, true] {
        for exit_before_release in [false, true] {
            for (delta, expected) in [
                (egui::vec2(24.0, -12.0), "Open file "),
                (egui::vec2(24.0, 24.0), "Undo "),
                (egui::vec2(-12.0, 24.0), "Toggle fullscreen "),
            ] {
                let (mut app, origin) = setup(&root);
                let size = egui::vec2(640.0, 480.0);
                let target = origin + delta;
                let history = app.edits.clone();
                let mut events = vec![pointer(origin, true), egui::Event::PointerMoved(target)];
                if !batched_press {
                    frame(&mut app, size, std::mem::take(&mut events));
                }
                if exit_before_release {
                    events.extend([egui::Event::PointerGone, pointer(target, false)]);
                } else {
                    events.extend([pointer(target, false), egui::Event::PointerGone]);
                }
                frame(&mut app, size, events);
                for _ in 0..3 {
                    frame(&mut app, size, vec![]);
                }
                assert_eq!(
                    egui::Popup::is_any_open(app.ui_context.as_ref().expect("context")),
                    !exit_before_release,
                    "batched_press={batched_press}, exit_before_release={exit_before_release}, delta={delta:?}"
                );
                assert!(app.edits == history && app.pending_dialog.is_none());
                let output = frame(&mut app, size, vec![]);
                let tree = output.platform_output.accesskit_update.expect("tree");
                assert_eq!(
                    tree.nodes.iter().any(|(_, node)| node
                        .label()
                        .is_some_and(|label| label.starts_with(expected))),
                    !exit_before_release
                );
                frame(&mut app, size, vec![key(egui::Key::Escape)]);
                assert!(!egui::Popup::is_any_open(
                    app.ui_context.as_ref().expect("context")
                ));
            }
        }
    }
}

#[test]
fn logo_drag_cancellation_never_replays_a_click_and_plain_uia_click_still_opens_root() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::logo_drag_cancellation_never_replays_a_click_and_plain_uia_click_still_opens_root",
    ) else {
        return;
    };
    for mode in 0..16 {
        let (mut app, origin) = setup(&root);
        let size = egui::vec2(640.0, 480.0);
        let target = origin + egui::vec2(24.0, 24.0);
        frame(
            &mut app,
            size,
            vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
        );
        frame(&mut app, size, vec![egui::Event::PointerMoved(target)]);
        let events = match mode {
            0 => vec![key(egui::Key::Escape)],
            1 => vec![egui::Event::WindowFocused(false)],
            2 => vec![egui::Event::PointerGone],
            3 => {
                app.media_generation += 1;
                vec![]
            }
            4 => {
                app.graphics_epoch += 1;
                vec![]
            }
            5 => {
                app.palette_open = true;
                vec![]
            }
            6 => {
                app.fullscreen = true;
                vec![]
            }
            7 => vec![egui::Event::PointerMoved(origin)],
            8 => vec![egui::Event::PointerMoved(origin - egui::vec2(9.0, 9.0))],
            10 => {
                app.tabs.open_new(root.join("other.png"), MediaKind::Image);
                vec![]
            }
            11 => {
                app.ui_context
                    .as_ref()
                    .expect("context")
                    .set_pixels_per_point(2.0);
                vec![]
            }
            12 => {
                app.ui_context
                    .as_ref()
                    .expect("context")
                    .set_dragged_id("another-widget".into());
                vec![]
            }
            13 => {
                app.pending_guard = Some(GuardedAction::Exit);
                vec![]
            }
            14 => {
                cancel(app.ui_context.as_ref().expect("context"));
                vec![]
            }
            15 => {
                app.dispatch(CommandId::ToggleGridMenu);
                vec![]
            }
            _ => vec![],
        };
        let output = frame(
            &mut app,
            if mode == 9 {
                egui::vec2(480.0, 480.0)
            } else {
                size
            },
            events,
        );
        assert!(
            direction_labels(&output).is_empty(),
            "cancelled gesture has no direction label: {mode}"
        );
        app.palette_open = false;
        app.fullscreen = false;
        app.pending_guard = None;
        if mode == 12 {
            app.ui_context.as_ref().expect("context").stop_dragging();
        }
        let release = if mode == 7 {
            origin
        } else if mode == 8 {
            origin - egui::vec2(9.0, 9.0)
        } else {
            target
        };
        frame(
            &mut app,
            size,
            vec![egui::Event::WindowFocused(true), pointer(release, false)],
        );
        assert!(
            !egui::Popup::is_any_open(app.ui_context.as_ref().expect("context")),
            "cancel mode {mode}"
        );
        let tree = frame(&mut app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("tree");
        frame(
            &mut app,
            size,
            vec![access(node(&tree, "towavue menu"), None)],
        );
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        let tree = frame(&mut app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("root menu");
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label().is_some_and(|label| label.starts_with("File"))),
            "normal activation mode {mode}"
        );
        assert!(app.edits.is_empty() && app.pending_dialog.is_none());
    }
}

#[test]
fn logo_drag_uses_the_first_owned_press_and_release_and_dispatches_menu_commands_once() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::logo_drag_uses_the_first_owned_press_and_release_and_dispatches_menu_commands_once",
    ) else {
        return;
    };
    for guard in [false, true] {
        let (mut app, origin) = setup(&root);
        let tab = app.tabs.active().expect("tab").id;
        let size = egui::vec2(640.0, 480.0);
        let history = app.edits.entry(tab).or_default();
        for _ in 0..2 {
            history.push(EditOperation::RotateClockwise, MediaKind::Image);
        }
        let target = origin
            + if guard {
                egui::vec2(24.0, -12.0)
            } else {
                egui::vec2(24.0, 24.0)
            };
        frame(
            &mut app,
            size,
            vec![
                egui::Event::PointerMoved(egui::pos2(600.0, 400.0)),
                pointer(origin, true),
                pointer(target, false),
                pointer(origin, true),
                pointer(origin + egui::vec2(-12.0, 24.0), false),
            ],
        );
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        let tree = frame(&mut app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("menu");
        let label = if guard { "Close tab " } else { "Undo " };
        let target = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label().is_some_and(|value| value.starts_with(label)))
            .unwrap_or_else(|| {
                panic!(
                    "chosen {label}; labels={:?}",
                    tree.nodes
                        .iter()
                        .filter_map(|(_, node)| node.label())
                        .collect::<Vec<_>>()
                )
            })
            .0;
        frame(&mut app, size, vec![access(target, None)]);
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        assert_eq!(
            app.edits[&tab].operations().len(),
            if guard { 2 } else { 1 }
        );
        assert_eq!(app.pending_guard.is_some(), guard);
        assert!(app.tabs.active().is_some() && app.pending_dialog.is_none());
        if guard {
            app.handle_ui_action(UiAction::ResolveGuard(GuardDecision::Cancel));
        }
    }
    for outside in [false, true] {
        let (mut app, origin) = setup(&root);
        let size = egui::vec2(640.0, 480.0);
        let start = if outside {
            egui::pos2(600.0, 400.0)
        } else {
            origin
        };
        frame(
            &mut app,
            size,
            vec![
                egui::Event::PointerMoved(egui::pos2(300.0, 200.0)),
                pointer(start, true),
                pointer(origin, false),
            ],
        );
        assert_eq!(
            egui::Popup::is_any_open(app.ui_context.as_ref().expect("context")),
            !outside,
            "movement before owned press is not a drag"
        );
    }
}

#[test]
fn logo_reopens_different_submenus_after_closed_frames_at_compact_sizes_and_dpi() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::logo_reopens_different_submenus_after_closed_frames_at_compact_sizes_and_dpi",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for width in [240.0, 480.0, 960.0] {
            let (mut app, origin) = setup(&root);
            app.ui_context
                .as_ref()
                .expect("context")
                .set_pixels_per_point(density);
            let size = egui::vec2(width, 300.0);
            for _ in 0..2 {
                for (delta, expected) in [
                    (egui::vec2(24.0, -12.0), "Open file "),
                    (egui::vec2(24.0, 24.0), "Undo "),
                    (egui::vec2(-12.0, 24.0), "Toggle fullscreen "),
                ] {
                    for _ in 0..5 {
                        frame(&mut app, size, vec![]);
                    }
                    let target = origin + delta;
                    frame(
                        &mut app,
                        size,
                        vec![
                            pointer(origin, true),
                            egui::Event::PointerMoved(target),
                            pointer(target, false),
                        ],
                    );
                    for _ in 0..3 {
                        frame(&mut app, size, vec![]);
                    }
                    let tree = frame(&mut app, size, vec![])
                        .platform_output
                        .accesskit_update
                        .expect("reopened category");
                    assert!(
                        tree.nodes.iter().any(|(_, node)| node
                            .label()
                            .is_some_and(|label| label.starts_with(expected))),
                        "{width}px / {density}x / {expected}"
                    );
                    frame(&mut app, size, vec![key(egui::Key::Escape)]);
                    frame(&mut app, size, vec![key(egui::Key::Escape)]);
                    assert!(!egui::Popup::is_any_open(
                        app.ui_context.as_ref().expect("context")
                    ));
                }
            }
        }
    }
}

#[test]
fn logo_shaft_animation_settles_while_held_and_returns_to_the_original_after_cancel() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::logo_shaft_animation_settles_while_held_and_returns_to_the_original_after_cancel",
    ) else {
        return;
    };
    let (mut app, origin) = setup(&root);
    let size = egui::vec2(640.0, 480.0);
    let first_shaft = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points, stroke }
                    if (stroke.width - 1.1).abs() < 0.0001
                        && points[0].x < 28.0
                        && points[0].y < 26.0 =>
                {
                    Some(points[0])
                }
                _ => None,
            })
            .expect("logo shaft")
    };
    let initial = frame(&mut app, size, vec![]);
    let initial = first_shaft(&initial);
    frame(&mut app, size, vec![pointer(origin, true)]);
    let target = origin + egui::vec2(24.0, 24.0);
    frame(&mut app, size, vec![egui::Event::PointerMoved(target)]);
    for _ in 0..20 {
        frame(&mut app, size, vec![]);
    }
    let held = frame(&mut app, size, vec![]);
    assert!(
        (first_shaft(&held) - initial - egui::Vec2::splat(15.3 * 16.0 / 27.68)).length() < 0.01
    );
    assert!(
        held.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_millis(80),
        "settled feedback must not animate forever"
    );
    frame(&mut app, size, vec![key(egui::Key::Escape)]);
    frame(&mut app, size, vec![pointer(target, false)]);
    for _ in 0..20 {
        frame(&mut app, size, vec![]);
    }
    let restored = frame(&mut app, size, vec![]);
    assert!((first_shaft(&restored) - initial).length() < 0.01);
    assert!(
        restored.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_millis(80)
    );
}

#[test]
fn logo_corners_follow_the_supplied_round_outline_at_button_and_drop_guide_sizes() {
    for density in [1.0, 1.25, 2.0] {
        for size in [16.0, 48.0] {
            let context = egui::Context::default();
            context.set_pixels_per_point(density);
            let rect = egui::Rect::from_min_size(egui::pos2(12.0, 12.0), egui::Vec2::splat(size));
            let output = context.run_ui(Default::default(), |ui| {
                chrome::paint_logo(ui.painter(), rect, None, 0.0, true);
            });
            let corners: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| {
                    if let egui::Shape::Path(path) = &shape.shape {
                        Some(path)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(corners.len(), 4);
            let scale = size / 27.68;
            for (path, center) in corners.into_iter().zip([
                egui::pos2(5.0, 5.0),
                egui::pos2(22.68, 5.0),
                egui::pos2(22.68, 22.68),
                egui::pos2(5.0, 22.68),
            ]) {
                let center = rect.min + center.to_vec2() * scale;
                // The supplied SVG uses radius-four rounded corners. Check segment
                // midpoints too: vertices on the arc alone also admit visible bevels.
                let arc = &path.points[1..path.points.len() - 1];
                for segment in arc.windows(2) {
                    for point in [segment[0], segment[0].lerp(segment[1], 0.5), segment[1]] {
                        let error = ((point - center).length() - 4.0 * scale).abs() * density;
                        assert!(
                            error < 0.075,
                            "rounded outline error {error} px at {size} / {density}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn logo_feedback_preserves_geometry_and_only_highlights_the_selected_arrow() {
    for density in [1.0, 1.25, 2.0] {
        let context = egui::Context::default();
        context.set_pixels_per_point(density);
        let idle = context.run_ui(Default::default(), |ui| {
            chrome::logo(
                ui,
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(28.0, 26.0)),
                None,
                0.0,
                false,
            );
        });
        for shape in idle.shapes {
            match shape.shape {
                egui::Shape::LineSegment { stroke, .. } => assert_eq!(stroke.color, chrome::MUTED),
                egui::Shape::Path(path) => assert_eq!(
                    path.stroke.color,
                    egui::epaint::ColorMode::Solid(chrome::MUTED)
                ),
                _ => {}
            }
        }
        for selected in [
            None,
            Some(Section::File),
            Some(Section::Edit),
            Some(Section::View),
        ] {
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(28.0, 26.0));
            let output = context.run_ui(Default::default(), |ui| {
                chrome::logo(
                    ui,
                    rect,
                    selected,
                    if selected.is_some() { 1.0 } else { 0.0 },
                    true,
                )
            });
            let mut opaque = 0;
            let mut shafts = Vec::new();
            let mut shapes = 0;
            for shape in output.shapes {
                match shape.shape {
                    egui::Shape::LineSegment { points, stroke } => {
                        shapes += 1;
                        opaque += usize::from(stroke.color == chrome::FOREGROUND);
                        assert!(points.into_iter().all(|point| rect.contains(point)));
                        shafts.push(points);
                    }
                    egui::Shape::Path(path) => {
                        shapes += 1;
                        opaque += usize::from(
                            path.stroke.color == egui::epaint::ColorMode::Solid(chrome::FOREGROUND),
                        );
                        assert!(path.points.into_iter().all(|point| rect.contains(point)));
                    }
                    _ => {}
                }
            }
            assert_eq!(shapes, 7);
            assert_eq!(opaque, if selected.is_some() { 2 } else { 7 });
            assert_eq!(shafts[0][0].x > rect.center().x, selected.is_some());
        }
    }
}

pub(crate) fn hardware_round_trip<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
) {
    let history = app.edits.clone();
    let state = app.state;
    let generation = app.media_generation;
    for (delta, expected) in [
        (egui::vec2(24.0, -12.0), "Open file "),
        (egui::vec2(24.0, 24.0), "Undo "),
        (egui::vec2(-12.0, 24.0), "Toggle fullscreen "),
    ] {
        let tree = crate::video_rotation::tests::frame(app, vec![]);
        let bounds = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("towavue menu"))
            .expect("logo")
            .1
            .bounds()
            .expect("bounds");
        let origin = egui::pos2(
            (bounds.x0 + bounds.x1) as f32 * 0.5,
            (bounds.y0 + bounds.y1) as f32 * 0.5,
        );
        let target = origin + delta;
        crate::video_rotation::tests::frame(
            app,
            vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
        );
        crate::video_rotation::tests::frame(app, vec![egui::Event::PointerMoved(target)]);
        for _ in 0..6 {
            crate::video_rotation::tests::frame(app, vec![]);
        }
        crate::video_rotation::tests::frame(app, vec![pointer(target, false)]);
        for _ in 0..3 {
            crate::video_rotation::tests::frame(app, vec![]);
        }
        let tree = crate::video_rotation::tests::frame(app, vec![]);
        assert!(
            tree.nodes.iter().any(|(_, node)| node
                .label()
                .is_some_and(|label| label.starts_with(expected))),
            "hardware {expected}; origin={origin:?} target={target:?} labels={:?}",
            tree.nodes
                .iter()
                .filter_map(|(_, node)| node.label())
                .collect::<Vec<_>>()
        );
        crate::video_rotation::tests::frame(app, vec![key(egui::Key::Escape)]);
        crate::video_rotation::tests::frame(app, vec![key(egui::Key::Escape)]);
        assert!(!egui::Popup::is_any_open(
            app.ui_context.as_ref().expect("context")
        ));
    }
    assert_eq!(app.edits, history);
    assert_eq!(app.state, state);
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
        "PASS hardware logo gesture: three directions, held feedback, submenu/Escape, unchanged history/transport and CPU transfers 0"
    );
}

#[test]
fn choice_submenus_from_logo_click_and_drag_apply_once_without_inheriting_parent_geometry() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::choice_submenus_from_logo_click_and_drag_apply_once_without_inheriting_parent_geometry",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for drag in [false, true] {
            for edit in [false, true] {
                let (mut app, origin) = setup(&root);
                app.ui_context
                    .as_ref()
                    .expect("context")
                    .set_pixels_per_point(density);
                let size = egui::vec2(640.0, 480.0);
                let end = origin
                    + if drag {
                        if edit {
                            egui::vec2(24.0, 24.0)
                        } else {
                            egui::vec2(-12.0, 24.0)
                        }
                    } else {
                        egui::Vec2::ZERO
                    };
                frame(
                    &mut app,
                    size,
                    vec![pointer(origin, true), pointer(end, false)],
                );
                // Subsequent actions use accessibility focus, without leaving a
                // stationary mouse over a different entry in the scrolled menu.
                frame(&mut app, size, vec![egui::Event::PointerGone]);
                let title = if edit {
                    "Listening volume step"
                } else {
                    "Folder navigation"
                };
                let labels = if drag {
                    vec![title]
                } else {
                    vec![if edit { "Edit" } else { "View" }, title]
                };
                for label in labels {
                    for _ in 0..20 {
                        frame(&mut app, size, vec![]);
                    }
                    let tree = frame(&mut app, size, vec![])
                        .platform_output
                        .accesskit_update
                        .expect("menu tree");
                    let id = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| {
                            node.label().is_some_and(|text| {
                                text.trim_end_matches('\u{23f5}').trim() == label
                            })
                        })
                        .expect("submenu trigger")
                        .0;
                    frame(
                        &mut app,
                        size,
                        vec![egui::Event::AccessKitActionRequest(
                            egui::accesskit::ActionRequest {
                                action: egui::accesskit::Action::Focus,
                                target_tree: egui::accesskit::TreeId::ROOT,
                                target_node: id,
                                data: None,
                            },
                        )],
                    );
                    for _ in 0..20 {
                        frame(&mut app, size, vec![]);
                    }
                    let before = frame(&mut app, size, vec![]);
                    let trigger = before
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("visible submenu trigger")
                        .nodes
                        .iter()
                        .find(|(node_id, _)| *node_id == id)
                        .expect("visible submenu trigger");
                    assert!(before.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().starts_with(label) && shape.clip_rect.contains(text.pos))), "trigger is visible: label={label}, drag={drag}, edit={edit}, bounds={:?}, focus={:?}", trigger.1.bounds(), before.platform_output.accesskit_update.as_ref().expect("visible submenu trigger").focus);
                    frame(&mut app, size, vec![access(id, None)]);
                }
                for _ in 0..20 {
                    frame(&mut app, size, vec![]);
                }
                let output = frame(&mut app, size, vec![]);
                let bounds = popup_bounds(&output);
                assert!(
                    bounds.len() >= 2,
                    "parent and choice menus remain visible: density={density}, drag={drag}, edit={edit}, bounds={bounds:?}, labels={:?}",
                    output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("tree")
                        .nodes
                        .iter()
                        .filter_map(|(_, node)| node.label())
                        .collect::<Vec<_>>()
                );
                for pair in bounds.windows(2) {
                    assert!(
                        pair[0].right() <= pair[1].left(),
                        "menu frames cannot overlap: {bounds:?}"
                    );
                }
                let tree = output.platform_output.accesskit_update.expect("choices");
                let selected = if edit { "5%" } else { "Stop at ends" };
                let row = tree.nodes.iter().find(|(_, node)| node.label() == Some(selected)).unwrap_or_else(|| panic!("choice {selected} is visible: density={density}, drag={drag}, edit={edit}")).1.bounds().expect("choice bounds");
                let center = egui::pos2(
                    (row.x0 + row.x1) as f32 * 0.5,
                    (row.y0 + row.y1) as f32 * 0.5,
                );
                let child = bounds
                    .iter()
                    .find(|bounds| bounds.contains(center))
                    .unwrap_or_else(|| panic!("choice popup frame: density={density}, drag={drag}, edit={edit}, center={center:?}, row={row:?}, frames={bounds:?}"));
                assert!(
                    child.width() < 180.0 && child.height() < 100.0,
                    "choice popup sizes from its own two or three rows: {child:?}"
                );
                frame(&mut app, size, vec![access(node(&tree, selected), None)]);
                assert_eq!(app.volume_step_percent, if edit { 5 } else { 2 });
                assert_eq!(app.folder_navigation_loop, edit);
                for _ in 0..3 {
                    frame(&mut app, size, vec![]);
                }
                assert_eq!(app.volume_step_percent, if edit { 5 } else { 2 });
                assert_eq!(app.folder_navigation_loop, edit);
            }
        }
    }
}
