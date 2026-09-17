use super::*;

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn frame(
    app: &mut Application<fn(AppEvent)>,
    size: egui::Vec2,
    focused: bool,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Vec<UiAction>) {
    let context = app.ui_context.clone().expect("context");
    let mut actions = Vec::new();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            focused,
            events,
            ..Default::default()
        },
        |ui| {
            tab_focus::begin(&context, app.tabs.active().map(|tab| tab.id), true);
            app.draw_top_bar(ui, &mut actions);
            let response = ui.interact(
                egui::Rect::from_min_size(egui::pos2(20.0, 200.0), egui::vec2(100.0, 20.0)),
                egui::Id::new("tab-drag-numeric-focus"),
                egui::Sense::focusable_noninteractive(),
            );
            assert_eq!(
                seekbar::value_input(
                    &response,
                    "Playback position (seconds)",
                    25.0,
                    0.0..=100.0,
                    5.0,
                    true,
                ),
                None,
                "tab dragging must not edit a numeric control"
            );
            tab_focus::finish(&context, false, true);
        },
    );
    (output, actions)
}

fn setup(root: &Path) -> Application<fn(AppEvent)> {
    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
    let context = fonts::test_context();
    context.global_style_mut(chrome::style);
    context.enable_accesskit();
    app.ui_context = Some(context);
    for name in ["a.png", "b.png", "c.png"] {
        app.tabs.open_new(root.join(name), MediaKind::Image);
    }
    app.tabs
        .close_gallery(app.tabs.gallery().expect("media-only fixture"));
    app.path = Some(root.join("c.png"));
    app.media_kind = Some(MediaKind::Image);
    for _ in 0..3 {
        frame(&mut app, egui::vec2(960.0, 576.0), true, vec![]);
    }
    app
}

fn state<N>(app: &Application<N>) -> State {
    app.ui_context
        .as_ref()
        .expect("context")
        .data(|data| data.get_temp::<State>(state_id()))
        .expect("state")
}

pub(crate) fn drop_point(context: &egui::Context, gap: usize) -> egui::Pos2 {
    let layout = context
        .data(|data| data.get_temp::<DropStrip>("incoming-tab-strip".into()))
        .expect("strip");
    let x = layout
        .rectangles
        .get(gap)
        .map_or(layout.strip.right() - 4.0, |rect| rect.left() + 4.0);
    egui::pos2(
        x.clamp(layout.strip.left() + 2.0, layout.strip.right() - 2.0),
        layout.strip.center().y,
    )
}

pub(crate) fn label_center<N>(app: &Application<N>, tab: TabId) -> egui::Pos2 {
    state(app)
        .widgets
        .iter()
        .find(|(id, _, _)| *id == tab)
        .expect("tab")
        .2
        .center()
}

#[test]
fn middle_tab_close_releases_numeric_focus_without_activating_or_dragging() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::middle_tab_close_releases_numeric_focus_without_activating_or_dragging",
    ) else {
        return;
    };
    let size = egui::vec2(960.0, 576.0);
    for density in [1.0, 1.25, 2.0] {
        for index in [0, 2] {
            for batched in [false, true] {
                let mut app = setup(&root);
                let context = app.ui_context.clone().expect("context");
                context.set_pixels_per_point(density);
                for _ in 0..3 {
                    frame(&mut app, size, true, vec![]);
                }
                let tab = app.tabs.tabs()[index].id;
                let active = app.tabs.active().expect("active").id;
                let other = app.tabs.tabs()[1].id;
                let saved = egui::Id::new("background-value");
                tab_focus::adopt(&context, other, saved);
                let point = label_center(&app, tab);
                let outside = point + egui::vec2(0.0, 100.0);
                let middle = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Middle,
                    modifiers: egui::Modifiers::NONE,
                };
                for cancelled in [true, false] {
                    let value = egui::Id::new("tab-drag-numeric-focus");
                    context.memory_mut(|memory| memory.request_focus(value));
                    frame(&mut app, size, true, vec![egui::Event::PointerMoved(point)]);
                    assert!(
                        context.memory(|memory| memory.has_focus(value)),
                        "hover preserves focus"
                    );
                    let end = if cancelled { outside } else { point };
                    let mut events = vec![middle(point, true)];
                    if !batched {
                        assert!(frame(&mut app, size, true, events).1.is_empty());
                        assert_eq!(
                            context.memory(|memory| memory.focused()),
                            None,
                            "middle press releases numeric focus before a close or cancellation"
                        );
                        events = vec![];
                    }
                    events.extend([egui::Event::PointerMoved(end), middle(end, false)]);
                    let (_, actions) = frame(&mut app, size, true, events);
                    assert_eq!(
                        context.memory(|memory| memory.focused()),
                        None,
                        "density={density}, index={index}, batched={batched}, cancelled={cancelled}"
                    );
                    assert!(tab_focus::take(&context, active).is_none());
                    assert_eq!(app.tabs.active().expect("active unchanged").id, active);
                    assert!(state(&app).drag.is_none());
                    if cancelled {
                        assert!(actions.is_empty());
                    } else {
                        assert!(actions == vec![UiAction::CloseTab(tab)]);
                        // A middle close uses the same dirty guard as the close button.
                        app.edits
                            .entry(tab)
                            .or_default()
                            .push(EditOperation::RotateClockwise, MediaKind::Image);
                        let tabs = app.tabs.tabs().to_vec();
                        let history = app.edits[&tab].clone();
                        app.handle_ui_action(actions[0].clone());
                        assert!(app.pending_guard.is_some());
                        assert_eq!(app.tabs.tabs(), tabs);
                        assert_eq!(app.edits[&tab], history);
                    }
                }
                assert_eq!(tab_focus::take(&context, other), Some(saved));
            }
        }
    }
}

