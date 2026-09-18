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

#[cfg(test)]
mod files_tests;
mod history;
mod preview;

pub struct CommandPalette {
    query: String,
    selected: Option<usize>,
    selected_path: Option<PathBuf>,
    selected_command: Option<CommandId>,
    hidden_paths: std::collections::HashSet<String>,
    removed_selection: Option<usize>,
    folders: bool,
    fresh: bool,
    ime_composing: bool,
    preview: Option<preview::FilePreview>,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self {
            query: ">".into(),
            selected: None,
            selected_path: None,
            selected_command: None,
            hidden_paths: Default::default(),
            removed_selection: None,
            folders: false,
            fresh: true,
            ime_composing: false,
            preview: None,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct OpenSources<'a> {
    pub commands: &'a [CommandId],
    pub files: &'a [PathBuf],
    pub folders: &'a [PathBuf],
    pub folder: Option<&'a towavue_core::FolderSnapshot>,
    pub search: Option<&'a towavue_runtime_windows::FileSearchResult>,
    pub searching: bool,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Choice {
    Command(CommandId),
    RemoveCommand(CommandId),
    Open(RecentAction),
}

impl CommandPalette {
    pub fn outside_press(&self, context: &egui::Context) -> bool {
        !self.fresh
            && !egui::Popup::is_any_open(context)
            && context
                .memory(|memory| memory.area_rect("command-palette"))
                .is_some_and(|rect| {
                    context.input(|input| {
                        input.events.iter().any(|event| matches!(event,
                    egui::Event::PointerButton { pos, pressed: true, .. } if !rect.contains(*pos)
                ))
                    })
                })
    }

    pub fn file_query(&self) -> Option<String> {
        (!self.folders && !self.query.starts_with('>') && !self.query.trim().is_empty())
            .then(|| normalized(self.query.trim()))
    }
    pub fn with_previews(
        cache: towavue_runtime_windows::PreviewCache,
        notify: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        Ok(Self {
            preview: Some(preview::FilePreview::new(cache, notify)?),
            ..Self::default()
        })
    }

    pub fn clear_preview(&mut self) {
        if let Some(preview) = &mut self.preview {
            preview.clear();
        }
    }

    pub fn finish_preview(&mut self, context: &egui::Context) -> bool {
        self.preview
            .as_mut()
            .is_some_and(|preview| preview.finish(context))
    }

    pub fn reset(&mut self) {
        self.clear_preview();
        let preview = self.preview.take();
        *self = Self {
            preview,
            ..Self::default()
        };
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
                Choice::Open(_) | Choice::RemoveCommand(_) => None,
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
                let inactive_stroke = ui.visuals().widgets.inactive.bg_stroke;
                ui.visuals_mut().widgets.inactive.bg_stroke =
                    ui.visuals().widgets.hovered.bg_stroke;
                ui.memory_mut(|memory| {
                    if !memory.has_focus(query_id) {
                        memory.request_focus(query_id);
                    }
                });
                let weak_text_color = ui.visuals().weak_text_color;
                ui.visuals_mut().weak_text_color = Some(crate::chrome::BORDER);
                ui.add_sized(
                    [ui.available_width(), 24.0],
                    egui::TextEdit::singleline(&mut self.query)
                        .vertical_align(egui::Align::Center)
                        .id(query_id)
                        .text_color(crate::chrome::FOREGROUND)
                        .desired_width(f32::INFINITY)
                        .hint_text(if self.folders {
                            "Select to open (hold Ctrl-key to force new window or Alt-key for same window)"
                        } else if previous_query.starts_with('>') {
                            "Type the name of a command to run."
                        } else {
                            "Search files by name (hold Ctrl-key to force new window or Alt-key for same window)"
                        }),
                )
                .help_text("Up / Down: select   Enter: open/run   Ctrl: new window   Alt: same tab   Esc: close");
                ui.visuals_mut().weak_text_color = weak_text_color;
                ui.visuals_mut().widgets.inactive.bg_stroke = inactive_stroke;
                let command_mode = self.query.starts_with('>');
                if command_mode {
                    self.clear_preview();
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
                    self.selected_command = None;
                    self.hidden_paths.clear();
                    self.removed_selection = None;
                }
                if !command_mode {
                    chosen = self.show_files(ui, &sources, (up, down), enter, query_changed)
                        .map(Choice::Open);
                    return;
                }
                chosen = self.show_commands(ui, commands, shortcuts, sources.commands, (up, down, enter.is_some()), query_changed);
            });
        (chosen, close || (!opened && self.outside_press(context)))
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
                let key = normalized(&path.to_string_lossy());
                score.is_some() && !self.hidden_paths.contains(&key) && seen.insert(key)
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
            paths.extend(result.paths.iter().filter(|path| {
                let key = normalized(&path.to_string_lossy());
                !self.hidden_paths.contains(&key) && seen.insert(key)
            }));
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
                    let key = normalized(&item.path.to_string_lossy());
                    (!self.hidden_paths.contains(&key) && seen.insert(key))
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
            .or_else(|| {
                let index = self.removed_selection.take().unwrap_or(0);
                (!paths.is_empty()).then_some(index.min(paths.len().saturating_sub(1)))
            });
        let selected = if up || down {
            next_enabled(selected, &vec![true; paths.len()], down)
        } else {
            selected
        };
        let selection_moved = self.selected != selected;
        self.selected = selected;
        self.selected_path = selected.map(|index| paths[index].clone());
        if self.folders {
            self.clear_preview();
        } else if let Some(preview) = &mut self.preview {
            preview.show(ui, self.selected_path.as_deref());
        } else {
            preview::paint(ui, self.selected_path.as_deref(), None);
        }
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
                    ui.add(
                        egui::Label::new(egui::RichText::new(error).weak())
                            .truncate()
                            .show_tooltip_when_elided(false),
                    )
                    .help_text(error);
                } else if result.matches > result.paths.len() as u64 || result.skipped != 0 {
                    let summary = format!(
                        "Showing {} of {} matches; {} entries skipped",
                        result.paths.len(),
                        result.matches,
                        result.skipped,
                    );
                    ui.add(
                        egui::Label::new(egui::RichText::new(&summary).weak())
                            .truncate()
                            .show_tooltip_when_elided(false),
                    )
                    .help_text(format!("{summary}\nUnreadable entries, links/junctions and folders deeper than 128 levels are skipped. Refine the query to narrow results."));
                }
            } else if sources.searching && !query.is_empty() {
                ui.weak("Searching subfolders...");
            }
        }
        let row_height = 22.0;
        let row_count = paths.len().max(1);
        // The window can constrain the inner height before reaching the screen edge.
        // Use the same effective viewport as ScrollArea when revealing a selected row.
        let height = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 8.0)
            .clamp(1.0, 264.0)
            .min(ui.available_height().max(1.0));
        let scroll_salt = ("quick-open-results", self.folders);
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(scroll_salt)
            .max_height(height);
        if (up || down || query_changed || selection_moved)
            && let Some(index) = selected
        {
            // Match ScrollArea's wrapped salt in egui 0.35. Loading the raw
            // tuple gives a different ID and loses the saved viewport offset.
            // Reveal before virtualization, moving only far enough to expose the row.
            let offset = egui::scroll_area::State::load(
                ui.ctx(),
                ui.make_persistent_id(egui::IdSalt::new(scroll_salt)),
            )
            .map_or(0.0, |state| state.offset.y);
            let top = index as f32 * row_height;
            let bottom = top + row_height;
            scroll = scroll
                .vertical_scroll_offset(offset.clamp((bottom - height).max(0.0).min(top), top));
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        scroll.show_rows_styled(ui, row_height, row_count, |ui, rows| {
            if paths.is_empty() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(if self.folders {
                            "No matching recent folders"
                        } else {
                            "No matching files"
                        })
                        .weak(),
                    )
                    .truncate(),
                );
                return;
            }
            for index in rows {
                let path = paths[index];
                let name = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy();
                let parent = path.parent().unwrap_or(Path::new("")).to_string_lossy();
                let (_, row) = ui.allocate_space(egui::vec2(ui.available_width(), 22.0));
                let selected_row = selected == Some(index);
                let close = selected_row || ui.rect_contains_pointer(row);
                let mut body = row;
                if close {
                    body.max.x -= 22.0;
                }
                let background = ui.painter().add(egui::Shape::Noop);
                let group = if self.folders && index == 0 {
                    "folders"
                } else if !self.folders && index == 0 && recent_count > 0 {
                    "recently opened"
                } else if !self.folders && index == recent_count {
                    "file results"
                } else {
                    ""
                };
                if !self.folders && index == recent_count && recent_count > 0 {
                    ui.painter().hline(
                        row.x_range(),
                        row.top(),
                        egui::Stroke::new(1.0, crate::chrome::BORDER),
                    );
                }
                let (name_width, parent_width, group_width) =
                    file_label_widths(ui, body.width(), &name, group);
                let response = ui
                    .push_id(path, |ui| {
                        crate::chrome::flat_buttons(ui);
                        ui.put(
                            body,
                            egui::Button::selectable(
                                selected_row,
                                (
                                    name.as_ref().atom_max_width(name_width),
                                    egui::RichText::new(parent.as_ref())
                                        .small()
                                        .color(crate::chrome::MUTED)
                                        .atom_max_width(parent_width)
                                        .atom_shrink(true),
                                    egui::Atom::grow(),
                                    egui::RichText::new(group)
                                        .small()
                                        .color(crate::chrome::MUTED)
                                        .atom_max_width(group_width),
                                ),
                            )
                            .truncate()
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .min_size(body.size()),
                        )
                    })
                    .inner
                    .help_text(path.to_string_lossy());
                ui.painter()
                    .set(background, row_background(ui, row, &response, selected_row));
                ui.ctx().accesskit_node_builder(response.id, |node| {
                    node.clear_toggled();
                    node.set_label(path.to_string_lossy().as_ref());
                });
                if close {
                    let close_rect =
                        egui::Rect::from_min_max(egui::pos2(body.right(), row.top()), row.max);
                    let label = if self.folders || index < recent_count {
                        "Remove from Recently Opened"
                    } else {
                        "Dismiss search result"
                    };
                    let remove = ui
                        .push_id(("remove-path", path), |ui| {
                            crate::chrome::tab_close(ui, close_rect, false)
                        })
                        .inner
                        .help_text(label);
                    ui.ctx().accesskit_node_builder(remove.id, |node| {
                        node.set_label(format!("{label}: {}", path.display()));
                    });
                    if remove.clicked() && !query_changed {
                        self.hidden_paths
                            .insert(normalized(&path.to_string_lossy()));
                        if selected_row {
                            self.selected_path = None;
                            self.removed_selection = Some(index);
                        }
                        chosen = Some(RecentAction::Remove((*path).clone(), kind));
                    }
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

fn file_label_widths(ui: &egui::Ui, width: f32, name: &str, group: &str) -> (f32, f32, f32) {
    let available =
        (width - 2.0 * ui.spacing().button_padding.x - 3.0 * ui.spacing().icon_spacing).max(0.0);
    let measure = |text: egui::WidgetText| {
        text.into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        )
        .size()
        .x
    };
    let name = measure(name.into()).min(available * 0.8);
    let remainder = (available - name).max(0.0);
    let group = measure(egui::RichText::new(group).small().into()).min(remainder * 0.6);
    (name, remainder - group, group)
}

fn row_background(
    ui: &egui::Ui,
    row: egui::Rect,
    response: &egui::Response,
    selected: bool,
) -> egui::Shape {
    let hovered = response.enabled() && ui.rect_contains_pointer(row);
    if !selected && !hovered && !response.has_focus() && !response.is_pointer_button_down_on() {
        return egui::Shape::Noop;
    }
    let visuals = if hovered && !selected {
        ui.visuals().widgets.hovered
    } else {
        ui.style().interact_selectable(response, selected)
    };
    egui::epaint::RectShape::new(
        row,
        visuals.corner_radius,
        visuals.weak_bg_fill,
        egui::Stroke::NONE,
        egui::StrokeKind::Inside,
    )
    .into()
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

    #[test]
    fn palette_centers_text_placeholder_and_white_caret() {
        for density in [1.0, 1.25, 2.0] {
            for query in ["", ">open", "Abc12あ"] {
                let context = crate::fonts::test_context();
                context.enable_accesskit();
                context.set_pixels_per_point(density);
                context.global_style_mut(|style| {
                    crate::chrome::style(style);
                    style.animation_time = 0.0;
                    style.visuals.text_cursor.blink = false;
                });
                let mut palette = CommandPalette {
                    query: query.into(),
                    ..Default::default()
                };
                let mut frame = || {
                    context.run_ui(egui::RawInput::default(), |_| {
                        palette.show(
                            &context,
                            CommandContext {
                                palette_open: true,
                                ..Default::default()
                            },
                            &ShortcutBindings::default(),
                            0.0,
                        );
                    })
                };
                for _ in 0..3 {
                    frame();
                }
                let output = frame();
                let bounds = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree")
                    .nodes
                    .iter()
                    .find(|(_, node)| {
                        matches!(node.label(), Some("Search commands" | "Search files"))
                    })
                    .expect("search field")
                    .1
                    .bounds()
                    .expect("bounds");
                let rect = egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                );
                crate::resize::tests::assert_centered_input(
                    &output,
                    rect,
                    if query.is_empty() {
                        "Search files by name (hold Ctrl-key to force new window or Alt-key for same window)"
                    } else {
                        query
                    },
                    density,
                );
                if query.is_empty() {
                    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                        egui::Shape::Text(text) if text.galley.text().starts_with("Search files by name")
                            && text.galley.job.sections.iter().all(|section|
                                section.format.color == crate::chrome::BORDER))));
                }
            }
        }
    }

    pub(super) fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
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
        open_frame_at(
            context,
            palette,
            sources,
            events,
            (egui::vec2(600.0, 400.0), 0.0, 1.0),
        )
    }

    pub(super) fn open_frame_at(
        context: &egui::Context,
        palette: &mut CommandPalette,
        sources: OpenSources<'_>,
        events: Vec<egui::Event>,
        layout: (egui::Vec2, f32, f32),
    ) -> (egui::FullOutput, Vec<Choice>) {
        context.set_pixels_per_point(layout.2);
        let mut choices = Vec::new();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, layout.0)),
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
                    layout.1,
                    sources,
                );
                choices.extend(choice);
            },
        );

        (output, choices)
    }

    #[test]
    fn thousand_results_virtualize_reveal_wrapped_selection_and_fit_compact_status() {
        use towavue_runtime_windows::{FileSearchRequest, FileSearchResult};
        for (size, scale) in [egui::vec2(600.0, 400.0), egui::vec2(240.0, 180.0)]
            .into_iter()
            .flat_map(|size| [1.0, 1.25, 2.0].map(move |scale| (size, scale)))
        {
            let context = crate::fonts::test_context();
            context.enable_accesskit();
            context.global_style_mut(|style| {
                crate::chrome::style(style);
                style.animation_time = 0.0;
            });
            let mut palette = CommandPalette::default();
            palette.open_files(false);
            palette.query = "image".into();
            let mut result = FileSearchResult {
                request: FileSearchRequest {
                    root: "C:/media".into(),
                    query: "image".into(),
                },
                paths: (0..1000)
                    .map(|index| {
                        PathBuf::from(format!(
                            "C:/media/long parent description/image-{index:04}.png"
                        ))
                    })
                    .collect(),
                matches: 2000,
                skipped: 7,
                error: None,
            };
            let recent = result.paths[..5].to_vec();
            fn sources<'a>(recent: &'a [PathBuf], result: &'a FileSearchResult) -> OpenSources<'a> {
                OpenSources {
                    files: recent,
                    search: Some(result),
                    ..Default::default()
                }
            }
            let layout = (size, 30.0, scale);
            for _ in 0..4 {
                open_frame_at(
                    &context,
                    &mut palette,
                    sources(&recent, &result),
                    vec![],
                    layout,
                );
            }
            let verify = |output: &egui::FullOutput, selected: &Path| {
                let panel = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.fill == crate::chrome::FLOATING_BACKGROUND
                                && rect.stroke.color == crate::chrome::BORDER =>
                        {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .expect("palette panel");
                assert!(
                    panel.left() >= 0.0 && panel.right() <= size.x && panel.bottom() <= size.y,
                    "bounded panel at {size:?}/{scale}: {panel:?}"
                );
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree");
                let rows: Vec<_> = tree
                    .nodes
                    .iter()
                    .filter(|(_, node)| {
                        node.role() == egui::accesskit::Role::Button
                            && node
                                .label()
                                .is_some_and(|label| label.starts_with("C:/media/"))
                    })
                    .collect();
                assert!(
                    rows.len() <= 16,
                    "only visible rows plus overscan: {}",
                    rows.len()
                );
                let (_, node) = rows
                    .into_iter()
                    .find(|(_, node)| node.label() == Some(selected.to_string_lossy().as_ref()))
                    .expect("selected virtual row is mounted");
                let bounds = node.bounds().expect("selected row bounds");
                let center = egui::pos2(
                    ((bounds.x0 + bounds.x1) / 2.0) as f32,
                    ((bounds.y0 + bounds.y1) / 2.0) as f32,
                );
                assert!(
                    panel.contains(center),
                    "selected row is visible: {center:?} in {panel:?}"
                );
            };
            verify(
                &open_frame_at(
                    &context,
                    &mut palette,
                    sources(&recent, &result),
                    vec![],
                    layout,
                )
                .0,
                &result.paths[0],
            );
            let pointer = egui::pos2(size.x * 0.5, 120.0);
            open_frame_at(
                &context,
                &mut palette,
                sources(&recent, &result),
                vec![egui::Event::PointerMoved(pointer)],
                layout,
            );
            open_frame_at(
                &context,
                &mut palette,
                sources(&recent, &result),
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -400.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                }],
                layout,
            );
            for _ in 0..40 {
                open_frame_at(
                    &context,
                    &mut palette,
                    sources(&recent, &result),
                    vec![],
                    layout,
                );
            }
            let scrolled = open_frame_at(
                &context,
                &mut palette,
                sources(&recent, &result),
                vec![],
                layout,
            )
            .0;
            assert_eq!(
                palette.selected_path.as_ref(),
                Some(&result.paths[0]),
                "manual scrolling does not change keyboard selection"
            );
            assert!(
                !scrolled
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("scrolled tree")
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label()
                        == Some(result.paths[0].to_string_lossy().as_ref())),
                "idle frames must not recenter a manually scrolled list"
            );
            let (clicked_path, point) = scrolled
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("tree")
                .nodes
                .iter()
                .find_map(|(_, node)| {
                    if node.role() != egui::accesskit::Role::Button {
                        return None;
                    }
                    let label = node
                        .label()
                        .filter(|label| label.starts_with("C:/media/"))?;
                    let rect = node.bounds()?;
                    let point = egui::pos2(
                        (rect.x0 + rect.x1) as f32 * 0.5,
                        (rect.y0 + rect.y1) as f32 * 0.5,
                    );
                    (point.y > 100.0 && point.y < size.y - 12.0)
                        .then(|| (PathBuf::from(label), point))
                })
                .expect("visible scrolled row");
            let button = |pressed| egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::ALT,
            };
            open_frame_at(
                &context,
                &mut palette,
                sources(&recent, &result),
                vec![egui::Event::PointerMoved(point), button(true)],
                layout,
            );
            assert_eq!(
                open_frame_at(
                    &context,
                    &mut palette,
                    sources(&recent, &result),
                    vec![button(false)],
                    layout
                )
                .1,
                [Choice::Open(RecentAction::Open(
                    clicked_path,
                    RecentKind::File,
                    OpenTarget::Replace
                ))],
                "scrolled pointer at {size:?}/{scale}: {point:?}"
            );
            open_frame_at(
                &context,
                &mut palette,
                sources(&recent, &result),
                vec![key(egui::Key::ArrowUp, egui::Modifiers::NONE)],
                layout,
            );
            verify(
                &open_frame_at(
                    &context,
                    &mut palette,
                    sources(&recent, &result),
                    vec![],
                    layout,
                )
                .0,
                &result.paths[999],
            );
            assert_eq!(
                open_frame_at(
                    &context,
                    &mut palette,
                    sources(&recent, &result),
                    vec![key(egui::Key::Enter, egui::Modifiers::CTRL)],
                    layout
                )
                .1,
                [Choice::Open(RecentAction::Open(
                    result.paths[999].clone(),
                    RecentKind::File,
                    OpenTarget::Window
                ))]
            );
            for index in 0..8 {
                open_frame_at(
                    &context,
                    &mut palette,
                    sources(&recent, &result),
                    vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
                    layout,
                );
                verify(
                    &open_frame_at(
                        &context,
                        &mut palette,
                        sources(&recent, &result),
                        vec![],
                        layout,
                    )
                    .0,
                    &result.paths[index],
                );
            }
            // An asynchronous reorder keeps the selected path and reveals its new row.
            let selected = result.paths[7].clone();
            result.paths.swap(7, 995);
            verify(
                &open_frame_at(
                    &context,
                    &mut palette,
                    sources(&recent, &result),
                    vec![],
                    layout,
                )
                .0,
                &selected,
            );
            result.error =
                Some("Cannot search this folder: ".to_owned() + &"long diagnostic ".repeat(40));
            for _ in 0..3 {
                open_frame_at(
                    &context,
                    &mut palette,
                    sources(&recent, &result),
                    vec![],
                    layout,
                );
            }
            verify(
                &open_frame_at(
                    &context,
                    &mut palette,
                    sources(&recent, &result),
                    vec![],
                    layout,
                )
                .0,
                &selected,
            );
        }
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
                    let context = crate::fonts::test_context();
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
        let context = crate::fonts::test_context();
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
        let context = crate::fonts::test_context();
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
        let context = crate::fonts::test_context();
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
            let context = crate::fonts::test_context();
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
    fn command_history_groups_keep_identity_and_remove_without_running() {
        for size in [egui::vec2(600.0, 400.0), egui::vec2(240.0, 180.0)] {
            for density in [1.0, 1.25, 2.0] {
                let context = crate::fonts::test_context();
                context.enable_accesskit();
                context.global_style_mut(|style| {
                    style.animation_time = 0.0;
                    style.scroll_animation = egui::style::ScrollAnimation::none();
                    style.interaction.tooltip_delay = 60.0;
                });
                let mut palette = CommandPalette::default();
                let mut history = vec![
                    CommandId::OpenFolder,
                    CommandId::OpenFile,
                    CommandId::TogglePause,
                ];
                let layout = (size, 0.0, density);
                let frame = |palette: &mut CommandPalette, history: &[CommandId], events| {
                    open_frame_at(
                        &context,
                        palette,
                        OpenSources {
                            commands: history,
                            ..Default::default()
                        },
                        events,
                        layout,
                    )
                };
                let mut output = egui::FullOutput::default();
                for _ in 0..5 {
                    output = frame(&mut palette, &history, vec![]).0;
                }
                assert_eq!(palette.selected_command, Some(CommandId::OpenFolder));
                assert!(picker_text(&output, "recently used").is_some());
                assert!(picker_text(&output, "other commands").is_some());
                let folder_y = picker_text(&output, "Open folder")
                    .expect("first MRU")
                    .0
                    .pos
                    .y;
                let file_y = picker_text(&output, "Open file")
                    .expect("second MRU")
                    .0
                    .pos
                    .y;
                assert!((file_y - folder_y - 22.0).abs() < 1.0);
                frame(
                    &mut palette,
                    &history,
                    vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
                );
                assert_eq!(palette.selected_command, Some(CommandId::OpenFile));
                history.swap(0, 1);
                frame(&mut palette, &history, vec![]);
                assert_eq!(
                    palette.selected_command,
                    Some(CommandId::OpenFile),
                    "asynchronous reorder retains command identity"
                );
                assert_eq!(
                    frame(
                        &mut palette,
                        &history,
                        vec![key(egui::Key::Enter, egui::Modifiers::NONE)]
                    )
                    .1,
                    [Choice::Command(CommandId::OpenFile)]
                );
                output = frame(&mut palette, &history, vec![]).0;
                let label = "Remove Open file from Recently Used";
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree");
                let bounds = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .expect("selected removal button")
                    .1
                    .bounds()
                    .expect("button bounds");
                let position = egui::pos2(
                    ((bounds.x0 + bounds.x1) / 2.0) as f32,
                    ((bounds.y0 + bounds.y1) / 2.0) as f32,
                );
                frame(
                    &mut palette,
                    &history,
                    vec![egui::Event::PointerMoved(position)],
                );
                let button = |pressed| egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                assert!(
                    frame(&mut palette, &history, vec![button(true)])
                        .1
                        .is_empty()
                );
                assert_eq!(
                    frame(&mut palette, &history, vec![button(false)]).1,
                    [Choice::RemoveCommand(CommandId::OpenFile)]
                );
                history.retain(|command| *command != CommandId::OpenFile);
                for _ in 0..3 {
                    output = frame(&mut palette, &history, vec![egui::Event::PointerGone]).0;
                }
                assert_eq!(palette.selected_command, Some(CommandId::OpenFolder));
                assert!(
                    picker_text(&output, "Open file").is_some(),
                    "removing MRU keeps the ordinary command"
                );
                let tree = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("tree");
                assert!(
                    !tree
                        .nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some(label))
                );
            }
        }
    }

    pub(super) fn picker_text<'a>(
        output: &'a egui::FullOutput,
        label: &str,
    ) -> Option<(&'a egui::epaint::TextShape, egui::Rect)> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some((text, shape.clip_rect))
            }
            _ => None,
        })
    }

    #[test]
    fn picker_keyboard_reveals_only_at_edges_and_wraps_virtual_rows() {
        for size in [egui::vec2(600.0, 400.0), egui::vec2(240.0, 180.0)] {
            for density in [1.0, 1.25, 2.0] {
                for mode in 0..3 {
                    let context = crate::fonts::test_context();
                    context.enable_accesskit();
                    context.global_style_mut(|style| {
                        crate::chrome::style(style);
                        style.animation_time = 0.0;
                        style.scroll_animation = egui::style::ScrollAnimation::none();
                        style.interaction.tooltip_delay = 60.0;
                    });
                    let paths: Vec<_> = (0..80)
                        .map(|index| PathBuf::from(format!("C:/media/item-{index:03}.png")))
                        .collect();
                    let sources = OpenSources {
                        files: &paths,
                        folders: &paths,
                        ..Default::default()
                    };
                    let mut palette = CommandPalette::default();
                    if mode != 0 {
                        palette.open_files(mode == 2);
                    }
                    let layout = (size, 30.0, density);
                    let render = |palette: &mut CommandPalette, events| {
                        open_frame_at(&context, palette, sources, events, layout).0
                    };
                    let mut output = render(&mut palette, vec![]);
                    for _ in 0..4 {
                        output = render(&mut palette, vec![]);
                    }
                    let definitions = command_definitions();
                    let title = |palette: &CommandPalette| -> String {
                        if mode == 0 {
                            definitions[palette.selected.expect("selection")]
                                .title
                                .into()
                        } else {
                            palette
                                .selected_path
                                .as_ref()
                                .expect("path")
                                .file_name()
                                .expect("fixture filename")
                                .to_string_lossy()
                                .into_owned()
                        }
                    };
                    let mut unchanged_visible_steps = 0;
                    let mut edge_steps = 0;
                    for _ in 0..24 {
                        let old_title = title(&palette);
                        let (old, clip) = picker_text(&output, &old_title).expect("selected text");
                        let old_position = old.pos;
                        let old_index = palette.selected.expect("selected row");
                        output = render(
                            &mut palette,
                            vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
                        );
                        for _ in 0..3 {
                            output = render(&mut palette, vec![]);
                        }
                        let index = palette.selected.expect("selected row");
                        let selected_title = title(&palette);
                        let (selected, selected_clip) =
                            picker_text(&output, &selected_title).expect("revealed selection");
                        let bottom = selected.pos.y + selected.galley.size().y;
                        assert!(
                            selected.pos.y >= selected_clip.top() - 1.0
                                && bottom <= selected_clip.bottom() + 1.0,
                            "selection outside viewport: {mode} {size:?} {density} {selected_title} {bottom} {selected_clip:?}"
                        );
                        if index > old_index {
                            let expected = old_position.y + (index - old_index) as f32 * 22.0;
                            if expected + selected.galley.size().y + 4.0 <= clip.bottom() {
                                assert!(
                                    (selected.pos.y - expected - selected_clip.top() + clip.top())
                                        .abs()
                                        <= 1.1,
                                    "visible rows must not recenter: {mode} {size:?} {density} {old_index}->{index} expected={expected} actual={} old_clip={clip:?} new_clip={selected_clip:?}",
                                    selected.pos.y
                                );
                                unchanged_visible_steps += 1;
                            } else {
                                edge_steps += 1;
                            }
                        }
                    }
                    assert!(unchanged_visible_steps > 0 && edge_steps > 0);
                    let mut reverse_visible = 0;
                    let mut reverse_edges = 0;
                    for _ in 0..24 {
                        let old_title = title(&palette);
                        let (old, clip) = picker_text(&output, &old_title).expect("selected text");
                        let old_y = old.pos.y;
                        let old_index = palette.selected.expect("selected row");
                        output = render(
                            &mut palette,
                            vec![key(egui::Key::ArrowUp, egui::Modifiers::NONE)],
                        );
                        for _ in 0..3 {
                            output = render(&mut palette, vec![]);
                        }
                        let index = palette.selected.expect("selected row");
                        let (selected, selected_clip) =
                            picker_text(&output, &title(&palette)).expect("revealed selection");
                        if index < old_index {
                            let expected = old_y - (old_index - index) as f32 * 22.0;
                            if expected - 4.0 >= clip.top() {
                                assert!(
                                    (selected.pos.y - expected - selected_clip.top() + clip.top())
                                        .abs()
                                        <= 1.1,
                                    "reverse inside viewport must not scroll: mode={mode}, {size:?}, {density}, {old_index}->{index}, expected={expected}, actual={}",
                                    selected.pos.y
                                );
                                reverse_visible += 1;
                            } else {
                                reverse_edges += 1;
                            }
                        }
                    }
                    assert!(reverse_visible > 0 && reverse_edges > 0);
                    // Reopen and wrap directly from the first to last enabled row, then back.
                    if mode == 0 {
                        palette.reset();
                    } else {
                        palette.open_files(mode == 2);
                    }
                    for _ in 0..4 {
                        render(&mut palette, vec![]);
                    }
                    let first = title(&palette);
                    render(
                        &mut palette,
                        vec![key(egui::Key::ArrowUp, egui::Modifiers::NONE)],
                    );
                    for _ in 0..3 {
                        output = render(&mut palette, vec![]);
                    }
                    let last = title(&palette);
                    assert_ne!(last, first);
                    let (text, clip) = picker_text(&output, &last).expect("last row rendered");
                    assert!(
                        text.pos.y >= clip.top() - 1.0
                            && text.pos.y + text.galley.size().y <= clip.bottom() + 1.0
                    );
                    render(
                        &mut palette,
                        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
                    );
                    for _ in 0..3 {
                        output = render(&mut palette, vec![]);
                    }
                    assert_eq!(title(&palette), first);
                    let (text, clip) = picker_text(&output, &first).expect("first row rendered");
                    assert!(
                        text.pos.y >= clip.top() - 1.0
                            && text.pos.y + text.galley.size().y <= clip.bottom() + 1.0
                    );
                    if mode != 0 {
                        let tree = output
                            .platform_output
                            .accesskit_update
                            .as_ref()
                            .expect("accessibility tree");
                        let visible_paths = tree
                            .nodes
                            .iter()
                            .filter(|(_, node)| {
                                node.label()
                                    .is_some_and(|label| label.starts_with("C:/media/"))
                            })
                            .count();
                        assert!(visible_paths < 20, "retain bounded virtual rows");
                    }
                    let before = picker_text(&output, &first).expect("first row").0.pos.y;
                    let point = picker_text(&output, &first).expect("first row").1.center();
                    render(
                        &mut palette,
                        vec![
                            egui::Event::PointerMoved(point),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                delta: egui::vec2(0.0, -88.0),
                                phase: egui::TouchPhase::Move,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                    for _ in 0..40 {
                        output = render(&mut palette, vec![]);
                    }
                    assert!(
                        picker_text(&output, &first)
                            .is_none_or(|(text, _)| text.pos.y < before - 20.0),
                        "manual wheel scrolls independently of selection"
                    );
                    let visible_text = |output: &egui::FullOutput| -> Vec<(String, egui::Pos2)> {
                        output
                            .shapes
                            .iter()
                            .filter_map(|shape| match &shape.shape {
                                egui::Shape::Text(text)
                                    if shape.clip_rect.intersects(egui::Rect::from_min_size(
                                        text.pos,
                                        text.galley.size(),
                                    )) =>
                                {
                                    Some((text.galley.job.text.clone(), text.pos))
                                }
                                _ => None,
                            })
                            .collect()
                    };
                    let settled = visible_text(&output);
                    for _ in 0..5 {
                        output = render(&mut palette, vec![]);
                    }
                    assert_eq!(
                        visible_text(&output),
                        settled,
                        "idle frames retain manual scroll"
                    );
                }
            }
        }
    }

    #[test]
    fn picker_paths_follow_names_with_smaller_muted_text_and_bounded_rows() {
        for folders in [false, true] {
            for size in [
                egui::vec2(600.0, 400.0),
                egui::vec2(240.0, 180.0),
                egui::vec2(180.0, 110.0),
            ] {
                for density in [1.0, 1.25, 2.0] {
                    let context = crate::fonts::test_context();
                    context.global_style_mut(crate::chrome::style);
                    let mut palette = CommandPalette::default();
                    palette.open_files(folders);
                    let paths = [
                        PathBuf::from("C:/media/short.png"),
                        PathBuf::from(
                            "C:/a much longer parent folder/a very long file name that must truncate.png",
                        ),
                    ];
                    let sources = OpenSources {
                        files: &paths,
                        folders: &paths,
                        ..Default::default()
                    };
                    let mut output = egui::FullOutput::default();
                    for _ in 0..5 {
                        output = open_frame_at(
                            &context,
                            &mut palette,
                            sources,
                            vec![],
                            (size, 0.0, density),
                        )
                        .0;
                    }
                    for path in &paths {
                        let (name, clip) = picker_text(
                            &output,
                            &path
                                .file_name()
                                .expect("fixture filename")
                                .to_string_lossy(),
                        )
                        .expect("name");
                        let (parent, _) = picker_text(
                            &output,
                            &path.parent().expect("fixture parent").to_string_lossy(),
                        )
                        .expect("parent");
                        let gap = parent.pos.x - (name.pos.x + name.galley.size().x);
                        assert!(
                            (0.0..=12.0).contains(&gap),
                            "adjacent description: {gap} at {size:?}"
                        );
                        assert!(
                            parent.galley.job.sections[0].format.font_id.size
                                < name.galley.job.sections[0].format.font_id.size
                        );
                        assert_eq!(
                            parent.galley.job.sections[0].format.color,
                            crate::chrome::MUTED
                        );
                        assert!(
                            name.pos.x >= clip.left() - 1.0
                                && parent.pos.x + parent.galley.size().x <= clip.right() + 1.0
                        );
                        assert!(
                            (name.pos.y + name.galley.size().y / 2.0
                                - parent.pos.y
                                - parent.galley.size().y / 2.0)
                                .abs()
                                < 1.0
                        );
                    }
                }
            }
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
            let context = crate::fonts::test_context();
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
            let group = text("other commands");
            assert!(first.pos.x + first.galley.size().x <= group.pos.x);
            assert!(group.pos.x + group.galley.size().x < panel.right());
            assert!(first.pos.x + first.galley.size().x <= second.pos.x + second.galley.size().x);
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
    fn outside_press_closes_palette_and_hands_off_to_the_background() {
        let context = crate::fonts::test_context();
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
            (false, true)
        );
        assert_eq!(
            frame(&mut palette, &mut open, vec![button(outside, false)]),
            (true, false)
        );
        assert!(!open);
        assert_eq!(
            frame(&mut palette, &mut open, vec![]),
            (false, false),
            "handoff is not replayed on an idle frame"
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
        let context = crate::fonts::test_context();
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
        let context = crate::fonts::test_context();
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
        let context = crate::fonts::test_context();
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
