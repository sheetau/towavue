use super::*;

impl CommandPalette {
    pub(super) fn show_commands(
        &mut self,
        ui: &mut egui::Ui,
        commands: CommandContext,
        shortcuts: &ShortcutBindings,
        history: &[CommandId],
        navigation: (bool, bool, bool),
        query_changed: bool,
    ) -> Option<Choice> {
        let (up, down, enter) = navigation;
        let query = self.query[1..].trim().to_ascii_lowercase();
        let mut matches: Vec<_> = command_definitions()
            .iter()
            .filter(|definition| definition.title.to_ascii_lowercase().contains(&query))
            .collect();
        matches.sort_by_key(|definition| {
            history
                .iter()
                .position(|command| *command == definition.id)
                .unwrap_or(usize::MAX)
        });
        let recent_count = matches
            .iter()
            .take_while(|definition| history.contains(&definition.id))
            .count();
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
        if up || down {
            self.selected = next_enabled(self.selected, &enabled, down);
        }
        let selection_changed = previous_index != self.selected
            || self.selected_command != self.selected.map(|index| matches[index].id);
        self.selected_command = self.selected.map(|index| matches[index].id);
        let mut chosen = enter
            .then_some(self.selected_command)
            .flatten()
            .map(Choice::Command);
        let context = ui.ctx().clone();
        egui::ScrollArea::vertical()
            .max_height((context.content_rect().height() - 90.0).clamp(40.0, 264.0))
            .show_styled(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if matches.is_empty() {
                    ui.weak("No matching commands");
                }
                for (index, definition) in matches.iter().enumerate() {
                    if index == recent_count && recent_count > 0 {
                        ui.separator();
                    }
                    let group = if index == 0 && recent_count > 0 {
                        "recently used"
                    } else if index == recent_count {
                        "other commands"
                    } else {
                        ""
                    };
                    let shortcut = shortcuts.label(definition.id, commands);
                    let (_, row) = ui.allocate_space(egui::vec2(ui.available_width(), 22.0));
                    let selected = self.selected == Some(index);
                    let close = index < recent_count && (selected || ui.rect_contains_pointer(row));
                    let mut body = row;
                    if close {
                        body.max.x -= 22.0;
                    }
                    let response = ui
                        .push_id(definition.id, |ui| {
                            ui.add_enabled_ui(enabled[index], |ui| {
                                ui.put(
                                    body,
                                    egui::Button::selectable(
                                        selected,
                                        (
                                            definition.title,
                                            egui::Atom::grow(),
                                            egui::RichText::new(&shortcut)
                                                .color(crate::chrome::MUTED)
                                                .atom_max_width(body.width() * 0.5),
                                            egui::RichText::new(group)
                                                .small()
                                                .color(crate::chrome::MUTED)
                                                .atom_max_width(body.width() * 0.3),
                                        ),
                                    )
                                    .truncate()
                                    .min_size(body.size()),
                                )
                            })
                            .inner
                        })
                        .inner
                        .help_ui(|ui| {
                            ui.set_max_width(
                                (context.content_rect().width() - 32.0).clamp(1.0, 588.0),
                            );
                            ui.add(
                                egui::Label::new(format!("{}  {}", definition.title, shortcut))
                                    .wrap(),
                            );
                        });
                    context.accesskit_node_builder(response.id, |node| {
                        node.clear_toggled();
                        node.set_label(definition.title);
                        if !shortcut.is_empty() {
                            node.set_description(shortcut.clone());
                        }
                    });
                    if (up || down || query_changed || selection_changed) && selected {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() && !query_changed {
                        chosen = Some(Choice::Command(definition.id));
                    }
                    if close {
                        let close_rect =
                            egui::Rect::from_min_max(egui::pos2(body.right(), row.top()), row.max);
                        let remove = ui
                            .push_id(("remove-command", definition.id), |ui| {
                                crate::chrome::tab_close(ui, close_rect, false)
                            })
                            .inner
                            .help_text("Remove from Recently Used");
                        context.accesskit_node_builder(remove.id, |node| {
                            node.set_label(format!(
                                "Remove {} from Recently Used",
                                definition.title
                            ));
                        });
                        if remove.clicked() && !query_changed {
                            chosen = Some(Choice::RemoveCommand(definition.id));
                            self.selected_command = None;
                        }
                    }
                }
            });
        chosen
    }
}
