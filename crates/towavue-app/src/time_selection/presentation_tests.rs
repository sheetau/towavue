use super::*;

#[test]
fn japanese_timeline_numeric_controls_keep_ids_units_and_edit_routing() {
    use egui::accesskit::{Action, ActionData, ActionRequest, TreeId};
    for density in [1.0, 1.25, 2.0] {
        let context = crate::localization::test_ui::japanese_context(density);
        let id = egui::Id::new("localized-timeline");
        let range = TimeRange::new(time(2), time(8)).expect("range");
        let frame = |events| {
            let mut results = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 220.0),
                    )),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.interact(
                        Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 100.0)),
                        id,
                        egui::Sense::click_and_drag(),
                    );
                    results.push(show(
                        ui,
                        &response,
                        time(10),
                        time(1),
                        Some(range),
                        None,
                        true,
                    ));
                },
            );
            (output, results)
        };
        let output = frame(vec![]).0;
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("tree");
        let controls = [
            ("再生位置（秒）", id, 1.0),
            (
                "選択範囲の開始位置（秒）",
                id.with(("selection-value", true)),
                2.0,
            ),
            (
                "選択範囲の終了位置（秒）",
                id.with(("selection-value", false)),
                8.0,
            ),
            (
                "相対音量（%）",
                id.with(("timeline-adjustment-value", false)),
                100.0,
            ),
            (
                "選択範囲の長さ（秒）",
                id.with(("timeline-adjustment-value", true)),
                6.0,
            ),
        ];
        for (label, id, value) in controls {
            let (node_id, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .expect("numeric control");
            assert_eq!(*node_id, id.accesskit_id());
            assert_eq!(node.numeric_value(), Some(value));
        }
        let event = |id: egui::Id, action, value: Option<f64>| {
            egui::Event::AccessKitActionRequest(ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: id.accesskit_id(),
                data: value.map(ActionData::NumericValue),
            })
        };
        let seconds = |value| crate::media_time(std::time::Duration::from_secs_f64(value));
        let result = frame(vec![event(id, Action::SetValue, Some(4.5))])
            .1
            .remove(0);
        assert_eq!(result.seek, Some(seconds(4.5)));
        assert!(result.selection.is_none() && result.edit.is_none());
        for (start, value) in [(true, 3.25), (false, 7.5)] {
            let result = frame(vec![event(
                id.with(("selection-value", start)),
                Action::SetValue,
                Some(value),
            )])
            .1
            .remove(0);
            assert_eq!(
                result.selection,
                Some(TimeRange::new(
                    if start { seconds(value) } else { time(2) },
                    if start { time(8) } else { seconds(value) }
                ))
            );
            assert!(result.seek.is_none() && result.edit.is_none());
        }
        for (stretch, value, expected) in [
            (false, 125.0, TimelineEdit::ScaleVolume(range, 1.25)),
            (true, 5.0, TimelineEdit::Stretch(range, time(5))),
        ] {
            let result = frame(vec![event(
                id.with(("timeline-adjustment-value", stretch)),
                Action::SetValue,
                Some(value),
            )])
            .1
            .remove(0);
            assert_eq!(result.edit, Some(expected));
            assert!(result.selection.is_none() && result.seek.is_none());
        }
        frame(vec![event(
            id.with(("selection-value", true)),
            Action::Focus,
            None,
        )]);
        frame(vec![]);
        let hint = focus_hint(&context).expect("focus hint");
        assert!(hint.starts_with("選択範囲の開始位置: 00:00:02:000"));
        assert!(hint.ends_with("←／→で調整"));
        let result = frame(vec![egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }])
        .1
        .remove(0);
        assert_eq!(
            result.selection,
            Some(TimeRange::new(seconds(2.1), time(8)))
        );
    }
}

fn context(density: f32) -> egui::Context {
    let context = crate::fonts::test_context();
    context.set_pixels_per_point(density);
    context.global_style_mut(crate::chrome::style);
    context.global_style_mut(|style| {
        style.animation_time = 0.0;
        style.interaction.tooltip_delay = 0.0;
    });
    context
}

fn time(seconds: u64) -> MediaTime {
    crate::media_time(std::time::Duration::from_secs(seconds))
}

fn text_bounds(output: &egui::FullOutput, prefix: &str) -> Rect {
    let texts: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text().starts_with(prefix) => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 1, "one {prefix} caption");
    let text = texts[0];
    assert!(
        text.galley
            .job
            .sections
            .iter()
            .all(|section| section.format.font_id.size == LABEL_SIZE)
    );
    Rect::from_min_size(text.pos, text.galley.size())
}

#[test]
fn timeline_captions_share_font_and_edges_without_covering_loading() {
    caption_layout(Language::English);
}

