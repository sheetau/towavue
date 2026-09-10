use super::*;

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
    let expected = winit::dpi::PhysicalPosition::new(
        origin.x + ((outside.x - start.x) * density).round() as i32,
        origin.y + ((outside.y - start.y) * density).round() as i32,
    );
    let release = winit::dpi::PhysicalPosition::new(
        origin.x + (outside.x * density).round() as i32,
        origin.y + (outside.y * density).round() as i32,
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
