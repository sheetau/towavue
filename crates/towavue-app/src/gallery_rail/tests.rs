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
    egui::pos2(
        616.0,
        egui::lerp(
            (40.0 + 320.0 * 320.0 / 1200.0 * 0.5)..=(360.0 - 320.0 * 320.0 / 1200.0 * 0.5),
            fraction,
        ),
    )
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
    let months: Vec<_> = offsets
        .iter()
        .enumerate()
        .map(|(index, offset)| Month {
            date: Some((2026, 12 - index as u16)),
            offset: *offset,
        })
        .collect();
    frame_months(context, &months, rail(), enabled, events)
}

fn frame_months(
    context: &egui::Context,
    months: &[Month],
    rect: egui::Rect,
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
            ui.add_space(rect.top());
            ui.add_enabled_ui(enabled, |ui| {
                advance(
                    ui.ctx(),
                    ui.make_persistent_id(egui::IdSalt::new("rail-test")),
                );
                let mut output = egui::ScrollArea::vertical()
                    .id_salt("rail-test")
                    .max_height(rect.height())
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_height(1200.0);
                        months
                            .iter()
                            .map(|month| Month {
                                date: month.date,
                                offset: month.offset,
                            })
                            .collect()
                    });
                show(ui, &mut output, rect);
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
            egui::Shape::Rect(shape)
                if shape.fill == color
                    && shape.rect.height() <= 1.01
                    && shape.rect.width() > 20.0 =>
            {
                assert_eq!(shape.corner_radius.nw, 0);
                Some([shape.rect.left_center(), shape.rect.right_center()])
            }
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
fn year_labels_mark_the_oldest_boundary_instead_of_the_newest_month_center() {
    month_labels_in_language(Language::English);
}

#[test]
fn japanese_month_labels_keep_dates_and_rail_geometry() {
    month_labels_in_language(Language::Japanese);
}

fn month_labels_in_language(language: Language) {
    for density in [1.0, 1.25, 2.0] {
        for dates in [
            vec![Some((2026, 9))],
            vec![Some((2026, 9)), Some((2026, 6)), Some((2026, 2))],
            vec![
                Some((2026, 9)),
                Some((2026, 6)),
                Some((2025, 11)),
                Some((2025, 1)),
                None,
            ],
            vec![None],
        ] {
            let context = crate::fonts::test_context();
            if language == Language::Japanese {
                crate::localization::test_ui::configure_japanese(&context, density);
            }
            context.set_pixels_per_point(density);
            context.enable_accesskit();
            context.global_style_mut(chrome::style);
            let months: Vec<_> = dates
                .iter()
                .enumerate()
                .map(|(index, date)| Month {
                    date: *date,
                    offset: index as f32 * 200.0,
                })
                .collect();
            frame_months(&context, &months, rail(), true, vec![]);
            let output = frame_months(&context, &months, rail(), true, vec![]);
            for (index, month) in months.iter().enumerate() {
                let year = month.date.map(|date| date.0);
                if months
                    .get(index + 1)
                    .is_some_and(|next| next.date.map(|date| date.0) == year)
                {
                    let boundary = rail().top()
                        + rail().height()
                            * months
                                .get(index + 1)
                                .map_or(1.0, |month| month.offset / 1200.0);
                    let x = rail().right() - rail().width() * 0.075;
                    assert!(
                        output.output.shapes.iter().any(|shape| matches!(
                            &shape.shape,
                            egui::Shape::Rect(line)
                                if (line.rect.width() - 10.0 / density).abs() < 0.01
                                    && (line.rect.height() - 1.0 / density).abs() < 0.01
                                    && (line.rect.right() - x).abs() <= 0.51 / density
                                    && (line.rect.center().y - boundary).abs() <= 0.51 / density
                        )),
                        "month dot must mark the tooltip boundary at the right edge"
                    );
                    continue;
                }
                let text = year.map_or_else(|| "?".into(), |year| year.to_string());
                let bounds = label(&output, &text);
                let boundary = rail().top()
                    + rail().height()
                        * months
                            .get(index + 1)
                            .map_or(1.0, |month| month.offset / 1200.0);
                assert!(
                    (bounds.bottom() - boundary).abs() <= 1.0 / density,
                    "{text} bottom {} must mark chronological start {boundary}",
                    bounds.bottom()
                );
                assert!(rail().contains_rect(bounds));
            }
            let tree = output
                .output
                .platform_output
                .accesskit_update
                .expect("month tree");
            for month in &months {
                assert!(
                    tree.nodes
                        .iter()
                        .any(|(_, node)| node.role() == egui::accesskit::Role::Button
                            && node.label() == Some(month.label(language).as_str()))
                );
            }
        }
    }
}

#[test]
fn crowded_year_boundaries_do_not_overlap_or_escape_the_rail() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        let months: Vec<_> = (0..12)
            .map(|index| Month {
                date: Some((2026 - index, 1)),
                offset: f32::from(index) * 100.0,
            })
            .collect();
        let rect = egui::Rect::from_min_size(rail().min, egui::vec2(32.0, 48.0));
        let output = frame_months(&context, &months, rect, true, vec![]);
        let mut previous_bottom = rect.top();
        let mut count = 0;
        for shape in output.output.shapes {
            if let egui::Shape::Text(text) = shape.shape {
                let bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
                assert!(rect.contains_rect(bounds));
                assert!(bounds.top() >= previous_bottom);
                previous_bottom = bounds.bottom();
                count += 1;
            }
        }
        assert!(
            count > 0 && count < months.len(),
            "show only labels that fit"
        );
    }
}

