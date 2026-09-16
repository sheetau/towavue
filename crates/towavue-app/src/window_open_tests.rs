use super::*;

pub(super) fn assert_drop_client_position(
    window: &Window,
    requested: winit::dpi::PhysicalPosition<i32>,
    release: winit::dpi::PhysicalPosition<i32>,
) {
    let inner = window.inner_position().expect("position");
    let outer = window.outer_position().expect("outer position");
    let size = window.outer_size();
    let inset = (inner.x - outer.x, inner.y - outer.y);
    let area = towavue_runtime_windows::monitor_work_area((release.x, release.y))
        .expect("drop monitor work area");
    // The grab origin is exact only when the new window fits there. Check the
    // edge correction independently, including a portrait/narrow desktop.
    let expected = winit::dpi::PhysicalPosition::new(
        (requested.x - inset.0)
            .max(area.0)
            .min((area.2 - size.width as i32).max(area.0))
            + inset.0,
        (requested.y - inset.1)
            .max(area.1)
            .min((area.3 - size.height as i32).max(area.1))
            + inset.1,
    );
    assert_eq!(
        inner, expected,
        "drop must respect the grab origin and work area: size={size:?}, work={area:?}"
    );
}

fn snapshot(paths: &[PathBuf]) -> FolderSnapshot {
    FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![0]),
        folder_path: paths[0].parent().expect("fixture folder").to_owned(),
        items: paths
            .iter()
            .enumerate()
            .map(|(index, path)| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![index as u8]),
                path: path.clone(),
                kind: MediaKind::from_path(path).expect("media"),
            })
            .collect(),
        sort_columns: Vec::new(),
        source: FolderSnapshotSource::LiveExplorerView,
        generation: 42,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    }
}

#[test]
fn drop_window_clamp_handles_taskbars_negative_desktops_and_oversized_windows() {
    use winit::dpi::{PhysicalPosition, PhysicalSize};
    for (point, size, area, expected) in [
        ((100, 80), (960, 576), (0, 0, 1920, 1032), (100, 80)),
        ((1910, 1022), (960, 576), (0, 0, 1920, 1032), (960, 456)),
        ((178, 138), (960, 576), (0, 0, 1080, 1872), (120, 138)),
        ((-100, -100), (960, 576), (0, 40, 1920, 1080), (0, 40)),
        (
            (-2400, 100),
            (1920, 1152),
            (-2560, -245, 0, 1355),
            (-2400, 100),
        ),
        (
            (-100, 1000),
            (1920, 1152),
            (-2560, -245, 0, 1355),
            (-1920, 203),
        ),
        ((0, 0), (960, 576), (1920, -406, 3000, 1514), (1920, 0)),
        ((500, 500), (960, 576), (0, 0, 480, 300), (0, 0)),
        (
            (i32::MAX, i32::MIN),
            (u32::MAX, u32::MAX),
            (-2560, -245, 0, 1355),
            (-2560, -245),
        ),
    ] {
        let actual = window_open::clamp_window_position(
            PhysicalPosition::new(point.0, point.1),
            PhysicalSize::new(size.0, size.1),
            area,
        );
        assert_eq!((actual.x, actual.y), expected);
    }
}

