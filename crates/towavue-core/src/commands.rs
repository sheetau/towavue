use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use crate::MediaKind;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CommandId {
    OpenFile,
    OpenFolder,
    OpenGallery,
    GoToFile,
    OpenRecentFolder,
    ShowLicenses,
    CheckForUpdates,
    About,
    CloseTab,
    NextTab,
    PreviousTab,
    TogglePause,
    SeekBackward,
    SeekForward,
    SeekVideo0,
    SeekVideo10,
    SeekVideo20,
    SeekVideo30,
    SeekVideo40,
    SeekVideo50,
    SeekVideo60,
    SeekVideo70,
    SeekVideo80,
    SeekVideo90,
    PreviousMedia,
    NextMedia,
    PreviousSameKind,
    NextSameKind,
    ToggleFilmstrip,
    ToggleCommandPalette,
    OpenKeyboardSettings,
    ReloadShortcuts,
    ZoomIn,
    ZoomOut,
    ActualSize,
    FitToWindow,
    ClearSelection,
    SelectAll,
    ZoomSelection,
    ToggleReadingMode,
    IncreaseReadingPages,
    DecreaseReadingPages,
    ToggleReadingAxis,
    ReverseReadingOrder,
    ReloadFolderOrder,
    ReadingLeft,
    ReadingRight,
    Undo,
    Redo,
    ApplyCrop,
    DeleteTimeSelection,
    KeepTimeSelection,
    PlayTimeSelection,
    RotateClockwise,
    RotateCounterclockwise,
    RotateFineClockwise,
    RotateFineCounterclockwise,
    FlipHorizontal,
    FlipVertical,
    SetTrimStart,
    SetTrimEnd,
    VolumeDown,
    VolumeUp,
    ToggleMute,
    CycleVolumeStep,
    VolumeStepTwo,
    VolumeStepFive,
    VolumeStepTen,
    AudioRepeatOff,
    AudioRepeatAll,
    AudioRepeatOne,
    FolderNavigationStop,
    FolderNavigationLoop,
    RateDown,
    RateUp,
    ResetRate,
    EditSpeed,
    DeleteFile,
    RenameFile,
    MoveFile,
    Save,
    ExportAs,
    ExportAudio,
    ExportFrame,
    AudioExportOptions,
    MetadataExportOptions,
    ToggleTimeline,
    ToggleGridMenu,
    ToggleHardwareEncode,
    ExportQualityHigh,
    ExportQualityBalanced,
    ExportQualitySmaller,
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
    PasteImage,
    ResizeImage,
    ToggleImageInterpolation,
    ToggleImageMinification,
    CycleAudioRepeat,
    CycleAudioTrack,
    ToggleVideoRepeat,
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
    pub const fn video_seek_percent(self) -> Option<u8> {
        match self {
            Self::SeekVideo0 => Some(0),
            Self::SeekVideo10 => Some(10),
            Self::SeekVideo20 => Some(20),
            Self::SeekVideo30 => Some(30),
            Self::SeekVideo40 => Some(40),
            Self::SeekVideo50 => Some(50),
            Self::SeekVideo60 => Some(60),
            Self::SeekVideo70 => Some(70),
            Self::SeekVideo80 => Some(80),
            Self::SeekVideo90 => Some(90),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenFile => "open_file",
            Self::OpenGallery => "open_gallery",
            Self::GoToFile => "go_to_file",
            Self::OpenRecentFolder => "open_recent_folder",
            Self::OpenFolder => "open_folder",
            Self::ShowLicenses => "show_licenses",
            Self::CheckForUpdates => "check_for_updates",
            Self::About => "about",
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
            Self::SeekVideo0 => "seek_video_0",
            Self::SeekVideo10 => "seek_video_10",
            Self::SeekVideo20 => "seek_video_20",
            Self::SeekVideo30 => "seek_video_30",
            Self::SeekVideo40 => "seek_video_40",
            Self::SeekVideo50 => "seek_video_50",
            Self::SeekVideo60 => "seek_video_60",
            Self::SeekVideo70 => "seek_video_70",
            Self::SeekVideo80 => "seek_video_80",
            Self::SeekVideo90 => "seek_video_90",
            Self::PreviousMedia => "previous_media",
            Self::NextMedia => "next_media",
            Self::PreviousSameKind => "previous_same_kind",
            Self::NextSameKind => "next_same_kind",
            Self::ToggleFilmstrip => "toggle_filmstrip",
            Self::ToggleCommandPalette => "toggle_command_palette",
            Self::OpenKeyboardSettings => "open_keyboard_settings",
            Self::ReloadShortcuts => "reload_shortcuts",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            Self::ActualSize => "actual_size",
            Self::FitToWindow => "fit_to_window",
            Self::CoverWindow => "cover_window",
            Self::ClearSelection => "clear_selection",
            Self::SelectAll => "select_all",
            Self::ZoomSelection => "zoom_selection",
            Self::ToggleReadingMode => "toggle_reading_mode",
            Self::IncreaseReadingPages => "increase_reading_pages",
            Self::DecreaseReadingPages => "decrease_reading_pages",
            Self::IncreaseReadingFirstPage => "increase_reading_first_page",
            Self::DecreaseReadingFirstPage => "decrease_reading_first_page",
            Self::ToggleReadingAxis => "toggle_reading_axis",
            Self::ReverseReadingOrder => "reverse_reading_order",
            Self::ReloadFolderOrder => "reload_folder_order",
            Self::ReadingLeft => "reading_left",
            Self::ReadingRight => "reading_right",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::ApplyCrop => "apply_crop",
            Self::DeleteTimeSelection => "delete_time_selection",
            Self::KeepTimeSelection => "keep_time_selection",
            Self::PlayTimeSelection => "play_time_selection",
            Self::RotateClockwise => "rotate_clockwise",
            Self::RotateCounterclockwise => "rotate_counterclockwise",
            Self::RotateFineClockwise => "rotate_fine_clockwise",
            Self::RotateFineCounterclockwise => "rotate_fine_counterclockwise",
            Self::FlipHorizontal => "flip_horizontal",
            Self::FlipVertical => "flip_vertical",
            Self::SetTrimStart => "set_trim_start",
            Self::SetTrimEnd => "set_trim_end",
            Self::VolumeDown => "volume_down",
            Self::VolumeUp => "volume_up",
            Self::ToggleMute => "toggle_mute",
            Self::CycleVolumeStep => "cycle_volume_step",
            Self::VolumeStepTwo => "volume_step_two",
            Self::VolumeStepFive => "volume_step_five",
            Self::VolumeStepTen => "volume_step_ten",
            Self::AudioRepeatOff => "audio_repeat_off",
            Self::AudioRepeatAll => "audio_repeat_all",
            Self::AudioRepeatOne => "audio_repeat_one",
            Self::FolderNavigationStop => "folder_navigation_stop",
            Self::FolderNavigationLoop => "folder_navigation_loop",
            Self::RateDown => "rate_down",
            Self::RateUp => "rate_up",
            Self::ResetRate => "reset_rate",
            Self::EditSpeed => "edit_speed",
            Self::DeleteFile => "delete_file",
            Self::RenameFile => "rename_file",
            Self::MoveFile => "move_file",
            Self::Save => "save",
            Self::ExportAs => "export_as",
            Self::ExportAudio => "export_audio",
            Self::ExportFrame => "export_frame",
            Self::AudioExportOptions => "audio_export_options",
            Self::MetadataExportOptions => "metadata_export_options",
            Self::ToggleTimeline => "toggle_timeline",
            Self::ToggleGridMenu => "toggle_grid_menu",
            Self::ToggleHardwareEncode => "toggle_hardware_encode",
            Self::ExportQualityHigh => "export_quality_high",
            Self::ExportQualityBalanced => "export_quality_balanced",
            Self::ExportQualitySmaller => "export_quality_smaller",
            Self::ToggleFullscreen => "toggle_fullscreen",
            Self::PreviousImage => "previous_image",
            Self::NextImage => "next_image",
            Self::FirstImage => "first_image",
            Self::LastImage => "last_image",
            Self::CopyImage => "copy_image",
            Self::PasteImage => "paste_image",
            Self::ResizeImage => "resize_image",
            Self::CycleAudioRepeat => "cycle_audio_repeat",
            Self::CycleAudioTrack => "cycle_audio_track",
            Self::ToggleVideoRepeat => "toggle_video_repeat",
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
            Self::ToggleImageMinification => "toggle_image_minification",
        }
    }
}