#[test]
fn tab_close_returns_focus_without_closing_on_cancel_or_bypassing_dirty_guards() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::tab_close_returns_focus_without_closing_on_cancel_or_bypassing_dirty_guards",
    ) else {
        return;
    };
    let size = egui::vec2(960.0, 576.0);
    for density in [1.0, 1.25, 2.0] {
        for index in [0, 2] {
            let mut app = setup(&root);
            let context = app.ui_context.clone().expect("context");
            context.set_pixels_per_point(density);
            let tab = app.tabs.tabs()[index].id;
            let active = app.tabs.active().expect("active").id;
            app.edits
                .entry(tab)
                .or_default()
                .push(EditOperation::RotateClockwise, MediaKind::Image);
            for _ in 0..3 {
                frame(&mut app, size, true, vec![]);
            }
            let (output, _) = frame(&mut app, size, true, vec![]);
            let label = format!("Close tab: {}", if index == 0 { "a.png" } else { "c.png" });
            let tree = output.platform_output.accesskit_update.expect("tree");
            let (id, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(&label))
                .expect("close button");
            let bounds = node.bounds().expect("bounds");
            let point = egui::pos2(
                (bounds.x0 + bounds.x1) as f32 / 2.0,
                (bounds.y0 + bounds.y1) as f32 / 2.0,
            );
            let outside = point + egui::vec2(0.0, 80.0);
            let original = app.tabs.clone();
            for batched in [false, true] {
                let value = egui::Id::new("tab-drag-numeric-focus");
                context.memory_mut(|memory| memory.request_focus(value));
                frame(&mut app, size, true, vec![egui::Event::PointerMoved(point)]);
                assert!(
                    context.memory(|memory| memory.has_focus(value)),
                    "hover keeps numeric focus"
                );
                let mut events = vec![pointer(point, true)];
                if batched {
                    events.extend([egui::Event::PointerMoved(outside), pointer(outside, false)]);
                }
                assert!(frame(&mut app, size, true, events).1.is_empty());
                assert_eq!(
                    context.memory(|memory| memory.focused()),
                    None,
                    "close press/cancel releases numeric focus: batched={batched}"
                );
                if !batched {
                    assert!(
                        frame(
                            &mut app,
                            size,
                            true,
                            vec![egui::Event::PointerMoved(outside), pointer(outside, false)]
                        )
                        .1
                        .is_empty()
                    );
                }
                assert_eq!(context.memory(|memory| memory.focused()), None);
                assert_eq!(tab_focus::take(&context, active), None);
                assert_eq!(app.tabs, original);
                assert!(
                    state(&app).drag.is_none(),
                    "close never starts tab dragging"
                );
            }
            let (_, actions) = frame(
                &mut app,
                size,
                true,
                vec![
                    egui::Event::PointerMoved(point),
                    pointer(point, true),
                    pointer(point, false),
                ],
            );
            assert!(actions == vec![UiAction::CloseTab(tab)]);
            assert_eq!(context.memory(|memory| memory.focused()), None);
            assert!(frame(&mut app, size, true, vec![]).1.is_empty());
            frame(
                &mut app,
                size,
                true,
                vec![egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Focus,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: *id,
                        data: None,
                    },
                )],
            );
            let focused = context
                .memory(|memory| memory.focused())
                .expect("explicit close-button focus");
            let (_, actions) = frame(
                &mut app,
                size,
                true,
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(actions == vec![UiAction::CloseTab(tab)]);
            assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
            let history = app.edits[&tab].clone();
            app.handle_ui_action(actions[0].clone());
            assert!(app.pending_guard.is_some());
            assert_eq!(
                app.tabs.tabs(),
                original.tabs(),
                "dirty close waits for the existing guard"
            );
            assert_eq!(app.tabs.active().expect("guard target").id, tab);
            assert_eq!(app.edits[&tab], history);
        }
    }
}

#[test]
fn detached_tabs_keep_the_grab_offset_in_the_first_slot() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::detached_tabs_keep_the_grab_offset_in_the_first_slot",
    ) else {
        return;
    };
    for width in [480.0, 960.0, 1440.0] {
        for density in [1.0, 1.25, 2.0] {
            for index in 0..3 {
                let mut app = setup(&root);
                app.ui_context
                    .as_ref()
                    .expect("context")
                    .set_pixels_per_point(density);
                let size = egui::vec2(width, 576.0);
                for _ in 0..3 {
                    frame(&mut app, size, true, vec![]);
                }
                let layout = state(&app);
                let (tab, _, rect) = layout.widgets[index];
                let strip = layout.strip.expect("strip");
                let start = rect.intersect(strip).center();
                let anchor = strip.min.to_vec2() + (start - rect.min);
                let outside = egui::pos2(width + 80.0, 110.0);
                let tabs = app.tabs.clone();
                frame(&mut app, size, true, vec![pointer(start, true)]);
                let (_, actions) = frame(
                    &mut app,
                    size,
                    true,
                    vec![egui::Event::PointerMoved(outside), pointer(outside, false)],
                );
                assert!(
                    actions == vec![UiAction::DropTab(tab, outside, anchor)],
                    "tab {index} at width {width}, density {density}"
                );
                assert_eq!(app.tabs, tabs);
                assert!(frame(&mut app, size, true, vec![]).1.is_empty());
            }
        }
    }
}

