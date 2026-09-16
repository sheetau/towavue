use crate::scroll_style::ScrollAreaStyle;
use towavue_core::{CommandContext, CommandId, ShortcutBindings, command_definitions};

use CommandId::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenTarget {
    Tab,
    Window,
    Replace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecentAction {
    Open(
        std::path::PathBuf,
        towavue_runtime_windows::RecentKind,
        OpenTarget,
    ),
    Clear,
}

#[derive(Default)]
pub(crate) struct RecentMenu<'a> {
    pub folders: &'a [std::path::PathBuf],
    pub files: &'a [std::path::PathBuf],
    pub action: Option<RecentAction>,
}

/// Project folder history at delivery, without filesystem work or new persisted
/// entries. A file revisit also advances its parent's position in both pickers.
pub(crate) fn recent_folders(
    entries: &[towavue_runtime_windows::RecentEntry],
) -> Vec<std::path::PathBuf> {
    use towavue_runtime_windows::RecentKind;
    let mut ordered: Vec<_> = entries.iter().collect();
    ordered.sort_by_key(|entry| std::cmp::Reverse(entry.opened_at));
    let mut folders = Vec::new();
    for entry in ordered {
        let folder = match entry.kind {
            RecentKind::File => entry.path.parent(),
            RecentKind::Folder => Some(entry.path.as_path()),
        };
        if let Some(folder) = folder.filter(|path| !path.as_os_str().is_empty())
            && !folders.iter().any(|old| old == folder)
        {
            folders.push(folder.to_path_buf());
        }
    }
    folders
}

