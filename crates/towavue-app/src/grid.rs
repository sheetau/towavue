use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use towavue_core::{CommandId, MediaKind};
use winit::keyboard::{KeyCode, PhysicalKey};

pub const KEYS: [char; 16] = [
    '1', '2', '3', '4', 'q', 'w', 'e', 'r', 'a', 's', 'd', 'f', 'z', 'x', 'c', 'v',
];

pub fn key_index(key: PhysicalKey) -> Option<usize> {
    let PhysicalKey::Code(key) = key else {
        return None;
    };
    [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::KeyQ,
        KeyCode::KeyW,
        KeyCode::KeyE,
        KeyCode::KeyR,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
        KeyCode::KeyF,
        KeyCode::KeyZ,
        KeyCode::KeyX,
        KeyCode::KeyC,
        KeyCode::KeyV,
    ]
    .iter()
    .position(|candidate| *candidate == key)
}

#[derive(Clone, Debug)]
pub struct GridLayouts {
    image: [CommandId; 16],
    video: [CommandId; 16],
    audio: [CommandId; 16],
}

impl GridLayouts {
    pub fn get(&self, kind: MediaKind) -> &[CommandId; 16] {
        match kind {
            MediaKind::Image => &self.image,
            MediaKind::Video => &self.video,
            MediaKind::Audio => &self.audio,
        }
    }
}

pub fn load() -> Result<(GridLayouts, PathBuf), String> {
    let path = config_path()?;
    load_from(&path).map(|layouts| (layouts, path))
}

pub fn load_from(path: &Path) -> Result<GridLayouts, String> {
    let defaults = defaults();
    if !path.exists() {
        let parent = path.parent().ok_or("grid path has no parent")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .and_then(|mut file| file.write_all(serialize(&defaults).as_bytes()))
            .map_err(|error| error.to_string())?;
        return Ok(defaults);
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    parse(&text, defaults)
}

pub fn config_path() -> Result<PathBuf, String> {
    let app_data = std::env::var_os("APPDATA").ok_or("APPDATA is unavailable")?;
    Ok(PathBuf::from(app_data).join("towavue").join("grid.conf"))
}

pub fn defaults() -> GridLayouts {
    GridLayouts {
        image: [
            CommandId::PreviousMedia,
            CommandId::NextMedia,
            CommandId::PreviousSameKind,
            CommandId::NextSameKind,
            CommandId::ZoomOut,
            CommandId::ZoomIn,
            CommandId::ActualSize,
            CommandId::FitToWindow,
            CommandId::RotateCounterclockwise,
            CommandId::RotateClockwise,
            CommandId::FlipHorizontal,
            CommandId::FlipVertical,
            CommandId::ToggleCropPreview,
            CommandId::ApplyCrop,
            CommandId::Undo,
            CommandId::Redo,
        ],
        video: [
            CommandId::SeekBackward,
            CommandId::SeekForward,
            CommandId::PreviousSameKind,
            CommandId::NextSameKind,
            CommandId::VolumeDown,
            CommandId::VolumeUp,
            CommandId::ToggleMute,
            CommandId::TogglePause,
            CommandId::SetTrimStart,
            CommandId::SetTrimEnd,
            CommandId::RateDown,
            CommandId::RateUp,
            CommandId::RotateCounterclockwise,
            CommandId::RotateClockwise,
            CommandId::Undo,
            CommandId::Redo,
        ],
        audio: [
            CommandId::SeekBackward,
            CommandId::SeekForward,
            CommandId::PreviousSameKind,
            CommandId::NextSameKind,
            CommandId::VolumeDown,
            CommandId::VolumeUp,
            CommandId::ToggleMute,
            CommandId::TogglePause,
            CommandId::SetTrimStart,
            CommandId::SetTrimEnd,
            CommandId::RateDown,
            CommandId::RateUp,
            CommandId::ResetRate,
            CommandId::ToggleFilmstrip,
            CommandId::Undo,
            CommandId::Redo,
        ],
    }
}

fn parse(text: &str, mut layouts: GridLayouts) -> Result<GridLayouts, String> {
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((kind, commands)) = line.split_once('=') else {
            return Err(format!("grid.conf line {} is missing '='", index + 1));
        };
        let parsed = commands
            .split(',')
            .map(|value| {
                value
                    .trim()
                    .parse::<CommandId>()
                    .map_err(|_| format!("unknown command on grid.conf line {}", index + 1))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let parsed: [CommandId; 16] = parsed.try_into().map_err(|values: Vec<_>| {
            format!(
                "grid.conf line {} has {} commands; expected 16",
                index + 1,
                values.len()
            )
        })?;
        match kind.trim() {
            "image" => layouts.image = parsed,
            "video" => layouts.video = parsed,
            "audio" => layouts.audio = parsed,
            _ => {
                return Err(format!(
                    "unknown media kind on grid.conf line {}",
                    index + 1
                ));
            }
        }
    }
    Ok(layouts)
}

fn serialize(layouts: &GridLayouts) -> String {
    let mut output = String::from(
        "# Commands follow the physical 1234/qwer/asdf/zxcv grid.\n# Each media kind must contain exactly 16 command IDs.\n",
    );
    for (kind, commands) in [
        ("image", &layouts.image),
        ("video", &layouts.video),
        ("audio", &layouts.audio),
    ] {
        output.push_str(kind);
        output.push_str(" = ");
        output.push_str(
            &commands
                .iter()
                .map(|command| command.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        );
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_positions_match_all_sixteen_cells_without_numpad_aliases() {
        let rows = [
            [
                KeyCode::Digit1,
                KeyCode::Digit2,
                KeyCode::Digit3,
                KeyCode::Digit4,
            ],
            [KeyCode::KeyQ, KeyCode::KeyW, KeyCode::KeyE, KeyCode::KeyR],
            [KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD, KeyCode::KeyF],
            [KeyCode::KeyZ, KeyCode::KeyX, KeyCode::KeyC, KeyCode::KeyV],
        ];
        for (index, key) in rows.into_iter().flatten().enumerate() {
            assert_eq!(key_index(PhysicalKey::Code(key)), Some(index));
        }
        for key in [
            KeyCode::Numpad1,
            KeyCode::Digit5,
            KeyCode::KeyG,
            KeyCode::Space,
        ] {
            assert_eq!(key_index(PhysicalKey::Code(key)), None);
        }
        assert_eq!(
            key_index(PhysicalKey::Unidentified(
                winit::keyboard::NativeKeyCode::Unidentified
            )),
            None
        );
    }

    #[test]
    fn parses_per_media_physical_grid() {
        let defaults = defaults();
        let commands = std::iter::repeat_n("toggle_pause", 16)
            .collect::<Vec<_>>()
            .join(",");
        let layouts = parse(&format!("video = {commands}"), defaults).expect("valid grid");

        assert_eq!(layouts.video, [CommandId::TogglePause; 16]);
        assert_eq!(KEYS[0], '1');
        assert_eq!(KEYS[15], 'v');
    }

    #[test]
    fn rejects_non_rectangular_grid() {
        let error = parse("image = zoom_in, zoom_out", defaults()).expect_err("invalid grid");

        assert!(error.contains("expected 16"));
    }
}