#[test]
fn incoming_tabs_show_clipped_gaps_and_reject_stale_layouts_without_activation() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::incoming_tabs_show_clipped_gaps_and_reject_stale_layouts_without_activation",
    ) else {
        return;
    };
    for width in [480.0, 960.0, 1440.0] {
        for density in [1.0, 1.25, 2.0] {
            let mut app = setup(&root);
            let context = app.ui_context.clone().expect("context");
            context.set_pixels_per_point(density);
            let size = egui::vec2(width, 576.0);
            for _ in 0..3 {
                frame(&mut app, size, false, vec![]);
            }
            let original = app.tabs.clone();
            let ids: Vec<_> = original.tabs().iter().map(|tab| tab.id).collect();
            for gap in 0..=ids.len() {
                let point = drop_point(&context, gap);
                app.incoming_tab_pointer = Some(point);
                let (output, actions) = frame(&mut app, size, false, vec![]);
                assert!(actions.is_empty());
                assert_eq!(incoming_gap(&context, &ids, point), Some(gap));
                let layout = context
                    .data(|data| data.get_temp::<DropStrip>("incoming-tab-strip".into()))
                    .expect("layout");
                let x = layout.gap(point).expect("gap").1;
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::LineSegment { points, stroke }
                    if *points == [egui::pos2(x, layout.strip.top()), egui::pos2(x, layout.strip.bottom())]
                        && stroke.width == 2.0 && stroke.color == chrome::FOREGROUND)));
                for y in [100.0, 300.0, 575.0] {
                    let body = egui::pos2(point.x, y);
                    assert_eq!(incoming_gap(&context, &ids, body), None);
                    assert_eq!(incoming_filmstrip_gap(&context, &ids, body), Some(gap));
                    app.incoming_tab_pointer = Some(body);
                    let (output, actions) = frame(&mut app, size, false, vec![]);
                    assert!(actions.is_empty());
                    let body_layout = context
                        .data(|data| data.get_temp::<DropStrip>("incoming-tab-strip".into()))
                        .expect("body layout");
                    let (body_gap, x) = body_layout.gap(body).expect("body gap");
                    assert_eq!(body_gap, gap);
                    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::LineSegment { points, stroke }
                        if *points == [egui::pos2(x, body_layout.strip.top()), egui::pos2(x, body_layout.strip.bottom())]
                            && stroke.width == 2.0 && stroke.color == chrome::FOREGROUND)));
                }
            }
            assert_eq!(app.tabs, original);
            let point = drop_point(&context, 1);
            assert!(incoming_gap(&context, &ids[..1], point).is_none());
            context.set_pixels_per_point(density + 0.5);
            // Density changes apply on the following pass; an undrawn strip stays stale.
            for _ in 0..2 {
                let _ = context.run_ui(egui::RawInput::default(), |_| {});
            }
            assert!(incoming_gap(&context, &ids, point).is_none());
        }
    }
}

#[test]
fn incoming_tabs_append_in_unused_toolbar_space_without_expanding_native_controls() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::incoming_tabs_append_in_unused_toolbar_space_without_expanding_native_controls",
    ) else {
        return;
    };
    for width in [960.0, 1440.0] {
        for density in [1.0, 1.25, 2.0] {
            let mut app = setup(&root);
            let context = app.ui_context.clone().expect("context");
            context.set_pixels_per_point(density);
            for _ in 0..3 {
                frame(&mut app, egui::vec2(width, 576.0), false, vec![]);
            }
            let original = app.tabs.clone();
            let ids: Vec<_> = original.tabs().iter().map(|tab| tab.id).collect();
            let layout = context
                .data(|data| data.get_temp::<DropStrip>("incoming-tab-strip".into()))
                .expect("layout");
            let point = egui::pos2(layout.strip.right() + 12.0, layout.strip.center().y);
            assert_eq!(
                incoming_gap(&context, &ids, point),
                Some(ids.len()),
                "blank toolbar {width}/{density}"
            );
            app.incoming_tab_pointer = Some(point);
            let (output, actions) = frame(&mut app, egui::vec2(width, 576.0), false, vec![]);
            assert!(actions.is_empty());
            let x = layout.strip.right() - 1.0;
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::LineSegment { points, stroke }
                if *points == [egui::pos2(x, layout.strip.top()), egui::pos2(x, layout.strip.bottom())]
                    && stroke.width == 2.0 && stroke.color == chrome::FOREGROUND)));
            for y in [point.y, 300.0, 575.0] {
                assert_eq!(
                    incoming_gap(&context, &ids, egui::pos2(width - 1.0, y)),
                    if y == point.y { Some(ids.len()) } else { None }
                );
                assert_eq!(
                    incoming_gap(&context, &ids, egui::pos2(1.0, y)),
                    if y == point.y { Some(0) } else { None }
                );
                assert_eq!(
                    incoming_filmstrip_gap(&context, &ids, egui::pos2(1.0, y)),
                    Some(0)
                );
            }
            for outside in [
                egui::pos2(-1.0, 100.0),
                egui::pos2(width + 1.0, 100.0),
                egui::pos2(100.0, -1.0),
                egui::pos2(100.0, 577.0),
                egui::pos2(f32::NAN, 100.0),
            ] {
                assert!(incoming_gap(&context, &ids, outside).is_none());
            }
            assert_eq!(app.tabs, original);
        }
    }
}