const MENUS: &[(&str, &[&[CommandId]])] = &[
    (
        "File",
        &[
            &[OpenFile, OpenFolder, OpenGallery],
            &[GoToFile, OpenRecentFolder],
            &[
                Save,
                ExportAs,
                ExportAudio,
                ExportFrame,
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
            &[VolumeDown, VolumeUp, ToggleMute, CycleVolumeStep],
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
            &[CycleAudioRepeat, ToggleVideoRepeat, ToggleAudioShuffle],
            &[PreviousMedia, NextMedia, PreviousSameKind, NextSameKind],
            &[PreviousImage, NextImage, FirstImage, LastImage],
            &[PreviousTab, NextTab],
            &[
                ZoomIn,
                ZoomOut,
                ActualSize,
                FitToWindow,
                CoverWindow,
                ZoomSelection,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Section {
    File,
    Edit,
    View,
}

impl Section {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::View => "View",
        }
    }
}

#[cfg(test)]
pub fn show(
    ui: &mut egui::Ui,
    context: CommandContext,
    shortcuts: &ShortcutBindings,
) -> Option<CommandId> {
    show_section(ui, context, shortcuts, None)
}

#[cfg(test)]
pub(crate) fn show_section(
    ui: &mut egui::Ui,
    context: CommandContext,
    shortcuts: &ShortcutBindings,
    initial: Option<Section>,
) -> Option<CommandId> {
    show_section_with_recent(ui, context, shortcuts, initial, &mut RecentMenu::default())
}

pub(crate) fn show_section_with_recent(
    ui: &mut egui::Ui,
    context: CommandContext,
    shortcuts: &ShortcutBindings,
    initial: Option<Section>,
    recent: &mut RecentMenu<'_>,
) -> Option<CommandId> {
    let mut chosen = None;
    if let Some(section) = initial {
        return show_items(ui, section.title(), context, shortcuts, recent, None).0;
    }
    let keyboard = MenuKeyboard::begin(ui);
    let requested_category = keyboard
        .right
        .then(|| ui.memory(|memory| memory.focused()))
        .flatten();
    let mut categories = Vec::new();
    for (title, _) in MENUS.iter().filter(|(title, _)| *title != "Image jump") {
        let (response, command) =
            submenu(ui, title, context, shortcuts, requested_category, recent);
        categories.push(response.id);
        chosen = chosen.or(command);
    }
    keyboard.finish(ui, categories);
    chosen
}

fn submenu(
    ui: &mut egui::Ui,
    title: &str,
    context: CommandContext,
    shortcuts: &ShortcutBindings,
    requested: Option<egui::Id>,
    recent: &mut RecentMenu<'_>,
) -> (egui::Response, Option<CommandId>) {
    let category = ui.next_auto_id();
    if requested == Some(category) {
        let submenu = egui::containers::menu::SubMenu::id_from_widget_id(category);
        egui::containers::menu::MenuState::mark_shown(ui.ctx(), submenu);
        egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item = Some(submenu));
    }
    let root = egui::containers::menu::find_menu_root(ui);
    let parent = ui.ctx().read_response(root.id).expect("parent menu").rect;
    let menu = ui.menu_button(title, |ui| {
        show_items(ui, title, context, shortcuts, recent, Some(parent))
    });
    let (chosen, back) = menu.inner.unwrap_or_default();
    if back {
        egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item = None);
        menu.response.request_focus();
    }
    (menu.response, chosen)
}

fn show_items(
    ui: &mut egui::Ui,
    title: &str,
    context: CommandContext,
    shortcuts: &ShortcutBindings,
    recent: &mut RecentMenu<'_>,
    ancestor: Option<egui::Rect>,
) -> (Option<CommandId>, bool) {
    let groups = MENUS
        .iter()
        .find(|(name, _)| *name == title)
        .expect("registered menu")
        .1;
    let keyboard = MenuKeyboard::begin(ui);
    let back = keyboard.left;
    let requested = keyboard
        .right
        .then(|| ui.memory(|memory| memory.focused()))
        .flatten();
    let mut chosen = None;
    let mut items = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt(title)
        .max_height((ui.ctx().content_rect().height() - 64.0).max(100.0))
        .show_styled(ui, |ui| {
            for (index, group) in groups.iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                for id in *group {
                    let definition = command_definitions()
                        .iter()
                        .find(|definition| definition.id == *id)
                        .expect("menu command is registered");
                    let response = ui.add_enabled(
                        definition.is_enabled(context),
                        egui::Button::new(definition.title)
                            .shortcut_text(shortcuts.label(*id, context)),
                    );
                    if response.enabled() {
                        items.push(response.id);
                    }
                    if response.gained_focus()
                        || (response.has_focus()
                            && (response.rect.top() < ui.clip_rect().top()
                                || response.rect.bottom() > ui.clip_rect().bottom()))
                    {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        chosen = Some(*id);
                        ui.close();
                    }
                }
                if title == "File" && index == 0 {
                    let category = ui.next_auto_id();
                    if requested == Some(category) {
                        let id = egui::containers::menu::SubMenu::id_from_widget_id(category);
                        egui::containers::menu::MenuState::mark_shown(ui.ctx(), id);
                        egui::containers::menu::MenuState::from_ui(ui, |state, _| {
                            state.open_item = Some(id)
                        });
                    }
                    let root = egui::containers::menu::find_menu_root(ui);
                    let parent = ui.ctx().read_response(root.id).expect("parent menu").rect;
                    let screen = ui.ctx().content_rect();
                    // Match the submenu's two-point gap outside the parent's frame.
                    // Constraining against the whole viewport makes egui slide a wide
                    // child back over its parent instead of placing it alongside.
                    let right = screen.right() - parent.right();
                    let left = parent.left() - screen.left();
                    // Keep the normal left-to-right cascade away from its root menu.
                    let side_width = if ancestor.is_some_and(|root| root.right() <= parent.left()) {
                        right
                    } else {
                        right.max(left)
                    };
                    let available_width = side_width - 2.0 - 1.0 / ui.ctx().pixels_per_point();
                    let menu = ui
                        .menu_button("Open Recent", |ui| show_recent(ui, recent, available_width));
                    if menu.response.enabled() {
                        items.push(menu.response.id);
                    }
                    if menu.response.gained_focus() {
                        menu.response.scroll_to_me(None);
                    }
                    if menu.inner == Some(true) {
                        egui::containers::menu::MenuState::from_ui(ui, |state, _| {
                            state.open_item = None
                        });
                        menu.response.request_focus();
                    }
                }
            }
            if title == "View" {
                ui.separator();
                let (response, command) =
                    submenu(ui, "Image jump", context, shortcuts, requested, recent);
                if response.gained_focus() {
                    response.scroll_to_me(None);
                }
                if response.enabled() {
                    items.push(response.id);
                }
                chosen = chosen.or(command);
            }
        });
    keyboard.finish(ui, items);
    (chosen, back)
}

