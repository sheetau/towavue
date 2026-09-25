use super::*;
use towavue_core::{CommandContext, MediaKind, ShortcutMatch};

fn resolve(bindings: &ShortcutBindings, key: &str, reading: bool) -> ShortcutMatch {
    bindings.resolve(
        key.parse::<KeySequence>()
            .expect("valid shortcut")
            .strokes(),
        CommandContext {
            media_kind: Some(MediaKind::Image),
            reading_mode: reading,
            ..Default::default()
        },
    )
}

#[test]
fn reload_folder_order_retires_reverse_without_stealing_custom_keys() {
    let command = CommandId::ReloadFolderOrder;
    let defaults = defaults();
    assert!("reverse_reading_folder_order".parse::<CommandId>().is_err());
    for reading in [false, true] {
        assert_eq!(
            resolve(&defaults, "F5", reading),
            ShortcutMatch::Command(command)
        );
        assert_eq!(resolve(&defaults, "Alt+H", reading), ShortcutMatch::None);
    }
    for kind in [MediaKind::Audio, MediaKind::Video] {
        assert_eq!(
            defaults.resolve(
                "F5".parse::<KeySequence>().expect("key").strokes(),
                CommandContext {
                    media_kind: Some(kind),
                    ..Default::default()
                }
            ),
            ShortcutMatch::Command(command)
        );
    }
    assert_eq!(
        defaults.resolve(
            "F5".parse::<KeySequence>().expect("key").strokes(),
            CommandContext::default()
        ),
        ShortcutMatch::None
    );
    for custom in ["open_file = F5\n", "open_file = F5 N\n"] {
        let bindings = parse(custom, defaults.clone()).expect("custom binding");
        assert!(bindings.all(command).is_empty());
        assert_eq!(
            parse(&serialize(&bindings), defaults.clone()).expect("round trip"),
            bindings
        );
    }
    for old in ["Alt+H", "Alt+J", "", "retired syntax"] {
        let text = format!(
            "# towavue shortcuts v9\nreverse_reading_folder_order = {old}\nopen_file = Alt+O\nreload_folder_order =\n"
        );
        let bindings = parse(&text, defaults.clone()).expect("retired setting");
        assert!(bindings.all(command).is_empty());
        assert_eq!(
            bindings
                .get(CommandId::OpenFile)
                .expect("custom key")
                .to_string(),
            "Alt+O"
        );
        let rewritten = serialize(&bindings);
        assert!(!rewritten.contains("reverse_reading_folder_order"));
        assert_eq!(
            parse(&rewritten, defaults.clone()).expect("round trip"),
            bindings
        );
    }
}

#[test]
fn reading_arrows_migrate_only_unchanged_defaults_and_preserve_custom_commands() {
    for old in [
        "",
        "previous_image = Left\nnext_image = Right\n",
        "# towavue shortcuts v5\nprevious_image = Left | PageUp | Backspace | A\nnext_image = Right | PageDown | Space | D\n",
    ] {
        let bindings = parse(old, defaults()).expect("legacy bindings");
        let round_trip = parse(&serialize(&bindings), defaults()).expect("serialized bindings");
        for bindings in [&bindings, &round_trip] {
            for (key, ordinary, reading) in [
                ("Left", CommandId::PreviousImage, CommandId::ReadingLeft),
                ("Right", CommandId::NextImage, CommandId::ReadingRight),
            ] {
                assert_eq!(
                    resolve(bindings, key, false),
                    ShortcutMatch::Command(ordinary)
                );
                assert_eq!(
                    resolve(bindings, key, true),
                    ShortcutMatch::Command(reading)
                );
            }
            assert_eq!(
                resolve(bindings, "Space", true),
                ShortcutMatch::Command(CommandId::NextImage)
            );
        }
    }
    for (text, key, command) in [
        (
            "# towavue shortcuts v5\nnext_image = Right\n",
            "Right",
            CommandId::NextImage,
        ),
        (
            "# towavue shortcuts v5\nprevious_image = Left | Ctrl+P\n",
            "Left",
            CommandId::PreviousImage,
        ),
        ("next_image = Right N\n", "Right N", CommandId::NextImage),
        ("next_image = N\n", "N", CommandId::NextImage),
        ("next_media = Left\n", "Left", CommandId::NextMedia),
        (
            "reading_right = Ctrl+R\n",
            "Ctrl+R",
            CommandId::ReadingRight,
        ),
    ] {
        let bindings = parse(text, defaults()).expect("custom bindings");
        assert_eq!(
            resolve(&bindings, key, true),
            ShortcutMatch::Command(command),
            "{text}"
        );
        let restored = parse(&serialize(&bindings), defaults()).expect("serialized bindings");
        assert_eq!(
            resolve(&restored, key, true),
            ShortcutMatch::Command(command),
            "round trip: {text}"
        );
        if key == "Right N" {
            assert_eq!(resolve(&bindings, "Right", true), ShortcutMatch::Prefix);
        }
    }
}

