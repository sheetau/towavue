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
        |ui| app.draw_top_bar(ui, &mut actions),
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

#[test]
fn tab_drag_projects_the_grabbed_offset_and_neighbors_without_mutating_tabs() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::tab_drag_projects_the_grabbed_offset_and_neighbors_without_mutating_tabs",
    ) else {
        return;
    };
    let mut app = setup(&root);
    let size = egui::vec2(960.0, 576.0);
    let original = state(&app).widgets;
    let start = original[0].2.center();
    let target = egui::pos2(original[2].2.right() + 10.0, start.y);
    let tabs = app.tabs.clone();
    frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    let (output, actions) = frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(target)],
    );
    assert!(actions.is_empty());
    let projected = state(&app);
    assert_eq!(projected.drag.as_ref().expect("drag").tab, original[0].0);
    assert_eq!(
        projected.widgets[0].2.min,
        original[0].2.min + (target - start)
    );
    assert_eq!(projected.widgets[1].2.min, original[0].2.min);
    assert_eq!(projected.widgets[2].2.min, original[1].2.min);
    assert_eq!(app.tabs, tabs);
    let tree = output.platform_output.accesskit_update.expect("tree");
    let label = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("a.png"))
        .expect("floating semantics");
    assert_eq!(
        label.1.bounds().expect("bounds").x0 as f32,
        projected.widgets[0].2.left()
    );
    let over_media = target + egui::vec2(0.0, 90.0);
    let (floating_output, actions) = frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(over_media)],
    );
    assert!(actions.is_empty());
    let label = state(&app).widgets[0].2;
    let full = egui::Rect::from_min_max(
        label.min,
        label.max + egui::vec2(chrome::TAB_CLOSE_WIDTH, 0.0),
    );
    assert!(
        floating_output.shapes.iter().any(
            |clipped| matches!(&clipped.shape, egui::Shape::Rect(shape) if shape.rect == full)
                && clipped.clip_rect == egui::Rect::from_min_size(egui::Pos2::ZERO, size)
        ),
        "floating background must escape the strip clip, like its label"
    );
    frame(
        &mut app,
        size,
        true,
        vec![egui::Event::PointerMoved(target)],
    );
    let (_, actions) = frame(&mut app, size, true, vec![pointer(target, false)]);
    assert!(actions == vec![UiAction::ReorderTab(original[0].0, 3)]);
    assert!(state(&app).drag.is_none());
    assert!(frame(&mut app, size, true, vec![]).1.is_empty());
    app.handle_ui_action(actions[0].clone());
    assert_eq!(
        app.tabs.tabs().iter().map(|tab| tab.id).collect::<Vec<_>>(),
        [original[1].0, original[2].0, original[0].0]
    );
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
    assert!(actions == vec![UiAction::DetachTab(original[0].0)]);
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
fn tab_drag_scrolls_clipped_tabs_without_losing_the_grabbed_offset() {
    let Some(root) = crate::tests::isolated_test_root(
        "tab_drag::tests::tab_drag_scrolls_clipped_tabs_without_losing_the_grabbed_offset",
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
            let float_left = state(&app).widgets[0].2.left();
            for _ in 0..12 {
                frame(&mut app, size, true, vec![]);
            }
            let scrolled = state(&app);
            assert_eq!(scrolled.widgets[0].2.left(), float_left);
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
        "PASS hardware tab drag: floating tab/neighbor projection, reorder/return and unchanged history/transport/session generation; CPU transfers 0"
    );
}