impl FromStr for CommandId {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "toggle_crop_preview" {
            return Ok(Self::ZoomSelection);
        }
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
    Enter,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Tab,
    Escape,
    Function(u8),
    Insert,
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
            Key::Enter => formatter.write_str("Enter"),
            Key::ArrowLeft => formatter.write_str("Left"),
            Key::ArrowRight => formatter.write_str("Right"),
            Key::ArrowUp => formatter.write_str("Up"),
            Key::ArrowDown => formatter.write_str("Down"),
            Key::Tab => formatter.write_str("Tab"),
            Key::Escape => formatter.write_str("Escape"),
            Key::Function(number) => write!(formatter, "F{number}"),
            Key::Insert => formatter.write_str("Insert"),
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
                "enter" if key.is_none() => key = Some(Key::Enter),
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
                "insert" if key.is_none() => key = Some(Key::Insert),
                function
                    if key.is_none()
                        && function.starts_with('f')
                        && function[1..]
                            .parse::<u8>()
                            .is_ok_and(|number| (1..=24).contains(&number)) =>
                {
                    key = Some(Key::Function(function[1..].parse().map_err(|_| ())?))
                }
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
    pub has_video_frame: bool,
    pub image_transition: bool,
    pub playback_blocked: bool,
    pub timeline_open: bool,
    pub has_time_selection: bool,
    pub media_kind: Option<MediaKind>,
    pub palette_open: bool,
    pub filmstrip_open: bool,
    pub reading_mode: bool,
    pub has_unsaved_edits: bool,
    pub source_deleted: bool,
    pub source_untitled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandDefinition {
    pub id: CommandId,
    pub title: &'static str,
    pub media_kinds: &'static [MediaKind],
    pub requires_reading_mode: bool,
}

impl CommandDefinition {
    pub const fn title_in(self, language: crate::localization::Language) -> &'static str {
        crate::localization::command_text(self.id).in_language(language)
    }

