pub fn install(context: &egui::Context) -> bool {
    let japanese = towavue_runtime_windows::japanese_ui_font();
    let available = japanese.is_some();
    if !available {
        eprintln!(
            "towavue: no installed Japanese UI font was found; using bundled UI and default fallback fonts"
        );
    }
    context.set_fonts(definitions(japanese));
    available
}

fn definitions(japanese: Option<(Vec<u8>, u32)>) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "figtree".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Figtree-Tabular.ttf")).into(),
    );
    fonts.font_data.insert(
        "codicon".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/codicon.ttf")).into(),
    );
    fonts.families.insert(
        egui::FontFamily::Name("codicon".into()),
        vec!["codicon".into()],
    );
    if let Some((bytes, index)) = japanese {
        let mut data = egui::FontData::from_owned(bytes);
        data.index = index;
        fonts
            .font_data
            .insert("japanese-fallback".into(), data.into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "japanese-fallback".into());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("japanese-fallback".into());
    }
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "figtree".into());
    fonts
}

pub fn icon_font() -> egui::FontId {
    egui::FontId::new(16.0, egui::FontFamily::Name("codicon".into()))
}

#[cfg(test)]
pub fn test_context() -> egui::Context {
    let context = egui::Context::default();
    context.set_fonts(definitions(None));
    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_font_covers_japanese_without_replacing_bundled_latin_metrics() {
        let context = egui::Context::default();
        context.set_fonts(definitions(None));
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
                    let font = egui::FontId::new(14.0, family);
                    let missing = fonts.layout_no_wrap(
                        "\u{10ffff}".into(),
                        font.clone(),
                        egui::Color32::WHITE,
                    );
                    let replacement = missing.rows[0].glyphs[0].uv_rect;
                    // egui's has_glyph compares face identity, so it rejects real glyphs
                    // when the same fallback face also supplies the replacement glyph.
                    for chr in "日本語画像ひらがなカタカナ".chars() {
                        let galley = fonts.layout_no_wrap(
                            chr.to_string(),
                            font.clone(),
                            egui::Color32::WHITE,
                        );
                        let uv = galley.rows[0].glyphs[0].uv_rect;
                        assert!(uv.max[0] > uv.min[0] && uv.max[1] > uv.min[1]);
                        assert_ne!(uv, replacement, "missing Japanese glyph {chr}");
                    }
                }
                assert_eq!(
                    fonts.glyph_width(&egui::FontId::proportional(14.0), 'A'),
                    original
                );
            });
        });
    }

    #[test]
    fn bundled_ui_font_has_tabular_digits_and_isolated_icons_without_os_fonts() {
        let context = egui::Context::default();
        let fonts = definitions(None);
        assert_eq!(
            fonts.families[&egui::FontFamily::Proportional][0],
            "figtree"
        );
        assert!(
            !fonts.families[&egui::FontFamily::Proportional]
                .iter()
                .any(|name| name == "codicon")
        );
        context.set_fonts(fonts);
        let _ = context.run_ui(Default::default(), |ui| {
            ui.fonts_mut(|fonts| {
                for size in [11.0, 12.0, 14.0, 20.0] {
                    let font = egui::FontId::proportional(size);
                    let width = fonts.glyph_width(&font, '0');
                    assert!(width > 0.0);
                    for digit in '1'..='9' {
                        assert_eq!(fonts.glyph_width(&font, digit), width);
                    }
                    let narrow = fonts.layout_no_wrap("11:11 / 1111".into(), font.clone(), egui::Color32::WHITE);
                    let wide = fonts.layout_no_wrap("88:88 / 8888".into(), font, egui::Color32::WHITE);
                    assert!((narrow.size().x - wide.size().x).abs() < 0.01);
                }
                assert!(fonts.has_glyphs(&icon_font(), "\u{ea71}\u{ea76}\u{eaa4}\u{eab8}\u{eab9}\u{eaba}\u{eabb}\u{ead1}\u{eb2c}\u{eb31}\u{eb4d}\u{eaee}\u{eaf7}"));
            });
        });
    }
}
