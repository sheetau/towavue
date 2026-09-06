use egui::AtomExt;
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
        let query_id = egui::Id::new("command-palette-query");
        let (up, down, enter, close) = context.input_mut(|input| {
            // The pinned TextEdit exposes UIA ValuePattern but does not handle SetValue.
            if input.has_accesskit_action_request(query_id, egui::accesskit::Action::SetValue) {
                for event in std::mem::take(&mut input.events) {
                    if let egui::Event::AccessKitActionRequest(request) = &event
                        && request.target_tree == egui::accesskit::TreeId::ROOT
                        && request.target_node == query_id.accesskit_id()
                        && request.action == egui::accesskit::Action::SetValue
                        && let Some(egui::accesskit::ActionData::Value(value)) = &request.data
                    {
                        for (key, modifiers) in [
                            (egui::Key::A, egui::Modifiers::COMMAND),
                            (egui::Key::Backspace, egui::Modifiers::NONE),
                        ] {
                            input.events.push(egui::Event::Key {
                                key,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers,
                            });
                        }
                        input.events.push(egui::Event::Paste(value.to_string()));
                    } else {
                        input.events.push(event);
                    }
                }
            }
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
            .anchor(egui::Align2::CENTER_TOP, [0.0, 34.0])
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_gray(16))
                    .inner_margin(6)
                    .corner_radius(4),
            )
            .show(context, |ui| {
                ui.set_width((context.content_rect().width() - 48.0).clamp(120.0, 588.0));
                let previous_query = self.query.clone();
                ui.memory_mut(|memory| {
                    if !memory.has_focus(query_id) {
                        memory.request_focus(query_id);
                    }
                });
                ui.add_sized(
                    [ui.available_width(), 24.0],
                    egui::TextEdit::singleline(&mut self.query)
                        .id(query_id)
                        .desired_width(f32::INFINITY)
                        .hint_text("> Search commands"),
                )
                .on_hover_text("Up / Down: select   Enter: run   Esc: close");
                context.accesskit_node_builder(query_id, |node| {
                    node.set_label("Search commands");
                    node.add_action(egui::accesskit::Action::SetValue);
                });
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
                    .max_height((context.content_rect().height() - 90.0).clamp(40.0, 264.0))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if matches.is_empty() {
                            ui.weak("No matching commands");
                        }
                        for (index, definition) in matches.iter().enumerate() {
                            let shortcut = shortcuts
                                .get(definition.id)
                                .map(ToString::to_string)
                                .unwrap_or_default();
                            let response = ui
                                .add_enabled(
                                    enabled[index],
                                    egui::Button::selectable(
                                        self.selected == Some(index),
                                        (
                                            definition.title,
                                            egui::Atom::grow(),
                                            egui::RichText::new(&shortcut)
                                                .color(crate::chrome::MUTED)
                                                .atom_max_width(ui.available_width() * 0.5),
                                        ),
                                    )
                                    .truncate()
                                    .min_size(egui::vec2(ui.available_width(), 22.0)),
                                )
                                .on_hover_ui(|ui| {
                                    ui.set_max_width(
                                        (context.content_rect().width() - 32.0).clamp(1.0, 588.0),
                                    );
                                    ui.add(
                                        egui::Label::new(format!(
                                            "{}  {}",
                                            definition.title, shortcut
                                        ))
                                        .wrap(),
                                    );
                                });
                            // Selection is a visual command cursor, not an on/off setting.
                            context.accesskit_node_builder(response.id, |node| {
                                node.clear_toggled();
                            });
                            if (up || down || query_changed) && self.selected == Some(index) {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                            if response.clicked() {
                                chosen = Some(definition.id);
                            }
                        }
                    });
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
    fn accessibility_names_and_replaces_the_query_without_running_commands() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut palette = CommandPalette::default();
        let frame = |palette: &mut CommandPalette, events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_| {
                    assert_eq!(
                        palette.show(
                            &context,
                            CommandContext {
                                palette_open: true,
                                ..Default::default()
                            },
                            &ShortcutBindings::default(),
                        ),
                        (None, false),
                    );
                },
            )
        };
        for _ in 0..3 {
            frame(&mut palette, vec![]);
        }
        let query_id = egui::Id::new("command-palette-query").accesskit_id();
        let request = |target_node, value: &str| {
            egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetValue,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node,
                data: Some(egui::accesskit::ActionData::Value(value.into())),
            })
        };
        for (value, expected) in [
            ("Open", "Open"),
            ("日本語 café 🎞️", "日本語 café 🎞️"),
            ("", ""),
            ("Open\nfile", "Open file"),
        ] {
            let output = frame(&mut palette, vec![request(query_id, value)]);
            assert_eq!(palette.query, expected);
            let tree = output.platform_output.accesskit_update.expect("tree");
            let (_, node) = tree
                .nodes
                .iter()
                .find(|(id, _)| *id == query_id)
                .expect("accessible query field");
            assert_eq!(node.label(), Some("Search commands"));
            assert_eq!(node.role(), egui::accesskit::Role::TextInput);
            assert!(node.supports_action(egui::accesskit::Action::SetValue));
            assert_eq!(node.value(), Some(expected));
            for (_, node) in &tree.nodes {
                if node.role() == egui::accesskit::Role::Button {
                    assert!(node.toggled().is_none(), "commands are not toggle switches");
                }
            }
            assert!(output.platform_output.commands.is_empty());
        }
        frame(
            &mut palette,
            vec![request(
                egui::Id::new("other").accesskit_id(),
                "wrong target",
            )],
        );
        assert_eq!(palette.query, "Open file");
        frame(
            &mut palette,
            vec![
                request(query_id, "first"),
                request(query_id, "Open"),
                egui::Event::Text(" folder".into()),
            ],
        );
        assert_eq!(palette.query, "Open folder");
    }

    #[test]
    fn palette_pastes_copies_and_cuts_unicode_without_running_commands() {
        for text in ["open", "日本語 café 🎞️"] {
            let context = egui::Context::default();
            let mut palette = CommandPalette::default();
            let commands = CommandContext {
                palette_open: true,
                ..Default::default()
            };
            let shortcuts = ShortcutBindings::default();
            let frame = |palette: &mut CommandPalette, events| {
                context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(480.0, 300.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |_| {
                        assert_eq!(palette.show(&context, commands, &shortcuts), (None, false));
                    },
                )
            };
            for _ in 0..3 {
                frame(&mut palette, vec![]);
            }
            frame(&mut palette, vec![egui::Event::Paste(text.into())]);
            assert_eq!(palette.query, text);
            frame(
                &mut palette,
                vec![egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        command: true,
                        ..Default::default()
                    },
                }],
            );
            let copied = frame(&mut palette, vec![egui::Event::Copy]);
            assert!(matches!(copied.platform_output.commands.as_slice(),
                [egui::OutputCommand::CopyText(value)] if value == text));
            assert_eq!(palette.query, text);
            let cut = frame(&mut palette, vec![egui::Event::Cut]);
            assert!(matches!(cut.platform_output.commands.as_slice(),
                [egui::OutputCommand::CopyText(value)] if value == text));
            assert!(palette.query.is_empty());
            frame(&mut palette, vec![egui::Event::Paste(text.into())]);
            assert_eq!(palette.query, text);
        }
    }

    #[test]
    fn compact_palette_keeps_search_and_shortcut_columns_inside_the_window() {
        for size in [
            egui::vec2(960.0, 576.0),
            egui::vec2(480.0, 300.0),
            egui::vec2(240.0, 180.0),
        ] {
            let context = egui::Context::default();
            context.global_style_mut(|style| {
                crate::chrome::style(style);
                style.animation_time = 0.0;
                style.interaction.tooltip_delay = 0.0;
            });
            let mut palette = CommandPalette {
                query: "open".into(),
                ..Default::default()
            };
            let commands = CommandContext {
                palette_open: true,
                ..Default::default()
            };
            let mut shortcuts = ShortcutBindings::default();
            shortcuts.set(
                CommandId::OpenFile,
                "Ctrl+O".parse().expect("file shortcut"),
            );
            let prefix = "Ctrl+Shift+K Ctrl+Shift+P Ctrl+Shift+S";
            shortcuts.set(
                CommandId::OpenFolder,
                prefix.parse().expect("folder prefix"),
            );
            let mut frame = |events| {
                let mut chosen = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events,
                        ..Default::default()
                    },
                    |_| {
                        if let (Some(command), _) = palette.show(&context, commands, &shortcuts) {
                            chosen.push(command);
                        }
                    },
                );
                (output, chosen)
            };
            for _ in 0..4 {
                frame(vec![]);
            }
            let (output, chosen) = frame(vec![]);
            assert!(chosen.is_empty());
            let panel = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect) if rect.fill == egui::Color32::from_gray(16) => {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("dark palette panel");
            assert!(panel.left() >= 0.0 && panel.right() <= size.x);
            assert!(panel.top() >= 32.0 && panel.bottom() <= size.y);
            assert!(panel.width() <= 600.0);
            assert!(
                output.platform_output.ime.is_some(),
                "focused search supports IME"
            );
            let search = context
                .read_response("command-palette-query".into())
                .expect("search widget")
                .rect;
            assert!(
                search.width() >= panel.width() - 24.0,
                "full-width search: {search:?} inside {panel:?} at {size:?}"
            );
            let text = |label: &str| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.job.text == label => Some(text),
                        _ => None,
                    })
                    .expect("separate command/shortcut text")
            };
            let file = text("Open file");
            let folder = text("Open folder");
            let first = text("Ctrl+O");
            let second = text(prefix);
            assert!((file.pos.x - folder.pos.x).abs() < 1.0);
            assert!(
                (first.pos.x + first.galley.size().x - second.pos.x - second.galley.size().x).abs()
                    < 1.0
            );
            assert!(first.pos.x >= file.pos.x + file.galley.size().x);
            assert!(second.pos.x >= folder.pos.x + folder.galley.size().x);
            assert!(second.pos.x + second.galley.size().x < panel.right());
            let pos = egui::pos2(panel.right() - 8.0, folder.pos.y + 7.0);
            for _ in 0..3 {
                frame(vec![egui::Event::PointerMoved(pos)]);
            }
            let tooltip = frame(vec![]).0;
            let label = format!("Open folder  {prefix}");
            let tooltip = tooltip
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == label => Some(text),
                    _ => None,
                })
                .expect("complete shortcut tooltip");
            assert!(tooltip.pos.x >= 0.0 && tooltip.pos.x + tooltip.galley.size().x <= size.x);
            frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }]);
            let (_, chosen) = frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            assert_eq!(chosen, [CommandId::OpenFolder]);
        }
    }

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
