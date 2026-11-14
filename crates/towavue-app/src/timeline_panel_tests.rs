use super::*;

type App = Application<fn(AppEvent)>;

#[test]
fn resize_feedback_keeps_e0_while_idle_separator_keeps_its_border() {
    let context = fonts::test_context();
    context.global_style_mut(chrome::style);
    let panel = egui::Id::new("separator-color");
    let mut height = 96.0;
    let mut top = egui::pos2(200.0, 204.0);
    for state in 0..3 {
        let events = match state {
            1 => vec![egui::Event::PointerMoved(top)],
            2 => vec![button(top, true)],
            _ => vec![],
        };
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 320.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let result = show_panel(ui, panel, &mut height, true, egui::Margin::ZERO, |_| {});
                top = result.response.rect.center_top();
            },
        );
        let stroke = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points, stroke }
                    if (points[1].x - points[0].x).abs() > 390.0 =>
                {
                    Some(*stroke)
                }
                _ => None,
            })
            .expect("panel separator");
        assert_eq!(
            stroke.color,
            if state == 0 {
                chrome::BORDER
            } else {
                chrome::FOREGROUND
            }
        );
        assert_eq!(stroke.width, if state == 0 { 1.0 } else { 2.0 });
    }
}

fn setup(kind: MediaKind, scale: f32) -> App {
    let mut app = Application::new(None, (|_| {}) as fn(AppEvent)).expect("app");
    let path = PathBuf::from(if kind == MediaKind::Audio {
        "unopened.wav"
    } else {
        "unopened.mp4"
    });
    let tab = app.tabs.open_new(path.clone(), kind);
    app.path = Some(path);
    app.displayed_tab = Some(tab);
    app.media_kind = Some(kind);
    app.media_generation = 7;
    app.media_duration = Some(Duration::from_secs(10));
    app.state = PlaybackState::Paused;
    app.clock = Some(PlaybackClock::paused(
        media_time(Duration::from_secs(2)),
        1.0,
    ));
    app.timeline_open = true;
    app.time_selection = towavue_core::TimeRange::new(
        media_time(Duration::from_secs(1)),
        media_time(Duration::from_secs(3)),
    );
    app.playback_selection = app.time_selection;
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::SetVolume(0.8), kind);
    let context = fonts::test_context();
    context.set_pixels_per_point(scale);
    app.ui_context = Some(context);
    for _ in 0..4 {
        assert!(frame(&mut app, vec![], false).is_empty());
    }
    app
}

fn frame(app: &mut App, events: Vec<egui::Event>, repeat: bool) -> Vec<UiAction> {
    let context = app.ui_context.clone().expect("context");
    let mut actions = Vec::new();
    let _ = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            app.draw_ui(ui, &mut actions);
            if repeat && context.current_pass_index() == 0 {
                context.request_discard("verify bounded resize across layout passes");
            }
        },
    );
    actions
}

