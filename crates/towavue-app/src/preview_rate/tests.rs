use super::*;

#[test]
fn preview_rate_commands_preserve_clean_state_history_redo_and_per_tab_values() {
    let Some(root) = crate::tests::isolated_test_root(
        "preview_rate::tests::preview_rate_commands_preserve_clean_state_history_redo_and_per_tab_values",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        let mut app = Application::new(None, |_| {}).expect("app");
        let first = app.tabs.open_new(root.join("first.mp4"), kind);
        app.media_kind = Some(kind);
        app.state = PlaybackState::Paused;
        app.media_duration = Some(Duration::from_secs(10));
        app.dispatch(CommandId::RateUp);
        assert_eq!(app.preview_rate(), 1.25);
        assert!(app.edits.is_empty());
        assert!(
            !app.status_notice()
                .expect("speed notice")
                .contains("export")
        );
        for _ in 0..30 {
            app.dispatch(CommandId::RateUp);
        }
        assert_eq!(app.preview_rate(), 4.0);
        for _ in 0..30 {
            app.dispatch(CommandId::RateDown);
        }
        assert_eq!(app.preview_rate(), 0.25);
        app.dispatch(CommandId::ResetRate);
        assert_eq!(app.preview_rate(), 1.0);
        let range =
            towavue_core::TimeRange::new(MediaTime::ZERO, media_time(Duration::from_secs(10)))
                .expect("range");
        app.push_edit(EditOperation::Timeline(
            towavue_core::TimelineEdit::ScaleVolume(range, 0.5),
        ));
        app.undo_edit(false);
        let history = app.edits[&first].clone();
        app.dispatch(CommandId::RateUp);
        assert_eq!(
            app.edits[&first], history,
            "speed preserves the redo branch"
        );
        app.undo_edit(true);
        assert_eq!(app.preview_rate(), 1.25);
        assert_eq!(app.edits[&first].state().rate, 1.0);
        let second = app.tabs.open_new(root.join("second.mp4"), kind);
        assert_eq!(app.preview_rate(), 1.0);
        app.set_preview_rate(3.0);
        app.tabs.activate(first);
        assert_eq!(app.preview_rate(), 1.25);
        app.tabs.activate(second);
        assert_eq!(app.preview_rate(), 3.0);
        app.remove_tab(second, false);
        assert!(!app.preview_rates.contains_key(&second));
        assert_eq!(app.preview_rates.get(&first), Some(&1.25));
    }
}