#[test]
fn clicks_interpolate_and_thumb_drag_preserves_grab_offset_with_retargeting() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        let offsets = [0.0, 200.0, 500.0, 1100.0];
        for _ in 0..3 {
            frame(&context, &offsets, true, vec![]);
        }
        let pos = point(0.625);
        frame(
            &context,
            &offsets,
            true,
            vec![egui::Event::PointerMoved(pos)],
        );
        let clicked = frame(
            &context,
            &offsets,
            true,
            vec![button(pos, true), button(pos, false)],
        );
        assert_eq!(clicked.maximum, 880.0);
        assert_eq!(clicked.offset, 0.0, "no instantaneous jump");
        let first = frame(&context, &offsets, true, vec![]);
        assert!(first.offset > 0.0 && first.offset < 550.0);
        for _ in 0..70 {
            frame(&context, &offsets, true, vec![]);
        }
        let settled = frame(&context, &offsets, true, vec![]);
        assert!((settled.offset - 550.0).abs() < 0.01);
        assert!((marker(&settled, chrome::FOREGROUND)[0].y - pos.y).abs() <= 0.51 / density);
        let thumb_height = rail().height() * 320.0 / 1200.0;
        let grab = egui::pos2(
            616.0,
            rail().top() + 0.625 * (rail().height() - thumb_height) + 8.0,
        );
        frame(
            &context,
            &offsets,
            true,
            vec![egui::Event::PointerMoved(grab)],
        );
        let pressed = frame(&context, &offsets, true, vec![button(grab, true)]);
        assert!(
            (pressed.offset - settled.offset).abs() < 0.01,
            "grabbing the thumb edge preserves its position"
        );
        let fill = pressed
            .output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(shape) if shape.fill == egui::Color32::from_rgb(24, 24, 24) => {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("hover thumb");
        assert!((fill.height() - thumb_height).abs() < 0.01);
        let moved = grab + egui::vec2(0.0, -40.0);
        frame(
            &context,
            &offsets,
            true,
            vec![egui::Event::PointerMoved(moved)],
        );
        let moving = frame(&context, &offsets, true, vec![]);
        assert!(moving.offset < settled.offset && moving.offset > 200.0);
        // Reverse before arrival: one target replaces the old one.
        let final_point = grab + egui::vec2(0.0, 20.0);
        frame(
            &context,
            &offsets,
            true,
            vec![egui::Event::PointerMoved(final_point)],
        );
        frame(&context, &offsets, true, vec![button(final_point, false)]);
        for _ in 0..70 {
            frame(&context, &offsets, true, vec![]);
        }
        let result = frame(&context, &offsets, true, vec![]);
        let expected = offset_at(880.0, 0.625 + 20.0 / (rail().height() - thumb_height));
        assert!(
            (result.offset - expected).abs() < 0.01,
            "anchored target {expected}, actual {}",
            result.offset
        );
        // Native wheel or keyboard changes take ownership from a pending rail motion.
        let mut state = egui::scroll_area::State::load(&context, result.id).expect("scroll state");
        state.offset.y = 100.0;
        state.store(&context, result.id);
        for _ in 0..10 {
            frame(&context, &offsets, true, vec![]);
        }
        assert_eq!(frame(&context, &offsets, true, vec![]).offset, 100.0);
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
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "September 2026" => Some(text),
                _ => None,
            })
            .expect("month label");
        let reference = context.fonts_mut(|fonts| {
            fonts.layout_no_wrap(
                "H".into(),
                egui::FontId::proportional(12.0),
                chrome::FOREGROUND,
            )
        });
        assert!(
            (text.pos.y + reference.mesh_bounds.center().y - position.y).abs() < 0.01,
            "capital height, not the descender in September, defines the baseline"
        );
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
fn linear_scroll_scale_round_trips_and_has_constant_drag_speed() {
    for maximum in [1.0, 900.0, 50000.0] {
        for index in 0..=100 {
            let fraction = index as f32 / 100.0;
            assert!(
                (fraction_at(maximum, offset_at(maximum, fraction)) - fraction).abs() < 0.00001
            );
        }
        for fraction in [0.1, 0.4, 0.8] {
            assert!(
                ((offset_at(maximum, fraction + 0.05) - offset_at(maximum, fraction)) / maximum
                    - 0.05)
                    .abs()
                    < 0.00001
            );
        }
    }
}