#[test]
fn incoming_tabs_scroll_without_pointer_capture_and_accept_empty_welcome() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::incoming_tabs_scroll_without_pointer_capture_and_accept_empty_welcome",
    ) else {
        return;
    };
    let mut app = setup(&root);
    let size = egui::vec2(480.0, 576.0);
    let first = app.tabs.tabs()[0].id;
    for index in 0..14 {
        app.tabs
            .open_new(root.join(format!("incoming-{index}.png")), MediaKind::Image);
    }
    app.tabs.activate(first);
    for _ in 0..20 {
        frame(&mut app, size, false, vec![]);
    }
    let original = app.tabs.clone();
    let initial = state(&app);
    let strip = initial.strip.expect("strip");
    app.incoming_tab_pointer = Some(egui::pos2(strip.right() - 2.0, strip.center().y));
    for _ in 0..12 {
        assert!(frame(&mut app, size, false, vec![]).1.is_empty());
    }
    assert!(
        state(&app).widgets.last().expect("last").2.left()
            < initial.widgets.last().expect("last").2.left() - 20.0
    );
    app.incoming_tab_pointer = None;
    for _ in 0..2 {
        frame(&mut app, size, false, vec![]);
    }
    let stopped = state(&app).widgets.last().expect("last").2.left();
    app.incoming_tab_pointer = Some(egui::pos2(strip.right() - 2.0, 300.0));
    for _ in 0..3 {
        frame(&mut app, size, false, vec![]);
    }
    assert_eq!(
        state(&app).widgets.last().expect("last").2.left(),
        stopped,
        "body hover must not edge-scroll the strip"
    );
    assert_eq!(app.tabs, original);
    app.tabs = Default::default();
    for _ in 0..3 {
        frame(&mut app, size, false, vec![]);
    }
    let context = app.ui_context.as_ref().expect("context");
    let gallery = app.tabs.gallery().expect("default Gallery");
    assert_eq!(
        incoming_gap(context, &[gallery], drop_point(context, 0)),
        Some(0)
    );
    assert_eq!(
        incoming_gap(context, &[gallery], egui::pos2(240.0, 300.0)),
        None
    );
}

#[test]
fn tab_scrollbar_owns_drag_and_wheel_without_widening_on_hover() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::tab_scrollbar_owns_drag_and_wheel_without_widening_on_hover",
    ) else {
        return;
    };
    let size = egui::vec2(640.0, 576.0);
    for ((delta, modifiers), density) in [
        (egui::vec2(0.0, -80.0), egui::Modifiers::NONE),
        (egui::vec2(0.0, -80.0), egui::Modifiers::SHIFT),
        (egui::vec2(-80.0, 0.0), egui::Modifiers::NONE),
    ]
    .into_iter()
    .zip([1.0, 1.25, 2.0])
    {
        let mut app = setup(&root);
        let context = app.ui_context.clone().expect("context");
        let value_id = egui::Id::new("tab-scrollbar-numeric-focus");
        let frame = |app: &mut Application<fn(AppEvent)>, size, focused, events| {
            let mut actions = Vec::new();
            let mut value = None;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    focused,
                    events,
                    viewports: [(
                        egui::ViewportId::ROOT,
                        egui::ViewportInfo {
                            native_pixels_per_point: Some(density),
                            ..Default::default()
                        },
                    )]
                    .into_iter()
                    .collect(),
                    ..Default::default()
                },
                |ui| {
                    tab_focus::begin(&context, app.tabs.active().map(|tab| tab.id), true);
                    app.draw_top_bar(ui, &mut actions);
                    let response = ui.interact(
                        egui::Rect::from_min_size(egui::pos2(20.0, 200.0), egui::vec2(100.0, 20.0)),
                        value_id,
                        egui::Sense::focusable_noninteractive(),
                    );
                    value = seekbar::value_input(
                        &response,
                        "Playback position (seconds)",
                        25.0,
                        0.0..=100.0,
                        5.0,
                        true,
                    );
                    tab_focus::finish(&context, false, true);
                },
            );
            assert_eq!(
                value, None,
                "tab scrolling must not edit the numeric control"
            );
            (output, actions)
        };
        let first = app.tabs.tabs()[0].id;
        for index in 0..14 {
            app.tabs
                .open_new(root.join(format!("scroll-{index}.png")), MediaKind::Image);
        }
        app.tabs.activate(first);
        let tabs = app.tabs.clone();
        for _ in 0..30 {
            frame(&mut app, size, true, vec![]);
        }
        let (output, _) = frame(&mut app, size, true, vec![]);
        let bounds = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree")
            .nodes
            .iter()
            .find(|(_, node)| node.role() == egui::accesskit::Role::ScrollBar)
            .expect("tab scrollbar")
            .1
            .bounds()
            .expect("bar bounds");
        let grab = egui::pos2(
            bounds.x0 as f32 + 5.0,
            ((bounds.y0 + bounds.y1) / 2.0) as f32,
        );
        let painted_bar = |output: &egui::FullOutput| {
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.rect.width() > 10.0
                    && rect.rect.height() <= 2.01 && rect.fill.a() > 0
                    && rect.rect.top() >= bounds.y0 as f32 - 0.01
                    && rect.rect.bottom() <= bounds.y1 as f32 + 0.01)
            })
        };
        assert!(
            !painted_bar(&output),
            "tab scrollbar is hidden outside the strip"
        );
        let initial = state(&app).widgets;
        context.memory_mut(|memory| memory.request_focus(value_id));
        frame(&mut app, size, true, vec![egui::Event::PointerMoved(grab)]);
        assert!(
            context.memory(|memory| memory.has_focus(value_id)),
            "hover preserves explicit focus"
        );
        frame(&mut app, size, true, vec![pointer(grab, true)]);
        assert_eq!(
            context.memory(|memory| memory.focused()),
            None,
            "scrollbar press must release numeric focus at {density}x"
        );
        frame(
            &mut app,
            size,
            true,
            vec![egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(context.input(|input| input.key_pressed(egui::Key::ArrowRight)));
        let moved = grab + egui::vec2(40.0, 0.0);
        let (_, actions) = frame(&mut app, size, true, vec![egui::Event::PointerMoved(moved)]);
        assert!(
            state(&app).drag.is_none(),
            "scrollbar must not start a tab drag"
        );
        assert!(actions.is_empty());
        assert!(
            frame(&mut app, size, true, vec![pointer(moved, false)])
                .1
                .is_empty()
        );
        for _ in 0..30 {
            frame(&mut app, size, true, vec![]);
        }
        let (hovered, _) = frame(&mut app, size, true, vec![]);
        assert!(state(&app).widgets[0].2.left() < initial[0].2.left() - 20.0);
        let bar_height = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect)
                        if rect.rect.width() > 10.0
                            && rect.rect.height() <= 12.0
                            && rect.fill.a() > 0
                            && rect.rect.contains(moved) =>
                    {
                        Some(rect.rect.height())
                    }
                    _ => None,
                })
                .reduce(f32::max)
                .expect("visible scrollbar")
        };
        let hovered_height = bar_height(&hovered);
        assert!(painted_bar(&hovered));
        assert!(
            hovered_height <= 2.01,
            "tab scrollbar stays thin: {hovered_height}"
        );
        frame(
            &mut app,
            size,
            true,
            vec![egui::Event::PointerMoved(egui::pos2(320.0, 200.0))],
        );
        for _ in 0..30 {
            frame(&mut app, size, true, vec![]);
        }
        let (outside, _) = frame(&mut app, size, true, vec![]);
        assert!(
            !painted_bar(&outside),
            "leaving the strip hides its scrollbar again"
        );
        assert_eq!(outside.pixels_per_point, density);
        let strip = state(&app).strip.expect("strip");
        let wheel = strip.center();
        context.memory_mut(|memory| memory.request_focus(value_id));
        frame(&mut app, size, true, vec![egui::Event::PointerMoved(wheel)]);
        let before = state(&app).widgets[0].2.left();
        frame(
            &mut app,
            size,
            true,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta,
                phase: egui::TouchPhase::Move,
                modifiers,
            }],
        );
        for _ in 0..30 {
            frame(&mut app, size, true, vec![]);
        }
        assert!(
            state(&app).widgets[0].2.left() < before - 20.0,
            "wheel {delta:?}/{modifiers:?}"
        );
        assert!(state(&app).drag.is_none());
        assert_eq!(app.tabs, tabs);
        assert!(
            context.memory(|memory| memory.has_focus(value_id)),
            "wheel does not steal explicit focus"
        );
        let (_, actions) = frame(
            &mut app,
            size,
            true,
            vec![
                pointer(grab, true),
                egui::Event::PointerMoved(wheel),
                pointer(wheel, false),
            ],
        );
        assert!(
            actions.is_empty(),
            "batched scrollbar press never becomes a tab action"
        );
        assert!(state(&app).drag.is_none());
        assert_eq!(
            context.memory(|memory| memory.focused()),
            None,
            "batched scrollbar use releases numeric focus"
        );
        app.tabs.activate(app.tabs.tabs()[1].id);
        frame(&mut app, size, true, vec![]);
        app.tabs.activate(first);
        frame(&mut app, size, true, vec![]);
        assert_eq!(
            context.memory(|memory| memory.focused()),
            None,
            "tab return must not restore the numeric role after scrollbar use"
        );
    }
}