fn rect(app: &App) -> egui::Rect {
    egui::containers::panel::PanelState::load(
        app.ui_context.as_ref().expect("context"),
        app.timeline_panel_id(),
    )
    .expect("panel state")
    .outer_rect
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn accepted_height_survives_tabs_reopening_and_a_fresh_preference_store() {
    let Some(root) = crate::tests::isolated_test_root(
        "timeline_edit::panel_tests::accepted_height_survives_tabs_reopening_and_a_fresh_preference_store",
    ) else {
        return;
    };
    let path = root.join("timeline-viewing.conf");
    let store = Arc::new(
        towavue_runtime_windows::ViewPreferences::open(path.clone(), |e| panic!("{e}"))
            .expect("store"),
    );
    let mut app = setup(MediaKind::Video, 1.25);
    app.restore_view_preferences(Some(store.clone()));
    let end = rect(&app).center_top() + egui::vec2(0.0, 16.0);
    drag(&mut app, end);
    frame(&mut app, vec![button(end, false)], true);
    assert!((app.timeline_height(false) - 80.0).abs() <= 1.0);
    let preferred = app.timeline_height(false);
    let first = app.tabs.active_id().expect("first tab");
    app.timeline_open = false;
    frame(&mut app, vec![], false);
    app.tabs
        .open_new(PathBuf::from("other-unopened.mp4"), MediaKind::Video);
    app.timeline_open = true;
    for _ in 0..3 {
        frame(&mut app, vec![], false);
    }
    assert!((rect(&app).height() - preferred).abs() <= 1.0);
    let second = app.tabs.active_id().expect("second tab");
    app.remember_timeline_height(100.0, false);
    app.remember_timeline_height(72.0, true);
    app.tabs.activate(first);
    for _ in 0..3 {
        frame(&mut app, vec![], false);
    }
    assert!((rect(&app).height() - preferred).abs() <= 1.0);
    assert_eq!(app.timeline_height(true), 96.0);
    app.tabs.activate(second);
    assert_eq!(app.timeline_height(false), 100.0);
    assert_eq!(app.timeline_height(true), 72.0);
    let third = app
        .tabs
        .open_new(PathBuf::from("next.gif"), MediaKind::Image);
    app.seed_tab_preferences(third);
    assert_eq!(app.timeline_height(false), 100.0);
    assert_eq!(app.timeline_height(true), 72.0);
    app.remember_timeline_height(104.0, true);
    assert_eq!(app.timeline_heights.tabs[&first], [preferred, 96.0]);
    assert_eq!(app.timeline_heights.tabs[&second], [100.0, 72.0]);
    drop(app);
    drop(store);
    let store = Arc::new(
        towavue_runtime_windows::ViewPreferences::open(path, |e| panic!("{e}"))
            .expect("reopened store"),
    );
    let mut app = setup(MediaKind::Audio, 2.0);
    app.restore_view_preferences(Some(store));
    app.timeline_heights.tabs.clear(); // The fixture drew once before restoring startup preferences.
    for _ in 0..3 {
        frame(&mut app, vec![], false);
    }
    assert!((rect(&app).height() - 100.0).abs() <= 1.0);
    assert_eq!(app.timeline_height(true), 104.0);
}

fn drag(app: &mut App, end: egui::Pos2) {
    let start = rect(app).center_top();
    for events in [
        vec![egui::Event::PointerMoved(start)],
        vec![button(start, true)],
        vec![egui::Event::PointerMoved(start + egui::vec2(0.0, 4.0))],
        vec![egui::Event::PointerMoved(end)],
    ] {
        assert!(
            frame(app, events, false).is_empty(),
            "resizing only changes panel height"
        );
        assert!(app.timeline_open);
    }
}

#[test]
fn resize_preserves_grab_offset_and_disabled_panel_uses_one_overlay() {
    let Some(_) = crate::tests::isolated_test_root(
        "timeline_edit::panel_tests::resize_preserves_grab_offset_and_disabled_panel_uses_one_overlay",
    ) else {
        return;
    };
    for scale in [1.0, 1.25, 2.0] {
        for offset in [-2.0, 2.0] {
            let mut app = setup(MediaKind::Video, scale);
            let before = rect(&app);
            let start = before.center_top() + egui::vec2(0.0, offset);
            frame(&mut app, vec![egui::Event::PointerMoved(start)], false);
            frame(&mut app, vec![button(start, true)], true);
            frame(&mut app, vec![], false);
            let context = app.ui_context.clone().expect("test fixture");
            let handle = app.timeline_panel_id().with("__resize");
            assert!(
                (context
                    .read_response(handle)
                    .expect("test fixture")
                    .rect
                    .center()
                    .y
                    - before.top())
                .abs()
                    < 0.6,
                "press must not snap"
            );
            let next = start + egui::vec2(0.0, 1.0);
            frame(&mut app, vec![egui::Event::PointerMoved(next)], true);
            frame(&mut app, vec![], false);
            assert!(
                (context
                    .read_response(handle)
                    .expect("test fixture")
                    .rect
                    .center()
                    .y
                    - before.top()
                    - 1.0)
                    .abs()
                    < 0.6,
                "first pixel preserves offset"
            );
            frame(&mut app, vec![button(next, false)], true);
            assert!((rect(&app).height() - before.height() + 1.0).abs() < 0.6);
        }
    }
    let context = fonts::test_context();
    let output = context.run_ui(Default::default(), |ui| {
        show_panel(
            ui,
            egui::Id::new("disabled-overlay"),
            &mut 96.0,
            false,
            egui::Margin::ZERO,
            |ui| {
                assert!(!ui.is_enabled());
                assert_eq!(ui.opacity(), 1.0);
                ui.painter().rect_filled(ui.max_rect(), 0.0, Color32::WHITE);
            },
        );
    });
    assert!(output.shapes.iter().any(
        |shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == Color32::WHITE)
    ));
    assert_eq!(output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(128))).count(), 1);
    let overlay = output.shapes.iter().position(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == Color32::from_black_alpha(128))).expect("overlay");
    assert!(output.shapes[overlay+1..].iter().any(|shape| matches!(&shape.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == chrome::BORDER)));
}

