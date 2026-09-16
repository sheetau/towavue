use super::*;

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
            let volume = text_bounds(&output, "Volume ");
            let length = text_bounds(&output, "Length ");
            let start = text_bounds(&output, "In ");
            let end = text_bounds(&output, "Out ");
            let loading = text_bounds(&output, "Loading waveform");
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
            (egui::pos2(220.0, 80.0), true, true, Some("Volume line")),
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
