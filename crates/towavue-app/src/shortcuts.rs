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
