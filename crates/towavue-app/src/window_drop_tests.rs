use super::*;

#[test]
fn drag_feedback_tracks_local_ownership_and_restores_after_release_or_cancel() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::dropping::tests::drag_feedback_tracks_local_ownership_and_restores_after_release_or_cancel",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let source = *host.windows.keys().next().expect("source");
    let app = host.windows.get_mut(&source).expect("source");
    app.ui_context = Some(fonts::test_context());
    for name in ["a.png", "b.png", "c.png"] {
        app.tabs.open_new(root.join(name), MediaKind::Image);
    }
    let render = |app: &mut WindowApplication, events| {
        let context = app.ui_context.clone().expect("context");
        let mut actions = Vec::new();
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 576.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.draw_top_bar(ui, &mut actions),
        );
        actions
    };
    for _ in 0..3 {
        render(app, vec![]);
    }
    let original = app.tabs.clone();
    let start = tab_drag::tests::label_center(app, original.tabs()[0].id);
    let end = tab_drag::tests::label_center(app, original.tabs()[2].id);
    let no_target = |_: &WindowHost, _, _| None;
    assert!(host.tab_drag_feedback(no_target).is_none());
    for cancel in [false, true] {
        let app = host.windows.get_mut(&source).expect("source");
        app.platform_cursor = egui::CursorIcon::Text;
        render(
            app,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
        );
        assert!(
            host.tab_drag_feedback(no_target).is_none(),
            "below drag threshold"
        );
        render(
            host.windows.get_mut(&source).expect("source"),
            vec![egui::Event::PointerMoved(end)],
        );
        let feedback = host.tab_drag_feedback(no_target).expect("active drag");
        assert_eq!(feedback.cursor, egui::CursorIcon::Move);
        assert!(feedback.target.is_none());
        host.windows
            .get_mut(&source)
            .expect("source")
            .graphics_epoch += 1;
        assert!(
            host.tab_drag_feedback(no_target).is_none(),
            "stale graphics identity before redraw"
        );
        host.windows
            .get_mut(&source)
            .expect("source")
            .graphics_epoch -= 1;
        let mut applied = Vec::new();
        host.update_tab_cursor_with(Some(feedback), |_, cursor| applied.push(cursor));
        host.update_tab_cursor_with(Some(feedback), |_, cursor| applied.push(cursor));
        assert_eq!(
            applied,
            [egui::CursorIcon::Move; 2],
            "reassert after native messages"
        );
        for point in [egui::pos2(500.0, 200.0), egui::pos2(-40.0, 90.0)] {
            render(
                host.windows.get_mut(&source).expect("source"),
                vec![egui::Event::PointerMoved(point)],
            );
            let feedback = host
                .tab_drag_feedback(no_target)
                .expect("owned invalid drag");
            assert_eq!(
                feedback.cursor,
                egui::CursorIcon::NoDrop,
                "media or unavailable transfer"
            );
            host.update_tab_cursor_with(Some(feedback), |_, cursor| applied.push(cursor));
        }
        for events in [vec![egui::Event::PointerGone], vec![], vec![]] {
            render(host.windows.get_mut(&source).expect("source"), events);
            assert_eq!(
                host.tab_drag_feedback(no_target)
                    .expect("capture retained outside")
                    .cursor,
                egui::CursorIcon::NoDrop
            );
        }
        let app = host.windows.get_mut(&source).expect("source");
        render(app, vec![egui::Event::PointerMoved(end)]);
        if cancel {
            assert!(tab_drag::cancel(app.ui_context.as_ref().expect("context")));
            assert!(
                host.tab_drag_feedback(no_target).is_none(),
                "native cancel needs no redraw"
            );
        } else {
            let actions = render(app, vec![pointer(end, false)]);
            assert!(matches!(actions.as_slice(), [UiAction::ReorderTab(_, _)]));
        }
        let feedback = host.tab_drag_feedback(no_target);
        assert!(feedback.is_none());
        host.update_tab_cursor_with(feedback, |_, cursor| applied.push(cursor));
        assert_eq!(applied.last(), Some(&egui::CursorIcon::Text));
        let restored = applied.len();
        host.update_tab_cursor_with(None, |_, cursor| applied.push(cursor));
        assert_eq!(applied.len(), restored, "restore once");
        let app = host.windows.get_mut(&source).expect("source");
        assert!(render(app, vec![pointer(end, false)]).is_empty());
        assert_eq!(app.tabs, original);
    }
}