#[test]
fn queued_filmstrip_windows_validate_source_identity_and_coalesce_duplicate_actions() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::opening_tests::queued_filmstrip_windows_validate_source_identity_and_coalesce_duplicate_actions",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let key = *host.windows.keys().next().expect("window");
    let app = host.windows.get_mut(&key).expect("app");
    let paths = [root.join("source.png"), root.join("target.png")];
    let tab = app.tabs.open_new(paths[0].clone(), MediaKind::Image);
    app.folder_snapshot = Some(snapshot(&paths));
    app.filmstrip_open = true;
    app.handle_ui_action(UiAction::OpenWindow(
        paths[1].clone(),
        41,
        egui::Pos2::ZERO,
        egui::Vec2::ZERO,
    ));
    app.handle_ui_action(UiAction::OpenWindow(
        root.join("foreign.png"),
        42,
        egui::Pos2::ZERO,
        egui::Vec2::ZERO,
    ));
    assert!(app.pending_window_open.is_none());
    app.palette_open = true;
    app.handle_ui_action(UiAction::OpenWindow(
        paths[1].clone(),
        42,
        egui::Pos2::ZERO,
        egui::Vec2::ZERO,
    ));
    assert!(app.pending_window_open.is_none());
    app.palette_open = false;
    for point in [egui::pos2(f32::NAN, 0.0), egui::pos2(0.0, f32::INFINITY)] {
        app.handle_ui_action(UiAction::OpenWindow(
            paths[1].clone(),
            42,
            point,
            egui::Vec2::ZERO,
        ));
        assert!(app.pending_window_open.is_none());
        app.handle_ui_action(UiAction::OpenWindow(
            paths[1].clone(),
            42,
            egui::Pos2::ZERO,
            point.to_vec2(),
        ));
        assert!(app.pending_window_open.is_none());
    }
    app.handle_ui_action(UiAction::OpenWindow(
        paths[1].clone(),
        42,
        egui::pos2(20.0, 40.0),
        egui::vec2(10.0, 15.0),
    ));
    app.handle_ui_action(UiAction::OpenWindow(
        paths[0].clone(),
        42,
        egui::Pos2::ZERO,
        egui::Vec2::ZERO,
    ));
    let request = app.pending_window_open.take().expect("one queued action");
    assert_eq!(request.path, paths[1]);
    assert_eq!(request.point, egui::pos2(20.0, 40.0));
    assert_eq!(request.anchor, egui::vec2(10.0, 15.0));
    assert!(app.window_open_request_is_current(&request));
    app.filmstrip_open = false;
    assert!(!app.window_open_request_is_current(&request));
    app.filmstrip_open = true;
    app.media_generation += 1;
    assert!(!app.window_open_request_is_current(&request));
    app.media_generation -= 1;
    app.tabs.open_new(paths[0].clone(), MediaKind::Image);
    assert!(!app.window_open_request_is_current(&request));
    app.tabs.activate(tab);
    app.folder_snapshot.as_mut().expect("snapshot").generation += 1;
    assert!(!app.window_open_request_is_current(&request));
    app.folder_snapshot.as_mut().expect("snapshot").generation -= 1;
    app.export_error = Some("modal".into());
    assert!(!app.window_open_request_is_current(&request));
    app.export_error = None;
    assert!(
        host.open_filmstrip_window_with(key, &request, false, |_, _| panic!(
            "renderer unavailable"
        ))
        .is_err()
    );
    assert_eq!(host.windows.len(), 1);
    host.windows.get_mut(&key).expect("source").exit_requested = true;
    assert!(
        host.open_filmstrip_window_with(key, &request, false, |_, _| panic!("closed source"))
            .expect("ignore closed")
            .is_none()
    );
    host.remove_closed();
    assert!(
        host.open_filmstrip_window_with(key, &request, false, |_, _| panic!("removed source"))
            .expect("ignore removed")
            .is_none()
    );
}

pub(super) fn finish_child(host: &mut WindowHost, key: WindowKey) -> &mut WindowApplication {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        tests::drain_captured(host);
        let app = host.windows.get_mut(&key).expect("child");
        app.finish_image_load();
        app.poll_audio();
        app.load_next_frame();
        app.render_frame();
        let ready = match app.media_kind.expect("child media") {
            MediaKind::Image => !app.image_loading,
            MediaKind::Video => app
                .session
                .as_ref()
                .is_some_and(|session| session.current_video_time().is_some()),
            MediaKind::Audio => app
                .session
                .as_ref()
                .and_then(PlaybackSession::audio_position)
                .is_some_and(|position| position > MediaTime::ZERO),
        };
        if ready || app.state == PlaybackState::Faulted {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "child loading deadline: {:?} {:?}",
            app.state,
            app.playback_error
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    host.windows.get_mut(&key).expect("child")
}

fn drag_frame(
    app: &mut WindowApplication,
    events: Vec<egui::Event>,
) -> egui::accesskit::TreeUpdate {
    let context = app.ui_context.clone().expect("context");
    context.enable_accesskit();
    let mut input = app
        .ui_state
        .as_mut()
        .expect("input")
        .take_egui_input(app.window.as_ref().expect("window"));
    input.focused = true;
    input.events = events;
    let mut actions = Vec::new();
    let output = context.run_ui(input, |ui| app.draw_ui(ui, &mut actions));
    if app.incoming_tab_pointer.is_some() {
        assert!(
            output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::LineSegment { points, stroke } if points[0].x == points[1].x
                && stroke.width == 2.0 && stroke.color == chrome::FOREGROUND
                && shape.clip_rect.height() < 64.0)),
            "shared tab insertion line is painted"
        );
    }
    let tree = output
        .platform_output
        .accesskit_update
        .clone()
        .expect("tree");
    let renderer = app.renderer.as_mut().expect("renderer");
    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
    renderer.render_ui(&context, output).expect("render");
    renderer.present_surface().expect("present");
    for action in actions {
        app.handle_ui_action(action);
    }
    tree
}

