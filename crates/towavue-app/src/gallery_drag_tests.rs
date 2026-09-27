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
) -> (egui::FullOutput, Vec<UiAction>) {
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
    assert_eq!(output.pixels_per_point, density);
    (output, actions)
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
fn gallery_thumbnail_gap_and_gutter_drags_scroll_without_opening_media() {
    let Some(root) = tests::isolated_test_root(
        "gallery_drag_tests::gallery_thumbnail_gap_and_gutter_drags_scroll_without_opening_media",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for (surface, batched) in [(0, false), (1, false), (2, false), (0, true), (1, true)] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let context = fonts::test_context();
            context.enable_accesskit();
            context.global_style_mut(chrome::style);
            app.ui_context = Some(context.clone());
            app.hosted_graphics = true;
            app.recent_paths = (0..120)
                .map(|index| root.join(format!("item-{index:03}.png")))
                .collect();
            for _ in 0..3 {
                frame(&mut app, density, vec![]);
            }
            let first = card(&frame(&mut app, density, vec![]).0, "item-000.png");
            let origin = match surface {
                0 => first.center(),
                1 => first.right_center() + egui::vec2(16.0, 0.0),
                _ => egui::pos2(16.0, first.center().y),
            };
            let end = origin - egui::vec2(0.0, 70.0);
            let batches = if batched {
                vec![vec![
                    egui::Event::PointerMoved(origin),
                    pointer(origin, true),
                    egui::Event::PointerMoved(end),
                    pointer(end, false),
                ]]
            } else {
                vec![
                    vec![egui::Event::PointerMoved(origin), pointer(origin, true)],
                    vec![egui::Event::PointerMoved(end)],
                    vec![pointer(end, false)],
                ]
            };
            for events in batches {
                let (_, actions) = frame(&mut app, density, events);
                assert!(actions.is_empty(), "drag must not activate a thumbnail");
                assert!(app.filmstrip.active_recent_drag(&context).is_none());
            }
            let after = card(&frame(&mut app, density, vec![]).0, "item-000.png");
            assert!(
                first.top() - after.top() >= 69.0,
                "surface={surface}, batched={batched}, density={density}: {first:?} -> {after:?}"
            );
            assert!(app.pending_window_open.is_none());
            // A query change retires any drag/momentum and starts at the first row.
            app.gallery_search = "item-000".into();
            frame(&mut app, density, vec![]);
            let reset = card(&frame(&mut app, density, vec![]).0, "item-000.png");
            assert!((reset.top() - first.top()).abs() < 1.0);
            let point = reset.center();
            frame(
                &mut app,
                density,
                vec![egui::Event::PointerMoved(point), pointer(point, true)],
            );
            let (_, actions) = frame(&mut app, density, vec![pointer(point, false)]);
            assert!(
                matches!(actions.as_slice(), [UiAction::OpenMedia(path, true)] if path == &app.recent_paths[0]),
                "ordinary thumbnail clicks still open media"
            );
        }
    }
}
