use crate::audio_export::tests::frame;
use crate::video_rotation::tests::node;
use crate::*;

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn action(target: egui::accesskit::NodeId, action: egui::accesskit::Action) -> egui::Event {
    egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: target,
        data: None,
    })
}

fn setup(root: &Path) -> Application<fn(AppEvent)> {
    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
    let context = fonts::test_context();
    context.enable_accesskit();
    context.global_style_mut(chrome::style);
    app.ui_context = Some(context);
    for name in ["first.png", "second.png", "third.png"] {
        app.tabs.open_new(root.join(name), MediaKind::Image);
    }
    app.path = Some(root.join("third.png"));
    app.media_kind = Some(MediaKind::Image);
    app
}

fn tree(
    app: &mut Application<fn(AppEvent)>,
    events: Vec<egui::Event>,
) -> egui::accesskit::TreeUpdate {
    frame(app, egui::vec2(640.0, 480.0), events)
        .platform_output
        .accesskit_update
        .expect("tree")
}

fn settle(app: &mut Application<fn(AppEvent)>) -> egui::accesskit::TreeUpdate {
    tree(app, vec![]);
    tree(app, vec![]);
    tree(app, vec![])
}

#[test]
fn tab_context_keyboard_and_accessibility_open_the_focused_tab_and_escape_restores_origin() {
    let Some(root) = tests::isolated_test_root(
        "tab_menu::keyboard_tests::tab_context_keyboard_and_accessibility_open_the_focused_tab_and_escape_restores_origin",
    ) else {
        return;
    };
    for label in ["first.png", "Close tab: first.png"] {
        for mode in 0..3 {
            let mut app = setup(&root);
            let context = app.ui_context.clone().expect("context");
            let active = app.tabs.active().expect("active").id;
            let origin = node(&settle(&mut app), label);
            tree(
                &mut app,
                vec![action(origin, egui::accesskit::Action::Focus)],
            );
            settle(&mut app);
            let event = match mode {
                0 => key(egui::Key::F10, egui::Modifiers::SHIFT),
                1 => super::context_key_event(
                    &WinitKey::Named(NamedKey::ContextMenu),
                    ModifiersState::empty(),
                    true,
                    false,
                )
                .expect("native key mapping"),
                _ => action(origin, egui::accesskit::Action::ShowContextMenu),
            };
            tree(&mut app, vec![egui::Event::PointerGone, event]);
            let menu = settle(&mut app);
            assert!(egui::Popup::is_any_open(&context), "{label} mode {mode}");
            assert_eq!(app.tabs.active().expect("unchanged active").id, active);
            assert_eq!(app.tabs.tabs().len(), 3);
            assert!(app.pending_guard.is_none() && app.edits.is_empty());
            let left = menu
                .nodes
                .iter()
                .find(|(_, n)| {
                    n.label()
                        .is_some_and(|l| l.starts_with("Close tabs to the left"))
                })
                .expect("left action");
            assert!(
                left.1.is_disabled(),
                "target is first tab, not active third"
            );
            assert_eq!(
                menu.focus,
                menu.nodes
                    .iter()
                    .find(|(_, n)| n.label().is_some_and(|l| l.starts_with("Close tab ")))
                    .expect("close action")
                    .0
            );
            tree(
                &mut app,
                vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            assert!(!egui::Popup::is_any_open(&context));
            assert_eq!(settle(&mut app).focus, origin);
        }
    }
}

#[test]
fn tab_context_rejects_modified_repeated_and_covered_requests() {
    let Some(root) = tests::isolated_test_root(
        "tab_menu::keyboard_tests::tab_context_rejects_modified_repeated_and_covered_requests",
    ) else {
        return;
    };
    for modifiers in [
        ModifiersState::SHIFT,
        ModifiersState::CONTROL,
        ModifiersState::ALT,
        ModifiersState::SUPER,
    ] {
        assert!(
            super::context_key_event(
                &WinitKey::Named(NamedKey::ContextMenu),
                modifiers,
                true,
                false
            )
            .is_none()
        );
    }
    assert!(matches!(
        super::context_key_event(
            &WinitKey::Named(NamedKey::ContextMenu),
            ModifiersState::empty(),
            false,
            false
        ),
        Some(egui::Event::Key { pressed: false, .. })
    ));
    assert!(
        super::context_key_event(
            &WinitKey::Named(NamedKey::ContextMenu),
            ModifiersState::empty(),
            true,
            true
        )
        .is_none()
    );
    assert!(
        super::context_key_event(
            &WinitKey::Named(NamedKey::F10),
            ModifiersState::empty(),
            true,
            false
        )
        .is_none()
    );
    for mode in 0..10 {
        let mut app = setup(&root);
        let context = app.ui_context.clone().expect("context");
        let origin = node(&settle(&mut app), "first.png");
        tree(
            &mut app,
            vec![action(origin, egui::accesskit::Action::Focus)],
        );
        settle(&mut app);
        let mut event = key(egui::Key::F10, egui::Modifiers::SHIFT);
        match mode {
            0 => {
                tree(&mut app, vec![key(egui::Key::F10, egui::Modifiers::SHIFT)]);
                settle(&mut app);
                tree(
                    &mut app,
                    vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
                );
                settle(&mut app);
                if let egui::Event::Key { repeat, .. } = &mut event {
                    *repeat = true;
                }
            }
            1 => event = key(egui::Key::F10, egui::Modifiers::NONE),
            2 => app.palette_open = true,
            3 => app.grid_open = true,
            4 => app.filmstrip_open = true,
            5 => app.fullscreen = true,
            6 => app.pending_guard = Some(GuardedAction::Exit),
            7 => context.set_dragged_id("foreign-drag".into()),
            9 => egui::Popup::open_id(&context, "foreign-menu".into()),
            _ => {}
        }
        let mut events = vec![event];
        if mode == 8 {
            events.insert(0, egui::Event::WindowFocused(false));
        }
        tree(&mut app, events);
        if mode == 9 {
            assert!(egui::Popup::is_id_open(&context, "foreign-menu".into()));
        } else {
            assert!(!egui::Popup::is_any_open(&context), "mode {mode}");
        }
        if mode >= 2 && mode != 7 && mode != 8 {
            if mode == 9 {
                egui::Popup::open_id(&context, "foreign-menu".into());
            }
            tree(
                &mut app,
                vec![action(origin, egui::accesskit::Action::ShowContextMenu)],
            );
            if mode == 9 {
                assert!(egui::Popup::is_id_open(&context, "foreign-menu".into()));
            } else {
                assert!(!egui::Popup::is_any_open(&context));
            }
        }
        assert_eq!(app.tabs.tabs().len(), 3);
    }
}

#[test]
fn tab_context_enter_runs_the_target_close_once_through_the_dirty_guard() {
    let Some(root) = tests::isolated_test_root(
        "tab_menu::keyboard_tests::tab_context_enter_runs_the_target_close_once_through_the_dirty_guard",
    ) else {
        return;
    };
    let mut app = setup(&root);
    let target = app.tabs.tabs()[0].id;
    app.edits
        .entry(target)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    let origin = node(&settle(&mut app), "first.png *");
    tree(
        &mut app,
        vec![action(origin, egui::accesskit::Action::ShowContextMenu)],
    );
    settle(&mut app);
    tree(&mut app, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    assert_eq!(app.tabs.tabs().len(), 3);
    assert_eq!(app.tabs.active().expect("guard target").id, target);
    assert!(app.pending_guard.is_some());
    assert_eq!(app.edits[&target].operations().len(), 1);
    app.resolve_guard(GuardDecision::Cancel);
    assert_eq!(settle(&mut app).focus, origin);
    assert!(app.pending_guard.is_none());
    assert!(app.closed_tabs.is_empty());
    tree(
        &mut app,
        vec![action(origin, egui::accesskit::Action::ShowContextMenu)],
    );
    settle(&mut app);
    tree(&mut app, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    assert!(app.pending_guard.is_some());
    app.resolve_guard(GuardDecision::Discard);
    let returned = settle(&mut app);
    assert_eq!(app.tabs.tabs().len(), 2);
    assert_eq!(
        returned.focus,
        node(
            &returned,
            &display_name(app.tabs.active().expect("survivor").target.current_path())
        )
    );
}

#[test]
fn tab_context_right_click_keeps_pointer_anchor_and_keyboard_navigation_tracks_reordered_identity()
{
    let Some(root) = tests::isolated_test_root(
        "tab_menu::keyboard_tests::tab_context_right_click_keeps_pointer_anchor_and_keyboard_navigation_tracks_reordered_identity",
    ) else {
        return;
    };
    let mut app = setup(&root);
    let active = app.tabs.active().expect("active").id;
    let initial = settle(&mut app);
    let origin = node(&initial, "first.png");
    assert!(
        initial
            .nodes
            .iter()
            .find(|(id, _)| *id == origin)
            .expect("tab")
            .1
            .supports_action(egui::accesskit::Action::ShowContextMenu)
    );
    let bounds = initial
        .nodes
        .iter()
        .find(|(id, _)| *id == origin)
        .expect("tab")
        .1
        .bounds()
        .expect("bounds");
    let position = egui::pos2(
        (bounds.x0 + bounds.x1) as f32 * 0.5,
        (bounds.y0 + bounds.y1) as f32 * 0.5,
    );
    let pointer = |pressed| egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    tree(
        &mut app,
        vec![egui::Event::PointerMoved(position), pointer(true)],
    );
    tree(&mut app, vec![pointer(false)]);
    let menu = settle(&mut app);
    assert_eq!(app.tabs.active().expect("unchanged active").id, active);
    let bounds = menu
        .nodes
        .iter()
        .find(|(_, n)| n.label().is_some_and(|l| l.starts_with("Close tab ")))
        .expect("close")
        .1
        .bounds()
        .expect("bounds");
    assert!((bounds.x0 as f32 - position.x).abs() < 20.0);
    assert!((bounds.y0 as f32 - position.y).abs() < 20.0);
    tree(
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert_eq!(settle(&mut app).focus, origin);
    let target = app.tabs.tabs()[0].id;
    app.tabs.reorder(target, 3);
    settle(&mut app);
    tree(
        &mut app,
        vec![
            egui::Event::PointerGone,
            key(egui::Key::F10, egui::Modifiers::SHIFT),
        ],
    );
    let menu = settle(&mut app);
    assert!(
        menu.nodes
            .iter()
            .find(|(_, n)| n
                .label()
                .is_some_and(|l| l.starts_with("Close tabs to the right")))
            .expect("right")
            .1
            .is_disabled()
    );
    for expected in [
        "Close other tabs",
        "Close tabs to the left",
        "Close all tabs",
        "Copy file path",
    ] {
        let menu = tree(
            &mut app,
            vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
        );
        let selected = menu
            .nodes
            .iter()
            .find(|(id, _)| *id == menu.focus)
            .expect("focused command");
        assert!(
            selected.1.label().is_some_and(|l| l.starts_with(expected)),
            "expected {expected}: {:?}",
            selected.1.label()
        );
    }
    tree(
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert_eq!(settle(&mut app).focus, origin);
    assert_eq!(app.tabs.active().expect("active").id, active);
    assert!(app.pending_guard.is_none() && app.closed_tabs.is_empty());
}

#[test]
fn tab_context_close_restores_a_surviving_tab_or_welcome_and_compact_menus_stay_visible() {
    let Some(root) = tests::isolated_test_root(
        "tab_menu::keyboard_tests::tab_context_close_restores_a_surviving_tab_or_welcome_and_compact_menus_stay_visible",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for width in [240.0, 480.0, 960.0] {
            let mut app = setup(&root);
            let context = app.ui_context.clone().expect("context");
            context.set_pixels_per_point(density);
            let size = egui::vec2(width, 360.0);
            for _ in 0..3 {
                frame(&mut app, size, vec![]);
            }
            let output = frame(&mut app, size, vec![]);
            let initial = output.platform_output.accesskit_update.expect("tree");
            let origin = node(&initial, "third.png");
            frame(
                &mut app,
                size,
                vec![action(origin, egui::accesskit::Action::ShowContextMenu)],
            );
            for _ in 0..3 {
                frame(&mut app, size, vec![]);
            }
            let output = frame(&mut app, size, vec![]);
            let menu = output.platform_output.accesskit_update.expect("menu");
            let close = menu
                .nodes
                .iter()
                .find(|(_, n)| n.label().is_some_and(|l| l.starts_with("Close tab ")))
                .expect("close command");
            let bounds = close.1.bounds().expect("bounds");
            assert!(
                bounds.x0 >= 0.0
                    && bounds.x1 <= (width * density) as f64 + 1.0
                    && bounds.y0 >= 0.0
                    && bounds.y1 <= 360.0 * density as f64,
                "{bounds:?} at {width}/{density}"
            );
            frame(
                &mut app,
                size,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            );
            assert_eq!(app.tabs.tabs().len(), 2);
            for _ in 0..3 {
                frame(&mut app, size, vec![]);
            }
            let menu = frame(&mut app, size, vec![])
                .platform_output
                .accesskit_update
                .expect("tree");
            let active = app.tabs.active().expect("surviving tab");
            assert_eq!(
                menu.focus,
                node(&menu, &display_name(active.target.current_path()))
            );
        }
    }
    let mut app = setup(&root);
    let origin = node(&settle(&mut app), "third.png");
    tree(
        &mut app,
        vec![action(origin, egui::accesskit::Action::ShowContextMenu)],
    );
    let menu = settle(&mut app);
    let all = menu
        .nodes
        .iter()
        .find(|(_, n)| n.label().is_some_and(|l| l.starts_with("Close all tabs")))
        .expect("close all")
        .0;
    tree(&mut app, vec![action(all, egui::accesskit::Action::Click)]);
    let welcome = settle(&mut app);
    assert!(app.tabs.tabs().is_empty());
    assert_eq!(welcome.focus, node(&welcome, "Welcome tab"));
}

pub(crate) fn hardware_round_trip<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
) {
    use crate::video_rotation::tests::frame;
    let history = app.edits.clone();
    let state = app.state;
    let generation = app.media_generation;
    let tab = app.tabs.active().expect("tab");
    let active = tab.id;
    let name = display_name(tab.target.current_path());
    let label = format!(
        "{name}{}",
        if app.edits.get(&active).is_some_and(EditHistory::is_dirty) {
            " *"
        } else {
            ""
        }
    );
    for label in [label, format!("Close tab: {name}")] {
        for mode in 0..3 {
            let origin = node(&frame(app, vec![]), &label);
            frame(app, vec![action(origin, egui::accesskit::Action::Focus)]);
            frame(app, vec![]);
            let event = match mode {
                0 => key(egui::Key::F10, egui::Modifiers::SHIFT),
                1 => super::context_key_event(
                    &WinitKey::Named(NamedKey::ContextMenu),
                    ModifiersState::empty(),
                    true,
                    false,
                )
                .expect("native mapping"),
                _ => action(origin, egui::accesskit::Action::ShowContextMenu),
            };
            frame(app, vec![egui::Event::PointerGone, event]);
            frame(
                app,
                vec![
                    super::context_key_event(
                        &WinitKey::Named(NamedKey::ContextMenu),
                        ModifiersState::empty(),
                        false,
                        false,
                    )
                    .expect("release"),
                ],
            );
            frame(app, vec![]);
            let menu = frame(app, vec![]);
            assert!(
                menu.nodes
                    .iter()
                    .any(|(_, n)| n.label().is_some_and(|l| l.starts_with("Close tab ")))
            );
            frame(app, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
            let tree = frame(app, vec![]);
            assert_eq!(tree.focus, origin);
            assert!(!egui::Popup::is_any_open(
                app.ui_context.as_ref().expect("context")
            ));
        }
    }
    assert_eq!(app.tabs.active().expect("tab").id, active);
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
        "PASS hardware tab context: keyboard/Menu key/UIA from tab and close, Escape focus, unchanged history/transport and CPU transfers 0"
    );
}