#[test]
fn spatial_aliases_and_previous_space_preserve_explicit_bindings_and_prefixes() {
    let old_defaults = serialize(&defaults())
        .replace(CURRENT_BINDING_HEADER, SELECTION_BINDING_HEADER)
        .replace(
            "previous_image = Left | PageUp | Backspace | A | Shift+Space",
            "previous_image = Left | PageUp | Backspace | A",
        )
        .replace("reading_left = Left | A", "reading_left = Left")
        .replace("reading_right = Right | D", "reading_right = Right");
    for text in ["", old_defaults.as_str()] {
        let migrated = parse(text, defaults()).expect("old configuration");
        for bindings in [
            &migrated,
            &parse(&serialize(&migrated), defaults()).expect("round trip"),
        ] {
            for reading in [false, true] {
                for (key, command) in [
                    (
                        "A",
                        if reading {
                            CommandId::ReadingLeft
                        } else {
                            CommandId::PreviousImage
                        },
                    ),
                    (
                        "D",
                        if reading {
                            CommandId::ReadingRight
                        } else {
                            CommandId::NextImage
                        },
                    ),
                    ("Shift+Space", CommandId::PreviousImage),
                ] {
                    assert_eq!(
                        resolve(bindings, key, reading),
                        ShortcutMatch::Command(command)
                    );
                }
            }
        }
    }
    for (key, command) in [
        ("A", CommandId::ReadingLeft),
        ("D", CommandId::ReadingRight),
        ("Shift+Space", CommandId::PreviousImage),
    ] {
        for suffix in ["", " N"] {
            let custom = format!("{old_defaults}open_file = {key}{suffix}\n");
            let bindings = parse(&custom, defaults()).expect("custom conflict");
            assert!(
                !bindings
                    .all(command)
                    .iter()
                    .any(|bound| bound.to_string() == key)
            );
            assert_eq!(
                resolve(&bindings, key, true),
                if suffix.is_empty() {
                    ShortcutMatch::Command(CommandId::OpenFile)
                } else {
                    ShortcutMatch::Prefix
                }
            );
        }
    }
    for command in [
        CommandId::PreviousImage,
        CommandId::ReadingLeft,
        CommandId::ReadingRight,
    ] {
        for header in [SELECTION_BINDING_HEADER, CURRENT_BINDING_HEADER] {
            for custom in ["", "Q", "Q | Ctrl+K N"] {
                let text = format!("{header}\n{} = {custom}\n", command.as_str());
                let bindings = parse(&text, defaults()).expect("explicit setting");
                let expected: Vec<_> = custom
                    .split('|')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.trim().parse::<KeySequence>().expect("key"))
                    .collect();
                assert_eq!(bindings.all(command), expected);
                assert_eq!(
                    parse(&serialize(&bindings), defaults()).expect("round trip"),
                    bindings
                );
            }
        }
    }
    let current = parse(&format!("{CURRENT_BINDING_HEADER}\nprevious_image = Left | PageUp | Backspace | A\nreading_left = Left\nreading_right = Right\n"), defaults()).expect("explicit current choices");
    assert_eq!(resolve(&current, "Shift+Space", false), ShortcutMatch::None);
    assert_eq!(
        resolve(&current, "A", true),
        ShortcutMatch::Command(CommandId::PreviousImage)
    );
    for kind in [MediaKind::Video, MediaKind::Audio] {
        assert_eq!(
            defaults().resolve(
                "Shift+Space".parse::<KeySequence>().expect("key").strokes(),
                CommandContext {
                    media_kind: Some(kind),
                    timeline_open: true,
                    has_time_selection: true,
                    ..Default::default()
                }
            ),
            ShortcutMatch::Command(CommandId::PlayTimeSelection)
        );
    }
}
