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
                context.request_discard("verify resize collapse across layout passes");
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
            "resize does not seek, edit or collapse while held"
        );
        assert!(app.timeline_open);
    }
}

#[test]
fn resize_below_minimum_collapses_once_without_changing_media_state() {
    let Some(_) = crate::tests::isolated_test_root(
        "timeline_edit::panel_tests::resize_below_minimum_collapses_once_without_changing_media_state",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        for scale in [1.0, 1.5, 2.0] {
            let mut app = setup(kind, scale);
            let tab = app.tabs.active_id().expect("tab");
            let history = app.edits[&tab].clone();
            let selection = app.time_selection;
            let generation = app.generation;
            let position = app.current_position();
            let minimum = rect(&app).center_bottom() - egui::vec2(0.0, 64.0);
            drag(&mut app, minimum);
            assert!(frame(&mut app, vec![button(minimum, false)], true).is_empty());
            assert!(app.timeline_open, "minimum alone does not collapse");
            assert!((rect(&app).height() - 64.0).abs() <= 1.0);
            let below = minimum + egui::vec2(0.0, 16.0);
            drag(&mut app, below);
            let actions = frame(
                &mut app,
                vec![
                    button(below, false),
                    egui::Event::PointerMoved(minimum - egui::vec2(0.0, 80.0)),
                ],
                true,
            );
            assert!(
                matches!(actions.as_slice(), [UiAction::CollapseTimeline(id, 7)] if *id == tab),
                "one owner-bound collapse after a real release"
            );
            let action = actions.into_iter().next().expect("collapse");
            app.handle_ui_action(action.clone());
            assert!(!app.timeline_is_visible());
            app.handle_ui_action(action.clone());
            assert!(
                !app.timeline_open,
                "a repeated action cannot reopen the timeline"
            );
            assert!(frame(&mut app, vec![], true).is_empty());
            assert_eq!(app.edits[&tab], history);
            assert_eq!(app.time_selection, selection);
            assert_eq!(app.playback_selection, selection);
            assert_eq!(app.current_position(), position);
            assert_eq!(app.generation, generation);
            assert_eq!(app.state, PlaybackState::Paused);
            app.timeline_open = true;
            app.media_generation = 8;
            app.handle_ui_action(action.clone());
            assert!(
                app.timeline_open,
                "old media cannot collapse its replacement"
            );
            app.media_generation = 7;
            let other = app.tabs.open_new("other.mp4".into(), MediaKind::Video);
            app.handle_ui_action(action);
            assert_eq!(app.tabs.active_id(), Some(other));
            assert!(
                app.timeline_open,
                "old tab cannot collapse the new active tab"
            );
        }
    }
}

#[test]
fn resize_overshoot_can_return_or_cancel_before_release() {
    let Some(_) = crate::tests::isolated_test_root(
        "timeline_edit::panel_tests::resize_overshoot_can_return_or_cancel_before_release",
    ) else {
        return;
    };
    for interruption in 0..6 {
        let mut app = setup(MediaKind::Video, 1.0);
        let before = rect(&app);
        let below = before.center_bottom() - egui::vec2(0.0, 48.0);
        drag(&mut app, below);
        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let events = match interruption {
            0 => vec![escape, button(below, false)],
            1 => vec![egui::Event::WindowFocused(false), button(below, false)],
            2 => {
                assert!(
                    app.cancel_view_drag(),
                    "native cancellation owns the resize"
                );
                vec![button(below, false)]
            }
            3 => {
                app.pending_guard = Some(GuardedAction::Exit);
                vec![button(below, false)]
            }
            4 => {
                let returned = before.center_bottom() - egui::vec2(0.0, 68.0);
                assert!(
                    frame(&mut app, vec![egui::Event::PointerMoved(returned)], true).is_empty()
                );
                vec![button(returned, false)]
            }
            _ => vec![button(below, false), escape],
        };
        let actions = frame(&mut app, events, true);
        if interruption == 5 {
            assert!(
                matches!(actions.as_slice(), [UiAction::CollapseTimeline(..)]),
                "release before Escape is already committed"
            );
            for action in actions {
                app.handle_ui_action(action);
            }
            assert!(!app.timeline_open);
        } else {
            assert!(
                actions.is_empty(),
                "cancelled or returned resize must not collapse"
            );
            app.pending_guard = None;
            assert!(frame(&mut app, vec![], false).is_empty());
            assert!(app.timeline_open);
            let expected = if interruption == 4 {
                68.0
            } else {
                before.height()
            };
            assert!((rect(&app).height() - expected).abs() <= 1.0);
        }
    }
}
