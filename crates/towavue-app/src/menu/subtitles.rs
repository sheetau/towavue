use super::*;
use crate::subtitles::{Action, Selection};
use towavue_core::SubtitleDelay;

pub(super) fn submenu(
    ui: &mut egui::Ui,
    context: CommandContext,
    requested: Option<egui::Id>,
    data: &mut MenuData<'_>,
    ancestor: Option<egui::Rect>,
) -> (egui::Response, Option<CommandId>) {
    let root = egui::containers::menu::find_menu_root(ui);
    let parent = ui.ctx().read_response(root.id).expect("parent menu").rect;
    let screen = ui.ctx().content_rect();
    let right = screen.right() - parent.right();
    let left = parent.left() - screen.left();
    let side = if ancestor.is_some_and(|root| root.right() <= parent.left()) {
        right
    } else {
        right.max(left)
    };
    let margin = egui::Frame::popup(ui.style()).total_margin().sum().x;
    let width = (side - margin - 2.0 - 1.0 / ui.ctx().pixels_per_point()).max(1.0);
    let (response, contents) = ui
        .add_enabled_ui(!context.playback_blocked, |ui| {
            let category = ui.next_auto_id();
            if requested == Some(category) || crate::logo_menu::drag::submenu(ui, category) {
                let id = egui::containers::menu::SubMenu::id_from_widget_id(category);
                egui::containers::menu::MenuState::mark_shown(ui.ctx(), id);
                egui::containers::menu::MenuState::from_ui(ui, |state, _| {
                    state.open_item = Some(id)
                });
            }
            egui::containers::menu::SubMenuButton::new(text(ui.ctx(), Text::Subtitles)).ui(
                ui,
                |ui| {
                    let delay_key = ui.id().with("subtitle-delay-control");
                    let editing = ui
                        .data(|data| data.get_temp::<egui::Id>(delay_key))
                        .is_some_and(|id| ui.memory(|memory| memory.has_focus(id)));
                    // Leave text editing's arrows/Tab/IME to the numeric editor.
                    let keyboard = (!editing).then(|| MenuKeyboard::begin(ui));
                    let back = keyboard.as_ref().is_some_and(|keyboard| keyboard.left);
                    let mut rows = vec![(
                        Selection::None,
                        text(ui.ctx(), Text::SubtitleNone).to_owned(),
                    )];
                    if let Some(path) = &data.subtitle_settings.external {
                        rows.push((
                            Selection::External,
                            path.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                        ));
                    }
                    if let Some(tracks) = data.subtitle_tracks {
                        rows.extend(tracks.iter().enumerate().map(|(index, track)| {
                            (
                                Selection::Embedded(track.id),
                                crate::subtitles::track_label(language(ui.ctx()), index, track),
                            )
                        }));
                    }
                    let natural = rows
                        .iter()
                        .map(|(_, label)| sizing::text_width(ui, label))
                        .chain(
                            [
                                Text::CommandLoadSubtitles,
                                Text::CommandToggleSubtitles,
                                Text::SubtitleDelay,
                            ]
                            .into_iter()
                            .map(|label| sizing::text_width(ui, text(ui.ctx(), label))),
                        )
                        .fold(120.0, f32::max)
                        + ui.spacing().icon_width
                        + ui.spacing().icon_spacing
                        + 2.0 * ui.spacing().button_padding.x
                        + ui.spacing().scroll.bar_width;
                    let width = natural.min(width);
                    ui.set_width(width);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    let mut command = None;
                    let mut items = Vec::new();
                    let height = sizing::height(ui);
                    crate::logo_menu::drag::scroll(ui, egui::Id::new("subtitles"), height, |ui| {
                        let response = ui.button(text(ui.ctx(), Text::CommandLoadSubtitles));
                        items.push(response.id);
                        if crate::logo_menu::drag::clicked(&response) {
                            command = Some(LoadSubtitles);
                            ui.close();
                        }
                        let response = ui
                            .checkbox(
                                &mut data.subtitle_settings.visible,
                                text(ui.ctx(), Text::CommandToggleSubtitles),
                            )
                            .on_hover_text(text(ui.ctx(), Text::SubtitleExportHelp));
                        items.push(response.id);
                        if crate::logo_menu::drag::released(&response) {
                            data.subtitle_settings.visible = !data.subtitle_settings.visible;
                        }
                        if response.changed() || crate::logo_menu::drag::released(&response) {
                            data.subtitle_action =
                                Some(Action::Show(data.subtitle_settings.visible));
                        }
                        crate::chrome::separator(ui);
                        for (selection, label) in rows {
                            let response = ui
                                .radio(data.subtitle_settings.selection == selection, &label)
                                .on_hover_text(label);
                            items.push(response.id);
                            if crate::logo_menu::drag::clicked(&response) {
                                data.subtitle_action = Some(Action::Select(selection));
                                ui.close();
                            }
                            if response.gained_focus() {
                                response.scroll_to_me(None);
                            }
                        }
                        crate::chrome::separator(ui);
                        let label = ui.label(text(ui.ctx(), Text::SubtitleDelay));
                        let mut delay = f64::from(data.subtitle_settings.delay.tenths()) / 10.0;
                        crate::chrome::input_style(ui);
                        let response = ui
                            .add_sized(
                                [width.min(120.0), crate::chrome::INPUT_HEIGHT],
                                egui::DragValue::new(&mut delay)
                                    .range(f64::from(i32::MIN) / 10.0..=f64::from(i32::MAX) / 10.0)
                                    .speed(0.1)
                                    .fixed_decimals(1)
                                    .suffix(" s")
                                    .custom_parser(|text| {
                                        text.trim()
                                            .trim_end_matches('s')
                                            .trim()
                                            .parse::<f64>()
                                            .ok()
                                            .filter(|number| number.is_finite())
                                    }),
                            )
                            .labelled_by(label.id)
                            .on_hover_text(text(ui.ctx(), Text::SubtitleDelayHelp));
                        ui.data_mut(|data| data.insert_temp(delay_key, response.id));
                        items.push(response.id);
                        if response.changed() && delay.is_finite() {
                            data.subtitle_action = Some(Action::Delay(SubtitleDelay::from_tenths(
                                (delay * 10.0).round() as i32,
                            )));
                        }
                    });
                    if let Some(keyboard) = keyboard {
                        keyboard.finish(ui, items);
                    }
                    (command, back)
                },
            )
        })
        .inner;
    let (command, back) = contents.map(|inner| inner.inner).unwrap_or_default();
    if back {
        egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item = None);
        response.request_focus();
    }
    (response, command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Action as AccessibleAction, ActionRequest, TreeId};
    use towavue_core::{MediaKind, SubtitleTrack, SubtitleTrackId};

    #[test]
    fn subtitles_menu_exposes_loading_visibility_tracks_and_signed_delay_with_bounded_rows() {
        for japanese in [false, true] {
            for density in [1.0, 1.25, 2.0] {
                for width in [480.0, 1200.0] {
                    let context = if japanese {
                        crate::localization::test_ui::japanese_context(density)
                    } else {
                        crate::fonts::test_context()
                    };
                    context.set_pixels_per_point(density);
                    context.enable_accesskit();
                    context.global_style_mut(crate::chrome::style);
                    let tracks = vec![SubtitleTrack {
                        id: SubtitleTrackId::from_index(3),
                        title: Some("Long subtitle title ".repeat(8)),
                        language: Some("jpn".into()),
                    }];
                    let mut settings = crate::subtitles::Settings::default();
                    let chosen = std::cell::RefCell::new(None);
                    let mut frame = |events| {
                        let mut data = MenuData {
                            subtitle_tracks: Some(&tracks),
                            subtitle_settings: settings.clone(),
                            ..Default::default()
                        };
                        let output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 600.0),
                                )),
                                events,
                                ..Default::default()
                            },
                            |ui| {
                                ui.menu_button("Menu", |ui| {
                                    show_items(
                                        ui,
                                        Text::MenuView,
                                        CommandContext {
                                            media_kind: Some(MediaKind::Video),
                                            ..Default::default()
                                        },
                                        &crate::shortcuts::defaults(),
                                        &mut data,
                                        None,
                                    );
                                });
                            },
                        );
                        if let Some(action) = data.subtitle_action {
                            match action {
                                Action::Select(selection) => settings.selection = selection,
                                Action::Show(visible) => settings.visible = visible,
                                Action::Delay(delay) => settings.delay = delay,
                            }
                            *chosen.borrow_mut() = Some(action);
                        }
                        output
                    };
                    let click = |id| {
                        egui::Event::AccessKitActionRequest(ActionRequest {
                            action: AccessibleAction::Click,
                            target_tree: TreeId::ROOT,
                            target_node: id,
                            data: None,
                        })
                    };
                    for label in ["Menu", text(&context, Text::Subtitles)] {
                        for _ in 0..4 {
                            frame(vec![]);
                        }
                        let tree = frame(vec![])
                            .platform_output
                            .accesskit_update
                            .expect("menu tree");
                        let id = tree
                            .nodes
                            .iter()
                            .find(|(_, node)| {
                                node.label().is_some_and(|value| {
                                    value.trim_end_matches('⏵').trim() == label
                                })
                            })
                            .unwrap_or_else(|| panic!("missing {label}"))
                            .0;
                        frame(vec![click(id)]);
                    }
                    for _ in 0..4 {
                        frame(vec![]);
                    }
                    let tree = frame(vec![])
                        .platform_output
                        .accesskit_update
                        .expect("subtitle tree");
                    let label = crate::subtitles::track_label(language(&context), 0, &tracks[0]);
                    for label in [
                        text(&context, Text::CommandLoadSubtitles),
                        text(&context, Text::CommandToggleSubtitles),
                        text(&context, Text::SubtitleDelay),
                        label.as_str(),
                    ] {
                        let (_, node) = tree
                            .nodes
                            .iter()
                            .find(|(_, node)| {
                                node.label() == Some(label) || node.value() == Some(label)
                            })
                            .unwrap_or_else(|| {
                                panic!(
                                    "missing {label}: {:?}",
                                    tree.nodes
                                        .iter()
                                        .map(|(_, node)| (node.role(), node.label(), node.value()))
                                        .collect::<Vec<_>>()
                                )
                            });
                        let bounds = node.bounds().expect("row geometry");
                        assert!(
                            bounds.x0 >= -1.0 && bounds.x1 <= f64::from(width) + 1.0,
                            "row outside screen {bounds:?}"
                        );
                    }
                    let numeric = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| node.numeric_value().is_some())
                        .expect("delay control")
                        .0;
                    frame(vec![click(numeric)]);
                    frame(vec![]);
                    let key = |key, modifiers| egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    };
                    frame(vec![
                        key(egui::Key::A, egui::Modifiers::CTRL),
                        egui::Event::Text("-0.3".into()),
                    ]);
                    frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
                    assert_eq!(
                        *chosen.borrow(),
                        Some(Action::Delay(SubtitleDelay::from_tenths(-3)))
                    );
                    let id = tree
                        .nodes
                        .iter()
                        .find(|(_, node)| node.label() == Some(label.as_str()))
                        .expect("test fixture state")
                        .0;
                    frame(vec![click(id)]);
                    assert_eq!(
                        *chosen.borrow(),
                        Some(Action::Select(Selection::Embedded(tracks[0].id)))
                    );
                }
            }
        }
    }
}
