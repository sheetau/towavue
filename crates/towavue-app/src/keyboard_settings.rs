use crate::*;
use towavue_core::{CommandDefinition, KeySequence};

#[cfg(test)]
mod tests;
mod view;

#[derive(Clone, Default)]
pub(super) struct KeyboardSettings {
    pub(super) query: String,
    precedence: bool,
    record_search: bool,
    pub edit: Option<Edit>,
    message: Option<String>,
    recorded: Vec<KeyStroke>,
    last_capture_frame: Option<u64>,
    focus_search: bool,
    search_id: Option<egui::Id>,
}

#[derive(Clone)]
pub(super) struct Edit {
    command: CommandId,
    slot: Option<usize>,
    expected: Vec<KeySequence>,
    text: String,
    submit: bool,
}

#[derive(Clone, PartialEq)]
pub(super) struct Change {
    pub command: CommandId,
    pub expected: Vec<KeySequence>,
    pub replacement: Vec<KeySequence>,
}

struct Row {
    command: &'static CommandDefinition,
    precedence: usize,
    slot: Option<usize>,
    keys: String,
    when: String,
}

impl KeyboardSettings {
    pub fn request_search_focus(&mut self) {
        self.focus_search = true;
        self.cancel_capture();
    }

    pub fn capturing(&self) -> bool {
        self.record_search || self.edit.is_some()
    }

    pub fn cancel_capture(&mut self) {
        self.record_search = false;
        self.recorded.clear();
        self.edit = None;
    }

    fn search_focused(&self, context: &egui::Context) -> bool {
        self.edit.is_none()
            && self
                .search_id
                .is_some_and(|id| context.memory(|memory| memory.has_focus(id)))
    }

    fn search_control(stroke: &KeyStroke) -> bool {
        (stroke.modifiers
            == Modifiers {
                alt: true,
                ..Default::default()
            }
            && matches!(stroke.key, Key::Character('k' | 'p')))
            || (stroke.modifiers == Modifiers::default() && stroke.key == Key::Escape)
    }

    pub(super) fn apply_search_control(&mut self, stroke: &KeyStroke) {
        match stroke.key {
            Key::Character('k') => {
                self.record_search = !self.record_search;
                self.recorded.clear();
            }
            Key::Character('p') => self.precedence = !self.precedence,
            Key::Escape => {
                self.query.clear();
                self.cancel_capture();
                self.focus_search = true;
            }
            _ => unreachable!("validated search control"),
        }
    }

    pub fn capture(&mut self, stroke: KeyStroke) {
        if stroke.key == Key::Enter
            && stroke.modifiers == Modifiers::default()
            && let Some(edit) = self.edit.as_mut()
        {
            edit.submit = true;
            return;
        }
        if stroke.key == Key::Escape {
            self.cancel_capture();
            self.edit = None;
            return;
        }
        if self.recorded.len() == 4 {
            self.recorded.clear();
        }
        self.recorded.push(stroke);
        let value = KeySequence::new(self.recorded.clone())
            .expect("recorded key")
            .to_string();
        if let Some(edit) = self.edit.as_mut() {
            edit.text = value;
        } else if self.record_search {
            self.query = format!("\"{value}\"");
        }
    }

    fn begin_edit(&mut self, command: CommandId, slot: Option<usize>, bindings: &ShortcutBindings) {
        self.cancel_capture();
        self.message = None;
        self.edit = Some(Edit {
            command,
            slot,
            expected: bindings.all(command).to_vec(),
            text: slot
                .and_then(|index| bindings.all(command).get(index))
                .map(ToString::to_string)
                .unwrap_or_default(),
            submit: false,
        });
    }

    fn rows(&self, bindings: &ShortcutBindings) -> Vec<Row> {
        let query = self.query.trim().to_lowercase();
        let exact = query
            .strip_prefix('"')
            .and_then(|query| query.strip_suffix('"'));
        let mut rows = Vec::new();
        for (precedence, command) in command_definitions().iter().enumerate() {
            let when = when_label(command);
            for index in 0..bindings.all(command.id).len().max(1) {
                let bound = bindings.all(command.id).get(index);
                let keys = bound.map(ToString::to_string).unwrap_or_default();
                let matches = if let Some(exact) = exact {
                    keys.to_lowercase() == exact
                } else {
                    let haystack = format!(
                        "{} {} {} {}",
                        command.title,
                        command.id.as_str(),
                        keys,
                        when
                    )
                    .to_lowercase();
                    query.split_whitespace().all(|word| haystack.contains(word))
                };
                if matches {
                    rows.push(Row {
                        command,
                        precedence,
                        slot: bound.map(|_| index),
                        keys,
                        when: when.clone(),
                    });
                }
            }
        }
        if self.precedence {
            rows.sort_by_key(|row| {
                (
                    row.slot.is_none(),
                    row.slot.is_some_and(|slot| slot > 0),
                    row.precedence,
                    row.slot,
                )
            });
        } else {
            rows.sort_by_key(|row| (row.command.title.to_lowercase(), row.slot));
        }
        rows
    }
}