#[test]
fn resize_clamps_at_minimum_without_closing_timeline() {
    let Some(_) = crate::tests::isolated_test_root(
        "timeline_edit::panel_tests::resize_clamps_at_minimum_without_closing_timeline",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        for scale in [1.0, 1.25, 2.0] {
            for released in [false, true] {
                let mut app = setup(kind, scale);
                let tab = app.tabs.active_id().expect("tab");
                let history = app.edits[&tab].clone();
                let selection = app.time_selection;
                let generation = app.generation;
                let position = app.current_position();
                let minimum = rect(&app).center_bottom() - egui::vec2(0.0, 64.0);
                let larger = minimum - egui::vec2(0.0, 16.0);
                drag(&mut app, larger);
                assert!(frame(&mut app, vec![button(larger, false)], false).is_empty());
                assert!((rect(&app).height() - 80.0).abs() <= 1.0);
                drag(&mut app, minimum + egui::vec2(0.0, 4.0));
                let below = minimum + egui::vec2(0.0, 80.0);
                let events = if released {
                    vec![button(below, false)]
                } else {
                    vec![egui::Event::PointerMoved(below)]
                };
                assert!(frame(&mut app, events, true).is_empty());
                assert!(
                    app.timeline_is_visible(),
                    "overshoot never closes the timeline"
                );
                if !released {
                    assert!(frame(&mut app, vec![button(below, false)], true).is_empty());
                }
                assert!(frame(&mut app, vec![], true).is_empty());
                assert!((rect(&app).height() - 64.0).abs() <= 1.0 / scale);
                assert_eq!(app.edits[&tab], history);
                assert_eq!(app.time_selection, selection);
                assert_eq!(app.playback_selection, selection);
                assert_eq!(app.current_position(), position);
                assert_eq!(app.generation, generation);
                assert_eq!(app.state, PlaybackState::Paused);
                app.dispatch(CommandId::ToggleTimeline);
                assert!(!app.timeline_open, "explicit toggle still closes");
                app.dispatch(CommandId::ToggleTimeline);
                assert!(frame(&mut app, vec![], true).is_empty());
                assert!((rect(&app).height() - 64.0).abs() <= 1.0 / scale);
                drag(&mut app, larger);
                assert!(frame(&mut app, vec![button(larger, false)], true).is_empty());
                assert!((rect(&app).height() - 80.0).abs() <= 1.0 / scale);
            }
        }
    }
}

