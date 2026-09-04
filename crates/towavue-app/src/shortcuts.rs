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
        (CommandId::ToggleCropPreview, "Ctrl+Y"),
        (CommandId::ToggleReadingMode, "B"),
        (CommandId::IncreaseReadingPages, "Ctrl+]"),
        (CommandId::DecreaseReadingPages, "Ctrl+["),
        (CommandId::ToggleReadingAxis, "R"),
        (CommandId::ReverseReadingOrder, "H"),
    ] {
        bindings.set(
            command,
            shortcut.parse().expect("built-in shortcut is valid"),
        );
    }
    bindings
}

fn parse(text: &str, mut bindings: ShortcutBindings) -> Result<ShortcutBindings, String> {
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
}