fn exercise_tab_drops(
    host: &mut WindowHost,
    event_loop: &ActiveEventLoop,
    source: WindowKey,
    paths: &[PathBuf],
) {
    let other = *host
        .windows
        .keys()
        .find(|key| **key != source)
        .expect("other host");
    let original = host.windows[&source].tabs.active().expect("source tab").id;
    let history = host.windows[&source].edits[&original].clone();
    let windows = host.windows.len();
    for target in [source, other] {
        let previous = host.windows[&target].tabs.clone();
        let app = host.windows.get_mut(&source).expect("source");
        app.folder_snapshot = Some(snapshot(paths));
        app.filmstrip_open = true;
        for _ in 0..3 {
            drag_frame(app, vec![]);
        }
        let tree = drag_frame(app, vec![]);
        let name = display_name(&paths[1]);
        let bounds = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(&name))
            .expect("card")
            .1
            .bounds()
            .expect("bounds");
        let origin = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.0) as f32,
            ((bounds.y0 + bounds.y1) / 2.0) as f32,
        );
        if target != source {
            for _ in 0..3 {
                drag_frame(host.windows.get_mut(&target).expect("target"), vec![]);
            }
        }
        let context = host.windows[&target].ui_context.as_ref().expect("context");
        let drop = tab_drag::tests::drop_point(context, 0) + egui::vec2(0.0, 120.0);
        let end = if target == source {
            drop
        } else {
            egui::pos2(-40.0, 120.0)
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        drag_frame(
            host.windows.get_mut(&source).expect("source"),
            vec![egui::Event::PointerMoved(origin), button(origin, true)],
        );
        drag_frame(
            host.windows.get_mut(&source).expect("source"),
            vec![egui::Event::PointerMoved(end)],
        );
        host.update_tab_drops_with(event_loop, false, |_, _, _| Some((target, drop)));
        assert_eq!(host.windows[&target].incoming_tab_pointer, Some(drop));
        dropping::tests::assert_filmstrip_feedback(
            host,
            Some((target, drop)),
            egui::CursorIcon::Move,
        );
        drag_frame(host.windows.get_mut(&target).expect("target"), vec![]);
        assert_eq!(host.tab_cursor_owner, Some(source));
        assert_eq!(
            host.windows[&target].tabs, previous,
            "hold must not open a tab"
        );
        // Blocking the target cancels the visual destination without changing either tab.
        if target != source {
            host.windows.get_mut(&target).expect("target").export_error =
                Some("injected modal".into());
            host.update_tab_drops_with(event_loop, false, |_, _, _| Some((target, drop)));
            assert!(host.windows[&target].incoming_tab_pointer.is_none());
            dropping::tests::assert_filmstrip_feedback(
                host,
                Some((target, drop)),
                egui::CursorIcon::NoDrop,
            );
            let app = host.windows.get_mut(&source).expect("source");
            app.request_filmstrip_window(paths[1].clone(), 42, end, egui::Vec2::ZERO);
            let request = app.pending_window_open.take().expect("guard request");
            assert!(
                host.open_filmstrip_tab(source, &request, target, drop)
                    .is_err()
            );
            assert_eq!(host.windows[&target].tabs, previous);
            host.windows.get_mut(&target).expect("target").export_error = None;
            host.windows.get_mut(&source).expect("source").fullscreen = true;
            dropping::tests::assert_filmstrip_feedback(
                host,
                Some((target, drop)),
                egui::CursorIcon::Move,
            );
            host.windows.get_mut(&source).expect("source").fullscreen = false;
        }
        drag_frame(
            host.windows.get_mut(&source).expect("source"),
            vec![button(end, false)],
        );
        assert!(host.windows[&source].pending_window_open.is_some());
        host.open_pending_windows_with(event_loop, false, |_, _, _| Some((target, drop)));
        host.update_tab_drops_with(event_loop, false, |_, _, _| Some((target, drop)));
        let app = host.windows.get_mut(&target).expect("target");
        let added = app.tabs.active().expect("new tab").id;
        assert_eq!(app.tabs.tabs().len(), previous.tabs().len() + 1);
        assert_eq!(app.tabs.tabs()[0].id, added);
        assert!(
            !app.edits[&added].is_dirty(),
            "open original without source edits"
        );
        let app = finish_child(host, target);
        assert_eq!(
            app.image.as_ref().expect("loaded original").dimensions(),
            (2, 1)
        );
        assert_eq!(host.windows[&source].edits[&original], history);
        assert_eq!(host.windows.len(), windows, "merge creates no HWND");
        assert!(host.tab_cursor_owner.is_none());
        host.open_pending_windows_with(event_loop, false, |_, _, _| panic!("release replay"));
        let app = host.windows.get_mut(&target).expect("target");
        app.close_tab_unchecked(added);
        app.activate_tab(previous.active().expect("previous").id);
        assert_eq!(app.tabs.tabs(), previous.tabs());
        assert_eq!(app.tabs.active(), previous.active());
        assert!(!host.windows[&source].filmstrip_open);
    }
    eprintln!(
        "PASS filmstrip tab drops: full UI press/hold/release to local and another host's body; shared feedback, modal suppression, one clean original tab at the gap, unchanged source history, no new HWND, late replay empty; native windows and GPU, external hit selection injected"
    );
}

