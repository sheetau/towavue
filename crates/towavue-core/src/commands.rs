use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use crate::MediaKind;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CommandId {
    OpenFile,
    OpenFolder,
    ShowLicenses,
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
    SelectAll,
    ToggleCropPreview,
    ToggleReadingMode,
    IncreaseReadingPages,
    DecreaseReadingPages,
    ToggleReadingAxis,
    ReverseReadingOrder,
    Undo,
    Redo,
    ApplyCrop,
    DeleteTimeSelection,
    KeepTimeSelection,
    PlayTimeSelection,
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
    ExportAudio,
    AudioExportOptions,
    MetadataExportOptions,
    ToggleTimeline,
    ToggleGridMenu,
    ToggleHardwareEncode,
    ToggleFullscreen,
    PreviousImage,
    NextImage,
    FirstImage,
    LastImage,
    CoverWindow,
    IncreaseReadingFirstPage,
    DecreaseReadingFirstPage,
    CloseOtherTabs,
    CloseTabsLeft,
    CloseTabsRight,
    CloseAllTabs,
    ReopenClosedTab,
    CopyFilePath,
    RevealFile,
    CopyImage,
    ResizeImage,
    ToggleImageInterpolation,
    CycleAudioRepeat,
    ToggleAudioShuffle,
    PreviousVideoFrame,
    NextVideoFrame,
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
    SelectAspectSquare,
    SelectAspectFourThree,
    SelectAspectThreeFour,
    SelectAspectThreeTwo,
    SelectAspectTwoThree,
    SelectAspectSixteenNine,
    SelectAspectNineSixteen,
    FreeRotateImage,
    FreeRotateVideo,
    ResizeVideo,
    StepAudioBackward,
    StepAudioForward,
}

