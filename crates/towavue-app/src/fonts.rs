// Share immutable font bytes across native windows. File reads and font selection
// happen once per process, never while laying out a filename or painting a frame.
static INSTALLED: std::sync::OnceLock<(egui::FontDefinitions, bool)> = std::sync::OnceLock::new();
// epaint clones owned FontData bytes when constructing each context's font faces.
// These process-lifetime buffers let every context borrow the same immutable bytes.
static FONT_BYTES: std::sync::OnceLock<towavue_runtime_windows::UiFonts> =
    std::sync::OnceLock::new();

#[cfg(any(test, feature = "presentation-verification"))]
pub fn install(context: &egui::Context) -> bool {
    install_prepared(context, None)
}

pub fn prepare() -> Option<towavue_runtime_windows::UiFontPreparation> {
    if INSTALLED.get().is_some() {
        return None;
    }
    towavue_runtime_windows::UiFontPreparation::start()
        .inspect_err(|error| {
            towavue_runtime_windows::diagnostic!("Could not prepare fonts in background: {error}");
        })
        .ok()
}

pub fn install_prepared(
    context: &egui::Context,
    prepared: Option<towavue_runtime_windows::UiFontPreparation>,
) -> bool {
    let (fonts, available) = INSTALLED.get_or_init(|| {
        let towavue_runtime_windows::UiFonts { japanese, symbols, fallbacks } = FONT_BYTES.get_or_init(|| prepared
            .map(towavue_runtime_windows::UiFontPreparation::finish)
            .unwrap_or_else(towavue_runtime_windows::UiFonts::read));
        let available = japanese.is_some();
        if !available {
            towavue_runtime_windows::diagnostic!("towavue: no installed Japanese UI font was found; using available system and bundled fallback fonts");
        }
        let mut fonts = definitions_from_data(
            japanese.as_ref().map(|(bytes, index)| {
                let mut data = egui::FontData::from_static(bytes);
                data.index = *index;
                data
            }),
            symbols.as_ref().map(|bytes| egui::FontData::from_static(bytes)),
        );
        for fallback in fallbacks {
            let mut data = egui::FontData::from_static(&fallback.data);
            data.index = fallback.index;
            fonts.font_data.insert(fallback.name.into(), data.into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push(fallback.name.into());
            }
        }
        // The dedicated reading-arrow family still takes precedence for those runs.
        if fonts.font_data.contains_key("windows-symbols") {
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("windows-symbols".into());
            }
        }
        (fonts, available)
    });
    context.set_fonts(fonts.clone());
    *available
}

#[cfg(test)]
fn definitions(
    japanese: Option<(Vec<u8>, u32)>,
    symbols: Option<Vec<u8>>,
) -> egui::FontDefinitions {
    definitions_from_data(
        japanese.map(|(bytes, index)| {
            let mut data = egui::FontData::from_owned(bytes);
            data.index = index;
            data
        }),
        symbols.map(egui::FontData::from_owned),
    )
}

