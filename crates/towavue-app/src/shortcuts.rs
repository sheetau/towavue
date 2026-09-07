use std::fs;
use std::path::PathBuf;

use towavue_core::{CommandId, KeySequence, ShortcutBindings};

pub fn load() -> Result<(ShortcutBindings, PathBuf), String> {
    let path = config_path()?;
    let defaults = defaults();
    if !path.exists() {
        let parent = path.parent().ok_or("shortcut path has no parent")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        fs::write(&path, serialize(&defaults)).map_err(|error| error.to_string())?;
        return Ok((defaults, path));
    }
    let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    parse(&text, defaults).map(|bindings| (bindings, path))
}

fn config_path() -> Result<PathBuf, String> {
    let app_data = std::env::var_os("APPDATA").ok_or("APPDATA is unavailable")?;
    Ok(PathBuf::from(app_data)
        .join("towavue")
        .join("shortcuts.conf"))
}

fn defaults() -> ShortcutBindings {
    let mut bindings = ShortcutBindings::default();
    for (command, shortcut) in [
        (CommandId::OpenFile, "Ctrl+O"),
        (CommandId::ToggleFullscreen, "F11"),
        (CommandId::OpenFolder, "Ctrl+Shift+O"),
        (CommandId::CloseTab, "Ctrl+W"),
        (CommandId::NextTab, "Ctrl+Tab"),
        (CommandId::PreviousTab, "Ctrl+Shift+Tab"),
        (CommandId::TogglePause, "Space"),
        (CommandId::SeekBackward, "Left"),
        (CommandId::SeekForward, "Right"),
        (CommandId::PreviousImage, "Left"),
        (CommandId::NextImage, "Right"),
        (CommandId::FirstImage, "Home"),
        (CommandId::LastImage, "End"),
        (CommandId::PreviousSameKind, "Ctrl+Left"),
        (CommandId::NextSameKind, "Ctrl+Right"),
        (CommandId::PreviousMedia, "Alt+Left"),
        (CommandId::NextMedia, "Alt+Right"),
        (CommandId::ToggleFilmstrip, "F"),
        (CommandId::ToggleCommandPalette, "Ctrl+Shift+P"),
        (CommandId::ReloadShortcuts, "Ctrl+K Ctrl+S"),
        (CommandId::ZoomIn, "Plus"),
        (CommandId::ZoomOut, "Minus"),
        (CommandId::ActualSize, "Ctrl+H"),
        (CommandId::FitToWindow, "Shift+W"),
        (CommandId::ClearSelection, "Escape"),
        (CommandId::SelectAll, "Ctrl+A"),
        (CommandId::ToggleCropPreview, "Ctrl+Shift+Y"),
        (CommandId::ToggleReadingMode, "B"),
        (CommandId::IncreaseReadingPages, "Ctrl+]"),
        (CommandId::DecreaseReadingPages, "Ctrl+["),
        (CommandId::ToggleReadingAxis, "R"),
        (CommandId::ReverseReadingOrder, "H"),
        (CommandId::Undo, "Ctrl+Z"),
        (CommandId::Redo, "Ctrl+Shift+Z"),
        (CommandId::ApplyCrop, "Ctrl+Y"),
        (CommandId::RotateClockwise, "R"),
        (CommandId::RotateCounterclockwise, "L"),
        (CommandId::FlipHorizontal, "H"),
        (CommandId::FlipVertical, "V"),
        (CommandId::SetTrimStart, "I"),
        (CommandId::SetTrimEnd, "O"),
        (CommandId::VolumeDown, "Down"),
        (CommandId::VolumeUp, "Up"),
        (CommandId::ToggleMute, "M"),
        (CommandId::RateDown, ","),
        (CommandId::RateUp, "."),
        (CommandId::ResetRate, "/"),
        (CommandId::Save, "Ctrl+S"),
        (CommandId::ExportAs, "Ctrl+Shift+S"),
        (CommandId::ToggleTimeline, "T"),
        (CommandId::ToggleGridMenu, "G"),
        (CommandId::ToggleHardwareEncode, "Ctrl+Shift+E"),
    ] {
        bindings.set(
            command,
            shortcut.parse().expect("built-in shortcut is valid"),
        );
    }
    bindings
}

fn parse(text: &str, mut bindings: ShortcutBindings) -> Result<ShortcutBindings, String> {
    let has_apply_crop = text
        .lines()
        .any(|line| line.trim_start().starts_with("apply_crop"));
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((command, sequence)) = line.split_once('=') else {
            return Err(format!("shortcuts.conf line {} is missing '='", index + 1));
        };
        let command = command
            .trim()
            .parse::<CommandId>()
            .map_err(|_| format!("unknown command on shortcuts.conf line {}", index + 1))?;
        let sequence = sequence
            .trim()
            .parse::<KeySequence>()
            .map_err(|_| format!("invalid shortcut on shortcuts.conf line {}", index + 1))?;
        bindings.set(command, sequence);
    }
    if !has_apply_crop
        && bindings
            .get(CommandId::ToggleCropPreview)
            .is_some_and(|sequence| sequence.to_string() == "Ctrl+Y")
    {
        bindings.set(
            CommandId::ToggleCropPreview,
            "Ctrl+Shift+Y".parse().expect("migration shortcut is valid"),
        );
        bindings.set(
            CommandId::ApplyCrop,
            "Ctrl+Y".parse().expect("migration shortcut is valid"),
        );
    }
    Ok(bindings)
}