impl CommandId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenFile => "open_file",
            Self::OpenFolder => "open_folder",
            Self::ShowLicenses => "show_licenses",
            Self::CloseTab => "close_tab",
            Self::CloseOtherTabs => "close_other_tabs",
            Self::CloseTabsLeft => "close_tabs_left",
            Self::CloseTabsRight => "close_tabs_right",
            Self::CloseAllTabs => "close_all_tabs",
            Self::ReopenClosedTab => "reopen_closed_tab",
            Self::CopyFilePath => "copy_file_path",
            Self::RevealFile => "reveal_file",
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
            Self::CoverWindow => "cover_window",
            Self::ClearSelection => "clear_selection",
            Self::SelectAll => "select_all",
            Self::ToggleCropPreview => "toggle_crop_preview",
            Self::ToggleReadingMode => "toggle_reading_mode",
            Self::IncreaseReadingPages => "increase_reading_pages",
            Self::DecreaseReadingPages => "decrease_reading_pages",
            Self::IncreaseReadingFirstPage => "increase_reading_first_page",
            Self::DecreaseReadingFirstPage => "decrease_reading_first_page",
            Self::ToggleReadingAxis => "toggle_reading_axis",
            Self::ReverseReadingOrder => "reverse_reading_order",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::ApplyCrop => "apply_crop",
            Self::DeleteTimeSelection => "delete_time_selection",
            Self::KeepTimeSelection => "keep_time_selection",
            Self::PlayTimeSelection => "play_time_selection",
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
            Self::ExportAudio => "export_audio",
            Self::AudioExportOptions => "audio_export_options",
            Self::MetadataExportOptions => "metadata_export_options",
            Self::ToggleTimeline => "toggle_timeline",
            Self::ToggleGridMenu => "toggle_grid_menu",
            Self::ToggleHardwareEncode => "toggle_hardware_encode",
            Self::ToggleFullscreen => "toggle_fullscreen",
            Self::PreviousImage => "previous_image",
            Self::NextImage => "next_image",
            Self::FirstImage => "first_image",
            Self::LastImage => "last_image",
            Self::CopyImage => "copy_image",
            Self::ResizeImage => "resize_image",
            Self::CycleAudioRepeat => "cycle_audio_repeat",
            Self::ToggleAudioShuffle => "toggle_audio_shuffle",
            Self::PreviousVideoFrame => "previous_video_frame",
            Self::NextVideoFrame => "next_video_frame",
            Self::JumpImagesBackward1 => "jump_images_backward_1",
            Self::JumpImagesBackward2 => "jump_images_backward_2",
            Self::JumpImagesBackward3 => "jump_images_backward_3",
            Self::JumpImagesBackward4 => "jump_images_backward_4",
            Self::JumpImagesBackward5 => "jump_images_backward_5",
            Self::JumpImagesBackward6 => "jump_images_backward_6",
            Self::JumpImagesBackward7 => "jump_images_backward_7",
            Self::JumpImagesBackward8 => "jump_images_backward_8",
            Self::JumpImagesBackward9 => "jump_images_backward_9",
            Self::JumpImagesBackward10 => "jump_images_backward_10",
            Self::JumpImagesForward1 => "jump_images_forward_1",
            Self::JumpImagesForward2 => "jump_images_forward_2",
            Self::JumpImagesForward3 => "jump_images_forward_3",
            Self::JumpImagesForward4 => "jump_images_forward_4",
            Self::JumpImagesForward5 => "jump_images_forward_5",
            Self::JumpImagesForward6 => "jump_images_forward_6",
            Self::JumpImagesForward7 => "jump_images_forward_7",
            Self::JumpImagesForward8 => "jump_images_forward_8",
            Self::JumpImagesForward9 => "jump_images_forward_9",
            Self::JumpImagesForward10 => "jump_images_forward_10",
            Self::SelectAspectSquare => "select_aspect_1_1",
            Self::SelectAspectFourThree => "select_aspect_4_3",
            Self::SelectAspectThreeFour => "select_aspect_3_4",
            Self::SelectAspectThreeTwo => "select_aspect_3_2",
            Self::SelectAspectTwoThree => "select_aspect_2_3",
            Self::SelectAspectSixteenNine => "select_aspect_16_9",
            Self::SelectAspectNineSixteen => "select_aspect_9_16",
            Self::FreeRotateImage => "free_rotate_image",
            Self::FreeRotateVideo => "free_rotate_video",
            Self::ResizeVideo => "resize_video",
            Self::StepAudioBackward => "step_audio_backward",
            Self::StepAudioForward => "step_audio_forward",
            Self::ToggleImageInterpolation => "toggle_image_interpolation",
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
    Home,
    End,
    Delete,
    PageUp,
    PageDown,
    Backspace,
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
            Key::Character('+') => formatter.write_str("Plus"),
            Key::Character('|') => formatter.write_str("Pipe"),
            Key::Character(character) => write!(formatter, "{}", character.to_ascii_uppercase()),
            Key::Space => formatter.write_str("Space"),
            Key::ArrowLeft => formatter.write_str("Left"),
            Key::ArrowRight => formatter.write_str("Right"),
            Key::ArrowUp => formatter.write_str("Up"),
            Key::ArrowDown => formatter.write_str("Down"),
            Key::Tab => formatter.write_str("Tab"),
            Key::Escape => formatter.write_str("Escape"),
            Key::F11 => formatter.write_str("F11"),
            Key::Home => formatter.write_str("Home"),
            Key::End => formatter.write_str("End"),
            Key::Delete => formatter.write_str("Delete"),
            Key::PageUp => formatter.write_str("PageUp"),
            Key::PageDown => formatter.write_str("PageDown"),
            Key::Backspace => formatter.write_str("Backspace"),
        }
    }
}