#[test]
fn resize_preserves_cancellation_release_order_and_pointer_reentry() {
    let Some(_) = crate::tests::isolated_test_root(
        "timeline_edit::panel_tests::resize_preserves_cancellation_release_order_and_pointer_reentry",
    ) else {
        return;
    };
    for interruption in 0..9 {
        let mut app = setup(MediaKind::Video, 1.0);
        let before = rect(&app);
        let inside = before.center_bottom() - egui::vec2(0.0, 60.0);
        let below = before.center_bottom() - egui::vec2(0.0, 48.0);
        drag(&mut app, inside);
        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let crossed = egui::Event::PointerMoved(below);
        let events = match interruption {
            0 => vec![escape, crossed, button(below, false)],
            1 => vec![
                egui::Event::WindowFocused(false),
                crossed,
                button(below, false),
            ],
            2 => {
                assert!(app.cancel_view_drag());
                vec![crossed, button(below, false)]
            }
            3 => {
                app.pending_guard = Some(GuardedAction::Exit);
                vec![crossed, button(below, false)]
            }
            4 => vec![button(inside, false), crossed],
            5 => vec![crossed, escape, button(below, false)],
            6 => vec![
                crossed,
                egui::Event::PointerMoved(inside),
                button(inside, false),
            ],
            7 => vec![egui::Event::PointerGone, crossed, button(below, false)],
            _ => {
                let boundary = before.center_bottom() - egui::vec2(0.0, 56.0);
                vec![egui::Event::PointerMoved(boundary), button(boundary, false)]
            }
        };
        assert!(frame(&mut app, events, true).is_empty());
        app.pending_guard = None;
        assert!(frame(&mut app, vec![], false).is_empty());
        assert!(app.timeline_open);
        let expected = if matches!(interruption, 0..=3 | 5) {
            before.height()
        } else {
            64.0
        };
        assert!(
            (rect(&app).height() - expected).abs() <= 1.0,
            "cancelled resize restores its initial height: {interruption}"
        );
    }
}

#[test]
fn resize_moves_on_first_pixel_and_filmstrip_has_no_resize_target() {
    let Some(_) = crate::tests::isolated_test_root(
        "timeline_edit::panel_tests::resize_moves_on_first_pixel_and_filmstrip_has_no_resize_target",
    ) else {
        return;
    };
    let mut app = setup(MediaKind::Video, 1.0);
    let context = app.ui_context.clone().expect("context");
    let id = app.timeline_panel_id().with("__resize");
    let start = rect(&app).center_top();
    frame(&mut app, vec![egui::Event::PointerMoved(start)], false);
    frame(&mut app, vec![button(start, true)], false);
    assert_eq!(context.dragged_id(), Some(id));
    let next = start + egui::vec2(0.0, 1.0);
    frame(&mut app, vec![egui::Event::PointerMoved(next)], false);
    frame(&mut app, vec![], false);
    assert!(
        (context
            .read_response(id)
            .expect("resize handle")
            .rect
            .center()
            .y
            - next.y)
            .abs()
            < 0.6
    );
    frame(&mut app, vec![button(next, false)], false);
    assert!((rect(&app).height() - 95.0).abs() < 0.6);
    drag(&mut app, egui::pos2(start.x, 0.0));
    frame(
        &mut app,
        vec![button(egui::pos2(start.x, 0.0), false)],
        false,
    );
    assert!(
        rect(&app).height() <= 120.0,
        "maximum is at most 30% of the window"
    );
    app.filmstrip_open = true;
    let before = rect(&app);
    for _ in 0..3 {
        frame(&mut app, vec![], false);
    }
    assert!(
        context.read_response(id).is_none(),
        "blocked resize has no hover or drag widget"
    );
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(before.center_top()),
            button(before.center_top(), true),
        ],
        false,
    );
    frame(
        &mut app,
        vec![egui::Event::PointerMoved(egui::pos2(start.x, 0.0))],
        false,
    );
    assert_eq!(rect(&app), before);
    assert_ne!(context.dragged_id(), Some(id));
}

