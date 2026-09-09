use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use towavue_core::{CommandId, KeySequence, ShortcutBindings};

const MULTI_BINDING_HEADER: &str = "# towavue shortcuts v2";

pub fn load() -> Result<(ShortcutBindings, PathBuf), String> {
    let path = config_path()?;
    load_from(&path).map(|bindings| (bindings, path))
}

pub fn load_from(path: &Path) -> Result<ShortcutBindings, String> {
    let defaults = defaults();
    if !path.exists() {
        let parent = path.parent().ok_or("shortcut path has no parent")?;
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
    Ok(PathBuf::from(app_data)
        .join("towavue")
        .join("shortcuts.conf"))
}

pub fn defaults() -> ShortcutBindings {
    let mut bindings = ShortcutBindings::default();
    for (command, shortcut) in [
        (CommandId::OpenFile, "Ctrl+O"),
        (CommandId::ToggleFullscreen, "F11"),
        (CommandId::OpenFolder, "Ctrl+Shift+O"),
        (CommandId::CloseTab, "Ctrl+W"),
        (CommandId::ReopenClosedTab, "Ctrl+Shift+T"),
        (CommandId::NextTab, "Ctrl+Tab"),
        (CommandId::PreviousTab, "Ctrl+Shift+Tab"),
        (CommandId::TogglePause, "Space"),
        (CommandId::PlayTimeSelection, "Shift+Space"),
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
        (CommandId::CoverWindow, "Shift+C"),
        (CommandId::ClearSelection, "Escape"),
        (CommandId::SelectAll, "Ctrl+A"),
        (CommandId::DeleteTimeSelection, "Delete"),
        (CommandId::KeepTimeSelection, "Ctrl+Y"),
        (CommandId::ToggleCropPreview, "Ctrl+Shift+Y"),
        (CommandId::ToggleReadingMode, "B"),
        (CommandId::IncreaseReadingPages, "Ctrl+]"),
        (CommandId::DecreaseReadingPages, "Ctrl+["),
        (CommandId::IncreaseReadingFirstPage, "Ctrl+Shift+Right"),
        (CommandId::DecreaseReadingFirstPage, "Ctrl+Shift+Left"),
        (CommandId::ToggleReadingAxis, "R"),
        (CommandId::ReverseReadingOrder, "H"),
        (CommandId::Undo, "Ctrl+Z"),
        (CommandId::Redo, "Ctrl+Shift+Z"),
        (CommandId::ApplyCrop, "Ctrl+Y"),
        (CommandId::CopyImage, "Ctrl+C"),
        (CommandId::ResizeImage, "Ctrl+R"),
        (CommandId::CycleAudioRepeat, "Ctrl+R"),
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
    for (command, key) in [
        (CommandId::SeekBackward, "J"),
        (CommandId::TogglePause, "K"),
        (CommandId::SeekForward, "L"),
    ] {
        bindings.add(command, key.parse().expect("built-in alternative"));
    }
    bindings
}

fn parse(text: &str, mut bindings: ShortcutBindings) -> Result<ShortcutBindings, String> {
    let legacy = !text.lines().any(|line| line.trim() == MULTI_BINDING_HEADER);
    let standard = defaults();
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
        let parts = if legacy {
            vec![sequence]
        } else {
            sequence.split('|').collect()
        };
        let mut sequences = parts
            .into_iter()
            .map(|part| part.trim().parse::<KeySequence>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| format!("invalid shortcut on shortcuts.conf line {}", index + 1))?;
        // Old generated files listed every default. Preserve new alternatives
        // only for unchanged defaults; custom bindings remain exact replacements.
        let inherit = legacy
            && sequences.len() == 1
            && matches!(
                command,
                CommandId::SeekBackward | CommandId::SeekForward | CommandId::TogglePause
            )
            && standard.get(command) == sequences.first();
        if inherit {
            sequences = standard.all(command).to_vec();
        }
        bindings.set(command, sequences[0].clone());
        for sequence in sequences.into_iter().skip(1) {
            bindings.add(command, sequence);
        }
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
    let mut output = format!(
        "{MULTI_BINDING_HEADER}\n# Separate alternatives with | and prefix chords with a space.\n# The first binding has priority over alternatives.\n# Example: seek_forward = Right | L\n"
    );
    for (command, _) in bindings.iter() {
        output.push_str(command.as_str());
        output.push_str(" = ");
        output.push_str(
            &bindings
                .all(command)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" | "),
        );
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use towavue_core::{CommandContext, MediaKind, ShortcutMatch};

    #[test]
    fn transport_alternatives_respect_context_main_bindings_and_prefixes() {
        let mut bindings = defaults();
        let resolve = |bindings: &ShortcutBindings, key: &str, kind, timeline_open| {
            bindings.resolve(
                key.parse::<KeySequence>().expect("key").strokes(),
                CommandContext {
                    media_kind: kind,
                    timeline_open,
                    ..Default::default()
                },
            )
        };
        for kind in [Some(MediaKind::Video), Some(MediaKind::Audio)] {
            for timeline in [false, true] {
                for (key, command) in [
                    ("Left", CommandId::SeekBackward),
                    ("J", CommandId::SeekBackward),
                    ("Space", CommandId::TogglePause),
                    ("K", CommandId::TogglePause),
                    ("Right", CommandId::SeekForward),
                    (
                        "L",
                        if kind == Some(MediaKind::Video) && timeline {
                            CommandId::RotateCounterclockwise
                        } else {
                            CommandId::SeekForward
                        },
                    ),
                ] {
                    assert_eq!(
                        resolve(&bindings, key, kind, timeline),
                        ShortcutMatch::Command(command),
                        "{key} {kind:?} {timeline}"
                    );
                }
            }
        }
        for kind in [None, Some(MediaKind::Image)] {
            for key in ["J", "K"] {
                assert_eq!(resolve(&bindings, key, kind, false), ShortcutMatch::None);
            }
        }
        assert_eq!(
            resolve(&bindings, "L", Some(MediaKind::Image), false),
            ShortcutMatch::Command(CommandId::RotateCounterclockwise)
        );
        let video = CommandContext {
            media_kind: Some(MediaKind::Video),
            ..Default::default()
        };
        assert_eq!(bindings.label(CommandId::SeekForward, video), "Right / L");
        assert_eq!(
            bindings.label(
                CommandId::SeekForward,
                CommandContext {
                    timeline_open: true,
                    ..video
                }
            ),
            "Right"
        );
        bindings.set(
            CommandId::ToggleFilmstrip,
            "K F".parse().expect("custom prefix"),
        );
        assert_eq!(
            resolve(&bindings, "K", video.media_kind, false),
            ShortcutMatch::Prefix
        );
        assert_eq!(
            resolve(&bindings, "K F", video.media_kind, false),
            ShortcutMatch::Command(CommandId::ToggleFilmstrip)
        );
        bindings.set(CommandId::ToggleFilmstrip, "J".parse().expect("custom key"));
        assert_eq!(
            resolve(&bindings, "J", video.media_kind, false),
            ShortcutMatch::Command(CommandId::ToggleFilmstrip)
        );
        bindings.set(CommandId::TogglePause, "P".parse().expect("replacement"));
        assert_eq!(
            resolve(&bindings, "K", video.media_kind, false),
            ShortcutMatch::None
        );
        assert_eq!(
            resolve(&bindings, "P", video.media_kind, false),
            ShortcutMatch::Command(CommandId::TogglePause)
        );
    }

    #[test]
    fn alternate_configuration_migrates_only_unchanged_defaults_and_round_trips() {
        let legacy = "seek_backward = Left\nseek_forward = Right\ntoggle_pause = Space\n";
        assert_eq!(parse(legacy, defaults()).expect("old defaults"), defaults());
        let custom = parse("seek_forward = N\ntoggle_pause = P\n", defaults()).expect("old custom");
        assert_eq!(custom.all(CommandId::SeekForward).len(), 1);
        assert_eq!(custom.all(CommandId::TogglePause).len(), 1);
        let single = parse(
            &format!("{MULTI_BINDING_HEADER}\nseek_forward = Right\n"),
            defaults(),
        )
        .expect("explicit single");
        assert_eq!(single.all(CommandId::SeekForward).len(), 1);
        let multiple = parse(&format!("{MULTI_BINDING_HEADER}\nseek_forward = Right | Ctrl+K L | L\ntoggle_pause = K | Space\n"), defaults()).expect("alternatives");
        assert_eq!(
            parse(&serialize(&multiple), defaults()).expect("round trip"),
            multiple
        );
        assert_eq!(
            parse(&serialize(&defaults()), defaults()).expect("default round trip"),
            defaults()
        );
        let duplicate = parse("seek_forward = N\nseek_forward = Right\n", defaults())
            .expect("last declaration wins");
        assert_eq!(
            duplicate.all(CommandId::SeekForward),
            defaults().all(CommandId::SeekForward)
        );
        for invalid in [
            "seek_forward = Right |",
            "toggle_pause = | K",
            "seek_forward = Right || L",
        ] {
            assert!(parse(&format!("{MULTI_BINDING_HEADER}\n{invalid}"), defaults()).is_err());
        }
        for legacy in [
            "toggle_pause = |",
            "toggle_pause = Ctrl+|",
            "toggle_pause = Ctrl+| P",
            "toggle_pause = J | L",
        ] {
            let pipe = parse(legacy, defaults()).expect("legacy pipe key");
            assert!(serialize(&pipe).contains("Pipe"));
            assert_eq!(
                parse(&serialize(&pipe), defaults()).expect("pipe round trip"),
                pipe
            );
        }
    }

    #[test]
    fn reopening_is_available_from_welcome_and_uses_custom_bindings() {
        let mut bindings = defaults();
        let sequence = "Ctrl+Shift+T".parse::<KeySequence>().expect("reopen key");
        for media_kind in [
            None,
            Some(MediaKind::Image),
            Some(MediaKind::Video),
            Some(MediaKind::Audio),
        ] {
            assert_eq!(
                bindings.resolve(
                    sequence.strokes(),
                    CommandContext {
                        media_kind,
                        ..Default::default()
                    }
                ),
                ShortcutMatch::Command(CommandId::ReopenClosedTab)
            );
        }
        bindings.set(
            CommandId::ReopenClosedTab,
            "Ctrl+K Ctrl+T".parse().expect("custom prefix"),
        );
        assert_eq!(
            bindings.resolve(sequence.strokes(), CommandContext::default()),
            ShortcutMatch::None
        );
        let sequence = "Ctrl+K Ctrl+T".parse::<KeySequence>().expect("custom key");
        assert_eq!(
            bindings.resolve(sequence.strokes(), CommandContext::default()),
            ShortcutMatch::Command(CommandId::ReopenClosedTab)
        );
    }

    #[test]
    fn cover_is_single_image_only_and_uses_customizable_bindings() {
        let mut bindings = defaults();
        let key = "Shift+C".parse::<KeySequence>().expect("cover key");
        for (kind, reading, enabled) in [
            (Some(MediaKind::Image), false, true),
            (Some(MediaKind::Image), true, false),
            (Some(MediaKind::Video), false, false),
            (Some(MediaKind::Audio), false, false),
            (None, false, false),
        ] {
            assert_eq!(
                bindings.resolve(
                    key.strokes(),
                    CommandContext {
                        media_kind: kind,
                        reading_mode: reading,
                        ..Default::default()
                    }
                ),
                if enabled {
                    ShortcutMatch::Command(CommandId::CoverWindow)
                } else {
                    ShortcutMatch::None
                }
            );
        }
        let context = CommandContext {
            media_kind: Some(MediaKind::Image),
            ..Default::default()
        };
        let custom = "Ctrl+K C".parse::<KeySequence>().expect("custom cover");
        bindings.set(CommandId::CoverWindow, custom.clone());
        assert_eq!(
            bindings.resolve(key.strokes(), context),
            ShortcutMatch::None
        );
        assert_eq!(
            bindings.resolve(custom.strokes(), context),
            ShortcutMatch::Command(CommandId::CoverWindow)
        );
        assert_eq!(
            parse(&serialize(&bindings), defaults()).expect("round trip"),
            bindings
        );
    }

    #[test]
    fn first_reading_page_shortcuts_are_contextual_and_customizable() {
        let mut bindings = defaults();
        for (command, key) in [
            (CommandId::IncreaseReadingFirstPage, "Ctrl+Shift+Right"),
            (CommandId::DecreaseReadingFirstPage, "Ctrl+Shift+Left"),
        ] {
            let key = key.parse::<KeySequence>().expect("default");
            for kind in [
                None,
                Some(MediaKind::Image),
                Some(MediaKind::Video),
                Some(MediaKind::Audio),
            ] {
                for reading_mode in [false, true] {
                    let context = CommandContext {
                        media_kind: kind,
                        reading_mode,
                        ..Default::default()
                    };
                    assert_eq!(
                        bindings.resolve(key.strokes(), context),
                        if kind == Some(MediaKind::Image) && reading_mode {
                            ShortcutMatch::Command(command)
                        } else {
                            ShortcutMatch::None
                        }
                    );
                }
            }
            let custom = "Ctrl+K Y".parse::<KeySequence>().expect("custom");
            bindings.set(command, custom.clone());
            let context = CommandContext {
                media_kind: Some(MediaKind::Image),
                reading_mode: true,
                ..Default::default()
            };
            assert_eq!(
                bindings.resolve(key.strokes(), context),
                ShortcutMatch::None
            );
            assert_eq!(
                bindings.resolve(custom.strokes(), context),
                ShortcutMatch::Command(command)
            );
            assert_eq!(
                parse(&serialize(&bindings), defaults()).expect("round trip"),
                bindings
            );
            bindings = defaults();
            bindings.set(CommandId::NextImage, key.clone());
            assert_eq!(
                bindings.resolve(key.strokes(), context),
                ShortcutMatch::Command(CommandId::NextImage),
                "existing custom commands retain priority over new defaults"
            );
            bindings = defaults();
        }
        assert_eq!(
            parse("# existing settings\n", defaults()).expect("old file"),
            defaults()
        );
    }

    #[test]
    fn select_all_supports_time_selection_and_keeps_custom_bindings() {
        let mut bindings = defaults();
        let select = "Ctrl+A".parse::<KeySequence>().expect("select all");
        for (kind, reading, enabled) in [
            (Some(MediaKind::Image), false, true),
            (Some(MediaKind::Video), false, true),
            (Some(MediaKind::Audio), false, true),
            (Some(MediaKind::Image), true, false),
            (None, false, false),
        ] {
            assert_eq!(
                bindings.resolve(
                    select.strokes(),
                    CommandContext {
                        media_kind: kind,
                        reading_mode: reading,
                        timeline_open: true,
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
    fn delete_and_keep_time_require_a_visible_selected_timeline_and_preserve_visual_crop() {
        let bindings = defaults();
        for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
            for timeline_open in [false, true] {
                for has_time_selection in [false, true] {
                    let context = CommandContext {
                        media_kind: Some(kind),
                        timeline_open,
                        has_time_selection,
                        ..Default::default()
                    };
                    let enabled = kind != MediaKind::Image && timeline_open && has_time_selection;
                    assert_eq!(
                        bindings.resolve(
                            "Shift+Space"
                                .parse::<KeySequence>()
                                .expect("selection play")
                                .strokes(),
                            context
                        ),
                        if enabled {
                            ShortcutMatch::Command(CommandId::PlayTimeSelection)
                        } else {
                            ShortcutMatch::None
                        }
                    );
                    assert_eq!(
                        bindings.resolve(
                            "Delete".parse::<KeySequence>().expect("delete").strokes(),
                            context
                        ),
                        if enabled {
                            ShortcutMatch::Command(CommandId::DeleteTimeSelection)
                        } else {
                            ShortcutMatch::None
                        }
                    );
                    if enabled {
                        assert_eq!(
                            bindings.resolve(
                                "Ctrl+Y".parse::<KeySequence>().expect("keep").strokes(),
                                context
                            ),
                            ShortcutMatch::Command(CommandId::KeepTimeSelection)
                        );
                    } else if kind == MediaKind::Video && !timeline_open {
                        assert_eq!(
                            bindings.resolve(
                                "Ctrl+Y".parse::<KeySequence>().expect("crop").strokes(),
                                context
                            ),
                            ShortcutMatch::None
                        );
                    } else if kind == MediaKind::Video && !has_time_selection {
                        assert_eq!(
                            bindings.resolve(
                                "Ctrl+Y"
                                    .parse::<KeySequence>()
                                    .expect("visual crop")
                                    .strokes(),
                                context
                            ),
                            ShortcutMatch::Command(CommandId::ApplyCrop)
                        );
                    }
                }
            }
        }
        assert_eq!(
            "Del".parse::<KeySequence>().expect("alias").to_string(),
            "Delete"
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
    fn resize_binding_is_image_only_and_custom_prefix_replaces_default() {
        use towavue_core::{CommandContext, MediaKind, ShortcutMatch};
        let bindings = defaults();
        let sequence = bindings
            .get(CommandId::ResizeImage)
            .expect("default resize");
        assert_eq!(sequence.to_string(), "Ctrl+R");
        let image = CommandContext {
            media_kind: Some(MediaKind::Image),
            ..Default::default()
        };
        assert_eq!(
            bindings.resolve(sequence.strokes(), image),
            ShortcutMatch::Command(CommandId::ResizeImage)
        );
        assert_eq!(
            bindings.resolve(
                sequence.strokes(),
                CommandContext {
                    media_kind: Some(MediaKind::Audio),
                    ..image
                }
            ),
            ShortcutMatch::Command(CommandId::CycleAudioRepeat)
        );
        for kind in [None, Some(MediaKind::Video)] {
            assert_eq!(
                bindings.resolve(
                    sequence.strokes(),
                    CommandContext {
                        media_kind: kind,
                        ..image
                    }
                ),
                ShortcutMatch::None
            );
        }
        assert_eq!(
            bindings.resolve(
                sequence.strokes(),
                CommandContext {
                    reading_mode: true,
                    ..image
                }
            ),
            ShortcutMatch::None
        );
        let custom = parse("resize_image = Ctrl+K R\n", defaults()).expect("custom resize");
        assert_eq!(
            custom.resolve(sequence.strokes(), image),
            ShortcutMatch::None
        );
        assert_eq!(
            custom.resolve(
                custom
                    .get(CommandId::ResizeImage)
                    .expect("prefix")
                    .strokes(),
                image
            ),
            ShortcutMatch::Command(CommandId::ResizeImage)
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
