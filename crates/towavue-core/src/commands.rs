use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use crate::MediaKind;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CommandId {
    OpenFile,
    OpenFolder,
    CloseTab,
    NextTab,
    PreviousTab,
    TogglePause,
    SeekBackward,
    SeekForward,
    PreviousMedia,
    NextMedia,
    PreviousSameKind,
    NextSameKind,
    ToggleFilmstrip,
    ToggleCommandPalette,
    ReloadShortcuts,
    ZoomIn,
    ZoomOut,
    ActualSize,
    FitToWindow,
    ClearSelection,
    ToggleCropPreview,
    ToggleReadingMode,
    IncreaseReadingPages,
    DecreaseReadingPages,
    ToggleReadingAxis,
    ReverseReadingOrder,
    Undo,
    Redo,
    ApplyCrop,
    RotateClockwise,
    RotateCounterclockwise,
    FlipHorizontal,
    FlipVertical,
    SetTrimStart,
    SetTrimEnd,
    VolumeDown,
    VolumeUp,
    ToggleMute,
    RateDown,
    RateUp,
    ResetRate,
    Save,
    ExportAs,
    ToggleTimeline,
    ToggleGridMenu,
    ToggleHardwareEncode,
    ToggleFullscreen,
}

impl CommandId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenFile => "open_file",
            Self::OpenFolder => "open_folder",
            Self::CloseTab => "close_tab",
            Self::NextTab => "next_tab",
            Self::PreviousTab => "previous_tab",
            Self::TogglePause => "toggle_pause",
            Self::SeekBackward => "seek_backward",
            Self::SeekForward => "seek_forward",
            Self::PreviousMedia => "previous_media",
            Self::NextMedia => "next_media",
            Self::PreviousSameKind => "previous_same_kind",
            Self::NextSameKind => "next_same_kind",
            Self::ToggleFilmstrip => "toggle_filmstrip",
            Self::ToggleCommandPalette => "toggle_command_palette",
            Self::ReloadShortcuts => "reload_shortcuts",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            Self::ActualSize => "actual_size",
            Self::FitToWindow => "fit_to_window",
            Self::ClearSelection => "clear_selection",
            Self::ToggleCropPreview => "toggle_crop_preview",
            Self::ToggleReadingMode => "toggle_reading_mode",
            Self::IncreaseReadingPages => "increase_reading_pages",
            Self::DecreaseReadingPages => "decrease_reading_pages",
            Self::ToggleReadingAxis => "toggle_reading_axis",
            Self::ReverseReadingOrder => "reverse_reading_order",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::ApplyCrop => "apply_crop",
            Self::RotateClockwise => "rotate_clockwise",
            Self::RotateCounterclockwise => "rotate_counterclockwise",
            Self::FlipHorizontal => "flip_horizontal",
            Self::FlipVertical => "flip_vertical",
            Self::SetTrimStart => "set_trim_start",
            Self::SetTrimEnd => "set_trim_end",
            Self::VolumeDown => "volume_down",
            Self::VolumeUp => "volume_up",
            Self::ToggleMute => "toggle_mute",
            Self::RateDown => "rate_down",
            Self::RateUp => "rate_up",
            Self::ResetRate => "reset_rate",
            Self::Save => "save",
            Self::ExportAs => "export_as",
            Self::ToggleTimeline => "toggle_timeline",
            Self::ToggleGridMenu => "toggle_grid_menu",
            Self::ToggleHardwareEncode => "toggle_hardware_encode",
            Self::ToggleFullscreen => "toggle_fullscreen",
        }
    }
}

impl FromStr for CommandId {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        command_definitions()
            .iter()
            .find_map(|definition| (definition.id.as_str() == value).then_some(definition.id))
            .ok_or(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Key {
    Character(char),
    Space,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Tab,
    Escape,
    F11,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KeyStroke {
    pub modifiers: Modifiers,
    pub key: Key,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KeySequence(Vec<KeyStroke>);

impl KeySequence {
    pub fn new(strokes: impl IntoIterator<Item = KeyStroke>) -> Option<Self> {
        let strokes = strokes.into_iter().collect::<Vec<_>>();
        (!strokes.is_empty()).then_some(Self(strokes))
    }

    pub fn strokes(&self) -> &[KeyStroke] {
        &self.0
    }
}

impl fmt::Display for KeySequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, stroke) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(" ")?;
            }
            write!(formatter, "{stroke}")?;
        }
        Ok(())
    }
}

