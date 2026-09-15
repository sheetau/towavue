use crate::scroll_style::ScrollAreaStyle;
use egui::AtomExt;
use towavue_core::{CommandContext, CommandId, ShortcutBindings, command_definitions};

use crate::hover_help::HoverHelp;
use crate::menu::{OpenTarget, RecentAction};
use std::path::{Path, PathBuf};
use towavue_core::{
    file_search_score as path_score, search_text as normalized, search_text_score as text_score,
};
use towavue_runtime_windows::RecentKind;

pub struct CommandPalette {
    query: String,
    selected: Option<usize>,
    selected_path: Option<PathBuf>,
    folders: bool,
    fresh: bool,
    ime_composing: bool,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self {
            query: ">".into(),
            selected: None,
            selected_path: None,
            folders: false,
            fresh: true,
            ime_composing: false,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct OpenSources<'a> {
    pub files: &'a [PathBuf],
    pub folders: &'a [PathBuf],
    pub folder: Option<&'a towavue_core::FolderSnapshot>,
    pub search: Option<&'a towavue_runtime_windows::FileSearchResult>,
    pub searching: bool,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Choice {
    Command(CommandId),
    Open(RecentAction),
}

impl CommandPalette {
    pub fn file_query(&self) -> Option<String> {
        (!self.folders && !self.query.starts_with('>') && !self.query.trim().is_empty())
            .then(|| normalized(self.query.trim()))
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn open_files(&mut self, folders: bool) {
        self.reset();
        self.query.clear();
        self.folders = folders;
    }

    #[cfg(test)]
    pub fn show(
        &mut self,
        context: &egui::Context,
        commands: CommandContext,
        shortcuts: &ShortcutBindings,
        top: f32,
    ) -> (Option<CommandId>, bool) {
        let (choice, close) =
            self.show_with_sources(context, commands, shortcuts, top, OpenSources::default());
        (
            choice.and_then(|choice| match choice {
                Choice::Command(command) => Some(command),
                Choice::Open(_) => None,
            }),
            close,
        )
    }

    pub(crate) fn show_with_sources(
        &mut self,
        context: &egui::Context,
        commands: CommandContext,
        shortcuts: &ShortcutBindings,
        top: f32,
        sources: OpenSources<'_>,
    ) -> (Option<Choice>, bool) {
        let mut chosen = None;
        let query_id = egui::Id::new("command-palette-query");
        let opened = self.fresh;
        if self.fresh {
            let mut state = egui::text_edit::TextEditState::default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(self.query.chars().count()),
                )));
            state.store(context, query_id);
            self.fresh = false;
        }
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
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            ..
                        }
                    )
                });
                return (false, false, None, false);
            }
            let enter = input.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.shift && !modifiers.mac_cmd => Some(*modifiers),
                _ => None,
            });
            if let Some(modifiers) = enter {
                input.consume_key(modifiers, egui::Key::Enter);
            }
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                enter,
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        let backdrop = egui::Area::new("command-palette-backdrop".into())
            .order(egui::Order::Foreground)
            .fixed_pos(context.content_rect().min)
            .movable(false)
            .sense(egui::Sense::CLICK | egui::Sense::DRAG)
            .show(context, |ui| ui.set_min_size(context.content_rect().size()));
        egui::Window::new("Command palette")
            .id("command-palette".into())
            .order(egui::Order::Foreground)
            .anchor(
                egui::Align2::CENTER_TOP,
                [0.0, top - context.content_rect().top()],
            )
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(crate::chrome::FLOATING_BACKGROUND)
                    .stroke(egui::Stroke::new(1.0, crate::chrome::BORDER))
                    .inner_margin(6)
                    .corner_radius(4),
            )
            .show(context, |ui| {
                ui.set_width((context.content_rect().width() - 50.0).clamp(120.0, 586.0));
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
                        .text_color(crate::chrome::FOREGROUND)
                        .desired_width(f32::INFINITY)
                        .hint_text(if self.folders {
                            "Select to open (hold Ctrl-key to force new window or Alt-key for same window)"
                        } else if previous_query.starts_with('>') {
                            "Type the name of a command to run."
                        } else {
                            "Search files by name"
                        }),
                )
                .help_text("Up / Down: select   Enter: open/run   Ctrl: new window   Alt: same tab   Esc: close");
                let command_mode = self.query.starts_with('>');
                if command_mode {
                    self.folders = false;
                }
                context.accesskit_node_builder(query_id, |node| {
                    node.set_label(if command_mode { "Search commands" }
                        else if self.folders { "Search recent folders" } else { "Search files" });
                    node.add_action(egui::accesskit::Action::SetValue);
                });
                let query_changed = opened || previous_query != self.query;
                if query_changed {
                    self.selected = None;
                    self.selected_path = None;
                }
                if !command_mode {
                    chosen = self.show_files(ui, &sources, (up, down), enter, query_changed)
                        .map(Choice::Open);
                    return;
                }
                let query = self.query[1..].trim().to_ascii_lowercase();
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
                if enter.is_some() && let Some(index) = self.selected {
                    chosen = Some(Choice::Command(matches[index].id));
                }
                egui::ScrollArea::vertical()
                    .max_height((context.content_rect().height() - 90.0).clamp(40.0, 264.0))
                    .show_styled(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if matches.is_empty() {
                            ui.weak("No matching commands");
                        }
                        for (index, definition) in matches.iter().enumerate() {
                            let shortcut = shortcuts.label(definition.id, commands);
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
                                .help_ui(|ui| {
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
                            if response.clicked() && !query_changed {
                                chosen = Some(Choice::Command(definition.id));
                            }
                        }
                    });
            });
        (
            chosen,
            close || (!egui::Popup::is_any_open(context) && backdrop.response.clicked()),
        )
    }

    fn show_files(
        &mut self,
        ui: &mut egui::Ui,
        sources: &OpenSources<'_>,
        navigation: (bool, bool),
        enter: Option<egui::Modifiers>,
        query_changed: bool,
    ) -> Option<RecentAction> {
        let (up, down) = navigation;
        let recent = if self.folders {
            sources.folders
        } else {
            sources.files
        };
        let query = normalized(self.query.trim());
        let mut seen = std::collections::HashSet::new();
        let mut paths: Vec<_> = recent
            .iter()
            .filter(|path| {
                let score = if self.folders {
                    text_score(&normalized(&path.to_string_lossy()), &query)
                } else {
                    path_score(path, &query)
                };
                score.is_some() && seen.insert(normalized(&path.to_string_lossy()))
            })
            .collect();
        let recent_count = paths.len();
        // Empty Go to File shows history immediately, as in the reference picker.
        let search = sources
            .search
            .filter(|result| result.request.query == query);
        if !self.folders
            && let Some(result) = search
        {
            paths.extend(
                result
                    .paths
                    .iter()
                    .filter(|path| seen.insert(normalized(&path.to_string_lossy()))),
            );
        } else if !self.folders
            && !query.is_empty()
            && let Some(folder) = sources.folder
        {
            let mut found: Vec<_> = folder
                .items
                .iter()
                .take(towavue_runtime_windows::FILE_SEARCH_LIMIT)
                .filter_map(|item| {
                    let score = path_score(&item.path, &query)?;
                    seen.insert(normalized(&item.path.to_string_lossy()))
                        .then_some((score, &item.path))
                })
                .collect();
            found.sort_by_key(|(score, _)| *score);
            paths.extend(found.into_iter().map(|(_, path)| path));
        }
        let selected = self
            .selected_path
            .as_ref()
            .and_then(|selected| paths.iter().position(|path| *path == selected))
            .or_else(|| (!paths.is_empty()).then_some(0));
        let selected = if up || down {
            next_enabled(selected, &vec![true; paths.len()], down)
        } else {
            selected
        };
        let selection_moved = self.selected != selected;
        self.selected = selected;
        self.selected_path = selected.map(|index| paths[index].clone());
        let kind = if self.folders {
            RecentKind::Folder
        } else {
            RecentKind::File
        };
        let mut chosen = enter.zip(selected).map(|(modifiers, index)| {
            RecentAction::Open(paths[index].clone(), kind, open_target(modifiers))
        });
        if !self.folders {
            if let Some(result) = search {
                if let Some(error) = &result.error {
                    ui.weak(error);
                } else if result.matches > result.paths.len() as u64 || result.skipped != 0 {
                    ui.weak(format!("Showing {} of {} matches; {} entries skipped",
                        result.paths.len(), result.matches, result.skipped))
                        .help_text("Unreadable entries, links/junctions and folders deeper than 128 levels are skipped. Refine the query to narrow results.");
                }
            } else if sources.searching && !query.is_empty() {
                ui.weak("Searching subfolders...");
            }
        }
        egui::ScrollArea::vertical()
            .id_salt(("quick-open-results", self.folders))
            .max_height((ui.ctx().content_rect().height() - 90.0).clamp(40.0, 264.0))
            .show_styled(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if paths.is_empty() {
                    ui.weak(if self.folders {
                        "No matching recent folders"
                    } else {
                        "No matching files"
                    });
                }
                for (index, path) in paths.iter().enumerate() {
                    if index == 0 || index == recent_count {
                        ui.weak(if index < recent_count {
                            "recently opened"
                        } else {
                            "file results"
                        });
                    }
                    let name = path
                        .file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy();
                    let parent = path.parent().unwrap_or(Path::new("")).to_string_lossy();
                    let response = ui
                        .push_id(path, |ui| {
                            ui.add(
                                egui::Button::selectable(
                                    selected == Some(index),
                                    (
                                        name.as_ref(),
                                        egui::Atom::grow(),
                                        egui::RichText::new(parent.as_ref())
                                            .color(crate::chrome::MUTED)
                                            .atom_max_width(ui.available_width() * 0.6),
                                    ),
                                )
                                .truncate()
                                .min_size(egui::vec2(ui.available_width(), 22.0)),
                            )
                        })
                        .inner
                        .help_text(path.to_string_lossy());
                    ui.ctx().accesskit_node_builder(response.id, |node| {
                        node.clear_toggled();
                        node.set_label(path.to_string_lossy().as_ref());
                    });
                    if (up || down || query_changed || selection_moved) && selected == Some(index) {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    if response.clicked() && !query_changed {
                        let modifiers = ui.input(|input| {
                            input
                                .events
                                .iter()
                                .rev()
                                .find_map(|event| match event {
                                    egui::Event::PointerButton {
                                        button: egui::PointerButton::Primary,
                                        pressed: false,
                                        modifiers,
                                        ..
                                    } => Some(*modifiers),
                                    _ => None,
                                })
                                .unwrap_or(input.modifiers)
                        });
                        chosen = Some(RecentAction::Open(
                            (*path).clone(),
                            kind,
                            open_target(modifiers),
                        ));
                    }
                }
            });
        chosen
    }
}