fn serialize(bindings: &ShortcutBindings) -> String {
    let mut output = String::from(
        "# One command per line. Separate prefix chords with a space.\n# Example: reload_shortcuts = Ctrl+K Ctrl+S\n",
    );
    for (command, sequence) in bindings.iter() {
        output.push_str(command.as_str());
        output.push_str(" = ");
        output.push_str(&sequence.to_string());
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_core::{CommandContext, MediaKind, ShortcutMatch};

    #[test]
    fn select_all_is_visual_only_and_keeps_custom_bindings() {
        let mut bindings = defaults();
        let select = "Ctrl+A".parse::<KeySequence>().expect("select all");
        for (kind, reading, enabled) in [
            (Some(MediaKind::Image), false, true),
            (Some(MediaKind::Video), false, true),
            (Some(MediaKind::Audio), false, false),
            (Some(MediaKind::Image), true, false),
            (None, false, false),
        ] {
            assert_eq!(
                bindings.resolve(
                    select.strokes(),
                    CommandContext {
                        media_kind: kind,
                        reading_mode: reading,
                        ..Default::default()
                    }
                ),
                if enabled {
                    ShortcutMatch::Command(CommandId::SelectAll)
                } else {
                    ShortcutMatch::None
                }
            );
        }
        bindings.set(
            CommandId::SelectAll,
            "Ctrl+K A".parse().expect("custom selection"),
        );
        let context = CommandContext {
            media_kind: Some(MediaKind::Image),
            ..Default::default()
        };
        assert_eq!(
            bindings.resolve(select.strokes(), context),
            ShortcutMatch::None
        );
        assert_eq!(
            bindings.resolve(
                "Ctrl+K A"
                    .parse::<KeySequence>()
                    .expect("custom selection")
                    .strokes(),
                context
            ),
            ShortcutMatch::Command(CommandId::SelectAll)
        );
    }

    #[test]
    fn image_boundary_shortcuts_round_trip_and_preserve_custom_bindings() {
        let bindings = parse("toggle_pause = P\n", defaults()).expect("old configuration");
        for (key, command) in [
            ("Home", CommandId::FirstImage),
            ("End", CommandId::LastImage),
        ] {
            let sequence: KeySequence = key.parse().expect("boundary key");
            assert_eq!(sequence.to_string(), key);
            assert_eq!(command.as_str().parse(), Ok(command));
            for reading_mode in [false, true] {
                for media_kind in [
                    None,
                    Some(MediaKind::Image),
                    Some(MediaKind::Video),
                    Some(MediaKind::Audio),
                ] {
                    let context = CommandContext {
                        media_kind,
                        reading_mode,
                        ..Default::default()
                    };
                    assert_eq!(
                        bindings.resolve(sequence.strokes(), context),
                        if media_kind == Some(MediaKind::Image) {
                            ShortcutMatch::Command(command)
                        } else {
                            ShortcutMatch::None
                        }
                    );
                }
            }
        }
        let bindings = parse(
            "first_image = Ctrl+K Home\nlast_image = Ctrl+End\n",
            defaults(),
        )
        .expect("custom boundaries");
        let restored = parse(&serialize(&bindings), defaults()).expect("round trip");
        let context = CommandContext {
            media_kind: Some(MediaKind::Image),
            ..Default::default()
        };
        for (key, expected) in [
            ("Home", ShortcutMatch::None),
            ("End", ShortcutMatch::None),
            ("Ctrl+K Home", ShortcutMatch::Command(CommandId::FirstImage)),
            ("Ctrl+End", ShortcutMatch::Command(CommandId::LastImage)),
        ] {
            let sequence: KeySequence = key.parse().expect("custom key");
            assert_eq!(restored.resolve(sequence.strokes(), context), expected);
        }
    }

    #[test]
    fn arrow_defaults_navigate_images_and_seek_playable_media() {
        let bindings = parse("toggle_pause = P\n", defaults()).expect("old configuration");
        for reading_mode in [false, true] {
            for (media_kind, commands) in [
                (None, [None, None]),
                (
                    Some(MediaKind::Image),
                    [Some(CommandId::PreviousImage), Some(CommandId::NextImage)],
                ),
                (
                    Some(MediaKind::Video),
                    [Some(CommandId::SeekBackward), Some(CommandId::SeekForward)],
                ),
                (
                    Some(MediaKind::Audio),
                    [Some(CommandId::SeekBackward), Some(CommandId::SeekForward)],
                ),
            ] {
                let context = CommandContext {
                    media_kind,
                    reading_mode,
                    ..CommandContext::default()
                };
                for (key, command) in ["Left", "Right"].into_iter().zip(commands) {
                    let sequence: KeySequence = key.parse().expect("arrow key");
                    assert_eq!(
                        bindings.resolve(sequence.strokes(), context),
                        command.map_or(ShortcutMatch::None, ShortcutMatch::Command),
                        "{key} in {context:?}"
                    );
                }
                if media_kind.is_some() {
                    for (key, command) in [
                        ("Ctrl+Left", CommandId::PreviousSameKind),
                        ("Ctrl+Right", CommandId::NextSameKind),
                    ] {
                        let sequence: KeySequence = key.parse().expect("modified arrow");
                        assert_eq!(
                            bindings.resolve(sequence.strokes(), context),
                            ShortcutMatch::Command(command)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn image_arrows_are_configurable_and_preserve_existing_custom_bindings() {
        let context = CommandContext {
            media_kind: Some(MediaKind::Image),
            ..CommandContext::default()
        };
        let custom = parse("previous_image = Ctrl+K A\nnext_image = D\n", defaults())
            .expect("custom image bindings");
        for key in ["Left", "Right"] {
            let sequence: KeySequence = key.parse().expect("arrow key");
            assert_eq!(
                custom.resolve(sequence.strokes(), context),
                ShortcutMatch::None
            );
        }
        for (key, expected) in [
            ("Ctrl+K", ShortcutMatch::Prefix),
            ("Ctrl+K A", ShortcutMatch::Command(CommandId::PreviousImage)),
            ("D", ShortcutMatch::Command(CommandId::NextImage)),
        ] {
            let sequence: KeySequence = key.parse().expect("custom key");
            assert_eq!(custom.resolve(sequence.strokes(), context), expected);
        }
        let existing = parse("flip_vertical = Right\n", defaults()).expect("existing custom key");
        let right: KeySequence = "Right".parse().expect("right key");
        assert_eq!(
            existing.resolve(right.strokes(), context),
            ShortcutMatch::Command(CommandId::FlipVertical)
        );
    }

    #[test]
    fn generated_defaults_can_be_loaded_again_without_changing_any_binding() {
        let bindings = defaults();
        let reloaded = parse(&serialize(&bindings), defaults()).expect("reload generated defaults");
        for (command, sequence) in bindings.iter() {
            assert_eq!(reloaded.get(command), Some(sequence));
        }
        let legacy = serialize(&bindings).replace("zoom_in = Plus", "zoom_in = +");
        let reloaded = parse(&legacy, defaults()).expect("reload old generated configuration");
        assert_eq!(
            reloaded.get(CommandId::ZoomIn),
            bindings.get(CommandId::ZoomIn)
        );
    }

    #[test]
    fn fullscreen_defaults_survive_old_configuration_and_can_be_overridden() {
        let bindings = parse("toggle_pause = P\n", defaults()).expect("old configuration");
        assert_eq!(
            bindings
                .get(CommandId::ToggleFullscreen)
                .expect("new default")
                .to_string(),
            "F11"
        );
        let custom =
            parse("toggle_fullscreen = Ctrl+K F11\n", defaults()).expect("fullscreen binding");
        let sequence = custom
            .get(CommandId::ToggleFullscreen)
            .expect("custom binding");
        assert_eq!(sequence.to_string(), "Ctrl+K F11");
        assert_eq!(
            custom.resolve(sequence.strokes(), towavue_core::CommandContext::default()),
            towavue_core::ShortcutMatch::Command(CommandId::ToggleFullscreen)
        );
    }

    #[test]
    fn custom_file_overrides_defaults_and_accepts_prefixes() {
        let bindings = parse(
            "toggle_pause = P\nreload_shortcuts = Ctrl+K Ctrl+R\n",
            defaults(),
        )
        .expect("parse shortcuts");

        assert_eq!(
            bindings
                .get(CommandId::TogglePause)
                .expect("pause binding")
                .to_string(),
            "P"
        );
        assert_eq!(
            bindings
                .get(CommandId::ReloadShortcuts)
                .expect("reload binding")
                .to_string(),
            "Ctrl+K Ctrl+R"
        );
    }

    #[test]
    fn migrates_m5_crop_preview_binding_to_m6_apply_crop() {
        let bindings =
            parse("toggle_crop_preview = Ctrl+Y\n", defaults()).expect("migrate shortcuts");

        assert_eq!(
            bindings
                .get(CommandId::ApplyCrop)
                .expect("apply crop binding")
                .to_string(),
            "Ctrl+Y"
        );
        assert_eq!(
            bindings
                .get(CommandId::ToggleCropPreview)
                .expect("preview binding")
                .to_string(),
            "Ctrl+Shift+Y"
        );
    }
}