    pub fn is_enabled(self, context: CommandContext) -> bool {
        if context.source_untitled
            && matches!(
                self.id,
                CommandId::CopyFilePath
                    | CommandId::RevealFile
                    | CommandId::RenameFile
                    | CommandId::MoveFile
                    | CommandId::DeleteFile
                    | CommandId::ToggleFilmstrip
                    | CommandId::ReloadFolderOrder
                    | CommandId::GoToFile
                    | CommandId::ToggleReadingMode
                    | CommandId::PreviousMedia
                    | CommandId::NextMedia
                    | CommandId::PreviousSameKind
                    | CommandId::NextSameKind
                    | CommandId::FolderNavigationStop
                    | CommandId::FolderNavigationLoop
                    | CommandId::PreviousImage
                    | CommandId::NextImage
                    | CommandId::FirstImage
                    | CommandId::LastImage
                    | CommandId::JumpImagesBackward1
                    | CommandId::JumpImagesBackward2
                    | CommandId::JumpImagesBackward3
                    | CommandId::JumpImagesBackward4
                    | CommandId::JumpImagesBackward5
                    | CommandId::JumpImagesBackward6
                    | CommandId::JumpImagesBackward7
                    | CommandId::JumpImagesBackward8
                    | CommandId::JumpImagesBackward9
                    | CommandId::JumpImagesBackward10
                    | CommandId::JumpImagesForward1
                    | CommandId::JumpImagesForward2
                    | CommandId::JumpImagesForward3
                    | CommandId::JumpImagesForward4
                    | CommandId::JumpImagesForward5
                    | CommandId::JumpImagesForward6
                    | CommandId::JumpImagesForward7
                    | CommandId::JumpImagesForward8
                    | CommandId::JumpImagesForward9
                    | CommandId::JumpImagesForward10
            )
        {
            return false;
        }

        if self.id == CommandId::EditSpeed
            && (!context.timeline_open || context.playback_blocked || context.source_deleted)
        {
            return false;
        }
        if self.id == CommandId::ExportFrame && !context.has_video_frame {
            return false;
        }
        if (matches!(self.id, CommandId::TogglePause | CommandId::CycleAudioTrack)
            || self.id.video_seek_percent().is_some())
            && context.playback_blocked
        {
            return false;
        }
        if context.image_transition
            && matches!(
                self.id,
                CommandId::Undo
                    | CommandId::Redo
                    | CommandId::ApplyCrop
                    | CommandId::RotateClockwise
                    | CommandId::RotateCounterclockwise
                    | CommandId::RotateFineClockwise
                    | CommandId::RotateFineCounterclockwise
                    | CommandId::FlipHorizontal
                    | CommandId::FlipVertical
                    | CommandId::ResizeImage
                    | CommandId::FreeRotateImage
                    | CommandId::DeleteFile
                    | CommandId::RenameFile
                    | CommandId::MoveFile
                    | CommandId::Save
                    | CommandId::ExportAs
                    | CommandId::MetadataExportOptions
                    | CommandId::CopyImage
                    | CommandId::CopyFilePath
                    | CommandId::RevealFile
                    | CommandId::ZoomIn
                    | CommandId::ZoomOut
                    | CommandId::ActualSize
                    | CommandId::FitToWindow
                    | CommandId::CoverWindow
                    | CommandId::ZoomSelection
                    | CommandId::SelectAll
                    | CommandId::ClearSelection
                    | CommandId::SelectAspectSquare
                    | CommandId::SelectAspectFourThree
                    | CommandId::SelectAspectThreeFour
                    | CommandId::SelectAspectThreeTwo
                    | CommandId::SelectAspectTwoThree
                    | CommandId::SelectAspectSixteenNine
                    | CommandId::SelectAspectNineSixteen
                    | CommandId::ToggleReadingMode
                    | CommandId::ToggleImageInterpolation
                    | CommandId::ToggleImageMinification
            )
        {
            return false;
        }
        let image_reading = context.media_kind == Some(MediaKind::Image) && context.reading_mode;
        if image_reading
            && matches!(
                self.id,
                CommandId::FolderNavigationStop | CommandId::FolderNavigationLoop
            )
        {
            return false;
        }
        (!self.requires_reading_mode || context.reading_mode)
            && (context.media_kind != Some(MediaKind::Audio)
                || context.timeline_open
                || self.id != CommandId::SelectAll)
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
                        | CommandId::RotateFineClockwise
                        | CommandId::RotateFineCounterclockwise
                        | CommandId::FlipHorizontal
                        | CommandId::FlipVertical
                        | CommandId::FreeRotateVideo
                        | CommandId::ResizeVideo
                        | CommandId::ZoomSelection
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
            && (!context.source_deleted
                || !matches!(
                    self.id,
                    CommandId::DeleteFile | CommandId::RenameFile | CommandId::MoveFile
                ))
            && (self.id != CommandId::DeleteFile || !context.timeline_open)
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
                        | CommandId::ZoomSelection
                        | CommandId::ApplyCrop
                        | CommandId::RotateClockwise
                        | CommandId::RotateCounterclockwise
                        | CommandId::RotateFineClockwise
                        | CommandId::RotateFineCounterclockwise
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
    command(CommandId::OpenFile, &[]),
    command(CommandId::PasteImage, &[]),
    command(CommandId::ToggleFullscreen, &[]),
    command(CommandId::OpenFolder, &[]),
    command(CommandId::OpenGallery, &[]),
    command(CommandId::GoToFile, &[]),
    command(CommandId::OpenRecentFolder, &[]),
    command(CommandId::ShowLicenses, &[]),
    command(CommandId::About, &[]),
    command(CommandId::CheckForUpdates, &[]),
    command(CommandId::CloseTab, &[]),
    command(CommandId::NextTab, &[]),
    command(CommandId::PreviousTab, &[]),
    command(CommandId::TogglePause, PLAYABLE_MEDIA),
    command(CommandId::PlayTimeSelection, PLAYABLE_MEDIA),
    command(CommandId::CycleAudioRepeat, &[MediaKind::Audio]),
    command(CommandId::CycleAudioTrack, &[MediaKind::Video]),
    command(CommandId::ToggleVideoRepeat, &[MediaKind::Video]),
    command(CommandId::ToggleAudioShuffle, &[MediaKind::Audio]),
    command(CommandId::SeekBackward, PLAYABLE_MEDIA),
    command(CommandId::SeekForward, PLAYABLE_MEDIA),
    command(CommandId::SeekVideo0, &[MediaKind::Video]),
    command(CommandId::SeekVideo10, &[MediaKind::Video]),
    command(CommandId::SeekVideo20, &[MediaKind::Video]),
    command(CommandId::SeekVideo30, &[MediaKind::Video]),
    command(CommandId::SeekVideo40, &[MediaKind::Video]),
    command(CommandId::SeekVideo50, &[MediaKind::Video]),
    command(CommandId::SeekVideo60, &[MediaKind::Video]),
    command(CommandId::SeekVideo70, &[MediaKind::Video]),
    command(CommandId::SeekVideo80, &[MediaKind::Video]),
    command(CommandId::SeekVideo90, &[MediaKind::Video]),
    command(CommandId::PreviousMedia, ANY_MEDIA),
    command(CommandId::NextMedia, ANY_MEDIA),
    command(CommandId::PreviousSameKind, ANY_MEDIA),
    command(CommandId::NextSameKind, ANY_MEDIA),
    command(CommandId::ToggleFilmstrip, ANY_MEDIA),
    command(CommandId::ToggleCommandPalette, &[]),
    command(CommandId::OpenKeyboardSettings, &[]),
    command(CommandId::ReloadShortcuts, &[]),
    command(CommandId::ZoomIn, VISUAL_MEDIA),
    command(CommandId::ZoomOut, VISUAL_MEDIA),
    command(CommandId::ActualSize, VISUAL_MEDIA),
    command(CommandId::FitToWindow, VISUAL_MEDIA),
    command(CommandId::CoverWindow, VISUAL_MEDIA),
    command(CommandId::SelectAll, ANY_MEDIA),
    command(CommandId::ClearSelection, ANY_MEDIA),
    command(CommandId::ZoomSelection, VISUAL_MEDIA),
    command(CommandId::ToggleReadingMode, &[MediaKind::Image]),
    reading_command(CommandId::ReadingLeft, &[MediaKind::Image]),
    reading_command(CommandId::ReadingRight, &[MediaKind::Image]),
    reading_command(CommandId::IncreaseReadingPages, &[MediaKind::Image]),
    reading_command(CommandId::DecreaseReadingPages, &[MediaKind::Image]),
    reading_command(CommandId::ToggleReadingAxis, &[MediaKind::Image]),
    reading_command(CommandId::ReverseReadingOrder, &[MediaKind::Image]),
    command(CommandId::ReloadFolderOrder, ANY_MEDIA),
    command(CommandId::Undo, ANY_MEDIA),
    command(CommandId::Redo, ANY_MEDIA),
    command(CommandId::ApplyCrop, &[MediaKind::Image, MediaKind::Video]),
    command(
        CommandId::RotateClockwise,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::RotateCounterclockwise,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(CommandId::RotateFineClockwise, VISUAL_MEDIA),
    command(CommandId::RotateFineCounterclockwise, VISUAL_MEDIA),
    command(
        CommandId::FlipHorizontal,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::FlipVertical,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(CommandId::SetTrimStart, PLAYABLE_MEDIA),
    command(CommandId::SetTrimEnd, PLAYABLE_MEDIA),
    command(CommandId::DeleteTimeSelection, PLAYABLE_MEDIA),
    command(CommandId::KeepTimeSelection, PLAYABLE_MEDIA),
    command(CommandId::VolumeDown, PLAYABLE_MEDIA),
    command(CommandId::VolumeUp, PLAYABLE_MEDIA),
    command(CommandId::ToggleMute, PLAYABLE_MEDIA),
    command(CommandId::CycleVolumeStep, &[]),
    command(CommandId::VolumeStepTwo, &[]),
    command(CommandId::VolumeStepFive, &[]),
    command(CommandId::VolumeStepTen, &[]),
    command(CommandId::AudioRepeatOff, &[MediaKind::Audio]),
    command(CommandId::AudioRepeatAll, &[MediaKind::Audio]),
    command(CommandId::AudioRepeatOne, &[MediaKind::Audio]),
    command(CommandId::FolderNavigationStop, VISUAL_MEDIA),
    command(CommandId::FolderNavigationLoop, VISUAL_MEDIA),
    command(CommandId::RateDown, PLAYABLE_MEDIA),
    command(CommandId::RateUp, PLAYABLE_MEDIA),
    command(CommandId::ResetRate, PLAYABLE_MEDIA),
    command(CommandId::EditSpeed, PLAYABLE_MEDIA),
    command(CommandId::DeleteFile, ANY_MEDIA),
    command(CommandId::RenameFile, ANY_MEDIA),
    command(CommandId::MoveFile, ANY_MEDIA),
    command(CommandId::Save, ANY_MEDIA),
    command(CommandId::ExportAs, ANY_MEDIA),
    command(CommandId::AudioExportOptions, PLAYABLE_MEDIA),
    command(CommandId::MetadataExportOptions, ANY_MEDIA),
    command(CommandId::ExportAudio, &[MediaKind::Video]),
    command(CommandId::ExportFrame, &[MediaKind::Video]),
    command(CommandId::ToggleTimeline, PLAYABLE_MEDIA),
    command(CommandId::ToggleGridMenu, ANY_MEDIA),
    command(CommandId::ExportQualityHigh, &[MediaKind::Video]),
    command(CommandId::ExportQualityBalanced, &[MediaKind::Video]),
    command(CommandId::ExportQualitySmaller, &[MediaKind::Video]),
    command(CommandId::ToggleHardwareEncode, &[MediaKind::Video]),
    command(CommandId::PreviousImage, &[MediaKind::Image]),
    command(CommandId::NextImage, &[MediaKind::Image]),
    command(CommandId::FirstImage, &[MediaKind::Image]),
    command(CommandId::LastImage, &[MediaKind::Image]),
    reading_command(CommandId::IncreaseReadingFirstPage, &[MediaKind::Image]),
    reading_command(CommandId::DecreaseReadingFirstPage, &[MediaKind::Image]),
    command(CommandId::CloseOtherTabs, ANY_MEDIA),
    command(CommandId::CloseTabsLeft, ANY_MEDIA),
    command(CommandId::CloseTabsRight, ANY_MEDIA),
    command(CommandId::CloseAllTabs, ANY_MEDIA),
    command(CommandId::ReopenClosedTab, &[]),
    command(CommandId::CopyFilePath, ANY_MEDIA),
    command(CommandId::RevealFile, ANY_MEDIA),
    command(CommandId::CopyImage, &[MediaKind::Image]),
    command(CommandId::ResizeImage, &[MediaKind::Image]),
    command(CommandId::ToggleImageInterpolation, &[MediaKind::Image]),
    command(CommandId::ToggleImageMinification, &[MediaKind::Image]),
    command(CommandId::PreviousVideoFrame, &[MediaKind::Video]),
    command(CommandId::NextVideoFrame, &[MediaKind::Video]),
    command(CommandId::JumpImagesBackward1, &[MediaKind::Image]),
    command(CommandId::JumpImagesBackward2, &[MediaKind::Image]),
    command(CommandId::JumpImagesBackward3, &[MediaKind::Image]),
    command(CommandId::JumpImagesBackward4, &[MediaKind::Image]),
    command(CommandId::JumpImagesBackward5, &[MediaKind::Image]),
    command(CommandId::JumpImagesBackward6, &[MediaKind::Image]),
    command(CommandId::JumpImagesBackward7, &[MediaKind::Image]),
    command(CommandId::JumpImagesBackward8, &[MediaKind::Image]),
    command(CommandId::JumpImagesBackward9, &[MediaKind::Image]),
    command(CommandId::JumpImagesBackward10, &[MediaKind::Image]),
    command(CommandId::JumpImagesForward1, &[MediaKind::Image]),
    command(CommandId::JumpImagesForward2, &[MediaKind::Image]),
    command(CommandId::JumpImagesForward3, &[MediaKind::Image]),
    command(CommandId::JumpImagesForward4, &[MediaKind::Image]),
    command(CommandId::JumpImagesForward5, &[MediaKind::Image]),
    command(CommandId::JumpImagesForward6, &[MediaKind::Image]),
    command(CommandId::JumpImagesForward7, &[MediaKind::Image]),
    command(CommandId::JumpImagesForward8, &[MediaKind::Image]),
    command(CommandId::JumpImagesForward9, &[MediaKind::Image]),
    command(CommandId::JumpImagesForward10, &[MediaKind::Image]),
    command(
        CommandId::SelectAspectSquare,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectFourThree,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectThreeFour,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectThreeTwo,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectTwoThree,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectSixteenNine,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(
        CommandId::SelectAspectNineSixteen,
        &[MediaKind::Image, MediaKind::Video],
    ),
    command(CommandId::FreeRotateImage, &[MediaKind::Image]),
    command(CommandId::FreeRotateVideo, &[MediaKind::Video]),
    command(CommandId::ResizeVideo, &[MediaKind::Video]),
    command(CommandId::StepAudioBackward, &[MediaKind::Audio]),
    command(CommandId::StepAudioForward, &[MediaKind::Audio]),
];

const fn command(id: CommandId, media_kinds: &'static [MediaKind]) -> CommandDefinition {
    CommandDefinition {
        id,
        title: crate::localization::command_text(id)
            .in_language(crate::localization::Language::English),
        media_kinds,
        requires_reading_mode: false,
    }
}

const fn reading_command(id: CommandId, media_kinds: &'static [MediaKind]) -> CommandDefinition {
    CommandDefinition {
        id,
        title: crate::localization::command_text(id)
            .in_language(crate::localization::Language::English),
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
    fn audio_timeline_toggle_is_available_while_time_edits_require_editing_mode() {
        let enabled = |id, kind, timeline_open| {
            command_definitions()
                .iter()
                .find(|definition| definition.id == id)
                .expect("command")
                .is_enabled(CommandContext {
                    media_kind: Some(kind),
                    timeline_open,
                    has_time_selection: true,
                    ..Default::default()
                })
        };
        for kind in [MediaKind::Image, MediaKind::Audio, MediaKind::Video] {
            for timeline in [false, true] {
                assert_eq!(
                    enabled(CommandId::ToggleTimeline, kind, timeline),
                    kind != MediaKind::Image
                );
                for command in [
                    CommandId::SetTrimStart,
                    CommandId::SetTrimEnd,
                    CommandId::DeleteTimeSelection,
                    CommandId::PlayTimeSelection,
                ] {
                    assert_eq!(
                        enabled(command, kind, timeline),
                        kind != MediaKind::Image && timeline
                    );
                }
                if kind == MediaKind::Audio {
                    assert_eq!(enabled(CommandId::SelectAll, kind, timeline), timeline);
                    assert!(enabled(CommandId::TogglePause, kind, timeline));
                    assert!(enabled(CommandId::StepAudioForward, kind, timeline));
                }
            }
        }
    }

    #[test]
    fn explicit_choice_commands_round_trip_and_follow_media_and_reading_guards() {
        use CommandId::*;
        for id in [
            VolumeStepTwo,
            VolumeStepFive,
            VolumeStepTen,
            AudioRepeatOff,
            AudioRepeatAll,
            AudioRepeatOne,
            FolderNavigationStop,
            FolderNavigationLoop,
        ] {
            assert_eq!(id.as_str().parse::<CommandId>(), Ok(id));
            let definition = command_definitions()
                .iter()
                .find(|definition| definition.id == id)
                .expect("choice definition");
            for media_kind in [
                None,
                Some(MediaKind::Image),
                Some(MediaKind::Video),
                Some(MediaKind::Audio),
            ] {
                for reading_mode in [false, true] {
                    let expected = match id {
                        VolumeStepTwo | VolumeStepFive | VolumeStepTen => true,
                        AudioRepeatOff | AudioRepeatAll | AudioRepeatOne => {
                            media_kind == Some(MediaKind::Audio)
                        }
                        _ => {
                            media_kind == Some(MediaKind::Video)
                                || (media_kind == Some(MediaKind::Image) && !reading_mode)
                        }
                    };
                    assert_eq!(
                        definition.is_enabled(CommandContext {
                            media_kind,
                            reading_mode,
                            ..Default::default()
                        }),
                        expected
                    );
                }
            }
        }
    }

    #[test]
    fn fine_rotations_follow_visual_media_reading_transition_and_timeline_guards() {
        for id in [
            CommandId::RotateFineClockwise,
            CommandId::RotateFineCounterclockwise,
        ] {
            let definition = command_definitions()
                .iter()
                .find(|definition| definition.id == id)
                .expect("command");
            for media_kind in [
                None,
                Some(MediaKind::Image),
                Some(MediaKind::Video),
                Some(MediaKind::Audio),
            ] {
                for reading_mode in [false, true] {
                    for timeline_open in [false, true] {
                        for image_transition in [false, true] {
                            let context = CommandContext {
                                media_kind,
                                reading_mode,
                                timeline_open,
                                image_transition,
                                ..Default::default()
                            };
                            assert_eq!(
                                definition.is_enabled(context),
                                !image_transition
                                    && match media_kind {
                                        Some(MediaKind::Image) => !reading_mode,
                                        Some(MediaKind::Video) => timeline_open,
                                        _ => false,
                                    },
                                "{id:?}: {context:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn frame_export_requires_a_video_picture_but_not_the_editing_timeline() {
        let definition = command_definitions()
            .iter()
            .find(|command| command.id == CommandId::ExportFrame)
            .expect("frame command");
        assert_eq!(
            CommandId::from_str("export_frame"),
            Ok(CommandId::ExportFrame)
        );
        for kind in [
            None,
            Some(MediaKind::Image),
            Some(MediaKind::Audio),
            Some(MediaKind::Video),
        ] {
            for has_video_frame in [false, true] {
                for timeline_open in [false, true] {
                    assert_eq!(
                        definition.is_enabled(CommandContext {
                            media_kind: kind,
                            has_video_frame,
                            timeline_open,
                            ..Default::default()
                        }),
                        kind == Some(MediaKind::Video) && has_video_frame
                    );
                }
            }
        }
    }

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
                    CommandId::ZoomIn,
                    CommandId::ZoomOut,
                    CommandId::ActualSize,
                    CommandId::FitToWindow,
                    CommandId::CoverWindow,
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
        for kind in [MediaKind::Audio, MediaKind::Video] {
            assert!(!pause.is_enabled(CommandContext {
                media_kind: Some(kind),
                playback_blocked: true,
                ..CommandContext::default()
            }));
        }
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
            CommandId::ZoomSelection,
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
            CommandId::ZoomIn,
            CommandId::ZoomOut,
            CommandId::ActualSize,
            CommandId::FitToWindow,
            CommandId::CoverWindow,
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
        assert_eq!(fullscreen.strokes()[1].key, Key::Function(11));
        assert_eq!("toggle_fullscreen".parse(), Ok(CommandId::ToggleFullscreen));
        for key in ["Enter", "Ctrl+Enter", "Ctrl+K Enter"] {
            assert_eq!(
                key.parse::<KeySequence>()
                    .expect("Enter shortcut")
                    .to_string(),
                key
            );
        }
    }

    fn sequence(character: char) -> KeySequence {
        KeySequence::new([KeyStroke {
            modifiers: Modifiers::default(),
            key: Key::Character(character),
        }])
        .expect("one stroke is valid")
    }
}