fn when_label(command: &CommandDefinition) -> String {
    if command.media_kinds.is_empty() {
        return "Always".into();
    }
    let mut terms = Vec::new();
    for kind in command.media_kinds {
        let name = match kind {
            MediaKind::Image => "image",
            MediaKind::Video => "video",
            MediaKind::Audio => "audio",
        };
        let enabled = |reading_mode, timeline_open| {
            command.is_enabled(CommandContext {
                media_kind: Some(*kind),
                reading_mode,
                timeline_open,
                has_time_selection: true,
                has_video_frame: true,
                ..Default::default()
            })
        };
        let base = if *kind == MediaKind::Image {
            match (enabled(false, false), enabled(true, false)) {
                (true, true) => name.to_owned(),
                (true, false) => format!("{name} && !reading"),
                (false, true) => format!("{name} && reading"),
                _ => continue,
            }
        } else if enabled(false, false) {
            name.to_owned()
        } else if enabled(false, true) {
            format!("{name} && timeline")
        } else {
            continue;
        };
        terms.push(base);
    }
    let mut label = terms.join(" || ");
    if matches!(
        command.id,
        CommandId::DeleteTimeSelection
            | CommandId::KeepTimeSelection
            | CommandId::PlayTimeSelection
    ) {
        label.push_str("; time selection");
    }
    if command.id == CommandId::ExportFrame {
        label.push_str("; frame available");
    }
    if command.id == CommandId::ToggleReadingMode {
        label.push_str("; no unsaved edits");
    }
    label
}

fn egui_stroke(key: egui::Key, modifiers: egui::Modifiers) -> Option<KeyStroke> {
    let name = match key {
        egui::Key::ArrowLeft => "Left",
        egui::Key::ArrowRight => "Right",
        egui::Key::ArrowUp => "Up",
        egui::Key::ArrowDown => "Down",
        _ => key.name(),
    };
    let mut stroke: KeyStroke = name.parse().ok()?;
    stroke.modifiers = Modifiers {
        control: modifiers.ctrl,
        shift: modifiers.shift,
        alt: modifiers.alt,
        logo: modifiers.mac_cmd,
    };
    Some(stroke)
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn keyboard_settings_active(&self) -> bool {
        self.tabs.keyboard_settings().is_some()
            && self.tabs.active_id() == self.tabs.keyboard_settings()
    }

    pub(super) fn keyboard_capture_active(&self) -> bool {
        self.keyboard_settings_active()
            && self.keyboard_settings.capturing()
            && !self.palette_open
            && !self.grid_open
            && (!self.modal_input_blocked() || self.keyboard_settings.edit.is_some())
            && !self
                .ui_context
                .as_ref()
                .is_some_and(egui::Popup::is_any_open)
    }

    pub(super) fn keyboard_search_owns_shortcut(&self, stroke: &KeyStroke) -> bool {
        self.keyboard_settings_active()
            && !self.native_ime_composing
            && !self.palette_open
            && !self.grid_open
            && !self.modal_input_blocked()
            && KeyboardSettings::search_control(stroke)
            && self.ui_context.as_ref().is_some_and(|context| {
                !egui::Popup::is_any_open(context) && self.keyboard_settings.search_focused(context)
            })
    }

    pub(super) fn apply_keybinding_change(&mut self, change: Change) {
        if !self.keyboard_settings_active() {
            return;
        }
        match shortcuts::save_command(
            &self.shortcut_path,
            change.command,
            &change.expected,
            &change.replacement,
        ) {
            Ok(bindings) => {
                self.shortcuts = bindings.clone();
                self.cancel_shortcut_prefix();
                self.keyboard_settings.edit = None;
                self.keyboard_settings.cancel_capture();
                self.keyboard_settings.message = Some("Keyboard shortcuts saved".into());
                (self.notify)(AppEvent::ShortcutsChanged(bindings));
            }
            Err(error) => self.keyboard_settings.message = Some(error),
        }
        self.request_redraw();
    }
}
