use towavue_core::{CommandContext, CommandId, ShortcutBindings, command_definitions};

#[derive(Default)]
pub struct CommandPalette {
    query: String,
    selected: Option<usize>,
    ime_composing: bool,
}

impl CommandPalette {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn show(
        &mut self,
        context: &egui::Context,
        commands: CommandContext,
        shortcuts: &ShortcutBindings,
    ) -> (Option<CommandId>, bool) {
        let mut chosen = None;
        let (up, down, enter, close) = context.input_mut(|input| {
            let mut ime_event = false;
            for event in &input.events {
                if let egui::Event::Ime(event) = event {
                    ime_event = true;
                    match event {
                        egui::ImeEvent::Preedit { text, .. } => {
                            self.ime_composing = !text.is_empty()
                        }
                        _ => self.ime_composing = false,
                    }
                }
            }
            // IME confirmation/cancellation and its accompanying key can share a frame.
            if self.ime_composing || ime_event {
                for key in [
                    egui::Key::ArrowUp,
                    egui::Key::ArrowDown,
                    egui::Key::Enter,
                    egui::Key::Escape,
                ] {
                    input.consume_key(egui::Modifiers::NONE, key);
                }
                return (false, false, false, false);
            }
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        egui::Window::new("Command palette")
            .id("command-palette".into())
            .anchor(egui::Align2::CENTER_TOP, [0.0, 48.0])
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
                ui.set_width((context.content_rect().width() - 48.0).clamp(200.0, 520.0));
                let previous_query = self.query.clone();
                let query_id = egui::Id::new("command-palette-query");
                ui.memory_mut(|memory| {
                    if !memory.has_focus(query_id) {
                        memory.request_focus(query_id);
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.query).id(query_id));
                let query_changed = previous_query != self.query;
                if query_changed {
                    self.selected = None;
                }
                let query = self.query.trim().to_ascii_lowercase();
                let matches: Vec<_> = command_definitions()
                    .iter()
                    .filter(|definition| definition.title.to_ascii_lowercase().contains(&query))
                    .collect();
                let enabled: Vec<_> = matches
                    .iter()
                    .map(|definition| definition.is_enabled(commands))
                    .collect();
                if self
                    .selected
                    .is_none_or(|index| !enabled.get(index).copied().unwrap_or(false))
                {
                    self.selected = enabled.iter().position(|enabled| *enabled);
                }
                if up || down {
                    self.selected = next_enabled(self.selected, &enabled, down);
                }
                if enter && let Some(index) = self.selected {
                    chosen = Some(matches[index].id);
                }
                egui::ScrollArea::vertical()
                    .max_height((context.content_rect().height() - 140.0).clamp(80.0, 320.0))
                    .show(ui, |ui| {
                        if matches.is_empty() {
                            ui.weak("No matching commands");
                        }
                        for (index, definition) in matches.iter().enumerate() {
                            let shortcut = shortcuts
                                .get(definition.id)
                                .map(ToString::to_string)
                                .unwrap_or_default();
                            let response = ui.add_enabled(
                                enabled[index],
                                egui::Button::new(format!("{}    {}", definition.title, shortcut))
                                    .selected(self.selected == Some(index))
                                    .min_size(egui::vec2(ui.available_width(), 24.0)),
                            );
                            if (up || down || query_changed) && self.selected == Some(index) {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                            if response.clicked() {
                                chosen = Some(definition.id);
                            }
                        }
                    });
                ui.weak("Up / Down: select   Enter: run   Esc: close");
            });
        (chosen, close)
    }
}

fn next_enabled(current: Option<usize>, enabled: &[bool], forward: bool) -> Option<usize> {
    let count = enabled.len();
    let current = current.unwrap_or(if forward { count.saturating_sub(1) } else { 0 });
    (1..=count)
        .map(|step| {
            if forward {
                (current + step) % count
            } else {
                (current + count - step) % count
            }
        })
        .find(|index| enabled[*index])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_accepts_text_navigation_enter_and_empty_results_across_layout_passes() {
        let context = egui::Context::default();
        let mut palette = CommandPalette::default();
        let commands = CommandContext {
            media_kind: Some(towavue_core::MediaKind::Video),
            palette_open: true,
            ..Default::default()
        };
        let shortcuts = ShortcutBindings::default();
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let run = |palette: &mut CommandPalette, events| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 576.0),
                )),
                events,
                ..Default::default()
            };
            let mut chosen = Vec::new();
            let _ = context.run_ui(input, |_| {
                if let (Some(command), _) = palette.show(&context, commands, &shortcuts) {
                    chosen.push(command);
                }
            });
            chosen
        };
        for _ in 0..3 {
            run(&mut palette, vec![]);
        }
        run(
            &mut palette,
            vec![
                egui::Event::Text("volume".into()),
                key(egui::Key::ArrowDown),
            ],
        );
        assert_eq!(palette.query, "volume");
        assert_eq!(palette.selected, Some(1));
        assert_eq!(
            run(&mut palette, vec![key(egui::Key::Enter)]),
            vec![CommandId::VolumeUp]
        );
        palette.query = "no matching command exists".into();
        assert!(run(&mut palette, vec![key(egui::Key::Enter)]).is_empty());
        assert_eq!(palette.selected, None);
        palette.query = "zoom".into();
        assert!(run(&mut palette, vec![key(egui::Key::Enter)]).is_empty());
        assert_eq!(palette.selected, None);
        let mut closed = false;
        let _ = context.run_ui(
            egui::RawInput {
                events: vec![key(egui::Key::Escape)],
                ..Default::default()
            },
            |_| {
                closed |= palette.show(&context, commands, &shortcuts).1;
            },
        );
        assert!(closed);
    }

    #[test]
    fn focused_palette_does_not_request_native_ime_cancellation() {
        let context = egui::Context::default();
        let mut palette = CommandPalette::default();
        let commands = CommandContext {
            palette_open: true,
            ..Default::default()
        };
        let shortcuts = ShortcutBindings::default();
        let mut run = |events| {
            context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |_| {
                    assert_eq!(palette.show(&context, commands, &shortcuts), (None, false));
                },
            )
        };
        for _ in 0..3 {
            run(vec![]);
        }
        for text in ["ｎ", "に", "にほ", "にほん", "にほんご", "日本語"] {
            for events in [
                vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                    text: text.into(),
                    active_range_chars: Some(0..text.chars().count()),
                })],
                vec![],
            ] {
                let output = run(events);
                assert!(
                    !output
                        .platform_output
                        .ime
                        .expect("focused text input")
                        .should_interrupt_composition
                );
            }
        }
        let output = run(vec![
            egui::Event::Ime(egui::ImeEvent::Preedit {
                text: String::new(),
                active_range_chars: None,
            }),
            egui::Event::Ime(egui::ImeEvent::Commit("日本語".into())),
        ]);
        assert!(
            !output
                .platform_output
                .ime
                .expect("committed text input")
                .should_interrupt_composition
        );
        assert_eq!(palette.query, "日本語");
    }

    #[test]
    fn ime_confirmation_and_cancel_do_not_run_or_close_the_palette() {
        let context = egui::Context::default();
        let mut palette = CommandPalette::default();
        let commands = CommandContext {
            palette_open: true,
            ..Default::default()
        };
        let shortcuts = ShortcutBindings::default();
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let run = |palette: &mut CommandPalette, events| {
            let mut result = (None, false);
            let _ = context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |_| {
                    let (chosen, close) = palette.show(&context, commands, &shortcuts);
                    result.0 = result.0.or(chosen);
                    result.1 |= close;
                },
            );
            result
        };
        for _ in 0..3 {
            run(&mut palette, vec![]);
        }
        run(
            &mut palette,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: "o".into(),
                active_range_chars: None,
            })],
        );
        assert_eq!(
            run(
                &mut palette,
                vec![
                    key(egui::Key::ArrowDown),
                    key(egui::Key::ArrowUp),
                    key(egui::Key::Enter)
                ]
            ),
            (None, false)
        );
        assert_eq!(
            run(
                &mut palette,
                vec![
                    egui::Event::Ime(egui::ImeEvent::Commit("open file".into())),
                    key(egui::Key::Enter)
                ]
            ),
            (None, false)
        );
        assert_eq!(palette.query, "open file");
        assert_eq!(
            run(&mut palette, vec![key(egui::Key::Enter)]),
            (Some(CommandId::OpenFile), false)
        );
        run(
            &mut palette,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: "a".into(),
                active_range_chars: None,
            })],
        );
        assert_eq!(
            run(&mut palette, vec![key(egui::Key::Escape)]),
            (None, false)
        );
        assert_eq!(
            run(
                &mut palette,
                vec![
                    egui::Event::Ime(egui::ImeEvent::Preedit {
                        text: String::new(),
                        active_range_chars: None
                    }),
                    key(egui::Key::Escape)
                ]
            ),
            (None, false)
        );
        assert_eq!(
            run(&mut palette, vec![key(egui::Key::Escape)]),
            (None, true)
        );
        palette.reset();
        assert!(!palette.ime_composing);
        run(&mut palette, vec![]);
        run(
            &mut palette,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: "にほんご".into(),
                active_range_chars: Some(0..4),
            })],
        );
        assert_eq!(palette.query, "にほんご");
        assert_eq!(
            run(
                &mut palette,
                vec![
                    key(egui::Key::Enter),
                    egui::Event::Ime(egui::ImeEvent::Commit("日本語".into()))
                ]
            ),
            (None, false)
        );
        assert_eq!(palette.query, "日本語");
        run(
            &mut palette,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: "a".into(),
                active_range_chars: None,
            })],
        );
        assert!(palette.ime_composing);
        palette.reset();
        assert!(!palette.ime_composing);
        assert!(palette.query.is_empty());
    }

    #[test]
    fn keyboard_selection_skips_disabled_commands_and_wraps() {
        let enabled = [true, false, true, false];
        assert_eq!(next_enabled(Some(0), &enabled, true), Some(2));
        assert_eq!(next_enabled(Some(2), &enabled, true), Some(0));
        assert_eq!(next_enabled(Some(0), &enabled, false), Some(2));
        assert_eq!(next_enabled(None, &[], true), None);
        assert_eq!(next_enabled(None, &[false, false], false), None);
        assert_eq!(next_enabled(None, &[false, true], true), Some(1));
    }
}
