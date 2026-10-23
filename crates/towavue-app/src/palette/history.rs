use super::*;
use crate::localization::{Text, language, text};
use towavue_core::localization::formatted;

impl CommandPalette {
    pub(super) fn show_commands(
        &mut self,
        ui: &mut egui::Ui,
        commands: CommandContext,
        shortcuts: &ShortcutBindings,
        history: &[CommandId],
        navigation: (Navigation, bool),
        query_changed: bool,
    ) -> Option<Choice> {
        let language = language(ui.ctx());
        let (navigation, enter) = navigation;
        let query = self.query[1..].trim().to_lowercase();
        let (matches, recent_count, direct_count) = search::ranked(&query, language, history);
        let enabled: Vec<_> = matches
            .iter()
            .map(|definition| definition.is_enabled(commands))
            .collect();
        let previous_index = self.selected;
        self.selected = self
            .selected_command
            .and_then(|command| {
                matches
                    .iter()
                    .position(|definition| definition.id == command)
            })
            .filter(|index| enabled[*index])
            .or_else(|| enabled.iter().position(|enabled| *enabled));
        self.selected = navigate_enabled(self.selected, &enabled, navigation, self.list_height);
        let selection_changed = previous_index != self.selected
            || self.selected_command != self.selected.map(|index| matches[index].id);
        self.selected_command = self.selected.map(|index| matches[index].id);
        let mut chosen = enter
            .then_some(self.selected_command)
            .flatten()
            .map(Choice::Command);
        let context = ui.ctx().clone();
        overlay_scroll_bar(ui);
        let height = (context.content_rect().height() - 90.0).clamp(40.0, 264.0);
        let output = egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .max_height(height)
            .show_popup_styled(ui, 6.0, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if matches.is_empty() {
                    ui.weak(text(ui.ctx(), Text::NoMatchingCommands));
                }
                for (index, definition) in matches.iter().enumerate() {
                    if index > 0 && (index == recent_count || index == direct_count) {
                        crate::chrome::separator(ui);
                    }
                    let group = if index == direct_count {
                        text(ui.ctx(), Text::SimilarCommands)
                    } else if index == 0 && recent_count > 0 {
                        text(ui.ctx(), Text::RecentlyUsed)
                    } else if index == recent_count && recent_count > 0 {
                        text(ui.ctx(), Text::OtherCommands)
                    } else {
                        ""
                    };
                    let shortcut = shortcuts.label(definition.id, commands);
                    let (_, row) = ui.allocate_space(egui::vec2(ui.available_width(), 22.0));
                    let selected = self.selected == Some(index);
                    let controls = selected || ui.rect_contains_pointer(row);
                    let row_actions =
                        RowActions::new(row, index < recent_count && controls, controls);
                    let body = row_actions.body;
                    let title = search::title(definition, language);
                    let title = self.next_values.get(&definition.id).map_or_else(
                        || title.to_owned(),
                        |value| {
                            let action = Text::PaletteSetTo
                                .in_language(language)
                                .replace("{value}", value);
                            format!("{title} ({action})")
                        },
                    );
                    let background = ui.painter().add(egui::Shape::Noop);
                    let mut atoms = egui::Atoms::new((title.as_str(), egui::Atom::grow()));
                    if !shortcut.is_empty() {
                        atoms.push_right(label_gap(ui));
                        atoms.push_right(
                            egui::RichText::new(&shortcut)
                                .color(crate::chrome::MUTED)
                                .atom_max_width(body.width() * 0.5),
                        );
                    }
                    if !group.is_empty() {
                        atoms.push_right(label_gap(ui));
                        atoms.push_right(
                            egui::RichText::new(group)
                                .small()
                                .color(crate::chrome::MUTED)
                                .atom_max_width(body.width() * 0.3),
                        );
                    }
                    let response = ui
                        .push_id(definition.id, |ui| {
                            ui.add_enabled_ui(enabled[index], |ui| {
                                row_button(ui, body, selected, atoms)
                            })
                            .inner
                        })
                        .inner
                        .help_ui(|ui| {
                            ui.set_max_width(
                                (context.content_rect().width() - 32.0).clamp(1.0, 588.0),
                            );
                            ui.add(
                                egui::Label::new(crate::hover_help::shortcut(&title, &shortcut))
                                    .wrap(),
                            );
                        });
                    ui.painter()
                        .set(background, row_background(ui, row, &response, selected));
                    context.accesskit_node_builder(response.id, |node| {
                        node.clear_toggled();
                        node.set_label(title.as_str());
                        if !shortcut.is_empty() {
                            node.set_description(shortcut.clone());
                        }
                    });
                    if (navigation.moved() || query_changed || selection_changed) && selected {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() && !query_changed {
                        chosen = Some(Choice::Command(definition.id));
                    }
                    if let Some(rect) = row_actions.configure {
                        let configure = ui
                            .push_id(("configure-command", definition.id), |ui| {
                                row_icon(ui, rect, '\u{eb51}')
                            })
                            .inner
                            .help_text(text(ui.ctx(), Text::ConfigureKeybinding));
                        context.accesskit_node_builder(configure.id, |node| {
                            node.set_label(formatted::configure_keybinding(
                                language,
                                options::title(definition.id, language)
                                    .unwrap_or_else(|| definition.title_in(language)),
                            ));
                        });
                        if configure.clicked() && !query_changed {
                            chosen = Some(Choice::Configure(definition.id));
                        }
                    }
                    if let Some(close_rect) = row_actions.remove {
                        let remove = ui
                            .push_id(("remove-command", definition.id), |ui| {
                                row_icon(ui, close_rect, '\u{ea76}')
                            })
                            .inner
                            .help_text(text(ui.ctx(), Text::RemoveRecentlyUsed));
                        context.accesskit_node_builder(remove.id, |node| {
                            node.set_label(formatted::remove_recent_command(
                                language,
                                options::title(definition.id, language)
                                    .unwrap_or_else(|| definition.title_in(language)),
                            ));
                        });
                        if remove.clicked() && !query_changed {
                            chosen = Some(Choice::RemoveCommand(definition.id));
                            self.selected_command = None;
                        }
                    }
                }
            });
        self.list_height = output.inner_rect.height();
        chosen
    }
}
