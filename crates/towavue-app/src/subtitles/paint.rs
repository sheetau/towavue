use super::*;
use towavue_runtime_windows::SubtitleContent;

#[derive(Default)]
pub(super) struct Cache {
    document: Option<Arc<SubtitleDocument>>,
    images: BTreeMap<usize, Vec<egui::TextureHandle>>,
}

impl Cache {
    pub(super) fn show(
        &mut self,
        ui: &egui::Ui,
        document: &Arc<SubtitleDocument>,
        position: MediaTime,
        delay: SubtitleDelay,
    ) {
        if !self
            .document
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, document))
        {
            self.images.clear();
            self.document = Some(Arc::clone(document));
        }
        let viewport = ui.max_rect().intersect(ui.clip_rect());
        if viewport.width() < 8.0 || viewport.height() < 8.0 {
            return;
        }
        let active = document.active(position, delay).collect::<Vec<_>>();
        self.images
            .retain(|index, _| active.iter().any(|(active, _)| active == index));
        let mut text = String::new();
        let painter = ui.painter().with_clip_rect(viewport);
        for (index, cue) in active {
            match cue.content() {
                SubtitleContent::Text(line) => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(line);
                }
                SubtitleContent::Bitmap(images) => {
                    let textures = self.images.entry(index).or_insert_with(|| {
                        images
                            .iter()
                            .map(|image| {
                                let (width, height) = image.size();
                                ui.ctx().load_texture(
                                    "subtitle",
                                    egui::ColorImage::from_rgba_unmultiplied(
                                        [width as usize, height as usize],
                                        &image.rgba(),
                                    ),
                                    egui::TextureOptions::LINEAR,
                                )
                            })
                            .collect()
                    });
                    for (image, texture) in images.iter().zip(textures.iter()) {
                        let (width, height) = image.size();
                        let bounds = if let Some((canvas_width, canvas_height)) = image.canvas() {
                            let scale = (viewport.width() / canvas_width as f32)
                                .min(viewport.height() / canvas_height as f32);
                            let canvas =
                                egui::vec2(canvas_width as f32, canvas_height as f32) * scale;
                            let origin = viewport.center() - canvas * 0.5;
                            egui::Rect::from_min_size(
                                origin + egui::vec2(image.x as f32, image.y as f32) * scale,
                                egui::vec2(width as f32, height as f32) * scale,
                            )
                        } else {
                            let scale = ((viewport.width() - 8.0) / width as f32)
                                .min((viewport.height() - 8.0) / height as f32)
                                .min(1.0);
                            let size = egui::vec2(width as f32, height as f32) * scale;
                            egui::Rect::from_min_size(
                                egui::pos2(
                                    viewport.center().x - size.x * 0.5,
                                    viewport.bottom() - size.y - 4.0,
                                ),
                                size,
                            )
                        };
                        painter.image(
                            texture.id(),
                            bounds,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                }
            }
        }
        if !text.is_empty() {
            paint_text(ui, viewport, &text);
        }
    }
}

