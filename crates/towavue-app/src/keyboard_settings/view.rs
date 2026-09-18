use super::*;

impl KeyboardSettings {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        bindings: &ShortcutBindings,
        enabled: bool,
    ) -> Option<Change> {
        let frame = ui.ctx().cumulative_frame_nr();
        if self.capturing()
            && (enabled || self.edit.is_some())
            && self.last_capture_frame != Some(frame)
        {
            self.last_capture_frame = Some(frame);
            let strokes = ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::Key {
                            key,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } => egui_stroke(*key, *modifiers),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            });
            for stroke in strokes {
                self.capture(stroke);
            }
            // Native capture is intercepted before egui; this covers offscreen input.
            ui.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(event, egui::Event::Key { .. } | egui::Event::Text(_))
                })
            });
        }
        let mut change = None;
        ui.add_enabled_ui(enabled && self.edit.is_none(), |ui| {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.heading("Keyboard Shortcuts");
            });
            ui.add_space(10.0);
            self.search(ui);
            if self.record_search {
                ui.label("Recording keys — press up to four strokes. Escape stops recording.");
            }
            if let Some(message) = &self.message {
                ui.colored_label(chrome::MUTED, message);
            }
            ui.add_space(12.0);
            let rows = self.rows(bindings);
            let row_width = ui.available_width().max(240.0);
            let command_width = (row_width * 0.42).max(100.0);
            let keys_width = (row_width * 0.25).max(100.0);
            let (header, _) =
                ui.allocate_exact_size(egui::vec2(row_width, 24.0), egui::Sense::hover());
            for (left, width, text) in [
                (34.0, command_width - 34.0, "Command"),
                (command_width, keys_width, "Keybindings"),
                (
                    command_width + keys_width,
                    (row_width - command_width - keys_width).max(0.0),
                    "When",
                ),
            ] {
                cell_label(
                    ui,
                    egui::Rect::from_min_size(
                        header.min + egui::vec2(left + 4.0, 0.0),
                        egui::vec2((width - 8.0).max(0.0), 24.0),
                    ),
                    text,
                );
            }
            ui.separator();
            if rows.is_empty() {
                ui.label("No matching keyboard shortcuts");
            }
            egui::ScrollArea::vertical()
                .id_salt("keyboard-rows")
                .show_rows_styled(ui, 34.0, rows.len(), |ui, range| {
                    for row in &rows[range] {
                        ui.push_id((row.command.id, row.slot), |ui| {
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(row_width, 34.0),
                                egui::Sense::click(),
                            );
                            let hovered = response.hovered() || response.has_focus();
                            if hovered {
                                ui.painter().rect_filled(rect, 0.0, chrome::HOVER);
                            }
                            let edit_rect = egui::Rect::from_min_size(
                                rect.min + egui::vec2(4.0, 5.0),
                                egui::Vec2::splat(24.0),
                            );
                            let edit = hovered
                                .then(|| icon(ui, edit_rect, '\u{ea73}', "Edit keybinding", false));
                            for (left, width, text) in [
                                (34.0, command_width - 34.0, row.command.title),
                                (
                                    command_width,
                                    keys_width,
                                    if row.keys.is_empty() {
                                        "Unassigned"
                                    } else {
                                        &row.keys
                                    },
                                ),
                                (
                                    command_width + keys_width,
                                    (row_width - command_width - keys_width).max(0.0),
                                    &row.when,
                                ),
                            ] {
                                let cell = egui::Rect::from_min_size(
                                    rect.min + egui::vec2(left, 0.0),
                                    egui::vec2(width, 34.0),
                                );
                                cell_label(ui, cell.shrink2(egui::vec2(4.0, 0.0)), text)
                                    .help_text(text);
                            }
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    response.enabled(),
                                    format!("{}: {}", row.command.title, row.keys),
                                )
                            });
                            if edit.is_some_and(|response| response.clicked())
                                || response.double_clicked()
                                || ui.input(|input| {
                                    input.has_accesskit_action_request(
                                        response.id,
                                        egui::accesskit::Action::Click,
                                    )
                                })
                                || response.has_focus()
                                    && ui.input(|input| input.key_pressed(egui::Key::Enter))
                            {
                                self.begin_edit(row.command.id, row.slot, bindings);
                            }
                            response.context_menu(|ui| {
                                if ui.button("Edit keybinding").clicked() {
                                    self.begin_edit(row.command.id, row.slot, bindings);
                                    ui.close();
                                }
                                if ui.button("Add keybinding").clicked() {
                                    self.begin_edit(row.command.id, None, bindings);
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        row.slot.is_some(),
                                        egui::Button::new("Remove keybinding"),
                                    )
                                    .clicked()
                                {
                                    let mut replacement = bindings.all(row.command.id).to_vec();
                                    if let Some(slot) = row.slot {
                                        replacement.remove(slot);
                                    }
                                    change = Some(Change {
                                        command: row.command.id,
                                        expected: bindings.all(row.command.id).to_vec(),
                                        replacement,
                                    });
                                    ui.close();
                                }
                                if ui.button("Reset command to defaults").clicked() {
                                    change = Some(Change {
                                        command: row.command.id,
                                        expected: bindings.all(row.command.id).to_vec(),
                                        replacement: shortcuts::defaults()
                                            .all(row.command.id)
                                            .to_vec(),
                                    });
                                    ui.close();
                                }
                            });
                        });
                    }
                });
        });
        self.edit_dialog(ui.ctx(), bindings).or(change)
    }

    fn search(&mut self, ui: &mut egui::Ui) {
        let width = (ui.available_width() - 32.0).max(120.0);
        let (outer, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
        let outer =
            egui::Rect::from_min_size(outer.min + egui::vec2(16.0, 0.0), egui::vec2(width, 30.0));
        ui.painter().rect_stroke(
            outer,
            3.0,
            egui::Stroke::new(1.0, chrome::BORDER),
            egui::StrokeKind::Inside,
        );
        let buttons = [0.0, 26.0, 52.0].map(|offset| {
            egui::Rect::from_min_size(
                outer.right_top() + egui::vec2(-28.0 - offset, 2.0),
                egui::vec2(26.0, 26.0),
            )
        });
        let text_rect = egui::Rect::from_min_max(
            outer.min + egui::vec2(6.0, 0.0),
            egui::pos2(buttons[2].left() - 4.0, outer.bottom()),
        );
        let search = ui.put(
            text_rect,
            egui::TextEdit::singleline(&mut self.query)
                .id_salt("keyboard-search")
                .hint_text("Search commands or keybindings")
                .frame(egui::Frame::NONE)
                .vertical_align(egui::Align::Center),
        );
        if self.focus_search {
            search.request_focus();
            self.focus_search = false;
        }
        if icon(
            ui,
            buttons[2],
            '\u{ea65}',
            "Record keys",
            self.record_search,
        )
        .clicked()
        {
            self.record_search = !self.record_search;
            self.recorded.clear();
            search.surrender_focus();
        }
        if icon(
            ui,
            buttons[1],
            '\u{eb55}',
            "Sort by precedence",
            self.precedence,
        )
        .clicked()
        {
            self.precedence = !self.precedence;
        }
        if icon(
            ui,
            buttons[0],
            '\u{eabf}',
            "Clear keybindings search input",
            false,
        )
        .clicked()
        {
            self.query.clear();
            self.recorded.clear();
            self.record_search = false;
            self.focus_search = true;
        }
    }

    fn edit_dialog(
        &mut self,
        context: &egui::Context,
        bindings: &ShortcutBindings,
    ) -> Option<Change> {
        let edit = self.edit.as_mut()?;
        let mut change = None;
        let mut close = false;
        egui::Modal::new(egui::Id::new("keyboard-edit")).show(context, |ui| {
            ui.set_width(460.0_f32.min(context.content_rect().width() - 40.0));
            ui.heading(
                command_definitions()
                    .iter()
                    .find(|command| command.id == edit.command)
                    .expect("command")
                    .title,
            );
            ui.label("Record keys or type a sequence, for example Ctrl+K Ctrl+S.");
            ui.add(
                egui::TextEdit::singleline(&mut edit.text)
                    .interactive(!edit.recording)
                    .desired_width(f32::INFINITY),
            );
            if ui.checkbox(&mut edit.recording, "Record keys").changed() {
                self.recorded.clear();
            }
            ui.label("Escape cancels. To bind Escape, stop recording and type Escape.");
            let parsed = edit.text.trim().parse::<KeySequence>();
            if let Ok(sequence) = &parsed {
                let matches: Vec<_> = command_definitions()
                    .iter()
                    .filter(|other| {
                        other.id != edit.command
                            && bindings.all(other.id).iter().any(|bound| {
                                bound.strokes().starts_with(sequence.strokes())
                                    || sequence.strokes().starts_with(bound.strokes())
                            })
                    })
                    .map(|other| other.title)
                    .collect();
                if !matches.is_empty() {
                    ui.label(format!(
                        "Also used by: {}. Context and precedence determine which command runs.",
                        matches.join(", ")
                    ));
                }
            } else if !edit.text.trim().is_empty() {
                ui.label("Enter a valid key or key sequence.");
            }
            if let Some(message) = &self.message {
                ui.colored_label(chrome::MUTED, message);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(parsed.is_ok(), egui::Button::new("Save"))
                    .clicked()
                {
                    let mut replacement = edit.expected.clone();
                    let sequence = parsed.expect("valid sequence");
                    if let Some(slot) = edit.slot {
                        replacement[slot] = sequence;
                    } else {
                        replacement.push(sequence);
                    }
                    change = Some(Change {
                        command: edit.command,
                        expected: edit.expected.clone(),
                        replacement,
                    });
                }
                if ui.button("Remove").clicked() {
                    let mut replacement = edit.expected.clone();
                    if let Some(slot) = edit.slot {
                        replacement.remove(slot);
                    }
                    change = Some(Change {
                        command: edit.command,
                        expected: edit.expected.clone(),
                        replacement,
                    });
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if context.input(|input| input.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if close {
            self.edit = None;
            self.cancel_capture();
        }
        change
    }
}

fn icon(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    glyph: char,
    label: &str,
    selected: bool,
) -> egui::Response {
    // Codicon record-keys / sort-precedence / clear-all / edit in the bundled font.
    let response = ui
        .put(
            rect,
            egui::Button::selectable(
                selected,
                RichText::new(glyph.to_string()).font(fonts::icon_font()),
            )
            .frame_when_inactive(false),
        )
        .help_text(label);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    response
}

fn cell_label(ui: &mut egui::Ui, rect: egui::Rect, text: &str) -> egui::Response {
    ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    )
    .add(egui::Label::new(text).truncate())
}