fn exercise_edge_placement(host: &mut WindowHost, event_loop: &ActiveEventLoop) {
    let key = host.add_application(None).expect("placement window");
    host.start_pending(event_loop, false);
    let app = &host.windows[&key];
    let window = app.window.as_ref().expect("window");
    let monitors: Vec<_> = window.available_monitors().collect();
    assert!(!monitors.is_empty());
    let logical_size = window.inner_size().to_logical::<f64>(window.scale_factor());
    if !monitors
        .iter()
        .any(|monitor| monitor.scale_factor() != window.scale_factor())
    {
        eprintln!("SKIP mixed-DPI size round trip: no monitor with a different DPI");
    }
    for monitor in monitors.iter().cycle().take(monitors.len() * 2) {
        let origin = monitor.position();
        let area = towavue_runtime_windows::monitor_work_area((origin.x + 10, origin.y + 10))
            .expect("work area");
        for point in [
            winit::dpi::PhysicalPosition::new(area.0 + 10, area.1 + 10),
            winit::dpi::PhysicalPosition::new(area.2 - 10, area.3 - 10),
            winit::dpi::PhysicalPosition::new((area.0 + area.2) / 2, (area.1 + area.3) / 2),
        ] {
            let anchor = egui::vec2(100.0, 15.0);
            app.position_window_at_drop(point, anchor).expect("place");
            let position = window.outer_position().expect("outer position");
            let inner = window.inner_position().expect("inner position");
            let size = window.outer_size();
            let scale = window.scale_factor() as f32;
            assert_eq!(
                window.inner_size(),
                logical_size.to_physical::<u32>(window.scale_factor()),
                "DPI changes must not accumulate standard-frame margins"
            );
            let requested = (
                point.x - (anchor.x * scale).round() as i32 - (inner.x - position.x),
                point.y - (anchor.y * scale).round() as i32 - (inner.y - position.y),
            );
            let expected = (
                requested
                    .0
                    .max(area.0)
                    .min((area.2 - size.width as i32).max(area.0)),
                requested
                    .1
                    .max(area.1)
                    .min((area.3 - size.height as i32).max(area.1)),
            );
            assert_eq!(
                (position.x, position.y),
                expected,
                "DPI-scaled anchor and work-area placement"
            );
            assert!(position.x >= area.0 && position.y >= area.1);
            if size.width as i32 <= area.2 - area.0 {
                assert!(position.x + size.width as i32 <= area.2);
            }
            if size.height as i32 <= area.3 - area.1 {
                assert!(position.y + size.height as i32 <= area.3);
            }
            assert_eq!(window.is_visible(), Some(false));
            eprintln!(
                "PASS hidden drop placement: work={area:?} scale={scale} point={point:?} position={position:?} size={size:?}"
            );
        }
    }
    host.windows
        .get_mut(&key)
        .expect("placement window")
        .exit_requested = true;
    host.remove_closed();
}

