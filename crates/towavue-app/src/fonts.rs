pub fn install(context: &egui::Context) -> bool {
    let Some(bytes) = towavue_runtime_windows::japanese_ui_font() else {
        eprintln!("towavue: no installed Japanese UI font was found; using default fonts");
        return false;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "japanese-fallback".into(),
        egui::FontData::from_owned(bytes).into(),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("japanese-fallback".into());
    }
    context.set_fonts(fonts);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_font_covers_japanese_without_replacing_latin_metrics() {
        let context = egui::Context::default();
        let mut original = 0.0;
        let _ = context.run_ui(Default::default(), |ui| {
            original =
                ui.fonts_mut(|fonts| fonts.glyph_width(&egui::FontId::proportional(14.0), 'A'));
        });
        if !install(&context) {
            eprintln!("SKIP Japanese glyph coverage: no Japanese Windows font is installed");
            return;
        }
        let _ = context.run_ui(Default::default(), |ui| {
            ui.fonts_mut(|fonts| {
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    assert!(fonts.has_glyphs(
                        &egui::FontId::new(14.0, family),
                        "日本語画像ひらがなカタカナ"
                    ));
                }
                assert_eq!(
                    fonts.glyph_width(&egui::FontId::proportional(14.0), 'A'),
                    original
                );
            });
        });
    }
}