fn show_recent(ui: &mut egui::Ui, recent: &mut RecentMenu<'_>, available_width: f32) -> bool {
    use crate::hover_help::HoverHelp;
    use towavue_runtime_windows::RecentKind;
    let keyboard = MenuKeyboard::begin(ui);
    let back = keyboard.left;
    let mut items = Vec::new();
    let frame_width = egui::Frame::popup(ui.style()).total_margin().sum().x;
    // Menu rows are justified. Measure their untruncated labels before setting
    // the limit so short lists do not stretch to the entire viewport.
    let text_width = recent
        .folders
        .iter()
        .take(10)
        .chain(recent.files.iter().take(10))
        .map(|path| path.display().to_string())
        .chain(std::iter::once("Clear Recently Opened".into()))
        .map(|text| {
            egui::WidgetText::from(text)
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Button,
                )
                .size()
                .x
        })
        .fold(0.0, f32::max);
    let width = text_width + 2.0 * ui.spacing().button_padding.x + ui.spacing().scroll.bar_width;
    ui.set_max_width(width.min((available_width - frame_width).max(1.0)));
    egui::ScrollArea::vertical()
        .max_height((ui.ctx().content_rect().height() - 64.0).max(80.0))
        .show_styled(ui, |ui| {
            for (kind, paths) in [
                (RecentKind::Folder, recent.folders),
                (RecentKind::File, recent.files),
            ] {
                for path in paths.iter().take(10) {
                    let response = ui
                        .push_id((kind == RecentKind::Folder, path), |ui| {
                            ui.add(egui::Button::new(path.display().to_string()).truncate())
                        })
                        .inner
                        .help_text(path.display().to_string());
                    if response.enabled() {
                        items.push(response.id);
                    }
                    if response.gained_focus() {
                        response.scroll_to_me(None);
                    }
                    let modifiers = ui.input(|input| {
                        input
                            .events
                            .iter()
                            .rev()
                            .find_map(|event| match event {
                                egui::Event::PointerButton {
                                    modifiers,
                                    pressed: false,
                                    ..
                                }
                                | egui::Event::Key {
                                    modifiers,
                                    key: egui::Key::Enter | egui::Key::Space,
                                    pressed: true,
                                    ..
                                } => Some(*modifiers),
                                _ => None,
                            })
                            .unwrap_or(input.modifiers)
                    });
                    let modified_enter = response.enabled()
                        && response.has_focus()
                        && (modifiers.ctrl || modifiers.alt)
                        && ui.input_mut(|input| input.consume_key(modifiers, egui::Key::Enter));
                    if response.clicked() || modified_enter {
                        let target = if modifiers.ctrl {
                            OpenTarget::Window
                        } else if modifiers.alt {
                            OpenTarget::Replace
                        } else {
                            OpenTarget::Tab
                        };
                        recent.action = Some(RecentAction::Open(path.clone(), kind, target));
                        ui.close();
                    }
                }
                if !paths.is_empty() {
                    ui.separator();
                }
            }
            // Resume positions can outlive the shorter recent-path lists.
            let response = ui.add(egui::Button::new("Clear Recently Opened").truncate());
            if response.enabled() {
                items.push(response.id);
            }
            if response.clicked() {
                recent.action = Some(RecentAction::Clear);
                ui.close();
            }
        });
    keyboard.finish(ui, items);
    back
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
        let modality = ui.input(|input| {
            input.events.iter().rev().find_map(|event| match event {
                egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } => Some(false),
                egui::Event::Key { pressed: true, .. } | egui::Event::AccessKitActionRequest(_) => {
                    Some(true)
                }
                _ => None,
            })
        });
        let frame = ui.ctx().cumulative_frame_nr();
        let keyboard = ui.data_mut(|data| {
            let state =
                data.get_temp_mut_or(egui::Id::new("menu-keyboard-input"), (u64::MAX, true));
            if state.0 != frame {
                state.0 = frame;
                if let Some(modality) = modality {
                    state.1 = modality;
                }
            }
            state.1
        });
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
            active: active && keyboard,
            left: false,
            right: false,
            requested_focus: None,
            initial_move: None,
        };
        if !keyboard {
            ui.memory_mut(|memory| {
                for id in &items {
                    memory.surrender_focus(*id);
                }
            });
        }
        if !result.active {
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
        if backward || forward || left || right {
            // Consuming the event does not clear egui's already queued spatial traversal.
            ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        }
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
            let current = items.iter().position(|id| Some(*id) == selected);
            let next = current.map_or_else(
                || if backward { items.len() - 1 } else { 0 },
                |current| {
                    if backward {
                        (current + items.len() - 1) % items.len()
                    } else {
                        (current + 1) % items.len()
                    }
                },
            );
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
    fn recent_folder_projection_merges_parents_and_explicit_opens_by_recency() {
        use towavue_runtime_windows::{RecentEntry, RecentKind};
        let entries = vec![
            RecentEntry {
                path: "C:/album".into(),
                kind: RecentKind::Folder,
                opened_at: Some(10),
            },
            RecentEntry {
                path: "C:/legacy/old.png".into(),
                kind: RecentKind::File,
                opened_at: None,
            },
            RecentEntry {
                path: "C:/other".into(),
                kind: RecentKind::Folder,
                opened_at: Some(20),
            },
            RecentEntry {
                path: "C:/album/new.png".into(),
                kind: RecentKind::File,
                opened_at: Some(30),
            },
            RecentEntry {
                path: "C:/album/second.png".into(),
                kind: RecentKind::File,
                opened_at: Some(25),
            },
            RecentEntry {
                path: "C:/legacy/second.png".into(),
                kind: RecentKind::File,
                opened_at: None,
            },
            RecentEntry {
                path: "C:/root.png".into(),
                kind: RecentKind::File,
                opened_at: Some(5),
            },
            RecentEntry {
                path: "bare.png".into(),
                kind: RecentKind::File,
                opened_at: None,
            },
        ];
        let original = entries.clone();
        assert_eq!(
            recent_folders(&entries),
            ["C:/album", "C:/other", "C:/", "C:/legacy"].map(std::path::PathBuf::from)
        );
        assert_eq!(entries, original);
        assert!(recent_folders(&[]).is_empty());
    }

    #[test]
    fn recent_menu_width_follows_path_text_without_overlapping_its_parent() {
        use egui::accesskit::{Action, ActionRequest, TreeId};
        for density in [1.0, 1.25, 2.0] {
            for width in [320.0, 480.0, 1200.0] {
                for (right_edge, nested) in [(false, false), (true, false), (false, true)] {
                    if nested && width < 480.0 {
                        continue; // The native application has a 480-point minimum width.
                    }
                    for long in [false, true] {
                        let context = crate::fonts::test_context();
                        context.enable_accesskit();
                        context.set_pixels_per_point(density);
                        context.global_style_mut(crate::chrome::style);
                        let path = std::path::PathBuf::from(if long {
                            format!("C:/{}/image.png", "a long folder description ".repeat(6))
                        } else {
                            "C:/short.png".into()
                        });
                        let files = [path.clone()];
                        let parent = std::cell::Cell::new(egui::Rect::NOTHING);
                        let ancestor = std::cell::Cell::new(egui::Rect::NOTHING);
                        let frame = |events| {
                            context.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(egui::Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(width, 480.0),
                                    )),
                                    events,
                                    ..Default::default()
                                },
                                |ui| {
                                    ui.horizontal(|ui| {
                                        if right_edge {
                                            ui.add_space(width - 120.0);
                                        }
                                        let file_menu = |ui: &mut egui::Ui| {
                                            let root = egui::containers::menu::find_menu_root(ui);
                                            parent.set(
                                                ui.ctx()
                                                    .read_response(root.id)
                                                    .expect("parent menu")
                                                    .rect,
                                            );
                                            show_items(
                                                ui,
                                                "File",
                                                CommandContext::default(),
                                                &crate::shortcuts::defaults(),
                                                &mut RecentMenu {
                                                    files: &files,
                                                    ..Default::default()
                                                },
                                                nested.then(|| ancestor.get()),
                                            );
                                        };
                                        ui.menu_button("Menu", |ui| {
                                            if nested {
                                                ui.set_min_width(110.0);
                                                let root =
                                                    egui::containers::menu::find_menu_root(ui);
                                                ancestor.set(
                                                    ui.ctx()
                                                        .read_response(root.id)
                                                        .expect("root menu")
                                                        .rect,
                                                );
                                                ui.menu_button("File", file_menu);
                                                for title in ["Edit", "View", "Help"] {
                                                    let _ = ui.button(title);
                                                }
                                            } else {
                                                file_menu(ui);
                                            }
                                        });
                                    });
                                },
                            )
                        };
                        let labels: &[&str] = if nested {
                            &["Menu", "File", "Open Recent"]
                        } else {
                            &["Menu", "Open Recent"]
                        };
                        for label in labels {
                            for _ in 0..4 {
                                frame(vec![]);
                            }
                            let output = frame(vec![]);
                            let trigger = output
                                .platform_output
                                .accesskit_update
                                .expect("tree")
                                .nodes
                                .iter()
                                .find(|(_, node)| {
                                    node.label().is_some_and(|text| {
                                        text.trim_end_matches('⏵').trim() == *label
                                    })
                                })
                                .expect("menu trigger")
                                .0;
                            frame(vec![egui::Event::AccessKitActionRequest(ActionRequest {
                                action: Action::Click,
                                target_tree: TreeId::ROOT,
                                target_node: trigger,
                                data: None,
                            })]);
                        }
                        for _ in 0..4 {
                            frame(vec![]);
                        }
                        let output = frame(vec![]);
                        let tree = output.platform_output.accesskit_update.expect("menu tree");
                        for label in [path.to_string_lossy().as_ref(), "Clear Recently Opened"] {
                            let bounds = tree
                                .nodes
                                .iter()
                                .find(|(_, node)| node.label() == Some(label))
                                .expect("recent button")
                                .1
                                .bounds()
                                .expect("button bounds");
                            assert!(
                                bounds.x0 >= 0.0
                                    && bounds.x1 <= f64::from(width) + 1.0 / f64::from(density),
                                "viewport: {width}, {density}, {right_edge}, {long}: {bounds:?}"
                            );
                            let parent = parent.get();
                            assert!(
                                bounds.x0 >= f64::from(parent.right())
                                    || bounds.x1 <= f64::from(parent.left()),
                                "overlap: {width}, {density}, {right_edge}, {long}: parent={parent:?}, child={bounds:?}"
                            );
                            if nested {
                                let ancestor = ancestor.get();
                                assert!(
                                    bounds.x0 >= f64::from(ancestor.right())
                                        || bounds.x1 <= f64::from(ancestor.left()),
                                    "ancestor overlap: {width}, {density}, {long}: ancestor={ancestor:?}, parent={parent:?}, child={bounds:?}"
                                );
                            }
                            if label != "Clear Recently Opened" && width == 1200.0 {
                                assert_eq!(
                                    bounds.width() > 520.0,
                                    long,
                                    "content-sized, not viewport-sized"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn recent_menu_groups_bounds_and_modified_keyboard_activation() {
        use egui::accesskit::{Action, ActionRequest, TreeId};
        for density in [1.0, 1.25, 2.0] {
            for (modifiers, expected, pointer) in [
                (egui::Modifiers::NONE, OpenTarget::Tab, false),
                (egui::Modifiers::CTRL, OpenTarget::Window, false),
                (egui::Modifiers::ALT, OpenTarget::Replace, false),
                (egui::Modifiers::NONE, OpenTarget::Tab, true),
                (egui::Modifiers::CTRL, OpenTarget::Window, true),
                (egui::Modifiers::ALT, OpenTarget::Replace, true),
            ] {
                let context = crate::fonts::test_context();
                context.enable_accesskit();
                context.set_pixels_per_point(density);
                let folders: Vec<_> = (0..12)
                    .map(|i| std::path::PathBuf::from(format!("C:/folders/{i:02}")))
                    .collect();
                let files: Vec<_> = (0..12)
                    .map(|i| std::path::PathBuf::from(format!("C:/files/{i:02}.png")))
                    .collect();
                let empty = std::cell::Cell::new(false);
                let mut time = 0.0;
                let mut frame = |events| {
                    time += 0.1;
                    let mut recent = RecentMenu {
                        folders: if empty.get() { &[] } else { &folders },
                        files: if empty.get() { &[] } else { &files },
                        action: None,
                    };
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(960.0, 760.0),
                            )),
                            events,
                            time: Some(time),
                            ..Default::default()
                        },
                        |ui| {
                            ui.menu_button("Menu", |ui| {
                                show_section_with_recent(
                                    ui,
                                    CommandContext::default(),
                                    &ShortcutBindings::default(),
                                    Some(Section::File),
                                    &mut recent,
                                );
                            });
                        },
                    );
                    (output, recent.action)
                };
                let node = |output: &egui::FullOutput, label: &str| {
                    output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .expect("tree")
                        .nodes
                        .iter()
                        .find(|(_, n)| {
                            n.label()
                                .is_some_and(|text| text.trim_end_matches('⏵').trim() == label)
                        })
                        .unwrap_or_else(|| {
                            panic!(
                                "node {label}: {:?}",
                                output
                                    .platform_output
                                    .accesskit_update
                                    .as_ref()
                                    .expect("tree")
                                    .nodes
                                    .iter()
                                    .filter_map(|(_, n)| n.label())
                                    .collect::<Vec<_>>()
                            )
                        })
                        .0
                };
                let action = |target, action| {
                    egui::Event::AccessKitActionRequest(ActionRequest {
                        target_tree: TreeId::ROOT,
                        target_node: target,
                        action,
                        data: None,
                    })
                };
                for _ in 0..3 {
                    frame(vec![]);
                }
                let output = frame(vec![]).0;
                frame(vec![action(node(&output, "Menu"), Action::Click)]);
                let output = frame(vec![]).0;
                frame(vec![action(node(&output, "Open Recent"), Action::Click)]);
                let output = frame(vec![]).0;
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("menu tree");
                let labels: Vec<_> = tree.nodes.iter().filter_map(|(_, n)| n.label()).collect();
                let folder = folders[0].display().to_string();
                let file = files[0].display().to_string();
                let top = |label: &str| {
                    tree.nodes
                        .iter()
                        .find(|(_, n)| n.label() == Some(label))
                        .expect("entry")
                        .1
                        .bounds()
                        .expect("bounds")
                        .y0
                };
                assert!(top(&folder) < top(&file), "folders precede files visually");
                for absent in [&folders[10], &files[10]] {
                    assert!(
                        !labels.contains(&absent.display().to_string().as_str()),
                        "ten per menu group"
                    );
                }
                node(&output, "Clear Recently Opened");
                let target = node(&output, &file);
                let result = if pointer {
                    let pos = text_position(&output, &file).expect("file text");
                    let button = |pressed| egui::Event::PointerButton {
                        pos,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers,
                    };
                    frame(vec![egui::Event::PointerMoved(pos), button(true)]);
                    frame(vec![button(false)]).1
                } else {
                    frame(vec![action(target, Action::Focus)]);
                    frame(vec![egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }])
                    .1
                };
                assert_eq!(
                    result,
                    Some(RecentAction::Open(
                        files[0].clone(),
                        towavue_runtime_windows::RecentKind::File,
                        expected
                    ))
                );
                for label in ["Menu", "Open Recent"] {
                    let output = frame(vec![]).0;
                    frame(vec![action(node(&output, label), Action::Click)]);
                    frame(vec![]);
                }
                let output = frame(vec![]).0;
                assert_eq!(
                    frame(vec![action(
                        node(&output, "Clear Recently Opened"),
                        Action::Click
                    )])
                    .1,
                    Some(RecentAction::Clear)
                );
                empty.set(true);
                for label in ["Menu", "Open Recent"] {
                    let output = frame(vec![]).0;
                    frame(vec![action(node(&output, label), Action::Click)]);
                    frame(vec![]);
                }
                let output = frame(vec![]).0;
                assert_eq!(
                    frame(vec![action(
                        node(&output, "Clear Recently Opened"),
                        Action::Click
                    )])
                    .1,
                    Some(RecentAction::Clear),
                    "empty path lists may still have resume positions"
                );
            }
        }
    }

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
            assert!(
                rect.top() >= 0.0 && rect.bottom() <= 300.0,
                "focused {label} is visible: {rect:?}"
            );
        };
        navigate(egui::Key::ArrowRight, false, "Open file");
        navigate(egui::Key::ArrowDown, false, "Open folder");
        navigate(egui::Key::ArrowDown, false, "Open Gallery");
        navigate(egui::Key::ArrowDown, false, "Open Recent");
        navigate(egui::Key::ArrowDown, false, "Go to File");
        navigate(egui::Key::ArrowDown, false, "Open Recent Folder");
        navigate(egui::Key::ArrowDown, false, "Close tab");
        navigate(egui::Key::ArrowDown, false, "Reopen closed tab");
        navigate(egui::Key::ArrowDown, false, "Reload keyboard shortcuts");
        navigate(egui::Key::Tab, false, "Open file");
        navigate(egui::Key::ArrowLeft, false, "File");
        navigate(egui::Key::ArrowDown, false, "Edit");
        navigate(
            egui::Key::ArrowRight,
            false,
            "Cycle volume step (2% / 5% / 10%)",
        );
        navigate(egui::Key::ArrowLeft, false, "Edit");
        navigate(egui::Key::ArrowDown, false, "View");
        navigate(egui::Key::ArrowRight, false, "Toggle fullscreen");
        navigate(egui::Key::Tab, true, "Image jump");
        navigate(egui::Key::Tab, true, "Show command palette");
        assert_eq!(
            frame(vec![key(egui::Key::Enter, false)]).1,
            [ToggleCommandPalette]
        );
        assert!(text_position(&frame(vec![]).0, "File").is_none());
        assert!(click(&mut frame, egui::pos2(20.0, 15.0)).is_empty());
        // File -> Help -> View.
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
        assert!(text_position(&output, "Image jump").is_none());
        for title in ["Edit", "View", "Help"] {
            assert!(text_position(&output, title).is_some());
        }
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
                2,
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
                "export_frame",
                ExportFrame,
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
            (
                "File",
                0,
                4,
                "metadata_export_options",
                MetadataExportOptions,
                towavue_core::MediaKind::Image,
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
                                    has_video_frame: expected == ExportFrame,
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
            for _ in 0..=steps {
                frame(vec![key(egui::Key::ArrowDown)]);
            }
            frame(vec![key(egui::Key::ArrowRight)]);
            if category == "Image jump" {
                frame(vec![egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::SHIFT,
                }]);
                frame(vec![key(egui::Key::ArrowRight)]);
            }
            for _ in 0..leading + 4 * usize::from(category == "File") {
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
