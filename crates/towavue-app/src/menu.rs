use towavue_core::{CommandContext, CommandId, ShortcutBindings, command_definitions};

use CommandId::*;

const MENUS: &[(&str, &[&[CommandId]])] = &[
    (
        "File",
        &[
            &[OpenFile, OpenFolder],
            &[
                Save,
                ExportAs,
                ExportAudio,
                AudioExportOptions,
                MetadataExportOptions,
                ToggleHardwareEncode,
            ],
            &[CopyFilePath, RevealFile],
            &[
                CloseTab,
                CloseOtherTabs,
                CloseTabsLeft,
                CloseTabsRight,
                CloseAllTabs,
                ReopenClosedTab,
            ],
            &[ReloadShortcuts],
        ],
    ),
    (
        "Edit",
        &[
            &[Undo, Redo],
            &[CopyImage],
            &[ResizeImage, ResizeVideo],
            &[FreeRotateImage, FreeRotateVideo],
            &[SelectAll, ApplyCrop, ClearSelection],
            &[
                SelectAspectSquare,
                SelectAspectFourThree,
                SelectAspectThreeFour,
                SelectAspectThreeTwo,
                SelectAspectTwoThree,
                SelectAspectSixteenNine,
                SelectAspectNineSixteen,
            ],
            &[
                RotateClockwise,
                RotateCounterclockwise,
                FlipHorizontal,
                FlipVertical,
            ],
            &[SetTrimStart, SetTrimEnd],
            &[DeleteTimeSelection, KeepTimeSelection, PlayTimeSelection],
            &[VolumeDown, VolumeUp, ToggleMute],
            &[RateDown, RateUp, ResetRate],
        ],
    ),
    (
        "View",
        &[
            &[ToggleFullscreen],
            &[ToggleImageInterpolation],
            &[TogglePause, SeekBackward, SeekForward],
            &[PreviousVideoFrame, NextVideoFrame],
            &[StepAudioBackward, StepAudioForward],
            &[CycleAudioRepeat, ToggleAudioShuffle],
            &[PreviousMedia, NextMedia, PreviousSameKind, NextSameKind],
            &[PreviousImage, NextImage, FirstImage, LastImage],
            &[PreviousTab, NextTab],
            &[
                ZoomIn,
                ZoomOut,
                ActualSize,
                FitToWindow,
                CoverWindow,
                ToggleCropPreview,
            ],
            &[
                ToggleReadingMode,
                IncreaseReadingPages,
                DecreaseReadingPages,
                IncreaseReadingFirstPage,
                DecreaseReadingFirstPage,
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
    (
        "Image jump",
        &[
            &[
                JumpImagesBackward1,
                JumpImagesBackward2,
                JumpImagesBackward3,
                JumpImagesBackward4,
                JumpImagesBackward5,
                JumpImagesBackward6,
                JumpImagesBackward7,
                JumpImagesBackward8,
                JumpImagesBackward9,
                JumpImagesBackward10,
            ],
            &[
                JumpImagesForward1,
                JumpImagesForward2,
                JumpImagesForward3,
                JumpImagesForward4,
                JumpImagesForward5,
                JumpImagesForward6,
                JumpImagesForward7,
                JumpImagesForward8,
                JumpImagesForward9,
                JumpImagesForward10,
            ],
        ],
    ),
    ("Help", &[&[ShowLicenses]]),
];

pub fn show(
    ui: &mut egui::Ui,
    context: CommandContext,
    shortcuts: &ShortcutBindings,
) -> Option<CommandId> {
    let mut chosen = None;
    let keyboard = MenuKeyboard::begin(ui);
    let mut categories = Vec::new();
    for (title, groups) in MENUS {
        let category = ui.next_auto_id();
        if keyboard.right && ui.memory(|memory| memory.has_focus(category)) {
            let submenu = egui::containers::menu::SubMenu::id_from_widget_id(category);
            // MenuState drops an open child unless it is marked live before the next lookup.
            egui::containers::menu::MenuState::mark_shown(ui.ctx(), submenu);
            egui::containers::menu::MenuState::from_ui(ui, |state, _| {
                state.open_item = Some(submenu);
            });
        }
        let mut return_to_category = false;
        let menu = ui.menu_button(*title, |ui| {
            let keyboard = MenuKeyboard::begin(ui);
            return_to_category = keyboard.left;
            let mut items = Vec::new();
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
                            let shortcut = shortcuts.label(*id, context);
                            let response = ui.add_enabled(
                                definition.is_enabled(context),
                                egui::Button::new(definition.title).shortcut_text(shortcut),
                            );
                            if response.enabled() {
                                items.push(response.id);
                            }
                            if response.gained_focus() {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                chosen = Some(*id);
                                ui.close();
                            }
                        }
                    }
                });
            keyboard.finish(ui, items);
        });
        categories.push(menu.response.id);
        if return_to_category {
            egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item = None);
            menu.response.request_focus();
        }
    }
    keyboard.finish(ui, categories);
    chosen
}