pub(super) fn exercise(host: &mut WindowHost, event_loop: &ActiveEventLoop) {
    let source = *host.windows.keys().next().expect("source");
    // The preceding image exercise has just reactivated this paused video.
    let app = finish_child(host, source);
    let source_path = app.path.clone().expect("silent video");
    let image_path = source_path.with_file_name("filmstrip [new] 日本語.bmp");
    let bad_image = source_path.with_file_name("filmstrip-corrupt.bmp");
    let audio_path = source_path.with_file_name("transfer-silence.wav");
    tab_transfer::tests::bitmap(&image_path);
    std::fs::write(&bad_image, b"owned invalid bitmap").expect("invalid fixture");
    assert!(audio_path.exists(), "silent fixture from transfer harness");
    let paths = [
        source_path.clone(),
        image_path.clone(),
        audio_path.clone(),
        bad_image.clone(),
    ];
    let tab = app.tabs.active().expect("source tab").id;
    let old_history = app.edits.remove(&tab);
    let old_export = app
        .export_paths
        .insert(tab, source_path.with_file_name("source-export.mp4"));
    let old_snapshot = app.folder_snapshot.replace(snapshot(&paths));
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Video);
    let tabs = app.tabs.clone();
    let edits = app.edits.clone();
    let exports = app.export_paths.clone();
    let generation = app.generation;
    let instance = app.media_generation;
    let position = app.current_position();
    let window_count = host.windows.len();

    let app = host.windows.get_mut(&source).expect("source");
    app.filmstrip_open = true;
    app.handle_ui_action(UiAction::OpenWindow(
        image_path.clone(),
        42,
        egui::Pos2::ZERO,
        egui::Vec2::ZERO,
    ));
    let request = app.pending_window_open.take().expect("request");
    assert!(
        host.open_filmstrip_window_with(source, &request, false, |_, _| Err(
            "injected startup failure".into()
        ))
        .is_err()
    );
    assert!(
        host.open_filmstrip_window_with(source, &request, false, |app, device| {
            app.start_on_device(event_loop, Some(device), false)
                .expect("hidden stage");
            Err("injected post-start failure".into())
        })
        .is_err()
    );
    assert_eq!(host.windows.len(), window_count);
    assert!(host.windows[&source].filmstrip_open);
    let mut audio_verified = false;
    for path in &paths {
        let app = host.windows.get_mut(&source).expect("source");
        app.filmstrip_open = true;
        let client_origin = egui::pos2(-40.0, 60.0);
        let origin = app
            .window
            .as_ref()
            .expect("source window")
            .inner_position()
            .expect("origin");
        let density = app.ui_context.as_ref().expect("context").pixels_per_point();
        let expected_position = winit::dpi::PhysicalPosition::new(
            origin.x + (client_origin.x * density).round() as i32,
            origin.y + (client_origin.y * density).round() as i32,
        );
        app.handle_ui_action(UiAction::OpenWindow(
            path.clone(),
            42,
            client_origin,
            egui::Vec2::ZERO,
        ));
        let keys: Vec<_> = host.windows.keys().copied().collect();
        host.open_pending_windows(event_loop, false);
        let child = *host
            .windows
            .keys()
            .find(|key| !keys.contains(key))
            .expect("hosted filmstrip child");
        assert_eq!(host.windows.len(), window_count + 1);
        let app = host.windows.get_mut(&child).expect("child");
        assert_drop_client_position(
            app.window.as_ref().expect("child window"),
            expected_position,
            expected_position,
        );
        assert_eq!(
            app.window.as_ref().expect("child window").is_visible(),
            Some(false)
        );
        assert_eq!(
            app.path.as_ref(),
            Some(&canonical_shell_path(path).expect("canonical path"))
        );
        assert_eq!(app.tabs.tabs().len(), 1);
        assert!(!app.command_context().has_unsaved_edits && app.export_paths.is_empty());
        let app = finish_child(host, child);
        if path == &bad_image {
            assert!(app.image_error.is_some() && app.state == PlaybackState::Faulted);
        } else if path == &audio_path
            && app.playback_error.as_ref().is_some_and(|error| {
                error.starts_with("audio output failed: WASAPI output failed:")
            })
        {
            eprintln!(
                "SKIP hosted filmstrip audio playback: shared WASAPI unavailable: {:?}",
                app.playback_error
            );
        } else {
            assert!(
                app.image_error.is_none() && app.playback_error.is_none(),
                "child errors: {:?} {:?}",
                app.image_error,
                app.playback_error
            );
            if path == &image_path {
                assert_eq!(
                    app.image.as_ref().expect("decoded image").dimensions(),
                    (2, 1)
                );
            }
            if path == &audio_path {
                audio_verified = true;
            }
        }
        if let Some(session) = &app.session {
            assert_eq!(session.metrics().cpu_transfer_count, 0);
        }
        // A source hardware frame must also draw on this newly opened child's device.
        let mut original = host.windows.remove(&source).expect("source");
        assert!(
            original
                .session
                .as_mut()
                .expect("source session")
                .draw_current(
                    host.windows
                        .get_mut(&child)
                        .expect("child")
                        .renderer
                        .as_mut()
                        .expect("renderer"),
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(160.0, 96.0)),
                    original.video_uv,
                )
                .expect("same-device cross-draw")
        );
        assert_eq!(original.tabs, tabs);
        assert_eq!(original.edits, edits);
        assert_eq!(original.export_paths, exports);
        assert_eq!(original.generation, generation);
        assert_eq!(original.media_generation, instance);
        assert_eq!(original.current_position(), position);
        assert!(!original.filmstrip_open && original.pending_guard.is_none());
        host.windows.insert(source, original);
        host.windows.get_mut(&child).expect("child").exit_requested = true;
        host.remove_closed();
    }
    exercise_tab_drops(host, event_loop, source, &paths);
    let app = host.windows.get_mut(&source).expect("source");
    let missing = source_path.with_file_name("missing-filmstrip.png");
    app.folder_snapshot = Some(snapshot(&[source_path, missing.clone()]));
    app.filmstrip_open = true;
    app.handle_ui_action(UiAction::OpenWindow(
        missing,
        42,
        egui::pos2(-20.0, 100.0),
        egui::Vec2::ZERO,
    ));
    host.open_pending_windows(event_loop, false);
    assert_eq!(host.windows.len(), window_count);
    let app = host.windows.get_mut(&source).expect("source");
    assert!(app.filmstrip_open);
    assert!(
        app.status_message
            .as_ref()
            .expect("failure status")
            .0
            .starts_with("Could not open dragged media:")
    );
    assert_eq!(app.edits, edits);
    if let Some(history) = old_history {
        app.edits.insert(tab, history);
    } else {
        app.edits.remove(&tab);
    }
    if let Some(path) = old_export {
        app.export_paths.insert(tab, path);
    } else {
        app.export_paths.remove(&tab);
    }
    app.folder_snapshot = old_snapshot;
    app.close_filmstrip();
    exercise_edge_placement(host, event_loop);
    eprintln!(
        "PASS hosted filmstrip windows: normal action opens/loads image, silent video and corrupt media independently; silent audio playback verified={audio_verified}; same-device cross-draw; source tabs/edits/export/clock/session unchanged; startup/post-start/missing-file failures preserve filmstrip and leave no child"
    );
}