fn open_target(modifiers: egui::Modifiers) -> OpenTarget {
    if modifiers.ctrl {
        OpenTarget::Window
    } else if modifiers.alt {
        OpenTarget::Replace
    } else {
        OpenTarget::Tab
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

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn open_frame(
        context: &egui::Context,
        palette: &mut CommandPalette,
        sources: OpenSources<'_>,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Vec<Choice>) {
        let mut choices = Vec::new();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |_| {
                let (choice, _) = palette.show_with_sources(
                    context,
                    CommandContext {
                        palette_open: true,
                        ..Default::default()
                    },
                    &crate::shortcuts::defaults(),
                    0.0,
                    sources,
                );
                choices.extend(choice);
            },
        );
        (output, choices)
    }

    #[test]
    fn file_and_folder_picker_preserve_event_modifiers_for_enter_and_click() {
        for folders in [false, true] {
            for pointer in [false, true] {
                for (modifiers, target) in [
                    (egui::Modifiers::NONE, OpenTarget::Tab),
                    (egui::Modifiers::CTRL, OpenTarget::Window),
                    (egui::Modifiers::ALT, OpenTarget::Replace),
                    (
                        egui::Modifiers::CTRL | egui::Modifiers::ALT,
                        OpenTarget::Window,
                    ),
                ] {
                    let context = egui::Context::default();
                    context.enable_accesskit();
                    let mut palette = CommandPalette::default();
                    palette.open_files(folders);
                    let files = [PathBuf::from("C:/media/日本語 image.png")];
                    let directories = [PathBuf::from("C:/media/folder.mp4")];
                    let sources = OpenSources {
                        files: &files,
                        folders: &directories,
                        folder: None,
                        ..Default::default()
                    };
                    for _ in 0..4 {
                        open_frame(&context, &mut palette, sources, vec![]);
                    }
                    let path = if folders { &directories[0] } else { &files[0] };
                    let choices = if pointer {
                        let output = open_frame(&context, &mut palette, sources, vec![]).0;
                        let tree = output.platform_output.accesskit_update.expect("tree");
                        let (_, node) = tree
                            .nodes
                            .iter()
                            .find(|(_, node)| node.label() == Some(path.to_string_lossy().as_ref()))
                            .expect("path row");
                        let rect = node.bounds().expect("row bounds");
                        let pos = egui::pos2(
                            (rect.x0 + rect.x1) as f32 / 2.0,
                            (rect.y0 + rect.y1) as f32 / 2.0,
                        );
                        let button = |pressed| egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers,
                        };
                        open_frame(
                            &context,
                            &mut palette,
                            sources,
                            vec![egui::Event::PointerMoved(pos)],
                        );
                        open_frame(&context, &mut palette, sources, vec![button(true)]);
                        open_frame(&context, &mut palette, sources, vec![button(false)]).1
                    } else {
                        open_frame(
                            &context,
                            &mut palette,
                            sources,
                            vec![key(egui::Key::Enter, modifiers)],
                        )
                        .1
                    };
                    assert_eq!(
                        choices,
                        [Choice::Open(RecentAction::Open(
                            path.clone(),
                            if folders {
                                RecentKind::Folder
                            } else {
                                RecentKind::File
                            },
                            target
                        ))]
                    );
                }
            }
        }
    }

    #[test]
    fn picker_prefix_reset_and_ime_keep_text_editing_separate_from_opening() {
        let context = egui::Context::default();
        let mut palette = CommandPalette::default();
        let files = [PathBuf::from("C:/media/open file.png")];
        let sources = OpenSources {
            files: &files,
            ..Default::default()
        };
        palette.open_files(true);
        for _ in 0..3 {
            open_frame(&context, &mut palette, sources, vec![]);
        }
        open_frame(
            &context,
            &mut palette,
            sources,
            vec![egui::Event::Text(">".into())],
        );
        assert!(
            !palette.folders,
            "command prefix leaves the recent-folder provider"
        );
        open_frame(
            &context,
            &mut palette,
            sources,
            vec![key(egui::Key::Backspace, egui::Modifiers::NONE)],
        );
        assert!(palette.query.is_empty());
        assert_eq!(
            open_frame(
                &context,
                &mut palette,
                sources,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)]
            )
            .1,
            [Choice::Open(RecentAction::Open(
                files[0].clone(),
                RecentKind::File,
                OpenTarget::Tab
            ))]
        );
        for modifiers in [egui::Modifiers::CTRL, egui::Modifiers::ALT] {
            palette.open_files(false);
            open_frame(&context, &mut palette, sources, vec![]);
            assert!(
                open_frame(
                    &context,
                    &mut palette,
                    sources,
                    vec![
                        egui::Event::Ime(egui::ImeEvent::Commit("open".into())),
                        key(egui::Key::Enter, modifiers),
                    ]
                )
                .1
                .is_empty()
            );
        }
        palette.reset();
        open_frame(
            &context,
            &mut palette,
            sources,
            vec![egui::Event::Text("open file".into())],
        );
        assert_eq!(
            palette.query, ">open file",
            "reopening resets the persisted text cursor"
        );
        assert_eq!(
            open_frame(
                &context,
                &mut palette,
                sources,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)]
            )
            .1,
            [Choice::Command(CommandId::OpenFile)]
        );
    }

    #[test]
    fn file_search_keeps_recent_priority_shell_ties_and_selected_path_after_refresh() {
        use towavue_core::{
            FolderMediaItem, FolderSnapshot, FolderSnapshotSource, MediaKind, ShellIdentity,
        };
        let context = egui::Context::default();
        let mut palette = CommandPalette::default();
        palette.open_files(false);
        palette.query = "image".into();
        let files = [PathBuf::from("C:/elsewhere/recent-image.png")];
        let mut folder = FolderSnapshot {
            folder_identity: ShellIdentity::new(vec![0]),
            folder_path: "C:/media".into(),
            items: ["recent-image.png", "image2.png", "image1.png"]
                .into_iter()
                .enumerate()
                .map(|(id, name)| FolderMediaItem {
                    identity: ShellIdentity::new(vec![id as u8]),
                    path: if id == 0 {
                        files[0].clone()
                    } else {
                        Path::new("C:/media").join(name)
                    },
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        };
        for _ in 0..3 {
            open_frame(
                &context,
                &mut palette,
                OpenSources {
                    files: &files,
                    folder: Some(&folder),
                    ..Default::default()
                },
                vec![],
            );
        }
        assert_eq!(palette.selected_path.as_ref(), Some(&files[0]));
        open_frame(
            &context,
            &mut palette,
            OpenSources {
                files: &files,
                folder: Some(&folder),
                ..Default::default()
            },
            vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
        );
        assert_eq!(
            palette.selected_path.as_ref(),
            Some(&folder.items[1].path),
            "deduplicate history and preserve Shell score ties"
        );
        let selected = palette.selected_path.clone().expect("selected path");
        folder.items.swap(1, 2);
        assert_eq!(
            open_frame(
                &context,
                &mut palette,
                OpenSources {
                    files: &files,
                    folder: Some(&folder),
                    ..Default::default()
                },
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)]
            )
            .1,
            [Choice::Open(RecentAction::Open(
                selected,
                RecentKind::File,
                OpenTarget::Tab
            ))]
        );
        assert_eq!(
            path_score(Path::new("C:/Media/日本語.png"), "日本"),
            Some(1)
        );
        assert_eq!(
            path_score(Path::new("C:/Media/holiday-image.png"), "hlimg"),
            Some(3)
        );
        assert_eq!(
            path_score(Path::new("C:/Media/holiday.png"), "media/hol"),
            Some(2)
        );
        assert_eq!(path_score(Path::new("C:/Media/holiday.png"), "other"), None);
    }

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
                            0.0,
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
            assert_eq!(node.label(), Some("Search files"));
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
            palette.open_files(false);
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
                        assert_eq!(
                            palette.show(&context, commands, &shortcuts, 0.0),
                            (None, false)
                        );
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
        for (size, top) in [
            egui::vec2(960.0, 576.0),
            egui::vec2(480.0, 300.0),
            egui::vec2(240.0, 180.0),
        ]
        .into_iter()
        .flat_map(|size| [0.0, 30.0].map(|top| (size, top)))
        {
            let context = egui::Context::default();
            context.global_style_mut(|style| {
                crate::chrome::style(style);
                style.animation_time = 0.0;
                style.interaction.tooltip_delay = 0.0;
            });
            let mut palette = CommandPalette {
                query: ">open".into(),
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
                        if let (Some(command), _) =
                            palette.show(&context, commands, &shortcuts, top)
                        {
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
                    egui::Shape::Rect(rect)
                        if rect.fill == crate::chrome::FLOATING_BACKGROUND
                            && rect.stroke.color == crate::chrome::BORDER
                            && rect.corner_radius == egui::CornerRadius::same(4) =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("dark palette panel");
            assert!(panel.left() >= 0.0 && panel.right() <= size.x);
            assert!((panel.top() - top).abs() <= 1.0 && panel.bottom() <= size.y);
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
            assert_eq!(
                text(">open").galley.job.sections[0].format.color,
                crate::chrome::FOREGROUND
            );
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
            let pos = egui::pos2(folder.pos.x + 32.0, folder.pos.y + 7.0);
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
    fn outside_click_closes_palette_without_activating_the_background() {
        let context = egui::Context::default();
        let mut palette = CommandPalette::default();
        let mut open = true;
        let outside = egui::pos2(40.0, 540.0);
        let frame = |palette: &mut CommandPalette, open: &mut bool, events| {
            let mut background_clicked = false;
            let mut closed = false;
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    background_clicked |= ui
                        .put(
                            egui::Rect::from_min_size(
                                egui::pos2(10.0, 520.0),
                                egui::vec2(100.0, 40.0),
                            ),
                            egui::Button::new("Background"),
                        )
                        .clicked();
                    if *open {
                        let (command, close) = palette.show(
                            &context,
                            CommandContext {
                                palette_open: true,
                                ..Default::default()
                            },
                            &ShortcutBindings::default(),
                            0.0,
                        );
                        assert!(command.is_none());
                        if close {
                            *open = false;
                            closed = true;
                        }
                    }
                },
            );
            (background_clicked, closed)
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for _ in 0..3 {
            frame(&mut palette, &mut open, vec![]);
        }
        let inside = context
            .read_response("command-palette-query".into())
            .expect("query")
            .rect
            .center();
        frame(
            &mut palette,
            &mut open,
            vec![egui::Event::PointerMoved(inside), button(inside, true)],
        );
        frame(
            &mut palette,
            &mut open,
            vec![egui::Event::PointerMoved(outside)],
        );
        assert_eq!(
            frame(&mut palette, &mut open, vec![button(outside, false)]),
            (false, false),
            "a drag from the query is not a backdrop click"
        );
        frame(
            &mut palette,
            &mut open,
            vec![egui::Event::PointerMoved(outside)],
        );
        assert_eq!(
            frame(&mut palette, &mut open, vec![button(outside, true)]),
            (false, false)
        );
        assert_eq!(
            frame(&mut palette, &mut open, vec![button(outside, false)]),
            (false, true)
        );
        assert!(!open);
        assert_eq!(
            frame(&mut palette, &mut open, vec![]),
            (false, false),
            "no click-through after closing"
        );
        for _ in 0..2 {
            frame(&mut palette, &mut open, vec![]);
        }
        frame(&mut palette, &mut open, vec![button(outside, true)]);
        assert_eq!(
            frame(&mut palette, &mut open, vec![button(outside, false)]),
            (true, false),
            "a fresh click reaches the background once the palette is closed"
        );
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
                if let (Some(command), _) = palette.show(&context, commands, &shortcuts, 0.0) {
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
        assert_eq!(palette.query, ">volume");
        assert_eq!(palette.selected, Some(1));
        assert_eq!(
            run(&mut palette, vec![key(egui::Key::Enter)]),
            vec![CommandId::VolumeUp]
        );
        palette.query = ">no matching command exists".into();
        assert!(run(&mut palette, vec![key(egui::Key::Enter)]).is_empty());
        assert_eq!(palette.selected, None);
        palette.query = ">zoom".into();
        assert_eq!(
            run(&mut palette, vec![key(egui::Key::Enter)]),
            vec![CommandId::ZoomIn]
        );
        palette.query = ">crop".into();
        assert!(run(&mut palette, vec![key(egui::Key::Enter)]).is_empty());
        assert_eq!(palette.selected, None);
        let mut closed = false;
        let _ = context.run_ui(
            egui::RawInput {
                events: vec![key(egui::Key::Escape)],
                ..Default::default()
            },
            |_| {
                closed |= palette.show(&context, commands, &shortcuts, 0.0).1;
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
                    assert_eq!(
                        palette.show(&context, commands, &shortcuts, 0.0),
                        (None, false)
                    );
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
        assert_eq!(palette.query, ">日本語");
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
                    let (chosen, close) = palette.show(&context, commands, &shortcuts, 0.0);
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
        assert_eq!(palette.query, ">open file");
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
        assert_eq!(palette.query, ">にほんご");
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
        assert_eq!(palette.query, ">日本語");
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
        assert_eq!(palette.query, ">");
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
