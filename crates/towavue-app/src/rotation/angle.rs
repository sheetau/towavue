use crate::{chrome, localization, resize};
use egui::{Response, Ui};
use localization::Text;

/// The same editable drag control used for loudness targets.
pub(crate) fn input(ui: &mut Ui, angle: &mut String, first_frame: &mut bool) -> Response {
    let language = localization::language(ui.ctx());
    let mut value = angle
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .unwrap_or(0.0);
    let response = ui
        .horizontal(|ui| {
            let label = Text::RotationAngle.in_language(language);
            let width = resize::label_width(ui, &[label]);
            resize::aligned_label(ui, label, width);
            chrome::number_input(
                ui,
                &mut value,
                -180.0..=180.0,
                120.0,
                "°",
                Text::RotationAngleInput.in_language(language),
            )
        })
        .inner;
    if response.changed() {
        *angle = format!("{value:.1}");
    }
    if *first_frame {
        response.request_focus();
        *first_frame = false;
    }
    response.on_hover_text(Text::RotationAngleHelp.in_language(language))
}
