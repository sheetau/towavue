use crate::*;

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn frame<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    density: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let context = app.ui_context.clone().expect("context");
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(960.0, 576.0),
        )),
        events,
        focused: true,
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .expect("root")
        .native_pixels_per_point = Some(density);
    let mut actions = Vec::new();
    let output = context.run_ui(input, |ui| app.draw_ui(ui, &mut actions));
    for action in actions {
        app.handle_ui_action(action);
    }
    assert_eq!(output.pixels_per_point, density);
    output
}

fn card(output: &egui::FullOutput, label: &str) -> egui::Rect {
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("tree")
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .expect("card")
        .1
        .bounds()
        .expect("bounds");
    egui::Rect::from_min_max(
        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
    )
}

#[test]
fn gallery_thumbnail_drag_excludes_only_its_source_and_cancels_stale_gestures() {
    let Some(root) = tests::isolated_test_root(
        "gallery_drag_tests::gallery_thumbnail_drag_excludes_only_its_source_and_cancels_stale_gestures",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let mut app = Application::new(None, |_| {}).expect("app");
        let context = fonts::test_context();
        context.enable_accesskit();
        context.global_style_mut(chrome::style);
        app.ui_context = Some(context.clone());
        app.hosted_graphics = true;
        let paths: Vec<_> = ["first.png", "second.mp4", "third.wav"]
            .map(|name| root.join(name))
            .into();
        app.recent_paths = paths.clone();
        for _ in 0..3 {
            frame(&mut app, density, vec![]);
        }
        let output = frame(&mut app, density, vec![]);
        let first = card(&output, "first.png");
        let second = card(&output, "second.mp4");
        let origin = first.center_top() + egui::vec2(0.0, 24.0);
        let neighbor = second.center_top() + egui::vec2(0.0, 24.0);
        let outside = egui::pos2(-40.0, 180.0);
        let original = app.tabs.clone();
        for (end, should_open, batched) in [
            (origin + egui::vec2(12.0, 0.0), false, false),
            (origin, false, false),
            (neighbor, true, false),
            (outside, true, false),
            (neighbor, true, true),
        ] {
            if batched {
                frame(
                    &mut app,
                    density,
                    vec![
                        egui::Event::PointerMoved(origin),
                        pointer(origin, true),
                        egui::Event::PointerMoved(end),
                        pointer(end, false),
                    ],
                );
            } else {
                frame(
                    &mut app,
                    density,
                    vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
                );
                // First leave, then return inside the same thumbnail where requested.
                frame(&mut app, density, vec![egui::Event::PointerMoved(neighbor)]);
                assert!(app.filmstrip.active_recent_drag(&context).is_some());
                frame(&mut app, density, vec![egui::Event::PointerMoved(end)]);
                assert_eq!(
                    app.filmstrip.active_recent_drag(&context).is_some(),
                    should_open
                );
                assert!(
                    app.pending_window_open.is_none(),
                    "holding does not open media"
                );
                frame(&mut app, density, vec![pointer(end, false)]);
            }
            assert_eq!(
                app.pending_window_open.is_some(),
                should_open,
                "end={end:?}, batched={batched}, density={density}"
            );
            if let Some(request) = app.pending_window_open.take() {
                assert_eq!(
                    request.path, paths[0],
                    "destination thumbnail does not replace the dragged path"
                );
                assert!(app.window_open_request_is_current(&request));
                assert_eq!(request.point, end);
                assert_eq!(request.anchor, origin - first.min);
                app.gallery_search = "second".into();
                assert!(
                    !app.window_open_request_is_current(&request),
                    "query change rejects a queued release before repaint"
                );
                app.gallery_search.clear();
                app.gallery_listing.invalidate();
                assert!(
                    !app.window_open_request_is_current(&request),
                    "history refresh invalidates the old projection"
                );
            }
            frame(&mut app, density, vec![pointer(end, false)]);
            assert!(app.pending_window_open.is_none(), "release is not replayed");
            assert_eq!(app.tabs, original);
            assert!(app.path.is_none());
        }
        // Labels remain clickable, but only the thumbnail starts a transfer.
        let label = first.center_bottom() - egui::vec2(0.0, 6.0);
        frame(
            &mut app,
            density,
            vec![egui::Event::PointerMoved(label), pointer(label, true)],
        );
        frame(&mut app, density, vec![egui::Event::PointerMoved(outside)]);
        assert!(app.filmstrip.active_recent_drag(&context).is_none());
        frame(&mut app, density, vec![pointer(outside, false)]);
        assert!(app.pending_window_open.is_none());
        for cancel in 0..5 {
            for _ in 0..2 {
                frame(&mut app, density, vec![]);
            }
            frame(
                &mut app,
                density,
                vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
            );
            frame(&mut app, density, vec![egui::Event::PointerMoved(neighbor)]);
            assert!(app.filmstrip.active_recent_drag(&context).is_some());
            match cancel {
                0 => app.gallery_search = "second".into(),
                1 => app.gallery_filter = Some(MediaKind::Audio),
                2 => {
                    app.recent_paths.remove(0);
                    app.gallery_listing.invalidate();
                }
                3 => app.export_error = Some("test modal".into()),
                4 => {
                    assert!(app.filmstrip.cancel_native_drag(&context));
                }
                _ => unreachable!(),
            }
            frame(&mut app, density, vec![pointer(neighbor, false)]);
            assert!(app.pending_window_open.is_none(), "cancel case {cancel}");
            assert_eq!(app.tabs, original);
            app.gallery_search.clear();
            app.gallery_filter = None;
            app.export_error = None;
            app.recent_paths = paths.clone();
            app.gallery_listing.invalidate();
        }
    }
}