#[test]
fn japanese_timeline_captions_preserve_alignment_and_leave_loading_visible() {
    caption_layout(Language::Japanese);
}

fn caption_layout(language: Language) {
    let Some(_root) = crate::tests::isolated_test_root(
        "time_selection::presentation_tests::timeline_captions_share_font_and_edges_without_covering_loading",
    ) else {
        return;
    };
    let mut app = crate::Application::new(None, |_| {}).expect("app");
    app.waveform_loading = true;
    for density in [1.0, 1.25, 2.0] {
        for width in [240.0, 640.0] {
            let context = context(density);
            if language == Language::Japanese {
                crate::localization::test_ui::configure_japanese(&context, density);
            }
            let rect = Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(width, 80.0));
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width + 40.0, 160.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let response =
                        ui.interact(rect, "caption-test".into(), egui::Sense::click_and_drag());
                    let result = show(
                        ui,
                        &response,
                        time(10),
                        time(0),
                        TimeRange::new(time(2), time(8)),
                        None,
                        true,
                    );
                    assert!(
                        result.selection.is_none()
                            && result.seek.is_none()
                            && result.edit.is_none()
                    );
                    app.draw_waveform_activity(ui, rect);
                },
            );
            let volume = text_bounds(
                &output,
                if language == Language::English {
                    "Gain "
                } else {
                    "倍率 "
                },
            );
            let length = text_bounds(
                &output,
                if language == Language::English {
                    "Length "
                } else {
                    "長さ "
                },
            );
            let start = text_bounds(
                &output,
                if language == Language::English {
                    "In "
                } else {
                    "開始 "
                },
            );
            let end = text_bounds(
                &output,
                if language == Language::English {
                    "Out "
                } else {
                    "終了 "
                },
            );
            let loading = text_bounds(
                &output,
                if language == Language::English {
                    "Loading waveform"
                } else {
                    "波形を読み込み中"
                },
            );
            let tolerance = 1.0 / density;
            for left in [volume.left(), start.left()] {
                assert!((left - rect.left() - LABEL_INSET).abs() <= tolerance);
            }
            for right in [length.right(), end.right()] {
                assert!((rect.right() - right - LABEL_INSET).abs() <= tolerance);
            }
            assert!((loading.center().y - rect.center().y).abs() <= tolerance);
            for caption in [volume, length, start, end] {
                assert!(!loading.intersects(caption));
                assert!(rect.contains_rect(caption));
            }
            assert!(rect.contains_rect(loading));
        }
    }
}

#[test]
fn timeline_help_is_local_to_the_hovered_part_and_absent_on_empty_space() {
    for density in [1.0, 1.25, 2.0] {
        for (pointer, selected, enabled, expected) in [
            (
                egui::pos2(20.0, 34.0),
                true,
                true,
                Some("Playback position"),
            ),
            (egui::pos2(220.0, 80.0), true, true, Some("Relative gain")),
            (egui::pos2(120.0, 60.0), true, true, Some("Selection start")),
            (egui::pos2(320.0, 60.0), true, true, Some("Selection end")),
            (egui::pos2(220.0, 55.0), true, true, Some("Time selection")),
            (egui::pos2(380.0, 55.0), true, true, None),
            (egui::pos2(220.0, 55.0), false, true, None),
            (egui::pos2(220.0, 80.0), true, false, None),
        ] {
            let context = context(density);
            let mut descriptions = Vec::new();
            for frame in 0..6 {
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(500.0, 240.0),
                        )),
                        time: Some(f64::from(frame)),
                        events: vec![egui::Event::PointerMoved(pointer)],
                        ..Default::default()
                    },
                    |ui| {
                        let rect =
                            Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(400.0, 100.0));
                        let response =
                            ui.interact(rect, "help-test".into(), egui::Sense::click_and_drag());
                        let selection = if selected {
                            TimeRange::new(
                                crate::media_time(std::time::Duration::from_millis(2500)),
                                crate::media_time(std::time::Duration::from_millis(7500)),
                            )
                        } else {
                            None
                        };
                        let result =
                            show(ui, &response, time(10), time(0), selection, None, enabled);
                        assert!(
                            result.selection.is_none()
                                && result.seek.is_none()
                                && result.edit.is_none()
                        );
                    },
                );
                descriptions = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text().contains(" · ") => {
                            Some(text.galley.text().to_owned())
                        }
                        _ => None,
                    })
                    .collect();
            }
            assert_eq!(
                descriptions.len(),
                usize::from(expected.is_some()),
                "{pointer:?} {expected:?}: {descriptions:?}"
            );
            if let Some(expected) = expected {
                assert!(descriptions[0].starts_with(expected));
            }
        }
    }
}