impl FromStr for KeySequence {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(
            value
                .split_whitespace()
                .map(KeyStroke::from_str)
                .collect::<Result<Vec<_>, _>>()?,
        )
        .ok_or(())
    }
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.control {
            formatter.write_str("Ctrl+")?;
        }
        if self.modifiers.alt {
            formatter.write_str("Alt+")?;
        }
        if self.modifiers.shift {
            formatter.write_str("Shift+")?;
        }
        if self.modifiers.logo {
            formatter.write_str("Win+")?;
        }
        match self.key {
            Key::Character(character) => write!(formatter, "{}", character.to_ascii_uppercase()),
            Key::Space => formatter.write_str("Space"),
            Key::ArrowLeft => formatter.write_str("Left"),
            Key::ArrowRight => formatter.write_str("Right"),
            Key::ArrowUp => formatter.write_str("Up"),
            Key::ArrowDown => formatter.write_str("Down"),
            Key::Tab => formatter.write_str("Tab"),
            Key::Escape => formatter.write_str("Escape"),
            Key::F11 => formatter.write_str("F11"),
        }
    }
}

impl FromStr for KeyStroke {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut modifiers = Modifiers::default();
        let mut key = None;
        for part in value.split('+') {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.control = true,
                "alt" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "win" | "logo" => modifiers.logo = true,
                "space" if key.is_none() => key = Some(Key::Space),
                "plus" if key.is_none() => key = Some(Key::Character('+')),
                "minus" if key.is_none() => key = Some(Key::Character('-')),
                "left" if key.is_none() => key = Some(Key::ArrowLeft),
                "right" if key.is_none() => key = Some(Key::ArrowRight),
                "up" if key.is_none() => key = Some(Key::ArrowUp),
                "down" if key.is_none() => key = Some(Key::ArrowDown),
                "tab" if key.is_none() => key = Some(Key::Tab),
                "escape" | "esc" if key.is_none() => key = Some(Key::Escape),
                "f11" if key.is_none() => key = Some(Key::F11),
                character if key.is_none() && character.chars().count() == 1 => {
                    key = character.chars().next().map(Key::Character)
                }
                _ => return Err(()),
            }
        }
        Ok(Self {
            modifiers,
            key: key.ok_or(())?,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CommandContext {
    pub media_kind: Option<MediaKind>,
    pub palette_open: bool,
    pub filmstrip_open: bool,
    pub reading_mode: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandDefinition {
    pub id: CommandId,
    pub title: &'static str,
    pub media_kinds: &'static [MediaKind],
    pub requires_reading_mode: bool,
}

impl CommandDefinition {
    pub fn is_enabled(self, context: CommandContext) -> bool {
        (!self.requires_reading_mode || context.reading_mode)
            && (self.media_kinds.is_empty()
                || context
                    .media_kind
                    .is_some_and(|kind| self.media_kinds.contains(&kind)))
    }
}

const PLAYABLE_MEDIA: &[MediaKind] = &[MediaKind::Video, MediaKind::Audio];
const ANY_MEDIA: &[MediaKind] = &[MediaKind::Image, MediaKind::Video, MediaKind::Audio];

const COMMANDS: &[CommandDefinition] = &[
    command(CommandId::OpenFile, "Open file", &[]),
    command(CommandId::ToggleFullscreen, "Toggle fullscreen", &[]),
    command(CommandId::OpenFolder, "Open folder", &[]),
    command(CommandId::CloseTab, "Close tab", &[]),
    command(CommandId::NextTab, "Next tab", &[]),
    command(CommandId::PreviousTab, "Previous tab", &[]),
    command(CommandId::TogglePause, "Play or pause", PLAYABLE_MEDIA),
    command(CommandId::SeekBackward, "Seek backward", PLAYABLE_MEDIA),
    command(CommandId::SeekForward, "Seek forward", PLAYABLE_MEDIA),
    command(CommandId::PreviousMedia, "Previous media", ANY_MEDIA),
    command(CommandId::NextMedia, "Next media", ANY_MEDIA),
    command(
        CommandId::PreviousSameKind,
        "Previous media of same kind",
        ANY_MEDIA,
    ),
    command(
        CommandId::NextSameKind,
        "Next media of same kind",
        ANY_MEDIA,
    ),
    command(CommandId::ToggleFilmstrip, "Toggle filmstrip", ANY_MEDIA),
    command(CommandId::ToggleCommandPalette, "Show command palette", &[]),
    command(CommandId::ReloadShortcuts, "Reload keyboard shortcuts", &[]),
    command(CommandId::ZoomIn, "Zoom in", &[MediaKind::Image]),
    command(CommandId::ZoomOut, "Zoom out", &[MediaKind::Image]),
    command(
        CommandId::ActualSize,
        "Zoom to actual size",
        &[MediaKind::Image],
    ),
    command(
        CommandId::FitToWindow,
        "Fit image to window",
        &[MediaKind::Image],
    ),
    command(
        CommandId::ClearSelection,
        "Clear visual selection",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::ToggleCropPreview,
        "Toggle crop preview",
        &[MediaKind::Image],
    ),
    command(
        CommandId::ToggleReadingMode,
        "Toggle reading mode",
        &[MediaKind::Image],
    ),
    reading_command(
        CommandId::IncreaseReadingPages,
        "Show more reading pages",
        &[MediaKind::Image],
    ),
    reading_command(
        CommandId::DecreaseReadingPages,
        "Show fewer reading pages",
        &[MediaKind::Image],
    ),
    reading_command(
        CommandId::ToggleReadingAxis,
        "Toggle reading direction",
        &[MediaKind::Image],
    ),
    reading_command(
        CommandId::ReverseReadingOrder,
        "Reverse reading order",
        &[MediaKind::Image],
    ),
    command(CommandId::Undo, "Undo edit", ANY_MEDIA),
    command(CommandId::Redo, "Redo edit", ANY_MEDIA),
    command(
        CommandId::ApplyCrop,
        "Apply crop selection",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::RotateClockwise,
        "Rotate clockwise",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::RotateCounterclockwise,
        "Rotate counterclockwise",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::FlipHorizontal,
        "Flip horizontally",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::FlipVertical,
        "Flip vertically",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(CommandId::SetTrimStart, "Set trim start", PLAYABLE_MEDIA),
    command(CommandId::SetTrimEnd, "Set trim end", PLAYABLE_MEDIA),
    command(CommandId::VolumeDown, "Decrease volume", PLAYABLE_MEDIA),
    command(CommandId::VolumeUp, "Increase volume", PLAYABLE_MEDIA),
    command(CommandId::ToggleMute, "Toggle mute", PLAYABLE_MEDIA),
    command(
        CommandId::RateDown,
        "Decrease playback rate",
        PLAYABLE_MEDIA,
    ),
    command(CommandId::RateUp, "Increase playback rate", PLAYABLE_MEDIA),
    command(CommandId::ResetRate, "Reset playback rate", PLAYABLE_MEDIA),
    command(CommandId::Save, "Save exported media", ANY_MEDIA),
    command(CommandId::ExportAs, "Export as", ANY_MEDIA),
    command(CommandId::ToggleTimeline, "Toggle timeline", PLAYABLE_MEDIA),
    command(CommandId::ToggleGridMenu, "Toggle grid menu", ANY_MEDIA),
    command(
        CommandId::ToggleHardwareEncode,
        "Prefer hardware encoding",
        &[MediaKind::Video],
    ),
];

const fn command(
    id: CommandId,
    title: &'static str,
    media_kinds: &'static [MediaKind],
) -> CommandDefinition {
    CommandDefinition {
        id,
        title,
        media_kinds,
        requires_reading_mode: false,
    }
}

const fn reading_command(
    id: CommandId,
    title: &'static str,
    media_kinds: &'static [MediaKind],
) -> CommandDefinition {
    CommandDefinition {
        id,
        title,
        media_kinds,
        requires_reading_mode: true,
    }
}

pub fn command_definitions() -> &'static [CommandDefinition] {
    COMMANDS
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShortcutBindings(BTreeMap<CommandId, KeySequence>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutMatch {
    None,
    Prefix,
    Command(CommandId),
}

impl ShortcutBindings {
    pub fn set(&mut self, command: CommandId, sequence: KeySequence) {
        self.0.insert(command, sequence);
    }

    pub fn get(&self, command: CommandId) -> Option<&KeySequence> {
        self.0.get(&command)
    }

    pub fn iter(&self) -> impl Iterator<Item = (CommandId, &KeySequence)> {
        self.0
            .iter()
            .map(|(command, sequence)| (*command, sequence))
    }

    pub fn resolve(&self, entered: &[KeyStroke], context: CommandContext) -> ShortcutMatch {
        let mut prefix = false;
        for definition in command_definitions()
            .iter()
            .filter(|definition| definition.is_enabled(context))
        {
            let Some(bound) = self.get(definition.id) else {
                continue;
            };
            if bound.strokes() == entered {
                return ShortcutMatch::Command(definition.id);
            }
            prefix |= bound.strokes().starts_with(entered);
        }

        if prefix {
            ShortcutMatch::Prefix
        } else {
            ShortcutMatch::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_commands_obey_the_active_context() {
        let pause = command_definitions()
            .iter()
            .find(|definition| definition.id == CommandId::TogglePause)
            .copied()
            .expect("pause command exists");

        assert!(pause.is_enabled(CommandContext {
            media_kind: Some(MediaKind::Video),
            ..CommandContext::default()
        }));
        assert!(!pause.is_enabled(CommandContext {
            media_kind: Some(MediaKind::Image),
            ..CommandContext::default()
        }));
    }

    #[test]
    fn reading_commands_require_reading_mode() {
        let increase = command_definitions()
            .iter()
            .find(|definition| definition.id == CommandId::IncreaseReadingPages)
            .copied()
            .expect("reading command exists");

        assert!(!increase.is_enabled(CommandContext {
            media_kind: Some(MediaKind::Image),
            ..CommandContext::default()
        }));
        assert!(increase.is_enabled(CommandContext {
            media_kind: Some(MediaKind::Image),
            reading_mode: true,
            ..CommandContext::default()
        }));
    }

    #[test]
    fn shortcut_bindings_can_replace_a_command_sequence() {
        let first = sequence('p');
        let replacement = sequence('k');
        let mut bindings = ShortcutBindings::default();

        bindings.set(CommandId::TogglePause, first);
        bindings.set(CommandId::TogglePause, replacement.clone());

        assert_eq!(bindings.get(CommandId::TogglePause), Some(&replacement));
        assert_eq!(
            bindings.resolve(
                replacement.strokes(),
                CommandContext {
                    media_kind: Some(MediaKind::Video),
                    ..CommandContext::default()
                }
            ),
            ShortcutMatch::Command(CommandId::TogglePause)
        );
    }

    #[test]
    fn resolves_prefixes_and_context_dependent_bindings() {
        let prefix = KeyStroke {
            modifiers: Modifiers {
                control: true,
                ..Modifiers::default()
            },
            key: Key::Character('k'),
        };
        let suffix = KeyStroke {
            modifiers: Modifiers::default(),
            key: Key::Character('f'),
        };
        let mut bindings = ShortcutBindings::default();
        bindings.set(
            CommandId::ToggleFilmstrip,
            KeySequence::new([prefix.clone(), suffix]).expect("two strokes are valid"),
        );

        assert_eq!(
            bindings.resolve(
                std::slice::from_ref(&prefix),
                CommandContext {
                    media_kind: Some(MediaKind::Video),
                    ..CommandContext::default()
                }
            ),
            ShortcutMatch::Prefix
        );
        assert_eq!(
            bindings.resolve(std::slice::from_ref(&prefix), CommandContext::default()),
            ShortcutMatch::None
        );
    }

    #[test]
    fn shortcut_text_round_trips_with_prefix_chords() {
        let sequence = "Ctrl+K Ctrl+S"
            .parse::<KeySequence>()
            .expect("valid prefix shortcut");

        assert_eq!(sequence.to_string(), "Ctrl+K Ctrl+S");
        assert_eq!("toggle_pause".parse(), Ok(CommandId::TogglePause));
        let fullscreen = "Ctrl+K F11"
            .parse::<KeySequence>()
            .expect("fullscreen prefix");
        assert_eq!(fullscreen.to_string(), "Ctrl+K F11");
        assert_eq!(fullscreen.strokes()[1].key, Key::F11);
        assert_eq!("toggle_fullscreen".parse(), Ok(CommandId::ToggleFullscreen));
    }

    fn sequence(character: char) -> KeySequence {
        KeySequence::new([KeyStroke {
            modifiers: Modifiers::default(),
            key: Key::Character(character),
        }])
        .expect("one stroke is valid")
    }
}