#[test]
fn filmstrip_allows_tab_reorder_and_detach_without_dismissing_the_overlay() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::filmstrip_allows_tab_reorder_and_detach_without_dismissing_the_overlay",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let mut app = setup(&root);
        let context = app.ui_context.clone().expect("context");
        context.set_pixels_per_point(density);
        app.filmstrip_open = true;
        let size = egui::vec2(960.0, 576.0);
        let frame = |app: &mut Application<_>, events| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            (output, actions)
        };
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let original = state(&app).widgets;
        let tabs = app.tabs.clone();
        let start = original[0].2.center();
        for target in [drop_point(&context, 3), egui::pos2(-20.0, 120.0)] {
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(start), pointer(start, true)],
            );
            let (_, actions) = frame(&mut app, vec![egui::Event::PointerMoved(target)]);
            assert!(actions.is_empty());
            assert!(state(&app).drag.as_ref().is_some_and(|drag| drag.crossed));
            assert!(
                active_pointer(
                    &context,
                    (
                        app.tabs.active().map(|tab| tab.id),
                        app.media_generation,
                        app.graphics_epoch
                    )
                )
                .is_some(),
                "host receives drag feedback"
            );
            assert_eq!(state(&app).widgets, original);
            assert_eq!(app.tabs, tabs, "order stays unchanged until release");
            let (_, actions) = frame(&mut app, vec![pointer(target, false)]);
            if target.x < 0.0 {
                assert!(
                    matches!(&actions[..], [UiAction::DropTab(tab, _, _)] if *tab == original[0].0)
                );
            } else {
                assert!(actions == [UiAction::ReorderTab(original[0].0, 3)]);
            }
            assert!(app.filmstrip_open);
            assert!(state(&app).drag.is_none());
            assert!(
                frame(&mut app, vec![]).1.is_empty(),
                "release is not replayed"
            );
        }
    }
}

