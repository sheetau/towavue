use towavue_core::{CommandContext, CommandId, ShortcutBindings, command_definitions};

use CommandId::*;

const MENUS: &[(&str, &[&[CommandId]])] = &[
    (
        "File",
        &[
            &[OpenFile, OpenFolder],
            &[Save, ExportAs, ToggleHardwareEncode],
            &[CloseTab],
            &[ReloadShortcuts],
        ],
    ),
    (
        "Edit",
        &[
            &[Undo, Redo],
            &[ApplyCrop, ClearSelection],
            &[
                RotateClockwise,
                RotateCounterclockwise,
                FlipHorizontal,
                FlipVertical,
            ],
            &[SetTrimStart, SetTrimEnd],
            &[VolumeDown, VolumeUp, ToggleMute],
            &[RateDown, RateUp, ResetRate],
        ],
    ),
    (
        "View",
        &[
            &[ToggleFullscreen],
            &[TogglePause, SeekBackward, SeekForward],
            &[PreviousMedia, NextMedia, PreviousSameKind, NextSameKind],
            &[PreviousTab, NextTab],
            &[ZoomIn, ZoomOut, ActualSize, FitToWindow, ToggleCropPreview],
            &[
                ToggleReadingMode,
                IncreaseReadingPages,
                DecreaseReadingPages,
                ToggleReadingAxis,
                ReverseReadingOrder,
            ],
            &[
                ToggleFilmstrip,
                ToggleTimeline,
                ToggleGridMenu,
                ToggleCommandPalette,
            ],
        ],
    ),
];

pub fn show(
    ui: &mut egui::Ui,
    context: CommandContext,
    shortcuts: &ShortcutBindings,
) -> Option<CommandId> {
    let mut chosen = None;
    for (title, groups) in MENUS {
        ui.menu_button(*title, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(title)
                .max_height((ui.ctx().content_rect().height() - 64.0).max(100.0))
                .show(ui, |ui| {
                    for (index, group) in groups.iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        for id in *group {
                            let definition = command_definitions()
                                .iter()
                                .find(|definition| definition.id == *id)
                                .expect("menu command is registered");
                            let shortcut = shortcuts
                                .get(*id)
                                .map(ToString::to_string)
                                .unwrap_or_default();
                            if ui
                                .add_enabled(
                                    definition.is_enabled(context),
                                    egui::Button::new(definition.title).shortcut_text(shortcut),
                                )
                                .clicked()
                            {
                                chosen = Some(*id);
                                ui.close();
                            }
                        }
                    }
                });
        });
    }
    chosen
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn menu_opens_and_dispatches_file_action() {
        let context = egui::Context::default();
        let mut shortcuts = ShortcutBindings::default();
        shortcuts.set(OpenFile, "Ctrl+K Ctrl+O".parse().expect("custom shortcut"));
        let mut time = 0.0;
        let mut frame = |events| {
            let mut chosen = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.menu_button("Menu", |ui| {
                        if let Some(command) = show(ui, CommandContext::default(), &shortcuts) {
                            chosen.push(command);
                        }
                    });
                },
            );
            time += 0.1;
            (output, chosen)
        };
        for _ in 0..3 {
            frame(Vec::new());
        }
        click(&mut frame, egui::pos2(20.0, 15.0));
        let (output, _) = frame(Vec::new());
        let file = text_position(&output, "File").expect("File category is visible");
        click(&mut frame, file);
        let (output, _) = frame(Vec::new());
        let open = text_position(&output, "Open file").expect("Open command is visible");
        let shortcut = text_position(&output, "Ctrl+K Ctrl+O").expect("custom shortcut is visible");
        assert!(shortcut.x > open.x);
        let export =
            text_position(&output, "Export as").expect("disabled export stays discoverable");
        assert!(
            click(&mut frame, export).is_empty(),
            "no media means no export action"
        );
        frame(Vec::new());
        click(&mut frame, egui::pos2(20.0, 15.0));
        frame(Vec::new());
        click(&mut frame, file);
        let (output, _) = frame(Vec::new());
        let open = text_position(&output, "Open file").expect("reopened File menu");
        assert_eq!(click(&mut frame, open), [OpenFile]);
        let (output, _) = frame(Vec::new());
        assert!(
            text_position(&output, "File").is_none(),
            "choosing a command closes the menu tree"
        );
        click(&mut frame, egui::pos2(20.0, 15.0));
        let (output, _) = frame(Vec::new());
        click(
            &mut frame,
            text_position(&output, "View").expect("View category"),
        );
        let (output, _) = frame(Vec::new());
        let top = text_position(&output, "Play or pause").expect("top of View menu");
        assert!(text_position(&output, "Show command palette").is_none());
        frame(vec![egui::Event::PointerMoved(top)]);
        frame(vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -10_000.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        }]);
        for _ in 0..20 {
            frame(Vec::new());
        }
        let (output, _) = frame(Vec::new());
        let last = text_position(&output, "Show command palette")
            .expect("scroll exposes last View command");
        assert!(last.y < 300.0);
        assert_eq!(click(&mut frame, last), [ToggleCommandPalette]);
    }

    fn text_position(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if text.galley.text() == label
                    && shape
                        .clip_rect
                        .contains(text.pos + text.galley.size() / 2.0) =>
            {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
    }

    fn click(
        frame: &mut impl FnMut(Vec<egui::Event>) -> (egui::FullOutput, Vec<CommandId>),
        pos: egui::Pos2,
    ) -> Vec<CommandId> {
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
        chosen
    }

    #[test]
    fn every_registered_command_has_exactly_one_menu_location() {
        let mut placed = BTreeSet::new();
        assert_eq!(
            MENUS.iter().map(|(title, _)| *title).collect::<Vec<_>>(),
            ["File", "Edit", "View"]
        );
        for (_, groups) in MENUS {
            assert!(!groups.is_empty());
            for group in *groups {
                assert!(!group.is_empty());
                for command in *group {
                    assert!(
                        placed.insert(*command),
                        "duplicate menu command: {command:?}"
                    );
                }
            }
        }
        assert_eq!(
            placed,
            command_definitions()
                .iter()
                .map(|definition| definition.id)
                .collect()
        );
    }
}
