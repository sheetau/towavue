use super::*;

pub(super) fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        )
        .size()
        .x
}

pub(super) fn height(ui: &mut egui::Ui) -> f32 {
    // Retain the existing top/bottom clearance independently of the popup's
    // cached placement, which can flip above its anchor in a small viewport.
    let height = (ui.ctx().content_rect().height() - 64.0).max(1.0);
    // Area remembers its last size; a small viewport must not become a lasting cap.
    ui.set_max_height(height);
    height
}

pub(super) fn items(
    ui: &mut egui::Ui,
    title: Text,
    groups: &[&[CommandId]],
    context: CommandContext,
    shortcuts: &ShortcutBindings,
    ancestor: Option<egui::Rect>,
) {
    let gap = ui.spacing().item_spacing.x;
    let row = |label: &str, shortcut: &str, submenu: bool| {
        text_width(ui, label)
            + if shortcut.is_empty() {
                0.0
            } else {
                text_width(ui, shortcut) + 2.0 * gap
            }
            + if submenu {
                egui::WidgetText::from(crate::chrome::chevron())
                    .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Button)
                    .size()
                    .x
                    + 2.0 * gap
            } else {
                0.0
            }
    };
    let mut width = 0.0_f32;
    for id in groups.iter().flat_map(|group| group.iter()) {
        if matches!(id, LoadSubtitles | CycleAudioTrack | ExportQualityHigh)
            && context.media_kind != Some(towavue_core::MediaKind::Video)
        {
            continue;
        }
        let (label, submenu, shortcut) = if *id == LoadSubtitles {
            (text(ui.ctx(), Text::Subtitles), true, String::new())
        } else if *id == CycleAudioTrack {
            (
                text(ui.ctx(), Text::AudioTracks),
                true,
                shortcuts.label(*id, context),
            )
        } else if let Some((label, _)) = choices::options(*id) {
            let shortcut = if matches!(id, ToggleImageInterpolation | ToggleImageMinification) {
                shortcuts.label(*id, context)
            } else {
                String::new()
            };
            (text(ui.ctx(), label), true, shortcut)
        } else {
            (
                command_definitions()
                    .iter()
                    .find(|definition| definition.id == *id)
                    .expect("registered menu command")
                    .title_in(language(ui.ctx())),
                false,
                shortcuts.label(*id, context),
            )
        };
        width = width.max(row(label, &shortcut, submenu));
    }
    let extras: &[Text] = match title {
        Text::MenuFile => &[Text::OpenRecent],
        Text::MenuView => &[
            Text::MenuImageJump,
            Text::MenuVideoSeek,
            Text::DisplayLanguage,
        ],
        _ => &[],
    };
    for label in extras {
        width = width.max(row(text(ui.ctx(), *label), "", true));
    }
    let frame = egui::Frame::popup(ui.style()).total_margin().sum().x;
    let available =
        (ui.ctx().content_rect().width() - frame - ancestor.map_or(0.0, |rect| rect.width() + 2.0))
            .max(1.0);
    ui.set_max_width(available);
    choices::reserve_cascade(ui, groups, context, ancestor);
    let width = crate::logo_menu::drag::scroll_width(
        ui,
        egui::Id::new(title),
        width + 2.0 * ui.spacing().button_padding.x,
    );
    ui.set_width(width.min(ui.available_width()).max(1.0));
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_scroll_does_not_reveal_the_old_keyboard_selection() {
        let context = crate::fonts::test_context();
        context.global_style_mut(crate::chrome::style);
        let label = command_definitions()
            .iter()
            .find(|definition| definition.id == ToggleFullscreen)
            .expect("test fixture")
            .title_in(crate::localization::Language::English);
        let mut time = 0.0;
        let mut draw = |events| {
            time += 0.1;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 300.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let opener = ui.button("Open");
                    egui::Popup::menu(&opener)
                        .style(crate::chrome::menu_style)
                        .open(true)
                        .show(|ui| {
                            show_items(
                                ui,
                                Text::MenuView,
                                CommandContext {
                                    media_kind: Some(towavue_core::MediaKind::Image),
                                    ..Default::default()
                                },
                                &crate::shortcuts::defaults(),
                                &mut MenuData::default(),
                                None,
                            );
                        });
                },
            );
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => Some(text.pos),
                _ => None,
            })
        };
        let mut initial = None;
        for _ in 0..5 {
            initial = draw(vec![]);
        }
        let initial = initial.expect("first menu item");
        draw(vec![egui::Event::PointerMoved(
            initial + egui::vec2(10.0, 60.0),
        )]);
        draw(vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        }]);
        for _ in 0..30 {
            let current = draw(vec![]);
            assert!(
                current.is_none_or(|point| point.y < initial.y - 20.0),
                "old focus must not snap back: {current:?}"
            );
        }
    }

    #[test]
    fn menus_fit_single_line_rows_and_recover_height_after_viewport_growth() {
        for density in [1.0, 1.25, 2.0] {
            for display in [
                crate::localization::Language::English,
                crate::localization::Language::Japanese,
            ] {
                for title in [Text::MenuView, Text::MenuImageJump] {
                    let context = crate::localization::test_ui::japanese_context(density);
                    crate::localization::set_language(&context, display);
                    let bindings = crate::shortcuts::defaults();
                    let commands = CommandContext {
                        media_kind: Some(towavue_core::MediaKind::Image),
                        ..Default::default()
                    };
                    let mut heights = Vec::new();
                    for size in [
                        egui::vec2(960.0, 1100.0),
                        egui::vec2(320.0, 240.0),
                        egui::vec2(960.0, 1100.0),
                    ] {
                        let mut bounds = egui::Rect::NOTHING;
                        let mut output = egui::FullOutput::default();
                        for _ in 0..4 {
                            output = context.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(egui::Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        size,
                                    )),
                                    ..Default::default()
                                },
                                |ui| {
                                    let opener = ui.button("Menu");
                                    let popup = egui::Popup::menu(&opener)
                                        .open(true)
                                        .show(|ui| {
                                            show_items(
                                                ui,
                                                title,
                                                commands,
                                                &bindings,
                                                &mut MenuData::default(),
                                                None,
                                            );
                                        })
                                        .expect("open menu");
                                    bounds = popup.response.rect;
                                },
                            );
                        }
                        assert!(
                            bounds.left() >= -1.0 && bounds.right() <= size.x + 1.0,
                            "{bounds:?}, viewport={size:?}"
                        );
                        assert!(
                            bounds.top() >= -1.0 && bounds.bottom() <= size.y + 1.0,
                            "{bounds:?}, viewport={size:?}"
                        );
                        for shape in &output.shapes {
                            if let egui::Shape::Text(text) = &shape.shape {
                                assert_eq!(
                                    text.galley.rows.len(),
                                    1,
                                    "menu row wrapped: {}",
                                    text.galley.text()
                                );
                            }
                        }
                        heights.push(bounds.height());
                    }
                    assert!(
                        heights[0] > heights[1] + 100.0,
                        "menu must use the tall viewport: {heights:?}"
                    );
                    assert!(
                        (heights[0] - heights[2]).abs() <= 2.0 / density,
                        "menu must recover its height: {heights:?}"
                    );
                    if title == Text::MenuView {
                        assert!(
                            heights[0] > 1000.0,
                            "long menu fills available height: {heights:?}"
                        );
                    }
                }
            }
        }
    }
}