#[test]
fn tab_drag_keeps_tab_geometry_fixed_until_release() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::tab_drag_keeps_tab_geometry_fixed_until_release",
    ) else {
        return;
    };
    let mut app = setup(&root);
    let size = egui::vec2(960.0, 576.0);
    let original = state(&app).widgets;
    let start = original[0].2.center();
    let target = egui::pos2(original[2].2.right() + 10.0, start.y);
    let tabs = app.tabs.clone();
    let context = app.ui_context.clone().expect("context");
    let value = egui::Id::new("tab-drag-numeric-focus");
    context.memory_mut(|memory| memory.request_focus(value));
    frame(&mut app, size, true, vec![egui::Event::PointerMoved(start)]);
    assert!(context.memory(|memory| memory.has_focus(value)));
    let background_role = egui::Id::new("background-tab-role");
    tab_focus::adopt(&context, original[0].0, background_role);
    frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    assert_eq!(
        context.memory(|memory| memory.focused()),
        None,
        "tab press must release the active media's numeric focus before a click or drag"
    );
    let (output, actions) = frame(
        &mut app,
        size,
        true,
        vec![
            egui::Event::PointerMoved(target),
            egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(context.input(|input| input.key_pressed(egui::Key::ArrowRight)));
    assert!(actions.is_empty());
    let dragging = state(&app);
    assert_eq!(dragging.drag.as_ref().expect("drag").tab, original[0].0);
    assert_eq!(dragging.widgets, original);
    assert_eq!(app.tabs, tabs);
    let tree = output.platform_output.accesskit_update.expect("tree");
    let label = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("a.png"))
        .expect("stable tab semantics");
    assert_eq!(
        label.1.bounds().expect("bounds").x0 as f32,
        original[0].2.left()
    );
    let over_media = target + egui::vec2(0.0, 90.0);
    let (output, actions) = frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(over_media)],
    );
    assert!(actions.is_empty());
    assert_eq!(state(&app).widgets, original);
    let label = original[0].2;
    let full = egui::Rect::from_min_max(
        label.min,
        label.max + egui::vec2(chrome::TAB_CLOSE_WIDTH, 0.0),
    );
    assert!(
        output.shapes.iter().any(
            |clipped| matches!(&clipped.shape, egui::Shape::Rect(shape) if shape.rect == full)
                && clipped.clip_rect.height() < size.y
        ),
        "the tab stays clipped to the strip while the pointer crosses the media"
    );
    assert!(
        !state(&app).local_drop,
        "the media area is outside the tab insertion band"
    );
    let over_strip = egui::pos2(over_media.x, original[0].2.center().y);
    let (_, actions) = frame(
        &mut app,
        size,
        true,
        vec![
            egui::Event::PointerMoved(over_strip),
            pointer(over_strip, false),
        ],
    );
    assert!(actions == vec![UiAction::ReorderTab(original[0].0, 3)]);
    assert!(state(&app).drag.is_none());
    assert!(frame(&mut app, size, true, vec![]).1.is_empty());
    assert_eq!(tab_focus::take(&context, original[2].0), None);
    assert_eq!(
        tab_focus::take(&context, original[0].0),
        Some(background_role)
    );
    app.handle_ui_action(actions[0].clone());
    assert_eq!(
        app.tabs.tabs().iter().map(|tab| tab.id).collect::<Vec<_>>(),
        [original[1].0, original[2].0, original[0].0]
    );
    frame(
        &mut app,
        size,
        true,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Focus,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: original[0].1.accesskit_id(),
                data: None,
            },
        )],
    );
    assert!(context.memory(|memory| memory.has_focus(original[0].1)));
    let (_, actions) = frame(
        &mut app,
        size,
        true,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(actions == vec![UiAction::ActivateTab(original[0].0)]);
    assert!(context.memory(|memory| memory.has_focus(original[0].1)));
}

#[test]
fn tab_drag_cancellation_rejects_late_release_after_context_changes() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::tab_drag_cancellation_rejects_late_release_after_context_changes",
    ) else {
        return;
    };
    for case in 0..11 {
        let mut app = setup(&root);
        let size = egui::vec2(960.0, 576.0);
        let original = app.tabs.clone();
        let start = state(&app).widgets[0].2.center();
        let target = start + egui::vec2(190.0, 0.0);
        let context = app.ui_context.clone().expect("context");
        context.memory_mut(|memory| {
            memory.request_focus(egui::Id::new("tab-drag-numeric-focus"));
        });
        frame(&mut app, size, true, vec![]);
        frame(
            &mut app,
            size,
            true,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        assert_eq!(context.memory(|memory| memory.focused()), None);
        frame(
            &mut app,
            size,
            true,
            vec![egui::Event::PointerMoved(target)],
        );
        assert!(state(&app).drag.is_some());
        let mut events = vec![];
        let mut dimensions = size;
        match case {
            0 => events.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }),
            1 => {}
            2 => dimensions.x = 640.0,
            3 => app.media_generation += 1,
            4 => app.graphics_epoch += 1,
            5 => app.palette_open = true,
            6 => {
                app.tabs.open_new(root.join("new.png"), MediaKind::Image);
            }
            7 => app.grid_open = true,
            8 => app
                .ui_context
                .as_ref()
                .expect("context")
                .set_pixels_per_point(2.0),
            9 => app
                .tabs
                .get_mut(original.tabs()[0].id)
                .expect("inactive tab")
                .target
                .set_current_path(root.join("changed.png"), MediaKind::Image),
            10 => {
                let _ = app
                    .ui_context
                    .as_ref()
                    .expect("context")
                    .run_ui(egui::RawInput::default(), |_| {});
            }
            _ => unreachable!(),
        }
        assert!(
            frame(&mut app, dimensions, case != 1, events).1.is_empty(),
            "case {case}"
        );
        assert!(state(&app).drag.is_none(), "case {case}");
        app.palette_open = false;
        app.grid_open = false;
        assert!(
            frame(
                &mut app,
                dimensions,
                true,
                vec![pointer(egui::pos2(-20.0, 90.0), false)]
            )
            .1
            .is_empty(),
            "late release {case}"
        );
        assert_eq!(context.memory(|memory| memory.focused()), None);
        assert_eq!(
            tab_focus::take(&context, original.active().expect("active").id),
            None
        );
        if case != 6 && case != 9 {
            assert_eq!(app.tabs, original);
        }
        // A new press works immediately after the rejected release; no idle frame is required.
        let start = state(&app).widgets[0].2.center();
        let target = start + egui::vec2(30.0, 0.0);
        frame(&mut app, dimensions, true, vec![pointer(start, true)]);
        frame(
            &mut app,
            dimensions,
            true,
            vec![egui::Event::PointerMoved(target)],
        );
        assert!(
            state(&app).drag.as_ref().is_some_and(|drag| drag.crossed),
            "fresh press {case}"
        );
        frame(&mut app, dimensions, true, vec![pointer(target, false)]);
    }
}

