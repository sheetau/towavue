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
    ) + egui::vec2(
        0.0,
        viewport.height() * f32::from(appearance.offset_percent) / 100.0,
    );
    let mut font = egui::TextStyle::Body.resolve(ui.style());
    font.size =
        (layout.font_size * scale * f32::from(appearance.scale_percent) / 100.0).clamp(8.0, 256.0);
    let horizontal_align = [egui::Align::Min, egui::Align::Center, egui::Align::Max][horizontal];
    let vertical_fraction = [1.0, 0.5, 0.0][vertical];
    if layout.vertical {
        // Vertical fonts use upright glyphs, top-to-bottom with columns advancing
        // right-to-left. Unicode vertical forms keep punctuation upright too.
        let columns: Vec<_> = line.text.lines().collect();
        let rows = columns
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
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
                outlined(ui, viewport, position, galley, font.size);
            }
        }
    } else {
        let width = (canvas.width() - (layout.margins[0] + layout.margins[1]) * scale).max(1.0);
        let mut job = egui::text::LayoutJob::simple(
            line.text.clone(),
            font.clone(),
            egui::Color32::WHITE,
            width,
        );
        job.halign = horizontal_align;
        job.wrap.break_anywhere = true;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let position = anchor - egui::vec2(0.0, galley.size().y * vertical_fraction);
        outlined(ui, viewport, position, galley, font.size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_runtime_windows::SubtitleLayout;

    #[test]
    fn independent_top_bottom_and_vertical_cues_follow_size_and_offset() {
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
            offset_percent: -10,
        }));
        let changed_dialogue = changed
            .iter()
            .find(|(text, _)| text == "Dialogue")
            .expect("changed")
            .1;
        assert!(changed_dialogue.width() > dialogue.width() * 1.4);
        assert!((dialogue.bottom() - changed_dialogue.bottom() - 60.0).abs() < 2.0);
    }
}
