use super::*;

#[test]
fn held_seek_keys_respect_bindings_prefixes_media_state_and_overlays() {
    let Some(_root) = tests::isolated_test_root(
        "seek_repeat_tests::held_seek_keys_respect_bindings_prefixes_media_state_and_overlays",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.shortcuts = shortcuts::defaults();
    app.state = PlaybackState::Paused;
    for kind in [MediaKind::Audio, MediaKind::Video] {
        app.media_kind = Some(kind);
        for state in [
            PlaybackState::Playing,
            PlaybackState::Paused,
            PlaybackState::Ended,
        ] {
            app.state = state;
            assert_eq!(
                app.repeated_seek_command(&"Right".parse().expect("key")),
                Some(CommandId::SeekForward)
            );
        }
        app.state = PlaybackState::Paused;
        for (key, expected) in [
            ("Left", Some(CommandId::SeekBackward)),
            ("J", Some(CommandId::SeekBackward)),
            ("Right", Some(CommandId::SeekForward)),
            ("L", Some(CommandId::SeekForward)),
            ("K", None),
            ("Space", None),
            ("F11", None),
            ("Up", None),
            ("Ctrl+S", None),
        ] {
            assert_eq!(
                app.repeated_seek_command(&key.parse().expect("key")),
                expected
            );
        }
        for blocked in 0..9 {
            app.palette_open = blocked == 0;
            app.grid_open = blocked == 1;
            app.filmstrip_open = blocked == 2;
            app.native_ime_composing = blocked == 3;
            app.pending_guard = (blocked == 4).then_some(GuardedAction::Exit);
            app.entered_shortcut = if blocked == 5 {
                vec!["Ctrl+K".parse().expect("prefix")]
            } else {
                vec![]
            };
            app.state = match blocked {
                6 => PlaybackState::Loading,
                7 => PlaybackState::Faulted,
                _ => PlaybackState::Paused,
            };
            let context = fonts::test_context();
            if blocked == 8 {
                egui::Popup::open_id(&context, egui::Id::new("repeat-menu"));
            }
            app.ui_context = Some(context);
            assert!(
                app.repeated_seek_command(&"Right".parse().expect("key"))
                    .is_none()
            );
        }
        app.ui_context = None;
    }
    app.media_kind = Some(MediaKind::Image);
    assert!(
        app.repeated_seek_command(&"Right".parse().expect("key"))
            .is_none()
    );
    app.media_kind = Some(MediaKind::Video);
    app.shortcuts = ShortcutBindings::default();
    app.shortcuts
        .set(CommandId::SeekForward, "N".parse().expect("custom key"));
    app.shortcuts
        .set(CommandId::SeekBackward, "Ctrl+K J".parse().expect("chord"));
    app.shortcuts.set(
        CommandId::ToggleFullscreen,
        "Right".parse().expect("rebound toggle"),
    );
    for key in ["Right", "Ctrl+K", "J"] {
        assert!(
            app.repeated_seek_command(&key.parse().expect("key"))
                .is_none()
        );
        app.repeat_media_shortcut(key.parse().expect("key"));
        assert!(!app.fullscreen);
    }
    assert_eq!(
        app.repeated_seek_command(&"N".parse().expect("key")),
        Some(CommandId::SeekForward)
    );
}

#[test]
fn held_seek_keys_leave_text_and_focused_control_navigation_with_egui() {
    let Some(_root) = tests::isolated_test_root(
        "seek_repeat_tests::held_seek_keys_leave_text_and_focused_control_navigation_with_egui",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.shortcuts = shortcuts::defaults();
    app.media_kind = Some(MediaKind::Video);
    app.state = PlaybackState::Paused;
    for text_edit in [false, true] {
        let context = fonts::test_context();
        app.ui_context = Some(context.clone());
        let mut text = String::new();
        for _ in 0..3 {
            let _ = context.run_ui(egui::RawInput::default(), |ui| {
                let response = if text_edit {
                    ui.text_edit_singleline(&mut text)
                } else {
                    ui.button("Control")
                };
                response.request_focus();
            });
        }
        assert!(context.egui_wants_keyboard_input());
        assert!(
            app.repeated_seek_command(&"Right".parse().expect("key"))
                .is_none()
        );
        assert_eq!(
            app.repeated_seek_command(&"J".parse().expect("key")),
            (!text_edit).then_some(CommandId::SeekBackward)
        );
    }
}