#[test]
fn local_tab_drops_project_client_positions_without_replaying_release() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::local_tab_drops_project_client_positions_without_replaying_release",
    ) else {
        return;
    };
    let size = egui::vec2(960.0, 576.0);
    for density in [1.0, 1.25, 2.0] {
        let mut app = setup(&root);
        let context = app.ui_context.clone().expect("context");
        context.set_pixels_per_point(density);
        for _ in 0..3 {
            frame(&mut app, size, true, vec![]);
        }
        let original = state(&app).widgets;
        let start = original[2].2.center();
        for batched in [false, true] {
            for y in [start.y, 100.0, size.y - 1.0] {
                for (x, gap) in [(1.0, 0), (drop_point(&context, 1).x, 1), (size.x - 1.0, 3)] {
                    let target = egui::pos2(x, y);
                    let mut events = vec![egui::Event::PointerMoved(start), pointer(start, true)];
                    if !batched {
                        assert!(frame(&mut app, size, true, events).1.is_empty());
                        assert!(
                            frame(
                                &mut app,
                                size,
                                true,
                                vec![egui::Event::PointerMoved(target)]
                            )
                            .1
                            .is_empty()
                        );
                        assert_eq!(state(&app).local_drop, y == start.y);
                        events = vec![];
                    }
                    events.extend([egui::Event::PointerMoved(target), pointer(target, false)]);
                    let (_, actions) = frame(&mut app, size, true, events);
                    if y == start.y {
                        assert!(actions == vec![UiAction::ReorderTab(original[2].0, gap)]);
                    } else {
                        assert!(
                            matches!(actions.as_slice(), [UiAction::DropTab(id, point, _)]
                            if *id == original[2].0 && *point == target),
                            "media release detaches"
                        );
                    }
                    assert_eq!(
                        state(&app).widgets,
                        original,
                        "geometry changes only after dispatch"
                    );
                    assert!(state(&app).drag.is_none());
                    assert!(frame(&mut app, size, true, vec![]).1.is_empty());
                }
            }
        }
    }
}

#[test]
fn tab_drag_batched_move_release_still_commits_once() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::tab_drag_batched_move_release_still_commits_once",
    ) else {
        return;
    };
    let mut app = setup(&root);
    let size = egui::vec2(960.0, 576.0);
    let original = state(&app).widgets;
    let start = original[0].2.center();
    let target = egui::pos2(original[2].2.right() + 10.0, start.y);
    frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    let (_, actions) = frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(target), pointer(target, false)],
    );
    assert!(actions == vec![UiAction::ReorderTab(original[0].0, 3)]);
    frame(&mut app, size, true, vec![]);
    frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(target)],
    );
    assert!(
        frame(&mut app, size, true, vec![egui::Event::PointerGone])
            .1
            .is_empty()
    );
    assert!(
        state(&app).drag.is_some(),
        "CursorLeft during capture must not discard detach ownership"
    );
    let outside = egui::pos2(-20.0, 90.0);
    let (_, actions) = frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(outside), pointer(outside, false)],
    );
    assert!(actions == vec![UiAction::DropTab(original[0].0, outside, start.to_vec2())]);
    assert!(frame(&mut app, size, true, vec![]).1.is_empty());
    // All three events may arrive between paints; the original press still owns the move.
    let (_, actions) = frame(
        &mut app,
        size,
        true,
        vec![
            pointer(start, true),
            egui::Event::PointerMoved(target),
            pointer(target, false),
        ],
    );
    assert!(actions == vec![UiAction::ReorderTab(original[0].0, 3)]);
}

