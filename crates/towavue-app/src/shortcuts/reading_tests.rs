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
fn reading_folder_reverse_shortcut_is_scoped_and_preserves_custom_bindings() {
    let command = CommandId::ReverseReadingFolderOrder;
    let defaults = defaults();
    assert_eq!(
        resolve(&defaults, "Alt+H", true),
        ShortcutMatch::Command(command)
    );
    assert_eq!(resolve(&defaults, "Alt+H", false), ShortcutMatch::None);
    for kind in [MediaKind::Audio, MediaKind::Video] {
        assert_eq!(
            defaults.resolve(
                "Alt+H".parse::<KeySequence>().expect("key").strokes(),
                CommandContext {
                    media_kind: Some(kind),
                    reading_mode: true,
                    ..Default::default()
                }
            ),
            ShortcutMatch::None
        );
    }
    for custom in ["next_image = Alt+H\n", "next_image = Alt+H N\n"] {
        let bindings = parse(custom, defaults.clone()).expect("custom binding");
        assert!(bindings.all(command).is_empty());
        assert_eq!(
            parse(&serialize(&bindings), defaults.clone()).expect("round trip"),
            bindings
        );
    }
    let bindings =
        parse("reverse_reading_folder_order = Alt+J\n", defaults.clone()).expect("custom reverse");
    assert_eq!(
        resolve(&bindings, "Alt+J", true),
        ShortcutMatch::Command(command)
    );
    assert_eq!(resolve(&bindings, "Alt+H", true), ShortcutMatch::None);
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