pub(crate) struct MenuKeyboard {
    active: bool,
    left: bool,
    right: bool,
    requested_focus: Option<egui::Id>,
    initial_move: Option<bool>,
}

impl MenuKeyboard {
    pub(crate) fn begin(ui: &egui::Ui) -> Self {
        let (last_pass, items, selected) = ui
            .data(|data| {
                data.get_temp::<(u64, Vec<egui::Id>, Option<egui::Id>)>(
                    ui.id().with("keyboard-items"),
                )
            })
            .unwrap_or_default();
        let reopened = last_pass + 1 < ui.ctx().cumulative_pass_nr();
        let active = egui::containers::menu::MenuState::from_ui(ui, |state, _| {
            if reopened {
                state.open_item = None;
            }
            state.open_item.is_none()
        });
        let mut result = Self {
            active,
            left: false,
            right: false,
            requested_focus: None,
            initial_move: None,
        };
        if !active {
            return result;
        }
        let (backward, forward, left, right) = ui.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                    | input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                    | input.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            )
        });
        result.left = left;
        result.right = right;
        // A submenu's first visible pass has no remembered items yet. Apply
        // its first arrow after collecting enabled items instead of losing it.
        result.initial_move = (items.is_empty() && (backward || forward)).then_some(forward);
        // Egui installs focus locks only after a full focused frame; keep our selection across that gap.
        if !reopened && let Some(id) = selected.filter(|id| items.contains(id)) {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        if (reopened || !ui.memory(|memory| items.iter().any(|id| memory.has_focus(*id))))
            && let Some(id) = items.first()
        {
            ui.memory_mut(|memory| memory.request_focus(*id));
            ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            });
        }
        if (backward || forward) && !items.is_empty() {
            let current = items
                .iter()
                .position(|id| Some(*id) == selected)
                .unwrap_or(0);
            let next = if backward {
                (current + items.len() - 1) % items.len()
            } else {
                (current + 1) % items.len()
            };
            ui.memory_mut(|memory| memory.request_focus(items[next]));
            result.requested_focus = Some(items[next]);
        }
        result
    }

    pub(crate) fn finish(self, ui: &egui::Ui, items: Vec<egui::Id>) {
        let requested_focus = self.requested_focus.or_else(|| {
            self.initial_move.and_then(|forward| {
                let index = if forward {
                    1.min(items.len().saturating_sub(1))
                } else {
                    items.len().saturating_sub(1)
                };
                items.get(index).copied()
            })
        });
        if self.active
            && egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item.is_none())
        {
            ui.memory_mut(|memory| {
                // A newly focused button may also receive egui's queued arrow traversal.
                // Retain our one-step result even when consecutive frames contain key events.
                if let Some(id) = requested_focus.filter(|id| items.contains(id)) {
                    memory.request_focus(id);
                }
                if !items.iter().any(|id| memory.has_focus(*id))
                    && let Some(id) = items.first()
                {
                    memory.request_focus(*id);
                }
                for id in &items {
                    memory.set_focus_lock_filter(
                        *id,
                        egui::EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            ..Default::default()
                        },
                    );
                }
            });
        }
        let pass = ui.ctx().cumulative_pass_nr();
        let selected = requested_focus.or_else(|| {
            ui.memory(|memory| items.iter().find(|id| memory.has_focus(**id)).copied())
        });
        ui.data_mut(|data| {
            data.insert_temp(ui.id().with("keyboard-items"), (pass, items, selected))
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn keyboard_stays_in_menu_tree_and_scrolls_to_enabled_items() {
        let context = egui::Context::default();
        let shortcuts = ShortcutBindings::default();
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
                    assert!(!ui.button("Background action").clicked());
                },
            );
            time += 0.1;
            (output, chosen)
        };
        for _ in 0..3 {
            frame(vec![]);
        }
        let key = |key, shift| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                shift,
                ..Default::default()
            },
        };
        frame(vec![key(egui::Key::Tab, false)]);
        frame(vec![key(egui::Key::Enter, false)]);
        let mut navigate = |key_code, shift, label| {
            let (_, chosen) = frame(vec![key(key_code, shift)]);
            assert!(chosen.is_empty());
            for _ in 0..15 {
                assert!(frame(vec![]).1.is_empty());
            }
            let (output, chosen) = frame(vec![]);
            assert!(chosen.is_empty());
            let position = text_position(&output, label)
                .unwrap_or_else(|| panic!("{label} is visible after {key_code:?}"));
            let focused = context
                .memory(|memory| memory.focused())
                .expect("menu focus");
            let response = context.read_response(focused).expect("focused response");
            let rect = context
                .layer_transform_to_global(response.layer_id)
                .unwrap_or_default()
                * response.rect;
            assert!(
                rect.contains(position),
                "focus belongs to {label} after {key_code:?}: {rect:?} vs {position:?}"
            );
        };
        navigate(egui::Key::ArrowRight, false, "Open file");
        navigate(egui::Key::ArrowDown, false, "Open folder");
        navigate(egui::Key::ArrowDown, false, "Close tab");
        navigate(egui::Key::ArrowDown, false, "Reopen closed tab");
        navigate(egui::Key::ArrowDown, false, "Reload keyboard shortcuts");
        navigate(egui::Key::Tab, false, "Open file");
        navigate(egui::Key::ArrowLeft, false, "File");
        navigate(egui::Key::ArrowDown, false, "Edit");
        navigate(egui::Key::ArrowRight, false, "Edit");
        navigate(egui::Key::ArrowLeft, false, "Edit");
        navigate(egui::Key::ArrowDown, false, "View");
        navigate(egui::Key::ArrowRight, false, "Toggle fullscreen");
        navigate(egui::Key::Tab, true, "Show command palette");
        assert_eq!(
            frame(vec![key(egui::Key::Enter, false)]).1,
            [ToggleCommandPalette]
        );
        assert!(text_position(&frame(vec![]).0, "File").is_none());
        assert!(click(&mut frame, egui::pos2(20.0, 15.0)).is_empty());
        // File -> Help -> Image jump -> View.
        frame(vec![key(egui::Key::ArrowUp, false)]);
        frame(vec![key(egui::Key::ArrowUp, false)]);
        frame(vec![key(egui::Key::ArrowUp, false)]);
        frame(vec![key(egui::Key::ArrowRight, false)]);
        for _ in 0..15 {
            frame(vec![]);
        }
        let (output, chosen) = frame(vec![]);
        assert!(chosen.is_empty());
        assert!(
            text_position(&output, "Toggle fullscreen").is_some(),
            "reopened submenu scrolls back to its focused first item"
        );
        frame(vec![key(egui::Key::Escape, false)]);
        assert!(text_position(&frame(vec![]).0, "File").is_none());
        assert!(click(&mut frame, egui::pos2(20.0, 15.0)).is_empty());
        frame(vec![key(egui::Key::ArrowUp, false)]);
        frame(vec![key(egui::Key::ArrowRight, false)]);
        for _ in 0..15 {
            frame(vec![]);
        }
        assert!(text_position(&frame(vec![]).0, "Show licenses and sources").is_some());
        assert_eq!(frame(vec![key(egui::Key::Enter, false)]).1, [ShowLicenses]);
    }

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
    fn image_jump_aspect_and_rotation_menus_scroll_and_dispatch_the_selected_command() {
        for (category, steps, leading, prefix, expected, kind) in [
            (
                "Image jump",
                3,
                0,
                "jump_images_",
                JumpImagesForward10,
                towavue_core::MediaKind::Image,
            ),
            (
                "Edit",
                1,
                8,
                "select_aspect_",
                SelectAspectNineSixteen,
                towavue_core::MediaKind::Image,
            ),
            (
                "Edit",
                1,
                4,
                "free_rotate_image",
                FreeRotateImage,
                towavue_core::MediaKind::Image,
            ),
            (
                "Edit",
                1,
                3,
                "free_rotate_video",
                FreeRotateVideo,
                towavue_core::MediaKind::Video,
            ),
            (
                "Edit",
                1,
                2,
                "resize_video",
                ResizeVideo,
                towavue_core::MediaKind::Video,
            ),
            (
                "View",
                2,
                4,
                "step_audio_",
                StepAudioForward,
                towavue_core::MediaKind::Audio,
            ),
            (
                "File",
                0,
                4,
                "export_audio",
                ExportAudio,
                towavue_core::MediaKind::Video,
            ),
            (
                "File",
                0,
                5,
                "audio_export_options",
                AudioExportOptions,
                towavue_core::MediaKind::Video,
            ),
            (
                "File",
                0,
                4,
                "audio_export_options",
                AudioExportOptions,
                towavue_core::MediaKind::Audio,
            ),
            (
                "File",
                0,
                6,
                "metadata_export_options",
                MetadataExportOptions,
                towavue_core::MediaKind::Video,
            ),
            (
                "File",
                0,
                5,
                "metadata_export_options",
                MetadataExportOptions,
                towavue_core::MediaKind::Audio,
            ),
        ] {
            let context = egui::Context::default();
            let shortcuts = crate::shortcuts::defaults();
            let mut time = 0.0;
            let mut frame = |events| {
                let mut chosen = Vec::new();
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(640.0, 300.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        ui.menu_button("Menu", |ui| {
                            if let Some(command) = show(
                                ui,
                                CommandContext {
                                    media_kind: Some(kind),
                                    timeline_open: kind == towavue_core::MediaKind::Video,
                                    ..Default::default()
                                },
                                &shortcuts,
                            ) {
                                chosen.push(command);
                            }
                        });
                        assert!(!ui.button("Background action").clicked());
                    },
                );
                time += 0.1;
                (output, chosen)
            };
            let key = |key| egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            };
            for _ in 0..3 {
                frame(vec![]);
            }
            assert!(click(&mut frame, egui::pos2(20.0, 15.0)).is_empty());
            frame(vec![]);
            for _ in 0..steps {
                frame(vec![key(egui::Key::ArrowDown)]);
            }
            frame(vec![key(egui::Key::ArrowRight)]);
            for _ in 0..leading {
                frame(vec![key(egui::Key::ArrowDown)]);
            }
            for (index, definition) in command_definitions()
                .iter()
                .filter(|definition| definition.id.as_str().starts_with(prefix))
                .enumerate()
            {
                if index != 0 {
                    frame(vec![key(egui::Key::ArrowDown)]);
                }
                for _ in 0..15 {
                    assert!(frame(vec![]).1.is_empty());
                }
                let (output, chosen) = frame(vec![]);
                assert!(chosen.is_empty());
                let position =
                    text_position(&output, definition.title).expect("focused command is visible");
                let focused = context.memory(|memory| memory.focused()).expect("focus");
                let response = context.read_response(focused).expect("response");
                let rect = context
                    .layer_transform_to_global(response.layer_id)
                    .unwrap_or_default()
                    * response.rect;
                assert!(
                    rect.contains(position),
                    "{} has focus: {rect:?} vs {position:?}",
                    definition.title
                );
            }
            assert_eq!(frame(vec![key(egui::Key::Enter)]).1, [expected]);
            assert!(text_position(&frame(vec![]).0, category).is_none());
        }
    }

    #[test]
    fn every_registered_command_has_exactly_one_menu_location() {
        let mut placed = BTreeSet::new();
        assert_eq!(
            MENUS.iter().map(|(title, _)| *title).collect::<Vec<_>>(),
            ["File", "Edit", "View", "Image jump", "Help"]
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
