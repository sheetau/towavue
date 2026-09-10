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
fn logo_direction_threshold_and_sector_boundaries_match_the_three_arrows() {
    for (delta, expected) in [
        (egui::vec2(7.99, 0.0), None),
        (egui::vec2(8.0, 0.0), Some(Section::Edit)),
        (egui::vec2(0.0, 8.0), Some(Section::Edit)),
        (egui::vec2(0.0, -8.0), Some(Section::File)),
        (egui::vec2(-8.0, 0.0), Some(Section::View)),
        (egui::vec2(-8.0, -8.0), None),
        (egui::vec2(10.0, -5.0), Some(Section::File)),
        (egui::vec2(-5.0, 10.0), Some(Section::View)),
    ] {
        assert_eq!(direction(delta), expected, "{delta:?}");
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
fn logo_drag_cancellation_never_replays_a_click_and_plain_uia_click_still_opens_root() {
    let Some(root) = crate::tests::isolated_test_root(
        "logo_menu::tests::logo_drag_cancellation_never_replays_a_click_and_plain_uia_click_still_opens_root",
    ) else {
        return;
    };
    for mode in 0..15 {
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
            _ => vec![],
        };
        frame(
            &mut app,
            if mode == 9 {
                egui::vec2(480.0, 480.0)
            } else {
                size
            },
            events,
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
fn logo_feedback_preserves_geometry_and_only_highlights_the_selected_arrow() {
    for density in [1.0, 1.25, 2.0] {
        let context = egui::Context::default();
        context.set_pixels_per_point(density);
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
