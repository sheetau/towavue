use super::*;

impl KeyboardSettings {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        bindings: &ShortcutBindings,
        enabled: bool,
    ) -> Option<Change> {
        if ui.input(|input| {
            !input.focused || input.events.contains(&egui::Event::WindowFocused(false))
        }) {
            self.cancel_capture();
        }
        let frame = ui.ctx().cumulative_frame_nr();
        if (enabled || self.edit.is_some()) && self.last_capture_frame != Some(frame) {
            self.last_capture_frame = Some(frame);
            let focused = enabled
                && (self.search_focused(ui.ctx())
                    || self.search_id.is_some_and(|id| {
                        ui.memory(|memory| {
                            memory.focused().is_none() && memory.had_focus_last_frame(id)
                        }) && ui.input(|input| input.key_pressed(egui::Key::Escape))
                    }));
            let capture = self.capturing();
            ui.input_mut(|input| {
                input.events.retain(|event| {
                    if let egui::Event::Key {
                        key,
                        pressed,
                        repeat,
                        modifiers,
                        ..
                    } = event
                    {
                        if let Some(stroke) = egui_stroke(*key, *modifiers) {
                            let control = focused && Self::search_control(&stroke);
                            if *pressed && !*repeat {
                                if control {
                                    self.apply_search_control(&stroke);
                                } else if capture {
                                    self.capture(stroke);
                                }
                            }
                            return !capture && !control;
                        }
                        return !capture;
                    }
                    // Native capture is intercepted before egui; this also prevents
                    // recorded characters from entering the offscreen search editor.
                    !(matches!(event, egui::Event::Text(_)) && (capture || self.capturing()))
                });
            });
        }
        let mut change = None;
        let body = ui.available_rect_before_wrap().shrink(8.0);
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            if !enabled || self.edit.is_some() {
                let opacity = ui.opacity();
                ui.disable();
                ui.set_opacity(opacity);
            }
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.visuals_mut().clip_rect_margin = 0.0;
            self.search(ui);
            if self.record_search {
                ui.label("Recording keys — press up to four strokes. Escape clears the search.");
            }
            if let Some(message) = &self.message {
                ui.colored_label(chrome::MUTED, message);
            }
            ui.add_space(8.0);
            let rows = self.rows(bindings);
            let row_width = ui.available_width();
            let command_width = row_width * 0.42;
            let keys_width = row_width * 0.25;
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
            ui.painter().hline(
                header.x_range(),
                header.bottom(),
                egui::Stroke::new(1.0, chrome::BORDER),
            );
            ui.add_space(1.0);
            if rows.is_empty() {
                ui.label("No matching keyboard shortcuts");
            }
            egui::ScrollArea::vertical()
                .id_salt("keyboard-rows")
                .show_rows_styled(ui, 24.0, rows.len(), |ui, range| {
                    for row in &rows[range] {
                        ui.push_id((row.command.id, row.slot), |ui| {
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(row_width, 24.0),
                                egui::Sense::click(),
                            );
                            // The edit button is a child hit target: keep the row hovered
                            // while the pointer crosses onto it, so it cannot disappear.
                            let hovered = response.contains_pointer() || response.has_focus();
                            if hovered {
                                ui.painter().rect_filled(rect, 0.0, chrome::HOVER);
                            }
                            let edit_rect = egui::Rect::from_min_size(
                                rect.min + egui::vec2(4.0, 2.0),
                                egui::Vec2::splat(20.0),
                            );
                            let edit = hovered.then(|| {
                                let (glyph, label) = if row.slot.is_some() {
                                    ('\u{ea73}', "Edit keybinding")
                                } else {
                                    ('\u{ea60}', "Add keybinding")
                                };
                                icon(ui, edit_rect, glyph, label, false)
                            });
                            for (left, width, text) in [
                                (34.0, command_width - 34.0, row.command.title),
                                (command_width, keys_width, &row.keys),
                                (
                                    command_width + keys_width,
                                    (row_width - command_width - keys_width).max(0.0),
                                    &row.when,
                                ),
                            ] {
                                let cell = egui::Rect::from_min_size(
                                    rect.min + egui::vec2(left, 0.0),
                                    egui::vec2(width, 24.0),
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
        let (outer, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
        let background = ui.painter().add(egui::Shape::Noop);
        let buttons = [0.0, 22.0, 44.0].map(|offset| {
            egui::Rect::from_min_size(
                outer.right_top() + egui::vec2(-22.0 - offset, 2.0),
                egui::Vec2::splat(20.0),
            )
        });
        let text_rect = egui::Rect::from_min_max(
            outer.min,
            egui::pos2(buttons[2].left() - 2.0, outer.bottom()),
        );
        let search = ui.put(text_rect, |ui: &mut egui::Ui| {
            ui.spacing_mut().text_edit_width = f32::INFINITY;
            crate::resize::unframed_text_input(
                ui,
                "Search commands or keybindings",
                &mut self.query,
            )
        });
        self.search_id = Some(search.id);
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
            search.request_focus();
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
            search.request_focus();
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
        if search.has_focus() {
            // Escape clears this search instead of moving focus out before UI routing.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    search.id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                        ..Default::default()
                    },
                )
            });
        }
        let stroke = if search.has_focus() {
            ui.visuals().selection.stroke
        } else {
            ui.visuals().widgets.hovered.bg_stroke
        };
        ui.painter().set(
            background,
            egui::epaint::RectShape::new(
                outer,
                2.0,
                egui::Color32::BLACK,
                stroke,
                egui::StrokeKind::Inside,
            ),
        );
    }

    fn edit_dialog(
        &mut self,
        context: &egui::Context,
        bindings: &ShortcutBindings,
    ) -> Option<Change> {
        let edit = self.edit.as_mut()?;
        let mut change = None;
        let submit = std::mem::take(&mut edit.submit)
            || context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        egui::Modal::new(egui::Id::new("keyboard-edit")).show(context, |ui| {
            ui.set_width(400.0_f32.min((context.content_rect().width() - 40.0).max(120.0)));
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.label("Press desired key combination and then press ENTER.");
                ui.add(
                    egui::TextEdit::singleline(&mut edit.text)
                        .interactive(false)
                        .horizontal_align(egui::Align::Center)
                        .desired_width(f32::INFINITY),
                );
                let parsed = edit.text.trim().parse::<KeySequence>();
                if let Ok(sequence) = &parsed {
                    keycaps(ui, sequence);
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
                        ui.label(format!("Also used by: {}", matches.join(", ")));
                    }
                    if submit {
                        let mut replacement = edit.expected.clone();
                        if let Some(slot) = edit.slot {
                            replacement[slot] = sequence.clone();
                        } else {
                            replacement.push(sequence.clone());
                        }
                        change = Some(Change {
                            command: edit.command,
                            expected: edit.expected.clone(),
                            replacement,
                        });
                    }
                }
                if let Some(message) = &self.message {
                    ui.colored_label(chrome::MUTED, message);
                }
            });
        });
        if context.input(|input| input.key_pressed(egui::Key::Escape)) {
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
    let response = chrome::icon_button_at(
        ui,
        rect,
        egui::Button::selectable(
            selected,
            RichText::new(glyph.to_string()).font(fonts::icon_font()),
        )
        .stroke(egui::Stroke::NONE)
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
    .add(egui::Label::new(text).selectable(false).truncate())
}

fn keycaps(ui: &mut egui::Ui, sequence: &KeySequence) {
    let mut tokens = Vec::new();
    for (index, stroke) in sequence.strokes().iter().enumerate() {
        if index > 0 {
            tokens.push(("chord to".to_owned(), false));
        }
        for (index, key) in stroke.to_string().split('+').enumerate() {
            if index > 0 {
                tokens.push(("+".to_owned(), false));
            }
            tokens.push((key.to_owned(), true));
        }
    }
    let width = ui.available_width();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut rows = vec![(0.0_f32, Vec::new())];
    for (text, cap) in tokens {
        let galley = ui
            .painter()
            .layout_no_wrap(text, font.clone(), chrome::MUTED);
        let size = galley.size().x + if cap { 10.0 } else { 0.0 };
        if rows
            .last()
            .is_some_and(|(used, row)| !row.is_empty() && used + 4.0 + size > width)
        {
            rows.push((0.0, Vec::new()));
        }
        let (used, row) = rows.last_mut().expect("keycap row");
        *used += size + if row.is_empty() { 0.0 } else { 4.0 };
        row.push((galley, cap, size));
    }
    // Measure complete rows before centering; egui's main alignment aligns each
    // allocation, rather than centering a sequence of differently sized widgets.
    for (used, row) in rows {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
        let mut left = rect.center().x - used * 0.5;
        for (galley, cap, size) in row {
            if cap {
                ui.painter().rect(
                    egui::Rect::from_center_size(
                        egui::pos2(left + size * 0.5, rect.center().y),
                        egui::vec2(size, galley.size().y + 6.0),
                    ),
                    3.0,
                    chrome::BACKGROUND,
                    egui::Stroke::new(1.0, chrome::BORDER),
                    egui::StrokeKind::Inside,
                );
            }
            let origin = egui::pos2(
                left + if cap { 5.0 } else { 0.0 },
                rect.center().y - galley.size().y * 0.5,
            );
            ui.painter().galley(origin, galley, chrome::MUTED);
            left += size + 4.0;
        }
    }
}
