use super::*;
use towavue_core::{PlaybackRange, localization::Language};

fn time(seconds: i64) -> MediaTime {
    MediaTime::from_nanoseconds(seconds * 1_000_000_000)
}
fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(time(start), time(end)).expect("range")
}

#[test]
fn duration_text_round_trips_nanoseconds_and_rejects_invalid_or_overflowing_input() {
    for nanos in [1, 999_999_999, 1_234_567_890, 3_661_001_000_001, i64::MAX] {
        let value = MediaTime::from_nanoseconds(nanos);
        assert_eq!(parse_duration(&format_duration(value)), Some(value));
    }
    for text in [
        "",
        "0",
        "00:00:00",
        "00:60:00",
        "00:00:60",
        "-1:00:00",
        "NaN",
        "1:2:3:4",
        "1:00:00.0000000001",
        "999999999999999999:00:00",
    ] {
        assert_eq!(parse_duration(text), None, "{text}");
    }
    assert_eq!(
        parse_duration("01:02:03.5"),
        Some(MediaTime::from_nanoseconds(3_723_500_000_000))
    );
}

#[test]
fn linked_speed_duration_edits_apply_only_to_the_captured_range_and_preserve_preview_rate() {
    let Some(root) = crate::tests::isolated_test_root(
        "speed_edit::tests::linked_speed_duration_edits_apply_only_to_the_captured_range_and_preserve_preview_rate",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        for selected in [None, Some(range(2, 6))] {
            let mut app = Application::new(None, |_| {}).expect("app");
            let path = root.join("untouched-source");
            std::fs::write(&path, b"unchanged source bytes").expect("source");
            let tab = app.tabs.open_new(path.clone(), kind);
            app.path = Some(path.clone());
            app.media_kind = Some(kind);
            app.media_duration = Some(Duration::from_secs(10));
            app.state = PlaybackState::Paused;
            app.timeline_open = true;
            app.time_selection = selected;
            app.set_preview_rate(1.75);
            app.open_speed_edit();
            assert!(app.modal_input_blocked());
            let dialog = app.speed_dialog.as_mut().expect("dialog");
            let target = selected.unwrap_or(range(0, 10));
            assert_eq!(dialog.range, target);
            assert_eq!(dialog.length(), Some(target.duration()));
            assert!(
                !dialog.valid_length(target.duration()),
                "identity cannot dirty the document"
            );
            dialog.speed = "2".into();
            dialog.sync_linked_input();
            let length = dialog.length().expect("linked duration");
            assert_eq!(
                length.as_nanoseconds(),
                target.duration().as_nanoseconds() / 2
            );
            assert_eq!(parse_duration(&dialog.duration), Some(length));
            dialog.from_speed = false;
            dialog.duration = format_duration(time(8));
            dialog.sync_linked_input();
            assert_eq!(
                dialog.speed.parse::<f64>().expect("speed"),
                target.duration().as_seconds_f64() / 8.0
            );
            let token = dialog.token;
            app.finish_speed_edit(token.wrapping_add(1), Some(length));
            assert!(app.speed_dialog.is_some(), "stale token is inert");
            app.finish_speed_edit(token, Some(length));
            assert!(!app.modal_input_blocked());
            assert_eq!(app.preview_rate(), 1.75);
            assert_eq!(
                app.edits[&tab].operations(),
                &[EditOperation::Timeline(TimelineEdit::Stretch(
                    target, length
                ))]
            );
            let edited = app.edits[&tab].timeline(time(10)).expect("timeline");
            assert_eq!(
                edited.duration().as_nanoseconds(),
                time(10).as_nanoseconds() - target.duration().as_nanoseconds()
                    + length.as_nanoseconds()
            );
            app.undo_edit(false);
            assert!(!app.edits[&tab].is_dirty());
            assert_eq!(
                app.preview_rate(),
                1.75,
                "editing Undo cannot reset listening speed"
            );
            app.undo_edit(true);
            assert_eq!(app.edits[&tab].timeline(time(10)).expect("redo"), edited);
            assert_eq!(
                std::fs::read(&path).expect("source intact"),
                b"unchanged source bytes"
            );
        }
    }
}