#[test]
fn tab_drag_edge_scroll_moves_the_strip_without_projecting_neighbors() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::tab_drag_edge_scroll_moves_the_strip_without_projecting_neighbors",
    ) else {
        return;
    };
    for width in [320.0, 640.0, 960.0] {
        for density in [1.0, 1.25, 2.0] {
            let mut app = setup(&root);
            let first = app.tabs.tabs()[0].id;
            for index in 0..14 {
                app.tabs
                    .open_new(root.join(format!("long-{index}.png")), MediaKind::Image);
            }
            app.tabs.activate(first);
            app.ui_context
                .as_ref()
                .expect("context")
                .set_pixels_per_point(density);
            let size = egui::vec2(width, 576.0);
            for _ in 0..20 {
                frame(&mut app, size, true, vec![]);
            }
            let initial = state(&app);
            let strip = initial.strip.expect("visible strip");
            let start = initial.widgets[0].2.intersect(strip).center();
            let right = egui::pos2(strip.right() - 2.0, start.y);
            frame(
                &mut app,
                size,
                true,
                vec![egui::Event::PointerMoved(start), pointer(start, true)],
            );
            frame(&mut app, size, true, vec![egui::Event::PointerMoved(right)]);
            for _ in 0..12 {
                frame(&mut app, size, true, vec![]);
            }
            let scrolled = state(&app);
            let displacement = scrolled.widgets[0].2.left() - initial.widgets[0].2.left();
            for (before, after) in initial.widgets.iter().zip(&scrolled.widgets) {
                assert!((after.2.left() - before.2.left() - displacement).abs() < 0.01);
            }
            assert!(
                scrolled.widgets.last().expect("last").2.left()
                    < initial.widgets.last().expect("last").2.left() - 20.0,
                "scroll {width}/{density}"
            );
            assert_eq!(app.tabs.tabs()[0].id, first);
            frame(&mut app, size, true, vec![egui::Event::PointerGone]);
            // ScrollArea applies the previous pass's accepted delta on the next layout.
            frame(&mut app, size, true, vec![]);
            let stopped = state(&app).widgets.last().expect("last").2.left();
            for _ in 0..3 {
                frame(&mut app, size, true, vec![]);
            }
            assert_eq!(
                state(&app).widgets.last().expect("last").2.left(),
                stopped,
                "no edge scroll without a current pointer"
            );
            let (_, actions) = frame(
                &mut app,
                size,
                true,
                vec![egui::Event::PointerMoved(right), pointer(right, false)],
            );
            assert!(
                matches!(actions.as_slice(), [UiAction::ReorderTab(id, gap)] if *id == first && *gap > 1)
            );
            assert!(state(&app).drag.is_none());
        }
    }
}

pub(crate) fn hardware_round_trip<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
) {
    use crate::video_rotation::tests::frame as native_frame;
    let saved_tabs = app.tabs.clone();
    let active = app.tabs.active().expect("active").id;
    let path = app.path.clone().expect("path");
    let history = app.edits.clone();
    let transport = app.state;
    let generation = app.media_generation;
    for _ in 0..2 {
        app.tabs.open_new(path.clone(), MediaKind::Video);
    }
    app.tabs.activate(active);
    native_frame(app, vec![]);
    native_frame(app, vec![]);
    let original = app.tabs.clone();
    let dragged = original.tabs()[0].id;
    for returning in [false, true] {
        let widgets = state(app).widgets;
        let start = widgets
            .iter()
            .find(|(id, _, _)| *id == dragged)
            .expect("tab")
            .2
            .center();
        let target = if returning {
            egui::pos2(widgets[0].2.left() + 2.0, start.y)
        } else {
            egui::pos2(widgets.last().expect("last").2.right() + 10.0, start.y)
        };
        native_frame(
            app,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        native_frame(app, vec![egui::Event::PointerMoved(target)]);
        assert_eq!(state(app).drag.as_ref().expect("native drag").tab, dragged);
        assert_eq!(app.tabs.active().expect("active").id, active);
        native_frame(app, vec![pointer(target, false)]);
        native_frame(app, vec![]);
        assert_eq!(
            app.tabs.tabs()[if returning {
                0
            } else {
                app.tabs.tabs().len() - 1
            }]
            .id,
            dragged
        );
    }
    assert_eq!(app.tabs, original);
    let start = state(app).widgets[0].2.center();
    let over_media = start + egui::vec2(60.0, 100.0);
    native_frame(
        app,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    native_frame(app, vec![egui::Event::PointerMoved(over_media)]);
    assert!(state(app).drag.as_ref().is_some_and(|drag| drag.crossed));
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
    native_frame(app, vec![pointer(over_media, false)]);
    assert_eq!(app.tabs, original);
    app.tabs = saved_tabs;
    native_frame(app, vec![]);
    assert_eq!(app.edits, history);
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
        "PASS hardware tab drag: release-only reorder/return and unchanged history/transport/session generation; CPU transfers 0"
    );
}

#[test]
fn toolbar_magnetism_detaches_over_media_and_dims_the_whole_source_tab() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::toolbar_magnetism_detaches_over_media_and_dims_the_whole_source_tab",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let mut app = setup(&root);
        let context = app.ui_context.clone().expect("context");
        context.set_pixels_per_point(density);
        let size = egui::vec2(960.0, 576.0);
        for _ in 0..3 {
            frame(&mut app, size, true, vec![]);
        }
        let layout = state(&app);
        let (tab, _, label) = layout.widgets[0];
        let strip = layout.strip.expect("strip");
        let ids: Vec<_> = app.tabs.tab_ids().collect();
        for y in [strip.top(), strip.bottom() + 14.9] {
            assert!(incoming_gap(&context, &ids, egui::pos2(label.center().x, y)).is_some());
        }
        let body = egui::pos2(label.center().x, strip.bottom() + 15.1);
        assert!(incoming_gap(&context, &ids, body).is_none());
        assert!(incoming_filmstrip_gap(&context, &ids, body).is_some());
        frame(&mut app, size, true, vec![pointer(label.center(), true)]);
        let (output, actions) = frame(&mut app, size, true, vec![egui::Event::PointerMoved(body)]);
        assert!(actions.is_empty());
        let expected = egui::Rect::from_min_max(
            label.min,
            label.max + egui::vec2(chrome::TAB_CLOSE_WIDTH, 0.0),
        );
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.rect == expected && rect.fill == egui::Color32::from_black_alpha(128))),
            "group opacity includes the close slot, not only the drag label");
        let (_, actions) = frame(&mut app, size, true, vec![pointer(body, false)]);
        assert!(
            matches!(actions.as_slice(), [UiAction::DropTab(id, point, _)] if *id == tab && *point == body)
        );
        let (output, actions) = frame(&mut app, size, true, vec![]);
        assert!(actions.is_empty());
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.rect == expected && rect.fill == egui::Color32::from_black_alpha(128))));
    }
}