#[test]
#[ignore = "requires hidden hardware D3D11; optional timeline separator readbacks"]
fn timeline_resize_feedback_reaches_gpu_without_opacity_changes() {
    let Some(_root) = crate::tests::isolated_test_root(
        "timeline_edit::panel_tests::timeline_resize_feedback_reaches_gpu_without_opacity_changes",
    ) else {
        return;
    };
    use winit::application::ApplicationHandler;
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::platform::windows::EventLoopBuilderExtWindows;
    struct Trial(bool);
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(winit::window::Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = FrameRenderer::new(&window).expect("D3D11");
            for density in [1.0, 1.25, 2.0] {
                let context = fonts::test_context();
                context.global_style_mut(chrome::style);
                let panel = egui::Id::new("gpu-resize-feedback");
                let mut height = 96.0;
                let mut top = egui::pos2(200.0, 224.0);
                let width = (400.0 * density) as u32;
                let height_pixels = (320.0 * density) as u32;
                renderer
                    .resize_surface(width, height_pixels)
                    .expect("surface");
                for state in 0..5 {
                    let events = match state {
                        1 => vec![egui::Event::PointerMoved(top)],
                        2 => vec![button(top, true)],
                        3 => vec![egui::Event::PointerMoved(top + egui::vec2(0.0, 12.0))],
                        4 => vec![button(top, false)],
                        _ => vec![],
                    };
                    for pass in 0..3 {
                        let mut input = egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(400.0, 320.0),
                            )),
                            events: if pass == 0 { events.clone() } else { vec![] },
                            ..Default::default()
                        };
                        input
                            .viewports
                            .entry(egui::ViewportId::ROOT)
                            .or_default()
                            .native_pixels_per_point = Some(density);
                        let output = context.run_ui(input, |ui| {
                            let result = show_panel(
                                ui,
                                panel,
                                &mut height,
                                state != 4,
                                egui::Margin::ZERO,
                                |_| {},
                            );
                            top = result.response.rect.center_top();
                        });
                        renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
                        renderer.render_ui(&context, output).expect("render");
                    }
                    if state == 3 {
                        assert!(resizing_panel(&context, panel), "owned resize drag");
                    }
                    if state == 4 {
                        assert!(
                            !resizing_panel(&context, panel),
                            "disabled panel owns no drag"
                        );
                    }
                    let pixels = renderer.verification_surface_rgba().expect("readback");
                    let x = (top.x * density).round() as usize;
                    let y = (top.y * density).round() as usize;
                    let peak = (y - 5..=y + 5)
                        .map(|y| pixels[(y * width as usize + x) * 4])
                        .max()
                        .expect("separator pixels");
                    assert_eq!(
                        peak,
                        if state == 0 || state == 4 {
                            chrome::BORDER.r()
                        } else {
                            chrome::FOREGROUND.r()
                        },
                        "state={state}, density={density}"
                    );
                    if let Some(directory) = std::env::var_os("TOWAVUE_REFERENCE_RENDER_DIR") {
                        std::fs::write(
                            PathBuf::from(directory).join(format!(
                                "timeline-resize-{state}-{width}x{height_pixels}.rgba"
                            )),
                            pixels,
                        )
                        .expect("readback file");
                    }
                }
            }
            self.0 = true;
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: winit::window::WindowId,
            _: winit::event::WindowEvent,
        ) {
        }
    }
    let mut trial = Trial(false);
    EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("GPU check");
    assert!(trial.0);
}

#[test]
fn filmstrip_preserves_the_combined_timeline_status_boundary() {
    let Some(_) = crate::tests::isolated_test_root(
        "timeline_edit::panel_tests::filmstrip_preserves_the_combined_timeline_status_boundary",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for kind in [MediaKind::Audio, MediaKind::Video] {
            let mut app = setup(kind, density);
            let context = app.ui_context.clone().expect("context");
            context.global_style_mut(chrome::style);
            for timeline in [true, false] {
                app.timeline_open = timeline;
                for filmstrip in [false, true] {
                    app.filmstrip_open = filmstrip;
                    for _ in 0..3 {
                        let output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(640.0, 400.0),
                                )),
                                ..Default::default()
                            },
                            |ui| app.draw_ui(ui, &mut Vec::new()),
                        );
                        let status = egui::containers::panel::PanelState::load(
                            &context,
                            egui::Id::new("status"),
                        )
                        .expect("status panel")
                        .outer_rect;
                        let has_status_border = output.shapes.iter().any(|shape| {
                            matches!(&shape.shape, egui::Shape::LineSegment { points, stroke }
                                if stroke.color == chrome::BORDER
                                    && (points[1].x - points[0].x).abs() > 630.0
                                    && (points[0].y - status.top()).abs() < 1.0)
                        });
                        assert_eq!(
                            has_status_border, !timeline,
                            "density={density}, kind={kind:?}, timeline={timeline}, filmstrip={filmstrip}"
                        );
                    }
                }
            }
        }
    }
}