#[test]
fn speed_dialog_cancel_stale_history_selection_and_hidden_timeline_never_commit() {
    let Some(root) = crate::tests::isolated_test_root(
        "speed_edit::tests::speed_dialog_cancel_stale_history_selection_and_hidden_timeline_never_commit",
    ) else {
        return;
    };
    for stale in 0..5 {
        let mut app = Application::new(None, |_| {}).expect("app");
        let tab = app.tabs.open_new(root.join("source.wav"), MediaKind::Audio);
        app.media_kind = Some(MediaKind::Audio);
        app.media_duration = Some(Duration::from_secs(10));
        app.state = PlaybackState::Paused;
        app.timeline_open = false;
        app.open_speed_edit();
        assert!(app.speed_dialog.is_none());
        app.timeline_open = true;
        app.open_speed_edit();
        let token = app.speed_dialog.as_ref().expect("dialog").token;
        app.request_guarded(GuardedAction::Exit);
        assert!(!app.exit_requested);
        match stale {
            0 => {}
            1 => app.media_generation = app.media_generation.wrapping_add(1),
            2 => app.time_selection = Some(range(1, 2)),
            3 => {
                app.edits.entry(tab).or_default().push(
                    EditOperation::Timeline(TimelineEdit::ScaleVolume(range(0, 10), 0.5)),
                    MediaKind::Audio,
                );
            }
            _ => app.timeline_open = false,
        }
        let before = app.edits.clone();
        app.finish_speed_edit(token, (stale != 0).then_some(time(5)));
        assert_eq!(app.edits, before);
        assert!(app.speed_dialog.is_none());
    }
}