#[test]
fn drag_cursor_restores_the_previous_owner_when_feedback_changes_windows() {
    let Some(_root) = crate::tests::isolated_test_root(
        "window_host::dropping::tests::drag_cursor_restores_the_previous_owner_when_feedback_changes_windows",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let first = *host.windows.keys().next().expect("first");
    let second = host.add_application(None).expect("second");
    host.windows.get_mut(&first).expect("first").platform_cursor = egui::CursorIcon::Text;
    host.windows
        .get_mut(&second)
        .expect("second")
        .platform_cursor = egui::CursorIcon::None;
    let mut applied = Vec::new();
    for source in [first, second] {
        host.update_tab_cursor_with(
            Some(DragFeedback {
                source,
                target: None,
                cursor: egui::CursorIcon::Move,
            }),
            |app, icon| applied.push((app.window_key.expect("key"), icon)),
        );
    }
    host.update_tab_cursor_with(None, |app, icon| {
        applied.push((app.window_key.expect("key"), icon))
    });
    assert_eq!(
        applied,
        [
            (first, egui::CursorIcon::Move),
            (first, egui::CursorIcon::Text),
            (second, egui::CursorIcon::Move),
            (second, egui::CursorIcon::None),
        ]
    );
    assert!(host.tab_cursor_owner.is_none());
}

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn frame(app: &mut WindowApplication, focused: bool, events: Vec<egui::Event>) {
    let context = app.ui_context.clone().expect("context");
    let window = app.window.as_ref().expect("window");
    let mut input = app
        .ui_state
        .as_mut()
        .expect("input")
        .take_egui_input(window);
    input.focused = focused;
    input.events = events;
    let mut actions = Vec::new();
    let output = context.run_ui(input, |ui| app.draw_top_bar(ui, &mut actions));
    let renderer = app.renderer.as_mut().expect("renderer");
    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
    renderer
        .render_ui(&context, output)
        .expect("tab strip GPU draw");
    renderer.present_surface().expect("present");
    for action in actions {
        app.handle_ui_action(action);
    }
}

pub(crate) fn exercise(host: &mut WindowHost, event_loop: &ActiveEventLoop) {
    let keys: Vec<_> = host.windows.keys().copied().collect();
    let (source, target) = (keys[0], keys[1]);
    let source_tabs = host.windows[&source].tabs.clone();
    let target_tabs = host.windows[&target].tabs.clone();
    let app = host.windows.get_mut(&source).expect("source");
    let path = app
        .path
        .as_ref()
        .expect("path")
        .with_file_name("merged-memory.png");
    let decoded = tab_transfer::tests::decoded(true);
    let tab = tab_transfer::tests::install(app, path, Arc::clone(&decoded));
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let edits = app.edits[&tab].clone();
    let original_count = host.windows.len();
    for key in [source, target] {
        for _ in 0..3 {
            frame(
                host.windows.get_mut(&key).expect("app"),
                key == source,
                vec![],
            );
        }
    }
    let point = tab_drag::tests::drop_point(
        host.windows[&target].ui_context.as_ref().expect("context"),
        1,
    );
    assert_eq!(host.windows[&target].incoming_gap(point), Some(1));
    let start = tab_drag::tests::label_center(&host.windows[&source], tab);
    let outside = egui::pos2(-40.0, 90.0);
    frame(
        host.windows.get_mut(&source).expect("source"),
        true,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    frame(
        host.windows.get_mut(&source).expect("source"),
        true,
        vec![egui::Event::PointerMoved(outside)],
    );
    assert!(host.windows[&source].pending_tab_drop.is_none());
    // Hidden HWNDs deliberately cannot pass WindowFromPoint. Inject only hit testing;
    // UI drag ownership, coordinates, layout validation and transfer are production paths.
    assert!(host.window_at_drop(source, outside).is_none());
    host.update_tab_drops_with(event_loop, false, |_, _, _| Some((target, point)));
    assert_eq!(host.windows[&target].incoming_tab_pointer, Some(point));
    assert_eq!(
        host.tab_drag_feedback(|_, _, _| Some((target, point)))
            .expect("merge feedback")
            .cursor,
        egui::CursorIcon::Move
    );
    assert_eq!(
        host.tab_drag_feedback(|_, _, _| None)
            .expect("detach feedback")
            .cursor,
        egui::CursorIcon::Move
    );
    frame(
        host.windows.get_mut(&target).expect("target"),
        false,
        vec![],
    );
    let request = host.windows[&source]
        .tab_detach_request(tab)
        .expect("request");
    host.windows.get_mut(&target).expect("target").export_error = Some("injected modal".into());
    host.update_tab_drops_with(event_loop, false, |_, _, _| Some((target, point)));
    assert!(host.windows[&target].incoming_tab_pointer.is_none());
    assert_eq!(
        host.tab_drag_feedback(|_, _, _| Some((target, point)))
            .expect("blocked feedback")
            .cursor,
        egui::CursorIcon::NoDrop
    );
    assert!(
        host.merge_tab_drop(source, &request, target, point)
            .is_err()
    );
    assert_eq!(host.windows[&source].edits[&tab], edits);
    host.windows.get_mut(&target).expect("target").export_error = None;
    assert!(
        host.merge_tab_drop(source, &request, target, point + egui::vec2(0.0, 100.0))
            .is_err()
    );
    host.update_tab_drops_with(event_loop, false, |_, _, _| Some((target, point)));
    assert_eq!(host.windows[&target].incoming_tab_pointer, Some(point));
    frame(
        host.windows.get_mut(&source).expect("source"),
        true,
        vec![pointer(outside, false)],
    );
    assert!(host.windows[&source].pending_tab_drop.is_some());
    host.update_tab_drops_with(event_loop, false, |_, _, _| Some((target, point)));
    assert_eq!(
        host.windows.len(),
        original_count,
        "merge must not detach another HWND"
    );
    assert_eq!(host.windows[&source].tabs.tabs(), source_tabs.tabs());
    assert_eq!(host.windows[&source].tabs.active(), source_tabs.active());
    let app = host.windows.get_mut(&target).expect("target");
    assert!(app.incoming_tab_pointer.is_none());
    let moved = app.tabs.active().expect("moved tab").id;
    assert_eq!(app.tabs.tabs()[1].id, moved);
    assert_eq!(app.edits[&moved], edits);
    assert!(Arc::ptr_eq(
        &app.image.as_ref().expect("image").decoded,
        &decoded
    ));
    assert!(
        host.merge_tab_drop(source, &request, target, point)
            .is_err(),
        "stale release cannot replay"
    );
    let welcome = host.add_application(None).expect("Welcome");
    host.start_pending(event_loop, false);
    for _ in 0..3 {
        frame(
            host.windows.get_mut(&welcome).expect("Welcome"),
            false,
            vec![],
        );
    }
    let point = tab_drag::tests::drop_point(
        host.windows[&welcome].ui_context.as_ref().expect("context"),
        0,
    );
    let request = host.windows[&target]
        .tab_detach_request(moved)
        .expect("return request");
    let returned = host
        .merge_tab_drop(target, &request, welcome, point)
        .expect("Welcome insertion");
    assert_eq!(host.windows[&welcome].tabs.tabs()[0].id, returned);
    assert_eq!(host.windows[&welcome].edits[&returned], edits);
    let app = host.windows.get_mut(&welcome).expect("Welcome source");
    app.window
        .as_ref()
        .expect("window")
        .set_outer_position(winit::dpi::PhysicalPosition::new(50, 50));
    for _ in 0..3 {
        frame(app, true, vec![]);
    }
    let start = tab_drag::tests::label_center(app, returned);
    let outside = egui::pos2(1000.0, 90.0);
    let origin = app
        .window
        .as_ref()
        .expect("window")
        .inner_position()
        .expect("origin");
    let density = app.ui_context.as_ref().expect("context").pixels_per_point();
    let release = winit::dpi::PhysicalPosition::new(
        origin.x + (outside.x * density).round() as i32,
        origin.y + (outside.y * density).round() as i32,
    );
    // Native release and grab offset are separately quantized to physical pixels.
    let expected = winit::dpi::PhysicalPosition::new(
        release.x - (start.x * density).round() as i32,
        release.y - (start.y * density).round() as i32,
    );
    frame(
        app,
        true,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    frame(app, true, vec![egui::Event::PointerMoved(outside)]);
    frame(app, true, vec![pointer(outside, false)]);
    assert!(app.pending_tab_drop.is_some());
    let previous: Vec<_> = host.windows.keys().copied().collect();
    host.update_tab_drops_with(event_loop, false, |_, _, _| None);
    let detached = *host
        .windows
        .keys()
        .find(|key| !previous.contains(key))
        .expect("detached window");
    let app = &host.windows[&detached];
    opening_tests::assert_drop_client_position(
        app.window.as_ref().expect("window"),
        expected,
        release,
    );
    let moved = app.tabs.active().expect("detached tab").id;
    assert_eq!(app.edits[&moved], edits);
    assert!(Arc::ptr_eq(
        &app.image.as_ref().expect("image").decoded,
        &decoded
    ));
    assert!(host.windows[&welcome].tabs.tabs().is_empty());
    host.windows
        .get_mut(&detached)
        .expect("detached")
        .exit_requested = true;
    host.remove_closed();
    assert_eq!(host.windows[&target].tabs.tabs(), target_tabs.tabs());
    host.windows
        .get_mut(&target)
        .expect("target")
        .activate_tab(target_tabs.active().expect("original active").id);
    host.windows
        .get_mut(&welcome)
        .expect("Welcome")
        .exit_requested = true;
    host.remove_closed();
    assert_eq!(host.windows.len(), original_count);
    eprintln!(
        "PASS hosted tab drop: captured drag/hover/release merges dirty in-memory animation at the indicated gap without a new HWND; hidden native GPU indicator and Welcome insertion; modal/body/stale-release rejection; source and destination neighbors retained; OS hit selection injected for hidden windows"
    );
}
