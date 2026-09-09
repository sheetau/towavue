use egui::RichText;
use towavue_core::{CommandId, ShortcutBindings};

use crate::chrome;

pub fn show(
    ui: &mut egui::Ui,
    shortcuts: &ShortcutBindings,
    recent: impl FnOnce(&mut egui::Ui),
) -> Option<CommandId> {
    let mut chosen = None;
    let width = (ui.available_width() - 32.0).clamp(0.0, 660.0);
    let top = (ui.available_height() * 0.1).clamp(12.0, 60.0);
    egui::ScrollArea::vertical()
        .id_salt("welcome")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(top);
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                ui.allocate_ui_with_layout(
                    egui::vec2(width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.label(
                            RichText::new("towavue")
                                .monospace()
                                .size(32.0)
                                .color(chrome::MUTED),
                        );
                        ui.add_space(32.0);
                        ui.label(RichText::new("START").size(12.0).color(chrome::MUTED));
                        ui.add_space(8.0);
                        for (command, label) in [
                            (CommandId::OpenFile, "Open File…"),
                            (CommandId::OpenFolder, "Open Folder…"),
                        ] {
                            let shortcut = shortcuts.label(command, Default::default());
                            let icon = if command == CommandId::OpenFolder {
                                chrome::Icon::OpenFolder
                            } else {
                                chrome::Icon::OpenFile
                            };
                            let response = ui
                                .add_sized(
                                    [width.min(340.0), 30.0],
                                    egui::Button::new((
                                        icon.text(),
                                        RichText::new(label).color(chrome::FOREGROUND),
                                    ))
                                    .frame_when_inactive(false)
                                    .truncate()
                                    .shortcut_text(if width >= 300.0 { &shortcut } else { "" }),
                                )
                                .on_hover_text(format!("{label}  {shortcut}"));
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    label,
                                )
                            });
                            if response.clicked() {
                                chosen = Some(command);
                            }
                        }
                        ui.add_space(24.0);
                        ui.label(RichText::new("RECENT").size(12.0).color(chrome::MUTED));
                        ui.add_space(8.0);
                        recent(ui);
                        ui.add_space(16.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new("Drop media files or a folder here to begin.")
                                    .color(chrome::MUTED),
                            )
                            .wrap(),
                        );
                        ui.add_space(16.0);
                    },
                );
            });
        });
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_idle_output_keeps_wordmark_and_shortcuts_visible() {
        let mut app = crate::Application::new(None, |_| {}).expect("headless app");
        let context = crate::fonts::test_context();
        context.global_style_mut(crate::chrome::style);
        app.ui_context = Some(context.clone());
        let mut last = egui::FullOutput::default();
        for index in 0..5 {
            last = context.run_ui(
                egui::RawInput {
                    time: Some(index as f64 * 0.1),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut Vec::new()),
            );
            if last.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                > std::time::Duration::from_millis(100)
            {
                break;
            }
        }
        for label in ["towavue", "START", "RECENT", "Ctrl+O", "Ctrl+Shift+O"] {
            assert!(
                text_rect(&last, label).is_some(),
                "{label} is present in idle CPU output"
            );
        }
    }

    #[test]
    fn welcome_keeps_open_actions_together_and_accessible_at_small_sizes() {
        for size in [
            egui::vec2(960.0, 514.0),
            egui::vec2(480.0, 238.0),
            egui::vec2(240.0, 119.0),
        ] {
            let context = crate::fonts::test_context();
            let mut shortcuts = ShortcutBindings::default();
            shortcuts.set(
                CommandId::OpenFile,
                "Ctrl+K Ctrl+O".parse().expect("shortcut"),
            );
            let frame = |events| {
                let mut commands = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        if let Some(command) = show(ui, &shortcuts, |_| {}) {
                            commands.push(command);
                        }
                    },
                );
                (output, commands)
            };
            for _ in 0..3 {
                frame(vec![]);
            }
            let (output, _) = frame(vec![]);
            let title = text_rect(&output, "towavue").expect("wordmark is visible");
            if size.y > 200.0 {
                let start = text_rect(&output, "START").expect("Start heading");
                assert!((title.left() - start.left()).abs() < 1.0);
                let open = text_rect(&output, "Open File…").expect("Open file");
                assert!(open.left() > title.left() && open.left() < title.left() + 50.0);
                assert!(text_rect(&output, "Ctrl+K Ctrl+O").is_some());
            } else {
                frame(vec![egui::Event::PointerMoved(egui::pos2(120.0, 70.0))]);
                frame(vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -80.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                }]);
                for _ in 0..30 {
                    frame(vec![]);
                }
            }
            for (label, command) in [
                ("Open File…", CommandId::OpenFile),
                ("Open Folder…", CommandId::OpenFolder),
            ] {
                let (output, _) = frame(vec![]);
                let rect = text_rect(&output, label)
                    .unwrap_or_else(|| panic!("whole {label} must be visible at {size:?}"));
                let pos = rect.center();
                frame(vec![egui::Event::PointerMoved(pos)]);
                let mut chosen = Vec::new();
                for pressed in [true, false] {
                    chosen.extend(
                        frame(vec![egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        }])
                        .1,
                    );
                }
                assert_eq!(chosen, [command]);
            }
        }
    }

    #[test]
    fn welcome_buttons_support_keyboard_focus_and_activation() {
        let context = crate::fonts::test_context();
        let frame = |events| {
            let mut commands = Vec::new();
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let Some(command) = show(ui, &ShortcutBindings::default(), |_| {}) {
                        commands.push(command);
                    }
                },
            );
            commands
        };
        for _ in 0..3 {
            frame(vec![]);
        }
        let key = |key| {
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]
        };
        frame(key(egui::Key::Tab));
        assert_eq!(frame(key(egui::Key::Enter)), [CommandId::OpenFile]);
        frame(key(egui::Key::Tab));
        assert_eq!(frame(key(egui::Key::Enter)), [CommandId::OpenFolder]);
    }

    fn text_rect(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
        output.shapes.iter().find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
            (text.galley.text() == label && shape.clip_rect.contains_rect(rect)).then_some(rect)
        })
    }
}