#[test]
fn mixed_speed_limits_reject_invalid_input_without_clamping_or_creating_history() {
    let Some(root) = crate::tests::isolated_test_root(
        "speed_edit::tests::mixed_speed_limits_reject_invalid_input_without_clamping_or_creating_history",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let tab = app.tabs.open_new(root.join("source.wav"), MediaKind::Audio);
    app.media_kind = Some(MediaKind::Audio);
    app.media_duration = Some(Duration::from_secs(10));
    app.state = PlaybackState::Paused;
    app.timeline_open = true;
    app.push_edit(EditOperation::Timeline(TimelineEdit::Stretch(
        range(0, 4),
        time(1),
    )));
    app.open_speed_edit();
    let dialog = app.speed_dialog.as_mut().expect("dialog");
    assert_eq!(dialog.plan.duration(), time(7));
    for speed in ["NaN", "inf", "0", "-2"] {
        dialog.speed = speed.into();
        assert_eq!(dialog.length(), None);
    }
    dialog.speed = "2".into();
    let invalid = dialog.length().expect("finite input");
    assert!(
        !dialog.valid_length(invalid),
        "already-4x span cannot become 8x"
    );
    let token = dialog.token;
    let before = app.edits[&tab].clone();
    app.finish_speed_edit(token, Some(invalid));
    assert_eq!(app.edits[&tab], before);
}

#[test]
fn dialog_controls_link_inputs_fit_media_bounds_and_emit_one_action() {
    use crate::localization::test_ui::{action, frame, settle, visible_button};
    for language in [Language::English, Language::Japanese] {
        for density in [1.0, 1.25, 2.0] {
            for size in [egui::vec2(960.0, 576.0), egui::vec2(420.0, 260.0)] {
                let context = crate::localization::test_ui::japanese_context(density);
                localization::set_language(&context, language);
                let bounds = egui::Rect::from_min_max(
                    egui::pos2(0.0, 32.0),
                    egui::pos2(size.x, size.y - 24.0),
                );
                let mut tabs = TabSet::default();
                let tab = tabs.open_new("clip.wav".into(), MediaKind::Audio);
                let mut dialog = Dialog {
                    token: 1,
                    tab,
                    instance: 1,
                    history: EditHistory::default(),
                    selection: None,
                    source_duration: Duration::from_secs(10),
                    plan: EditTimeline::new(time(10), PlaybackRange::default()).expect("plan"),
                    range: range(0, 10),
                    speed: "1.00".into(),
                    duration: "00:00:10".into(),
                    from_speed: true,
                };
                let mut show = |context: &egui::Context| {
                    chrome::set_modal_bounds(context, bounds);
                    dialog.show(context)
                };
                let output = settle(&context, size, &mut show);
                assert_eq!(output.pixels_per_point, density);
                let apply = Text::ApplySpeed.in_language(language);
                let cancel = Text::Cancel.in_language(language);
                visible_button(&output, apply, size, false);
                visible_button(&output, cancel, size, true);
                let area = context
                    .memory(|memory| memory.area_rect("speed-duration"))
                    .expect("area");
                assert!(bounds.contains_rect(area), "{area:?} in {bounds:?}");
                assert!(
                    area.width() < 340.0,
                    "compact speed fields: {language:?}, {density}, {area:?}"
                );
                assert!((area.right() - (bounds.right() - 8.0)).abs() < 1.1);
                assert!((area.bottom() - (bounds.bottom() - 8.0)).abs() < 1.1);
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree");
                for label in [Text::SpeedLabel, Text::DurationLabel] {
                    let input = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| {
                            node.label() == Some(label.in_language(language))
                                && node.value().is_some()
                        })
                        .expect("input");
                    let rect = input.1.bounds().expect("input bounds");
                    assert!(
                        (rect.height() - 26.0).abs() <= 1.1 / f64::from(density),
                        "{rect:?}"
                    );
                }
                let (output, actions) = frame(
                    &context,
                    size,
                    vec![action(
                        &output,
                        Text::SpeedLabel.in_language(language),
                        Some("2"),
                    )],
                    &mut show,
                );
                assert!(actions.is_empty());
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree");
                assert!(
                    tree.nodes
                        .iter()
                        .any(|(_, node)| node.value() == Some("00:00:05"))
                );
                let (output, actions) = frame(
                    &context,
                    size,
                    vec![action(
                        &output,
                        Text::DurationLabel.in_language(language),
                        Some("00:00:08"),
                    )],
                    &mut show,
                );
                assert!(actions.is_empty());
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree");
                // The speed widget was already drawn when Duration changed; the next pass must reflect it.
                assert!(
                    tree.nodes
                        .iter()
                        .any(|(_, node)| node.value() == Some("00:00:08"))
                );
                let output = settle(&context, size, &mut show);
                assert!(
                    output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("tree")
                        .nodes
                        .iter()
                        .any(|(_, node)| node.value() == Some("1.25"))
                );
                visible_button(&output, apply, size, true);
                let (_, actions) = frame(
                    &context,
                    size,
                    vec![action(&output, apply, None)],
                    &mut show,
                );
                assert_eq!(actions, vec![Some(time(8))]);
                let output = settle(&context, size, &mut show);
                let (output, actions) = frame(
                    &context,
                    size,
                    vec![action(
                        &output,
                        Text::SpeedLabel.in_language(language),
                        Some("0"),
                    )],
                    &mut show,
                );
                assert!(actions.is_empty());
                visible_button(&output, apply, size, false);
                let (_, actions) = frame(
                    &context,
                    size,
                    vec![action(&output, cancel, None)],
                    &mut show,
                );
                assert_eq!(actions, vec![None]);
                let (_, actions) = frame(
                    &context,
                    size,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    &mut show,
                );
                assert_eq!(actions, vec![None]);
            }
        }
    }
}