fn paint_text(ui: &egui::Ui, viewport: egui::Rect, text: &str) {
    let inset = (viewport.width() * 0.025).clamp(4.0, 24.0);
    let bounds = viewport.shrink2(egui::vec2(inset, 4.0));
    if !bounds.is_positive() {
        return;
    }
    let mut font = egui::TextStyle::Body.resolve(ui.style());
    font.size = (viewport.height() * 0.04).clamp(18.0, 28.0);
    let galley = loop {
        let mut job = egui::text::LayoutJob::simple(
            text.to_owned(),
            font.clone(),
            egui::Color32::WHITE,
            (bounds.width() - 4.0).max(1.0),
        );
        job.halign = egui::Align::Center;
        job.wrap.break_anywhere = true;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        if galley.size().y <= bounds.height() - 4.0 || font.size <= 8.0 {
            break galley;
        }
        font.size = (font.size * 0.8).max(8.0);
    };
    let position = egui::pos2(
        bounds.center().x,
        (bounds.bottom() - galley.size().y - 2.0).max(bounds.top() + 2.0),
    );
    let painter = ui.painter().with_clip_rect(viewport);
    let radius = (font.size / 14.0).clamp(1.0, 2.0);
    for (x, y) in [
        (-1.0, -1.0),
        (0.0, -1.0),
        (1.0, -1.0),
        (-1.0, 0.0),
        (1.0, 0.0),
        (-1.0, 1.0),
        (0.0, 1.0),
        (1.0, 1.0),
    ] {
        let mut shape = egui::epaint::TextShape::new(
            position + egui::vec2(x, y) * radius,
            Arc::clone(&galley),
            egui::Color32::BLACK,
        );
        shape.override_text_color = Some(egui::Color32::BLACK);
        painter.add(shape);
    }
    painter.galley(position, galley, egui::Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_core::{SubtitleCue, SubtitleTimeline};

    #[test]
    fn text_wraps_inside_the_viewport_with_centered_white_fill_and_black_outline() {
        for density in [1.0, 1.25, 2.0] {
            for size in [
                egui::vec2(1200.0, 700.0),
                egui::vec2(360.0, 200.0),
                egui::vec2(180.0, 100.0),
            ] {
                for text in [
                    "A long subtitle with multiple words that must wrap within a small video window.",
                    "これは字幕の折り返しと中央揃えを確認するための長い文章です。ウィンドウの端で文字が切れないことを確認します。",
                ] {
                    let context = crate::localization::test_ui::japanese_context(density);
                    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                    let mut cache = Cache::default();
                    let document = Arc::new(SubtitleTimeline::new(vec![
                        SubtitleCue::new(
                            MediaTime::from_nanoseconds(1_000_000_000),
                            MediaTime::from_nanoseconds(3_000_000_000),
                            SubtitleContent::Text(text.into()),
                        )
                        .expect("test fixture state"),
                    ]));
                    let mut frame = |time, delay| {
                        context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(viewport),
                                ..Default::default()
                            },
                            |root| {
                                egui::CentralPanel::default().frame(egui::Frame::NONE).show(
                                    root,
                                    |ui| {
                                        cache.show(
                                            ui,
                                            &document,
                                            MediaTime::from_nanoseconds(time),
                                            SubtitleDelay::from_tenths(delay),
                                        )
                                    },
                                );
                            },
                        )
                    };
                    frame(1_500_000_000, 0);
                    let output = frame(1_500_000_000, 0);
                    let shapes = output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) => Some(text),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    let fill = shapes
                        .iter()
                        .find(|shape| shape.override_text_color.is_none())
                        .expect("white subtitle");
                    assert_eq!(fill.fallback_color, egui::Color32::WHITE);
                    assert_eq!(fill.galley.text(), text);
                    assert_eq!(
                        shapes
                            .iter()
                            .filter(|shape| shape.override_text_color == Some(egui::Color32::BLACK))
                            .count(),
                        8
                    );
                    for shape in &shapes {
                        let bounds = shape.galley.rect.translate(shape.pos.to_vec2());
                        assert!(
                            viewport.expand(0.5).contains_rect(bounds),
                            "subtitle outside viewport: {bounds:?} {viewport:?}"
                        );
                    }
                    let bounds = fill.galley.rect.translate(fill.pos.to_vec2());
                    assert!(
                        (bounds.center().x - viewport.center().x).abs() < 1.0,
                        "centered: {bounds:?}"
                    );
                    assert!(
                        bounds.bottom() <= viewport.bottom()
                            && bounds.bottom() >= viewport.bottom() - 10.0
                    );
                    if size.x <= 360.0 {
                        assert!(fill.galley.rows.len() > 1);
                    }
                    for (time, delay) in [(3_000_000_000, 0), (900_000_000, 0), (1_000_000_000, 1)]
                    {
                        assert!(
                            !frame(time, delay)
                                .shapes
                                .iter()
                                .any(|shape| matches!(shape.shape, egui::Shape::Text(_)))
                        );
                    }
                    assert!(
                        frame(900_000_000, -1)
                            .shapes
                            .iter()
                            .any(|shape| matches!(shape.shape, egui::Shape::Text(_)))
                    );
                }
            }
        }
    }
}
