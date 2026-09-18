use super::*;

type App = Application<fn(AppEvent)>;

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
                let larger = minimum - egui::vec2(0.0, 80.0);
                drag(&mut app, larger);
                assert!(frame(&mut app, vec![button(larger, false)], false).is_empty());
                assert!((rect(&app).height() - 144.0).abs() <= 1.0);
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
                assert!((rect(&app).height() - 144.0).abs() <= 1.0 / scale);
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