impl FromStr for KeyStroke {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "+" {
            return "Plus".parse();
        }
        if let Some(prefix) = value.strip_suffix("++") {
            return format!("{prefix}+Plus").parse();
        }
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
                "delete" | "del" if key.is_none() => key = Some(Key::Delete),
                "pageup" | "pgup" if key.is_none() => key = Some(Key::PageUp),
                "pagedown" | "pgdn" if key.is_none() => key = Some(Key::PageDown),
                "backspace" if key.is_none() => key = Some(Key::Backspace),
                "f11" if key.is_none() => key = Some(Key::F11),
                "home" if key.is_none() => key = Some(Key::Home),
                "end" if key.is_none() => key = Some(Key::End),
                "pipe" if key.is_none() => key = Some(Key::Character('|')),
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
    pub timeline_open: bool,
    pub has_time_selection: bool,
    pub media_kind: Option<MediaKind>,
    pub palette_open: bool,
    pub filmstrip_open: bool,
    pub reading_mode: bool,
    pub has_unsaved_edits: bool,
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
        let image_reading = context.media_kind == Some(MediaKind::Image) && context.reading_mode;
        (!self.requires_reading_mode || context.reading_mode)
            && (context.media_kind != Some(MediaKind::Video)
                || context.timeline_open
                || !matches!(
                    self.id,
                    CommandId::SelectAll
                        | CommandId::SelectAspectSquare
                        | CommandId::SelectAspectFourThree
                        | CommandId::SelectAspectThreeFour
                        | CommandId::SelectAspectThreeTwo
                        | CommandId::SelectAspectTwoThree
                        | CommandId::SelectAspectSixteenNine
                        | CommandId::SelectAspectNineSixteen
                        | CommandId::ApplyCrop
                        | CommandId::RotateClockwise
                        | CommandId::RotateCounterclockwise
                        | CommandId::FlipHorizontal
                        | CommandId::FlipVertical
                        | CommandId::FreeRotateVideo
                        | CommandId::ResizeVideo
                        | CommandId::ZoomIn
                        | CommandId::ZoomOut
                        | CommandId::ActualSize
                        | CommandId::FitToWindow
                        | CommandId::CoverWindow
                ))
            && (!matches!(
                self.id,
                CommandId::DeleteTimeSelection
                    | CommandId::KeepTimeSelection
                    | CommandId::PlayTimeSelection
            ) || (context.timeline_open && context.has_time_selection))
            && (!matches!(self.id, CommandId::SetTrimStart | CommandId::SetTrimEnd)
                || context.timeline_open)
            && (self.id != CommandId::ApplyCrop
                || !context.timeline_open
                || !context.has_time_selection)
            && (self.id != CommandId::ToggleReadingMode
                || context.reading_mode
                || !context.has_unsaved_edits)
            && (!image_reading
                || !matches!(
                    self.id,
                    CommandId::SelectAll
                        | CommandId::SelectAspectSquare
                        | CommandId::SelectAspectFourThree
                        | CommandId::SelectAspectThreeFour
                        | CommandId::SelectAspectThreeTwo
                        | CommandId::SelectAspectTwoThree
                        | CommandId::SelectAspectSixteenNine
                        | CommandId::SelectAspectNineSixteen
                        | CommandId::CoverWindow
                        | CommandId::ToggleCropPreview
                        | CommandId::ApplyCrop
                        | CommandId::RotateClockwise
                        | CommandId::RotateCounterclockwise
                        | CommandId::FlipHorizontal
                        | CommandId::FlipVertical
                        | CommandId::Undo
                        | CommandId::Redo
                        | CommandId::ResizeImage
                        | CommandId::FreeRotateImage
                ))
            && (self.media_kinds.is_empty()
                || context
                    .media_kind
                    .is_some_and(|kind| self.media_kinds.contains(&kind)))
    }
}

const PLAYABLE_MEDIA: &[MediaKind] = &[MediaKind::Video, MediaKind::Audio];
const VISUAL_MEDIA: &[MediaKind] = &[MediaKind::Image, MediaKind::Video];
const ANY_MEDIA: &[MediaKind] = &[MediaKind::Image, MediaKind::Video, MediaKind::Audio];

