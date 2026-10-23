use super::*;
use towavue_runtime_windows::SubtitleText;

pub(super) fn paint_positioned(
    ui: &egui::Ui,
    viewport: egui::Rect,
    line: &SubtitleText,
    appearance: Appearance,
) {
    let layout = &line.layout;
    let scale = (viewport.width() / layout.canvas.0).min(viewport.height() / layout.canvas.1);
    let canvas = egui::Rect::from_center_size(
        viewport.center(),
        egui::vec2(layout.canvas.0, layout.canvas.1) * scale,
    );
    let alignment = layout.alignment.clamp(1, 9) - 1;
    let horizontal = usize::from(alignment % 3);
    let vertical = usize::from(alignment / 3);
    let anchor = layout.position.map_or_else(
        || {
            egui::pos2(
                [
                    canvas.left() + layout.margins[0] * scale,
                    canvas.center().x,
                    canvas.right() - layout.margins[1] * scale,
                ][horizontal],
                [
                    canvas.bottom() - layout.margins[2] * scale,
                    canvas.center().y,
                    canvas.top() + layout.margins[2] * scale,
                ][vertical],
            )
        },
        |(x, y)| canvas.min + egui::vec2(x, y) * scale,
    );
    let mut font = egui::TextStyle::Body.resolve(ui.style());
    font.size =
        (layout.font_size * scale * f32::from(appearance.scale_percent) / 100.0).clamp(8.0, 256.0);
    let horizontal_align = [egui::Align::Min, egui::Align::Center, egui::Align::Max][horizontal];
    let vertical_fraction = [1.0, 0.5, 0.0][vertical];
    let edge = if layout.vertical || vertical == 1 {
        [Edge::Left, Edge::Center, Edge::Right][horizontal]
    } else if vertical == 0 {
        Edge::Bottom
    } else {
        Edge::Top
    };
    let mut glyphs = Vec::new();
    if layout.vertical {
        // Vertical fonts use upright glyphs, top-to-bottom with columns advancing
        // right-to-left. Unicode vertical forms keep punctuation upright too.
        let columns: Vec<_> = line.text.lines().collect();
        let rows = columns
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        if rows == 0 || columns.is_empty() {
            return;
        }
        font.size = font
            .size
            .min((viewport.height() - 4.0) / rows as f32)
            .min((viewport.width() - 4.0) / columns.len() as f32)
            .max(1.0);
        let size = egui::vec2(columns.len() as f32, rows as f32) * font.size;
        let origin = anchor - egui::vec2(horizontal as f32 * 0.5, vertical_fraction) * size;
        for (column, text) in columns.iter().enumerate() {
            for (row, character) in text.chars().enumerate() {
                let character = match character {
                    '、' => '︑',
                    '。' => '︒',
                    '「' => '﹁',
                    '」' => '﹂',
                    '『' => '﹃',
                    '』' => '﹄',
                    '（' => '︵',
                    '）' => '︶',
                    'ー' | '―' => '︱',
                    other => other,
                };
                let galley = ui.fonts_mut(|fonts| {
                    fonts.layout_no_wrap(character.to_string(), font.clone(), egui::Color32::WHITE)
                });
                let cell = origin
                    + egui::vec2((columns.len() - column - 1) as f32, row as f32) * font.size;
                let position = cell + (egui::vec2(font.size, font.size) - galley.size()) * 0.5;
                glyphs.push((position, galley));
            }
        }
    } else {
        let width = (canvas.width() - (layout.margins[0] + layout.margins[1]) * scale)
            .max(1.0)
            .min((viewport.width() - 4.0).max(1.0));
        let galley = loop {
            let mut job = egui::text::LayoutJob::simple(
                line.text.clone(),
                font.clone(),
                egui::Color32::WHITE,
                width,
            );
            job.halign = horizontal_align;
            job.wrap.break_anywhere = true;
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            if galley.mesh_bounds.height() + 2.0 * outline_radius(font.size) <= viewport.height()
                || font.size <= 1.0
            {
                break galley;
            }
            font.size = (font.size * 0.8).max(1.0);
        };
        let position = anchor - egui::vec2(0.0, galley.size().y * vertical_fraction);
        glyphs.push((position, galley));
    }
    let bounds = glyphs
        .iter()
        .fold(egui::Rect::NOTHING, |bounds, (position, galley)| {
            bounds.union(ink_bounds(galley, *position, font.size))
        });
    let delta = translation(bounds, viewport, edge, appearance.edge_percent);
    for (position, galley) in glyphs {
        outlined(ui, viewport, position + delta, galley, font.size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_runtime_windows::SubtitleLayout;

    #[test]
    fn edge_distance_moves_each_cue_inward_and_ink_ignores_inherited_padding() {
        for density in [1.0, 1.25, 2.0] {
            let context = crate::localization::test_ui::japanese_context(density);
            let viewport =
                egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
            for (alignment, vertical, edge) in [
                (8, false, Edge::Top),
                (2, false, Edge::Bottom),
                (4, true, Edge::Left),
                (6, true, Edge::Right),
            ] {
                for percent in [0, 10, 50] {
                    let frame = || {
                        context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(viewport.expand(20.0)),
                                ..Default::default()
                            },
                            |ui| {
                                ui.set_clip_rect(viewport.shrink(24.0));
                                paint_positioned(
                                    ui,
                                    viewport,
                                    &SubtitleText {
                                        text: "日本語、。".into(),
                                        layout: SubtitleLayout {
                                            canvas: (800.0, 600.0),
                                            alignment,
                                            vertical,
                                            font_size: 32.0,
                                            ..Default::default()
                                        },
                                    },
                                    Appearance {
                                        scale_percent: 150,
                                        edge_percent: Some(percent),
                                    },
                                );
                            },
                        )
                    };
                    frame();
                    let output = frame();
                    let mut ink = egui::Rect::NOTHING;
                    for shape in &output.shapes {
                        if let egui::Shape::Text(text) = &shape.shape {
                            let bounds = text.galley.mesh_bounds.translate(text.pos.to_vec2());
                            assert!(
                                shape.clip_rect.expand(0.01).contains_rect(bounds),
                                "glyph/outline clipped: {bounds:?} / {:?}",
                                shape.clip_rect
                            );
                            ink = ink.union(bounds);
                        }
                    }
                    let (gap, expected) = match edge {
                        Edge::Top => (ink.top() - viewport.top(), viewport.height()),
                        Edge::Bottom => (viewport.bottom() - ink.bottom(), viewport.height()),
                        Edge::Left => (ink.left() - viewport.left(), viewport.width()),
                        Edge::Right => (viewport.right() - ink.right(), viewport.width()),
                        Edge::Center => unreachable!(),
                    };
                    assert!(
                        (gap - expected * f32::from(percent) / 100.0).abs() <= 0.02,
                        "edge distance at {density}x: {gap}"
                    );
                }
            }
        }
    }

    #[test]
    fn independent_top_bottom_and_vertical_cues_follow_size_and_edge_distance() {
        let context = crate::localization::test_ui::japanese_context(1.0);
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let frame = |appearance| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    ..Default::default()
                },
                |ui| {
                    for (text, alignment, vertical) in [
                        ("Name", 8, false),
                        ("Dialogue", 2, false),
                        ("日本語", 9, true),
                    ] {
                        paint_positioned(
                            ui,
                            viewport,
                            &SubtitleText {
                                text: text.into(),
                                layout: SubtitleLayout {
                                    canvas: (800.0, 600.0),
                                    font_size: 24.0,
                                    alignment,
                                    vertical,
                                    ..Default::default()
                                },
                            },
                            appearance,
                        );
                    }
                },
            )
        };
        frame(Appearance::default());
        let output = frame(Appearance::default());
        let fills = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.override_text_color.is_none() => Some((
                        text.galley.text().to_owned(),
                        text.galley.rect.translate(text.pos.to_vec2()),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let normal = fills(&output);
        let name = normal
            .iter()
            .find(|(text, _)| text == "Name")
            .expect("name")
            .1;
        let dialogue = normal
            .iter()
            .find(|(text, _)| text == "Dialogue")
            .expect("dialogue")
            .1;
        assert!(name.bottom() < 100.0 && dialogue.top() > 500.0);
        let vertical: Vec<_> = normal
            .iter()
            .filter(|(text, _)| ["日", "本", "語"].contains(&text.as_str()))
            .collect();
        assert_eq!(vertical.len(), 3);
        assert!(
            vertical
                .windows(2)
                .all(|pair| pair[0].1.top() < pair[1].1.top())
        );
        assert!(vertical.iter().all(|(_, rect)| rect.left() > 700.0));
        let changed = fills(&frame(Appearance {
            scale_percent: 150,
            edge_percent: Some(10),
        }));
        let changed_dialogue = changed
            .iter()
            .find(|(text, _)| text == "Dialogue")
            .expect("changed")
            .1;
        assert!(changed_dialogue.width() > dialogue.width() * 1.4);
        assert!(changed_dialogue.bottom() < dialogue.bottom());
        let changed_name = changed
            .iter()
            .find(|(text, _)| text == "Name")
            .expect("top caption")
            .1;
        assert!(changed_name.top() > name.top());
        let changed_vertical = changed
            .iter()
            .find(|(text, _)| text == "日")
            .expect("right caption")
            .1;
        assert!(changed_vertical.right() < vertical[0].1.right());
    }
}
