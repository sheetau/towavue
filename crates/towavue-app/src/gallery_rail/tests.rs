use super::*;

struct Frame {
    output: egui::FullOutput,
    offset: f32,
    maximum: f32,
    id: egui::Id,
}

fn rail() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(600.0, 40.0), egui::pos2(632.0, 360.0))
}

fn point(fraction: f32) -> egui::Pos2 {
    egui::pos2(616.0, egui::lerp(42.0..=358.0, fraction))
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}

fn frame(
    context: &egui::Context,
    offsets: &[f32],
    enabled: bool,
    events: Vec<egui::Event>,
) -> Frame {
    let mut offset = 0.0;
    let mut maximum = 0.0;
    let mut id = egui::Id::NULL;
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            ui.add_space(40.0);
            ui.add_enabled_ui(enabled, |ui| {
                let mut output = egui::ScrollArea::vertical()
                    .id_salt("rail-test")
                    .max_height(320.0)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_height(1200.0);
                        offsets
                            .iter()
                            .enumerate()
                            .map(|(index, offset)| Month {
                                date: Some((2026, 12 - index as u16)),
                                offset: *offset,
                            })
                            .collect()
                    });
                show(ui, &mut output, rail());
                offset = output.state.offset.y;
                maximum = (output.content_size.y - output.inner_rect.height()).max(0.0);
                id = output.id;
            });
        },
    );
    Frame {
        output,
        offset,
        maximum,
        id,
    }
}

fn marker(frame: &Frame, color: egui::Color32) -> [egui::Pos2; 2] {
    frame
        .output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::LineSegment { points, stroke } if stroke.color == color => Some(*points),
            _ => None,
        })
        .expect("position line")
}

fn label(frame: &Frame, expected: &str) -> egui::Rect {
    frame
        .output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == expected => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .expect("immediate month label")
}

#[test]
fn clicks_and_drag_release_share_coordinates_and_keep_ambiguous_positions() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        context.global_style_mut(|style| style.interaction.tooltip_delay = 60.0);
        let offsets = [0.0, 0.0, 500.0, 1100.0];
        frame(&context, &offsets, true, vec![]);
        frame(&context, &offsets, true, vec![]);
        for (fraction, expected) in [(0.125, 0.0), (0.375, 250.0), (0.625, 690.0), (0.875, 880.0)] {
            let pos = point(fraction);
            let hovered = frame(
                &context,
                &offsets,
                true,
                vec![egui::Event::PointerMoved(pos)],
            );
            let line = marker(&hovered, chrome::MUTED);
            assert!((line[1].x - line[0].x - rail().width() * 0.85).abs() < 0.01);
            let name = [
                "December 2026",
                "November 2026",
                "October 2026",
                "September 2026",
            ][(fraction * 4.0) as usize];
            let help = label(&hovered, name);
            assert!((help.center().y - line[0].y).abs() <= 1.0 / density);
            assert!(help.right() <= line[0].x - 4.0);
            let clicked = frame(
                &context,
                &offsets,
                true,
                vec![button(pos, true), button(pos, false)],
            );
            assert_eq!(clicked.maximum, 880.0);
            assert!(
                (clicked.offset - expected).abs() < 0.01,
                "click {fraction}: {}",
                clicked.offset
            );
            for output in [
                clicked,
                frame(&context, &offsets, true, vec![]),
                frame(&context, &offsets, true, vec![]),
            ] {
                assert!((marker(&output, chrome::FOREGROUND)[0].y - pos.y).abs() <= 1.0 / density);
            }
        }
        let start = point(0.1);
        frame(
            &context,
            &offsets,
            true,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        let end = point(0.6) + egui::vec2(-80.0, 0.0);
        let dragged = frame(
            &context,
            &offsets,
            true,
            vec![egui::Event::PointerMoved(end)],
        );
        let help = label(&dragged, "October 2026");
        assert!(
            (help.center().y - marker(&dragged, chrome::FOREGROUND)[0].y).abs() <= 1.0 / density
        );
        let released = point(0.8) + egui::vec2(-80.0, 0.0);
        let output = frame(
            &context,
            &offsets,
            true,
            vec![egui::Event::PointerMoved(released), button(released, false)],
        );
        assert!((marker(&output, chrome::FOREGROUND)[0].y - released.y).abs() <= 1.0 / density);
        let blocked = frame(
            &context,
            &offsets,
            false,
            vec![
                egui::Event::PointerMoved(point(0.3)),
                button(point(0.3), true),
                button(point(0.3), false),
            ],
        );
        assert_eq!(blocked.offset, output.offset);
        assert!(!blocked.output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().contains("2026") && text.galley.text() != "2026")));
        // A wheel/focus scroll invalidates ambiguous cursor memory, even if it returns later.
        let mut state = egui::scroll_area::State::load(&context, output.id).expect("scroll state");
        state.offset.y = 100.0;
        state.store(&context, output.id);
        frame(&context, &offsets, true, vec![]);
        state.offset.y = 880.0;
        state.store(&context, output.id);
        let output = frame(&context, &offsets, true, vec![]);
        assert_eq!(marker(&output, chrome::FOREGROUND)[0].y, 358.0);
        let pos = point(0.125);
        frame(
            &context,
            &offsets,
            true,
            vec![
                egui::Event::PointerMoved(pos),
                button(pos, true),
                button(pos, false),
            ],
        );
        let changed = frame(&context, &[0.0, 100.0, 500.0, 1100.0], true, vec![]);
        assert_eq!(
            marker(&changed, chrome::FOREGROUND)[0].y,
            42.0,
            "changed row offsets invalidate the remembered ambiguous position"
        );
        frame(
            &context,
            &offsets,
            true,
            vec![button(pos, true), button(pos, false)],
        );
        frame(&context, &[], true, vec![]);
        let restored = frame(&context, &offsets, true, vec![]);
        assert_eq!(
            marker(&restored, chrome::FOREGROUND)[0].y,
            42.0,
            "empty filtered results clear the remembered position"
        );
    }
}

#[test]
fn paint_only_month_help_does_not_intercept_underlying_clicks() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        let button_rect =
            egui::Rect::from_min_max(egui::pos2(160.0, 180.0), egui::pos2(300.0, 220.0));
        let position = button_rect.center();
        let frame = |events| {
            let mut clicked = false;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    clicked = ui
                        .interact(button_rect, "underlying-card".into(), egui::Sense::click())
                        .clicked();
                    show_label(
                        ui,
                        "label".into(),
                        egui::pos2(300.0, 200.0),
                        "September 2026".into(),
                    );
                },
            );
            (output, clicked)
        };
        frame(vec![egui::Event::PointerMoved(position)]);
        let output = frame(vec![]).0;
        assert!(
            output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == "September 2026"
                    && egui::Rect::from_min_size(text.pos, text.galley.size()).contains(position)
            )),
            "fixture must paint help over the click position"
        );
        assert!(frame(vec![button(position, true), button(position, false)]).1);
        assert!(!egui::Popup::is_any_open(&context));
    }
}

#[test]
fn strict_month_spans_round_trip_and_single_month_reaches_both_edges() {
    for offsets in [vec![0.0], vec![0.0, 150.0, 550.0, 650.0]] {
        let months: Vec<_> = offsets
            .into_iter()
            .map(|offset| Month { date: None, offset })
            .collect();
        for index in 0..=100 {
            let fraction = index as f32 / 100.0;
            assert!(
                (fraction_at(&months, 900.0, offset_at(&months, 900.0, fraction)) - fraction).abs()
                    < 0.00001
            );
        }
    }
}