const COMMANDS: &[CommandDefinition] = &[
    command(CommandId::OpenFile, "Open file", &[]),
    command(CommandId::ToggleFullscreen, "Toggle fullscreen", &[]),
    command(CommandId::OpenFolder, "Open folder", &[]),
    command(CommandId::ShowLicenses, "Show licenses and sources", &[]),
    command(CommandId::CloseTab, "Close tab", &[]),
    command(CommandId::NextTab, "Next tab", &[]),
    command(CommandId::PreviousTab, "Previous tab", &[]),
    command(CommandId::TogglePause, "Play or pause", PLAYABLE_MEDIA),
    command(
        CommandId::PlayTimeSelection,
        "Play selected time",
        PLAYABLE_MEDIA,
    ),
    command(
        CommandId::CycleAudioRepeat,
        "Cycle audio repeat",
        &[MediaKind::Audio],
    ),
    command(
        CommandId::ToggleAudioShuffle,
        "Toggle audio shuffle",
        &[MediaKind::Audio],
    ),
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
    command(CommandId::ZoomIn, "Zoom in", VISUAL_MEDIA),
    command(CommandId::ZoomOut, "Zoom out", VISUAL_MEDIA),
    command(CommandId::ActualSize, "Zoom to actual size", VISUAL_MEDIA),
    command(CommandId::FitToWindow, "Fit media to window", VISUAL_MEDIA),
    command(
        CommandId::CoverWindow,
        "Cover window with media",
        VISUAL_MEDIA,
    ),
    command(CommandId::SelectAll, "Select whole media", ANY_MEDIA),
    command(CommandId::ClearSelection, "Clear selection", ANY_MEDIA),
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
    command(
        CommandId::SetTrimStart,
        "Set time selection start",
        PLAYABLE_MEDIA,
    ),
    command(
        CommandId::SetTrimEnd,
        "Set time selection end",
        PLAYABLE_MEDIA,
    ),
    command(
        CommandId::DeleteTimeSelection,
        "Delete selected time",
        PLAYABLE_MEDIA,
    ),
    command(
        CommandId::KeepTimeSelection,
        "Keep only selected time",
        PLAYABLE_MEDIA,
    ),
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
    command(
        CommandId::AudioExportOptions,
        "Audio export options",
        PLAYABLE_MEDIA,
    ),
    command(
        CommandId::MetadataExportOptions,
        "Metadata export options",
        PLAYABLE_MEDIA,
    ),
    command(
        CommandId::ExportAudio,
        "Export audio only",
        &[MediaKind::Video],
    ),
    command(
        CommandId::ToggleTimeline,
        "Toggle video editing timeline",
        &[MediaKind::Video],
    ),
    command(CommandId::ToggleGridMenu, "Toggle grid menu", ANY_MEDIA),
    command(
        CommandId::ToggleHardwareEncode,
        "Prefer hardware encoding",
        &[MediaKind::Video],
    ),
    command(
        CommandId::PreviousImage,
        "Previous image",
        &[MediaKind::Image],
    ),
    command(CommandId::NextImage, "Next image", &[MediaKind::Image]),
    command(
        CommandId::FirstImage,
        "First image in folder",
        &[MediaKind::Image],
    ),
    command(
        CommandId::LastImage,
        "Last image in folder",
        &[MediaKind::Image],
    ),
    reading_command(
        CommandId::IncreaseReadingFirstPage,
        "Show more images on the first reading page",
        &[MediaKind::Image],
    ),
    reading_command(
        CommandId::DecreaseReadingFirstPage,
        "Show fewer images on the first reading page",
        &[MediaKind::Image],
    ),
    command(CommandId::CloseOtherTabs, "Close other tabs", ANY_MEDIA),
    command(
        CommandId::CloseTabsLeft,
        "Close tabs to the left",
        ANY_MEDIA,
    ),
    command(
        CommandId::CloseTabsRight,
        "Close tabs to the right",
        ANY_MEDIA,
    ),
    command(CommandId::CloseAllTabs, "Close all tabs", ANY_MEDIA),
    command(CommandId::ReopenClosedTab, "Reopen closed tab", &[]),
    command(CommandId::CopyFilePath, "Copy file path", ANY_MEDIA),
    command(CommandId::RevealFile, "Reveal in File Explorer", ANY_MEDIA),
    command(
        CommandId::CopyImage,
        "Copy image or selection",
        &[MediaKind::Image],
    ),
    command(
        CommandId::ResizeImage,
        "Resize / resample image",
        &[MediaKind::Image],
    ),
    command(
        CommandId::ToggleImageInterpolation,
        "Toggle image interpolation (smooth / nearest)",
        &[MediaKind::Image],
    ),
    command(
        CommandId::PreviousVideoFrame,
        "Previous video frame",
        &[MediaKind::Video],
    ),
    command(
        CommandId::NextVideoFrame,
        "Next video frame",
        &[MediaKind::Video],
    ),
    command(
        CommandId::JumpImagesBackward1,
        "Jump backward 1 image",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesBackward2,
        "Jump backward 2 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesBackward3,
        "Jump backward 3 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesBackward4,
        "Jump backward 4 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesBackward5,
        "Jump backward 5 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesBackward6,
        "Jump backward 6 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesBackward7,
        "Jump backward 7 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesBackward8,
        "Jump backward 8 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesBackward9,
        "Jump backward 9 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesBackward10,
        "Jump backward 10 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesForward1,
        "Jump forward 1 image",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesForward2,
        "Jump forward 2 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesForward3,
        "Jump forward 3 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesForward4,
        "Jump forward 4 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesForward5,
        "Jump forward 5 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesForward6,
        "Jump forward 6 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesForward7,
        "Jump forward 7 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesForward8,
        "Jump forward 8 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesForward9,
        "Jump forward 9 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::JumpImagesForward10,
        "Jump forward 10 images",
        &[MediaKind::Image],
    ),
    command(
        CommandId::SelectAspectSquare,
        "Select 1:1 aspect ratio",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectFourThree,
        "Select 4:3 aspect ratio",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectThreeFour,
        "Select 3:4 aspect ratio",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectThreeTwo,
        "Select 3:2 aspect ratio",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectTwoThree,
        "Select 2:3 aspect ratio",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectSixteenNine,
        "Select 16:9 aspect ratio",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectNineSixteen,
        "Select 9:16 aspect ratio",
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::FreeRotateImage,
        "Free rotate image",
        &[MediaKind::Image],
    ),
    command(
        CommandId::FreeRotateVideo,
        "Free rotate video",
        &[MediaKind::Video],
    ),
    command(
        CommandId::ResizeVideo,
        "Resize / resample video",
        &[MediaKind::Video],
    ),
    command(
        CommandId::StepAudioBackward,
        "Step audio backward (10 ms)",
        &[MediaKind::Audio],
    ),
    command(
        CommandId::StepAudioForward,
        "Step audio forward (10 ms)",
        &[MediaKind::Audio],
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
pub struct ShortcutBindings(BTreeMap<CommandId, Vec<KeySequence>>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutMatch {
    None,
    Prefix,
    Command(CommandId),
}

impl ShortcutBindings {
    pub fn remove(&mut self, command: CommandId) {
        self.0.remove(&command);
    }

    pub fn set(&mut self, command: CommandId, sequence: KeySequence) {
        self.0.insert(command, vec![sequence]);
    }

    pub fn add(&mut self, command: CommandId, sequence: KeySequence) {
        let sequences = self.0.entry(command).or_default();
        if !sequences.contains(&sequence) {
            sequences.push(sequence);
        }
    }

    pub fn get(&self, command: CommandId) -> Option<&KeySequence> {
        self.all(command).first()
    }

    pub fn all(&self, command: CommandId) -> &[KeySequence] {
        self.0.get(&command).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn label(&self, command: CommandId, context: CommandContext) -> String {
        self.all(command)
            .iter()
            .enumerate()
            .filter(|(index, sequence)| {
                *index == 0
                    || self.resolve(sequence.strokes(), context) == ShortcutMatch::Command(command)
            })
            .map(|(_, sequence)| sequence.to_string())
            .collect::<Vec<_>>()
            .join(" / ")
    }

    pub fn iter(&self) -> impl Iterator<Item = (CommandId, &KeySequence)> {
        self.0.iter().filter_map(|(command, sequences)| {
            sequences.first().map(|sequence| (*command, sequence))
        })
    }

    pub fn resolve(&self, entered: &[KeyStroke], context: CommandContext) -> ShortcutMatch {
        // Main bindings (including prefixes) win over alternatives. This keeps
        // contextual video rotation and user-remapped commands ahead of L Seek.
        for alternatives in [false, true] {
            let mut prefix = false;
            for definition in command_definitions()
                .iter()
                .filter(|definition| definition.is_enabled(context))
            {
                for (index, bound) in self.all(definition.id).iter().enumerate() {
                    if (index > 0) != alternatives {
                        continue;
                    }
                    if bound.strokes() == entered {
                        return ShortcutMatch::Command(definition.id);
                    }
                    prefix |= bound.strokes().starts_with(entered);
                }
            }
            if prefix {
                return ShortcutMatch::Prefix;
            }
        }
        ShortcutMatch::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn license_guide_is_available_without_media_and_in_every_media_context() {
        let id = CommandId::from_str("show_licenses").expect("registered license command");
        assert_eq!(id, CommandId::ShowLicenses);
        let definition = command_definitions()
            .iter()
            .find(|command| command.id == id)
            .expect("license command definition");
        for media_kind in [
            None,
            Some(MediaKind::Image),
            Some(MediaKind::Video),
            Some(MediaKind::Audio),
        ] {
            assert!(definition.is_enabled(CommandContext {
                media_kind,
                ..Default::default()
            }));
        }
    }

    #[test]
    fn video_visual_commands_require_the_timeline_but_viewing_and_recovery_do_not() {
        for kind in [MediaKind::Image, MediaKind::Video] {
            for timeline_open in [false, true] {
                let context = CommandContext {
                    media_kind: Some(kind),
                    timeline_open,
                    ..Default::default()
                };
                for id in [
                    CommandId::SelectAll,
                    CommandId::ApplyCrop,
                    CommandId::RotateClockwise,
                    CommandId::RotateCounterclockwise,
                    CommandId::FlipHorizontal,
                    CommandId::FlipVertical,
                    CommandId::ZoomIn,
                    CommandId::ZoomOut,
                    CommandId::ActualSize,
                    CommandId::FitToWindow,
                    CommandId::CoverWindow,
                ] {
                    let definition = command_definitions()
                        .iter()
                        .find(|d| d.id == id)
                        .expect("visual command");
                    assert_eq!(
                        definition.is_enabled(context),
                        kind == MediaKind::Image || timeline_open,
                        "{id:?} {context:?}"
                    );
                }
                for id in [
                    CommandId::Undo,
                    CommandId::Redo,
                    CommandId::Save,
                    CommandId::ClearSelection,
                ] {
                    assert!(
                        command_definitions()
                            .iter()
                            .find(|d| d.id == id)
                            .expect("recovery command")
                            .is_enabled(context)
                    );
                }
                if kind == MediaKind::Video {
                    for id in [
                        CommandId::TogglePause,
                        CommandId::SeekBackward,
                        CommandId::SeekForward,
                        CommandId::VolumeUp,
                        CommandId::RateUp,
                        CommandId::ToggleFullscreen,
                    ] {
                        assert!(
                            command_definitions()
                                .iter()
                                .find(|d| d.id == id)
                                .expect("viewing command")
                                .is_enabled(context)
                        );
                    }
                }
            }
        }
    }

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
    fn reading_blocks_dirty_entry_and_image_edits_but_not_other_media() {
        let enabled = |id, context| {
            command_definitions()
                .iter()
                .find(|definition| definition.id == id)
                .expect("command")
                .is_enabled(context)
        };
        let mut context = CommandContext {
            media_kind: Some(MediaKind::Image),
            has_unsaved_edits: true,
            ..Default::default()
        };
        assert!(!enabled(CommandId::ToggleReadingMode, context));
        context.has_unsaved_edits = false;
        assert!(enabled(CommandId::ToggleReadingMode, context));
        context.reading_mode = true;
        context.has_unsaved_edits = true;
        assert!(
            enabled(CommandId::ToggleReadingMode, context),
            "exiting remains possible"
        );
        for id in [
            CommandId::Undo,
            CommandId::Redo,
            CommandId::ApplyCrop,
            CommandId::RotateClockwise,
            CommandId::RotateCounterclockwise,
            CommandId::FlipHorizontal,
            CommandId::FlipVertical,
            CommandId::SelectAll,
            CommandId::ToggleCropPreview,
            CommandId::CoverWindow,
            CommandId::ResizeImage,
        ] {
            assert!(!enabled(id, context), "{id:?}");
            assert!(
                enabled(
                    id,
                    CommandContext {
                        reading_mode: false,
                        ..context
                    }
                ),
                "{id:?}"
            );
        }
        for id in [
            CommandId::NextImage,
            CommandId::IncreaseReadingPages,
            CommandId::Save,
        ] {
            assert!(enabled(id, context), "{id:?}");
        }
        context.media_kind = Some(MediaKind::Video);
        context.timeline_open = true;
        for id in [
            CommandId::Undo,
            CommandId::RotateClockwise,
            CommandId::SelectAll,
        ] {
            assert!(
                enabled(id, context),
                "image preference must not block video {id:?}"
            );
        }
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
    fn plus_shortcuts_round_trip_and_accept_legacy_serialized_spelling() {
        for (legacy, canonical) in [
            ("+", "Plus"),
            ("Ctrl++", "Ctrl+Plus"),
            ("Ctrl+Shift++", "Ctrl+Shift+Plus"),
            ("Ctrl+K Alt++", "Ctrl+K Alt+Plus"),
        ] {
            let sequence = legacy.parse::<KeySequence>().expect("legacy plus binding");
            assert_eq!(sequence.to_string(), canonical);
            assert_eq!(
                canonical
                    .parse::<KeySequence>()
                    .expect("canonical plus binding"),
                sequence
            );
        }
        for malformed in ["++", "Ctrl+", "Ctrl+++", "Ctrl++A"] {
            assert!(malformed.parse::<KeySequence>().is_err(), "{malformed}");
        }
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
