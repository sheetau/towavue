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
            if !app.timeline_open {
                // The fixture has no native session. Exercise the actual compact
                // widget against the resize's remaining pointer events nonetheless.
                let (_, target, opened) = seekbar::show(
                    &context,
                    egui::Rect::from_min_max(egui::pos2(0.0, 376.0), egui::pos2(640.0, 400.0)),
                    0.2,
                    None,
                    true,
                    true,
                );
                if target.is_some() {
                    actions.push(UiAction::Seek(MediaTime::ZERO));
                }
                if opened {
                    actions.push(UiAction::Command(CommandId::ToggleTimeline));
                }
            }
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
            "resize stays within the collapse dead zone"
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
        for scale in [1.0, 1.25, 2.0] {
            for released in [false, true] {
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
                let larger = minimum - egui::vec2(0.0, 80.0);
                drag(&mut app, larger);
                assert!(frame(&mut app, vec![button(larger, false)], false).is_empty());
                let before = rect(&app);
                assert!((before.height() - 144.0).abs() <= 1.0);
                drag(&mut app, minimum + egui::vec2(0.0, 4.0));
                let below = minimum + egui::vec2(0.0, 16.0);
                let events = if released {
                    vec![button(below, false)]
                } else {
                    vec![egui::Event::PointerMoved(below)]
                };
                let actions = frame(&mut app, events, true);
                assert!(
                    matches!(actions.as_slice(), [UiAction::CollapseTimeline(id, 7)] if *id == tab),
                    "one owner-bound action at the threshold, before release when held"
                );
                assert_eq!(
                    app.ui_context
                        .as_ref()
                        .expect("context")
                        .input(|input| input.pointer.primary_down()),
                    !released
                );
                assert_eq!(
                    rect(&app),
                    before,
                    "collapse retains the pre-drag size, including a batched release"
                );
                let action = actions.into_iter().next().expect("collapse");
                app.handle_ui_action(action.clone());
                assert!(!app.timeline_is_visible());
                app.handle_ui_action(action.clone());
                assert!(!app.timeline_open, "duplicate delivery is idempotent");
                assert!(frame(&mut app, vec![], true).is_empty());
                let compact = egui::pos2(500.0, 376.0);
                assert!(frame(&mut app, vec![egui::Event::PointerMoved(compact)], true).is_empty());
                assert!(
                    frame(&mut app, vec![button(compact, false)], true).is_empty(),
                    "the resize tail cannot seek or reopen through the compact control"
                );
                assert!(frame(&mut app, vec![button(compact, true)], false).is_empty());
                let fresh = frame(&mut app, vec![button(compact, false)], false);
                assert!(
                    matches!(fresh.as_slice(), [UiAction::Seek(_)]),
                    "a fresh compact click still works"
                );
                assert_eq!(app.edits[&tab], history);
                assert_eq!(app.time_selection, selection);
                assert_eq!(app.playback_selection, selection);
                assert_eq!(app.current_position(), position);
                assert_eq!(app.generation, generation);
                assert_eq!(app.state, PlaybackState::Paused);
                app.dispatch(CommandId::ToggleTimeline);
                assert!(frame(&mut app, vec![], true).is_empty());
                assert!(
                    (rect(&app).height() - before.height()).abs() <= 1.0 / scale,
                    "reopening restores the height before the closing gesture"
                );
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
}

#[test]
fn collapse_respects_event_order_cancellation_and_the_dead_zone() {
    let Some(_) = crate::tests::isolated_test_root(
        "timeline_edit::panel_tests::collapse_respects_event_order_cancellation_and_the_dead_zone",
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
        let actions = frame(&mut app, events, true);
        if matches!(interruption, 5..=7) {
            assert!(
                matches!(actions.as_slice(), [UiAction::CollapseTimeline(..)]),
                "a threshold crossing commits before later events"
            );
            for action in actions {
                app.handle_ui_action(action);
            }
            assert!(!app.timeline_open);
            assert_eq!(rect(&app), before);
        } else {
            assert!(
                actions.is_empty(),
                "cancellation or an earlier release wins: {interruption}"
            );
            app.pending_guard = None;
            assert!(frame(&mut app, vec![], false).is_empty());
            assert!(app.timeline_open);
            let expected = if matches!(interruption, 4 | 8) {
                64.0
            } else {
                before.height()
            };
            assert!(
                (rect(&app).height() - expected).abs() <= 1.0,
                "cancelled resize restores its initial height: {interruption}"
            );
        }
    }
}