fn definitions_from_data(
    japanese: Option<egui::FontData>,
    symbols: Option<egui::FontData>,
) -> egui::FontDefinitions {
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
    fonts.font_data.insert(
        "seti".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/seti.ttf")).into(),
    );
    fonts
        .families
        .insert(egui::FontFamily::Name("seti".into()), vec!["seti".into()]);
    if let Some(data) = japanese {
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
    // A Japanese fallback may supply only U+2194, leaving U+2195 to the
    // heavier emoji fallback. Keep both reading arrows in one symbol face.
    let mut arrows = vec!["Hack".into()];
    if let Some(data) = symbols {
        fonts
            .font_data
            .insert("windows-symbols".into(), data.into());
        arrows.insert(0, "windows-symbols".into());
    }
    fonts
        .families
        .insert(egui::FontFamily::Name("reading-arrows".into()), arrows);
    fonts
}

pub fn reading_hint(text: &str, size: f32, color: egui::Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    append_reading_hint(&mut job, text, size, color);
    job
}

pub fn append_reading_hint(
    job: &mut egui::text::LayoutJob,
    text: &str,
    size: f32,
    color: egui::Color32,
) {
    for part in text.split_inclusive(['\u{2194}', '\u{2195}']) {
        let split = part
            .char_indices()
            .next_back()
            .filter(|(_, chr)| matches!(chr, '\u{2194}' | '\u{2195}'));
        let (plain, arrow) = split.map_or((part, ""), |(index, _)| part.split_at(index));
        job.append(
            plain,
            0.0,
            egui::TextFormat::simple(egui::FontId::proportional(size), color),
        );
        if !arrow.is_empty() {
            job.append(
                arrow,
                0.0,
                egui::TextFormat::simple(
                    egui::FontId::new(size, egui::FontFamily::Name("reading-arrows".into())),
                    color,
                ),
            );
        }
    }
}

pub fn icon_font() -> egui::FontId {
    egui::FontId::new(16.0, egui::FontFamily::Name("codicon".into()))
}

pub fn image_icon_font() -> egui::FontId {
    egui::FontId::new(24.0, egui::FontFamily::Name("seti".into()))
}

#[cfg(test)]
pub fn test_context() -> egui::Context {
    let context = egui::Context::default();
    context.set_fonts(definitions(None, None));
    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_font_covers_japanese_without_replacing_bundled_latin_metrics() {
        let context = egui::Context::default();
        context.set_fonts(definitions(None, None));
        let mut original = 0.0;
        let _ = context.run_ui(Default::default(), |ui| {
            original =
                ui.fonts_mut(|fonts| fonts.glyph_width(&egui::FontId::proportional(14.0), 'A'));
        });
        if !install_prepared(&context, prepare()) {
            eprintln!("SKIP Japanese glyph coverage: no Japanese Windows font is installed");
            return;
        }
        // Include every non-ASCII character used by static and formatted UI text.
        // This stays tied to the catalogs as translations grow, without exposing
        // a production-only enumeration API merely for font verification.
        let characters: std::collections::BTreeSet<_> = concat!(
            include_str!("../../towavue-core/src/localization/catalog.rs"),
            include_str!("../../towavue-core/src/localization/formatted.rs"),
        )
        .chars()
        .filter(|chr| !chr.is_ascii() && !chr.is_whitespace())
        .collect();
        assert!(!characters.is_empty());
        eprintln!(
            "Japanese catalog glyph coverage: {} unique characters in both text families",
            characters.len()
        );
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
                    for chr in &characters {
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
        let fonts = definitions(None, None);
        assert_eq!(
            fonts.families[&egui::FontFamily::Proportional][0],
            "figtree"
        );
        assert!(
            !fonts.families[&egui::FontFamily::Proportional]
                .iter()
                .any(|name| name == "codicon" || name == "seti")
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
                assert!(fonts.has_glyphs(&image_icon_font(), "\u{e04c}"));
                let text = "(\u{2195}) Reading 3 · (\u{2194}) first 3";
                let job = reading_hint(text, 12.0, egui::Color32::WHITE);
                assert_eq!(job.text, text);
                let arrows = egui::FontId::new(12.0, egui::FontFamily::Name("reading-arrows".into()));
                // Compare atlas glyphs, as in the Japanese fallback control above:
                // has_glyphs rejects a face that also supplies the replacement glyph.
                let missing = fonts.layout_no_wrap("\u{10ffff}".into(), arrows.clone(), egui::Color32::WHITE);
                for chr in ['\u{2194}', '\u{2195}'] {
                    let glyph = fonts.layout_no_wrap(chr.to_string(), arrows.clone(), egui::Color32::WHITE);
                    assert_ne!(glyph.rows[0].glyphs[0].uv_rect, missing.rows[0].glyphs[0].uv_rect);
                }
                for section in &job.sections {
                    let run = &job.text[section.byte_range.start.0..section.byte_range.end.0];
                    let expected = if run == "\u{2194}" || run == "\u{2195}" { arrows.clone() } else { egui::FontId::proportional(12.0) };
                    assert_eq!(section.format.font_id, expected);
                }
                assert!(fonts.has_glyphs(&icon_font(), "\u{eabf}\u{eaf1}\u{ea71}\u{ea76}\u{eaa4}\u{eab8}\u{eab9}\u{eaba}\u{eabb}\u{ead1}\u{eb2c}\u{eb31}\u{eb4d}\u{eaee}\u{eaf7}"));
            });
        });
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;

    #[test]
    fn installed_system_font_data_borrows_process_lifetime_buffers() {
        let context = egui::Context::default();
        install_prepared(&context, prepare());
        let bytes = FONT_BYTES.get().expect("prepared system fonts");
        let definitions = &INSTALLED.get().expect("installed definitions").0;
        let check = |name: &str, original: &[u8], index: u32| {
            let data = &definitions.font_data[name];
            let std::borrow::Cow::Borrowed(shared) = &data.font else {
                panic!("system font {name} would be copied during face construction");
            };
            assert_eq!(shared.as_ptr(), original.as_ptr(), "{name}");
            assert_eq!(shared.len(), original.len(), "{name}");
            assert_eq!(data.index, index, "{name}");
        };
        if let Some((data, index)) = &bytes.japanese {
            check("japanese-fallback", data, *index);
        }
        if let Some(data) = &bytes.symbols {
            check("windows-symbols", data, 0);
        }
        for fallback in &bytes.fallbacks {
            check(fallback.name, &fallback.data, fallback.index);
        }
        // A new context still uses the same definitions after the earlier one drops.
        drop(context);
        let later = egui::Context::default();
        assert!(prepare().is_none());
        install_prepared(&later, None);
        let _ = later.run_ui(Default::default(), |ui| {
            ui.label("A\u{65e5}\u{672c}\u{2194}\u{2195}");
        });
    }

    #[test]
    fn borrowed_font_bytes_preserve_owned_layout_and_atlas_across_density_changes() {
        let shared = egui::Context::default();
        install_prepared(&shared, prepare());
        let mut owned_definitions = INSTALLED.get().expect("installed definitions").0.clone();
        for data in owned_definitions.font_data.values_mut() {
            let mut owned = (**data).clone();
            owned.font = std::borrow::Cow::Owned(owned.font.into_owned());
            *data = std::sync::Arc::new(owned);
        }
        let owned = egui::Context::default();
        owned.set_fonts(owned_definitions);
        let catalog: String = concat!(
            include_str!("../../towavue-core/src/localization/catalog.rs"),
            include_str!("../../towavue-core/src/localization/formatted.rs"),
        )
        .chars()
        .filter(|chr| !chr.is_whitespace())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
        let samples = [
            catalog.as_str(),
            "龙鸟龜體龍鳥國點कকਕકକகకಕകකกขກកᨀကခဂሀߊⴰꔀᎠᐁཀཁꡀꡁ↔↕",
            "0123456789 Aa ffi \u{eabf}\u{eaf1}\u{ea71}\u{10ffff}",
        ];
        for density in [1.0, 1.25, 1.5, 2.0, 1.0] {
            let mut snapshots = Vec::new();
            for context in [&owned, &shared] {
                context.set_pixels_per_point(density);
                let mut layouts = Vec::new();
                let _ = context.run_ui(Default::default(), |ui| {
                    ui.fonts_mut(|fonts| {
                        for size in [11.0, 12.0, 14.0, 20.0] {
                            for family in [
                                egui::FontFamily::Proportional,
                                egui::FontFamily::Monospace,
                                egui::FontFamily::Name("codicon".into()),
                                egui::FontFamily::Name("reading-arrows".into()),
                            ] {
                                for sample in samples {
                                    layouts.push(fonts.layout_no_wrap(
                                        sample.into(),
                                        egui::FontId::new(size, family.clone()),
                                        egui::Color32::WHITE,
                                    ));
                                }
                            }
                        }
                    });
                });
                snapshots.push((layouts, context.fonts(|fonts| fonts.image())));
            }
            assert_eq!(snapshots[0], snapshots[1], "density {density}");
            assert_eq!(owned.cumulative_frame_nr(), shared.cumulative_frame_nr());
        }
    }

    #[test]
    fn installed_windows_fallbacks_fill_missing_scripts_and_share_bytes_across_windows() {
        let baseline = egui::Context::default();
        baseline.set_fonts(definitions(
            towavue_runtime_windows::japanese_ui_font(),
            towavue_runtime_windows::ui_symbol_font(),
        ));
        let metrics = |fonts: &mut egui::epaint::text::FontsView<'_>| {
            [egui::FontFamily::Proportional, egui::FontFamily::Monospace]
                .into_iter()
                .map(|family| {
                    fonts
                        .layout_no_wrap(
                            "File 09:18 / 8888 日本語".into(),
                            egui::FontId::new(14.0, family),
                            egui::Color32::WHITE,
                        )
                        .size()
                })
                .collect::<Vec<_>>()
        };
        let mut old_metrics = Vec::new();
        let _ = baseline.run_ui(Default::default(), |ui| {
            old_metrics = ui.fonts_mut(metrics);
        });
        let started = std::time::Instant::now();
        let first = egui::Context::default();
        install(&first);
        let read_time = started.elapsed();
        let second = egui::Context::default();
        install(&second);
        let cached = INSTALLED.get().expect("installed definitions");
        let mut covered = 0;
        for context in [&first, &second] {
            let _ = context.run_ui(Default::default(), |ui| {
                ui.fonts_mut(|fonts| {
                    assert_eq!(
                        metrics(fonts),
                        old_metrics,
                        "existing Latin, digits and Japanese metrics stay unchanged"
                    );
                    for (name, data) in &cached.0.font_data {
                        assert!(
                            std::sync::Arc::ptr_eq(data, &fonts.definitions().font_data[name]),
                            "font bytes shared for {name}"
                        );
                    }
                    for (name, sample) in [
                        ("windows-malgun", "한국어사진동영상가힣"),
                        ("windows-segoe-ui", "ΩЖאبԱა"),
                        ("windows-yahei", "龙鸟龜體"),
                        ("windows-jhenghei", "龍鳥國點"),
                        ("windows-nirmala", "कকਕકକகకಕകක"),
                        ("windows-leelawadee", "กขກកᨀ"),
                        ("windows-myanmar", "ကခဂ"),
                        ("windows-ebrima", "ሀߊⴰꔀ"),
                        ("windows-gadugi", "Ꭰᐁ"),
                        ("windows-himalaya", "ཀཁ"),
                        ("windows-phagspa", "ꡀꡁ"),
                    ] {
                        if !fonts.definitions().font_data.contains_key(name) {
                            continue;
                        }
                        covered += 1;
                        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace]
                        {
                            let font = egui::FontId::new(14.0, family);
                            let missing = fonts.layout_no_wrap(
                                "\u{10ffff}".into(),
                                font.clone(),
                                egui::Color32::WHITE,
                            );
                            let replacement = missing.rows[0].glyphs[0].uv_rect;
                            for chr in sample.chars() {
                                let text = fonts.layout_no_wrap(
                                    chr.to_string(),
                                    font.clone(),
                                    egui::Color32::WHITE,
                                );
                                let glyph = text.rows[0].glyphs[0].uv_rect;
                                assert_ne!(
                                    glyph, replacement,
                                    "missing {chr} from installed {name}"
                                );
                                assert!(glyph.max[0] > glyph.min[0] && glyph.max[1] > glyph.min[1]);
                            }
                        }
                    }
                    assert_eq!(
                        fonts.definitions().families[&egui::FontFamily::Name("codicon".into())],
                        ["codicon"]
                    );
                });
            });
        }
        let bytes: usize = cached
            .0
            .font_data
            .values()
            .map(|data| data.font.len())
            .sum();
        eprintln!(
            "PASS installed UI fallback: {covered} family/window coverage controls; shared bytes={bytes}; first-install-or-cache time={read_time:?}; total two-context test time={:?}",
            started.elapsed()
        );
    }
}
