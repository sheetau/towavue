use egui::{Align2, Color32, Rect, Ui};
use towavue_core::{EditState, MediaTime};

pub fn label(state: &EditState, duration: MediaTime) -> Option<String> {
    if state.trim_start.is_none() && state.trim_end.is_none() {
        return None;
    }
    Some(format!(
        "Trim {} – {} · playback and export",
        timestamp(state.trim_start.unwrap_or(MediaTime::ZERO)),
        timestamp(state.trim_end.unwrap_or(duration)),
    ))
}

pub fn show(ui: &Ui, rect: Rect, state: &EditState, duration: MediaTime, source_preview: bool) {
    let Some(mut label) = label(state, duration) else {
        return;
    };
    let position = |time: MediaTime| {
        egui::lerp(
            rect.x_range(),
            (time.as_seconds_f64() / duration.as_seconds_f64()).clamp(0.0, 1.0) as f32,
        )
    };
    let start = position(state.trim_start.unwrap_or(MediaTime::ZERO));
    let end = position(state.trim_end.unwrap_or(duration));
    let painter = ui.painter().with_clip_rect(rect);
    let font = egui::FontId::proportional(12.0);
    let fits = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= rect.width() - 12.0
    };
    if !fits(&label) {
        label = format!(
            "Trim {} – {}",
            timestamp(state.trim_start.unwrap_or(MediaTime::ZERO)),
            timestamp(state.trim_end.unwrap_or(duration)),
        );
    }
    for excluded in [
        Rect::from_min_max(rect.min, egui::pos2(start, rect.bottom())),
        Rect::from_min_max(egui::pos2(end, rect.top()), rect.max),
    ] {
        painter.rect_filled(excluded, 0.0, Color32::from_black_alpha(160));
    }
    painter.hline(start..=end, rect.bottom() - 2.0, (2.0, Color32::WHITE));
    for x in [start, end] {
        painter.vline(
            x.clamp(rect.left() + 1.0, rect.right() - 1.0),
            rect.y_range(),
            (1.0, Color32::WHITE),
        );
    }
    painter.rect_filled(
        Rect::from_min_size(rect.min, egui::vec2(rect.width(), 20.0)),
        0.0,
        Color32::from_black_alpha(210),
    );
    painter.text(
        rect.min + egui::vec2(6.0, 3.0),
        Align2::LEFT_TOP,
        label,
        font.clone(),
        Color32::WHITE,
    );
    if source_preview {
        let label = if fits("Outside trim · Play returns to start") {
            "Outside trim · Play returns to start"
        } else {
            "Outside trim · Play → start"
        };
        painter.text(
            rect.left_bottom() + egui::vec2(6.0, -6.0),
            Align2::LEFT_BOTTOM,
            label,
            font,
            Color32::WHITE,
        );
    }
}

fn timestamp(time: MediaTime) -> String {
    let millis = time.as_nanoseconds().max(0) / 1_000_000;
    format!(
        "{:02}:{:02}.{:03}",
        millis / 60_000,
        millis / 1_000 % 60,
        millis % 1_000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_trim_feedback_keeps_both_source_endpoints_and_play_hint_visible() {
        let context = egui::Context::default();
        let rect = Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(224.0, 50.0));
        let state = EditState {
            trim_start: Some(MediaTime::from_nanoseconds(2_500_000_000)),
            trim_end: Some(MediaTime::from_nanoseconds(62_750_000_000)),
            ..Default::default()
        };
        let output = context.run_ui(Default::default(), |ui| {
            show(
                ui,
                rect,
                &state,
                MediaTime::from_nanoseconds(120_000_000_000),
                true,
            );
        });
        let texts = output
            .shapes
            .iter()
            .filter_map(|shape| {
                let egui::Shape::Text(text) = &shape.shape else {
                    return None;
                };
                let bounds = Rect::from_min_size(text.pos, text.galley.size());
                assert!(
                    shape.clip_rect.contains_rect(bounds),
                    "{} is clipped: {bounds:?}",
                    text.galley.text()
                );
                Some(text.galley.text())
            })
            .collect::<Vec<_>>();
        assert!(texts.contains(&"Trim 00:02.500 – 01:02.750"));
        assert!(
            texts
                .iter()
                .any(|text| text.contains("Play") && text.contains("start"))
        );
    }

    #[test]
    fn trim_overlay_shades_only_excluded_source_intervals() {
        let context = egui::Context::default();
        let rect = Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(420.0, 130.0));
        let state = EditState {
            trim_start: Some(MediaTime::from_nanoseconds(2_500_000_000)),
            trim_end: Some(MediaTime::from_nanoseconds(7_500_000_000)),
            ..Default::default()
        };
        let output = context.run_ui(Default::default(), |ui| {
            show(
                ui,
                rect,
                &state,
                MediaTime::from_nanoseconds(10_000_000_000),
                false,
            );
        });
        let shaded: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(shade) if shade.fill == Color32::from_black_alpha(160) => {
                    assert_eq!(shape.clip_rect, rect);
                    Some(shade.rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            shaded,
            [
                Rect::from_min_max(rect.min, egui::pos2(120.0, 130.0)),
                Rect::from_min_max(egui::pos2(320.0, 30.0), rect.max),
            ]
        );
    }

    #[test]
    fn trim_label_preserves_subsecond_endpoints_and_implicit_source_edges() {
        let duration = MediaTime::from_nanoseconds(30_000_000_000);
        let mut state = EditState::default();
        assert_eq!(label(&state, duration), None);
        state.trim_end = Some(MediaTime::from_nanoseconds(2_833_333_333));
        assert_eq!(
            label(&state, duration).as_deref(),
            Some("Trim 00:00.000 – 00:02.833 · playback and export")
        );
        state.trim_start = state.trim_end.take();
        assert_eq!(
            label(&state, duration).as_deref(),
            Some("Trim 00:02.833 – 00:30.000 · playback and export")
        );
    }
}
