use towavue_core::{CommandContext, CommandId, ShortcutBindings, command_definitions};

#[derive(Default)]
pub struct CommandPalette {
    query: String,
    selected: Option<usize>,
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
                let edit = ui.text_edit_singleline(&mut self.query);
                edit.request_focus();
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
