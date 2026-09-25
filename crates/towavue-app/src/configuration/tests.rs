use super::*;
use crate::{Application, CommandId, FallbackPrompt, PromptButtons, grid, shortcuts};

#[test]
fn japanese_configuration_warning_uses_host_language_after_loading_and_preserves_recovery() {
    let Some(root) = crate::tests::isolated_test_root(
        "configuration::tests::japanese_configuration_warning_uses_host_language_after_loading_and_preserves_recovery",
    ) else {
        return;
    };
    let config = root.join("config").join("towavue");
    std::fs::create_dir_all(&config).expect("owned config directory");
    let shortcut_path = config.join("shortcuts.conf");
    let grid_path = config.join("grid.conf");
    let shortcut_bytes = "# Keep 日本語 {notes}\nunknown = Ctrl+O\n";
    let grid_bytes = "# Keep grid notes\nvideo = open_file\n";
    std::fs::write(&shortcut_path, shortcut_bytes).expect("invalid shortcuts");
    std::fs::write(&grid_path, grid_bytes).expect("invalid grid");
    let mut app = Application::new(None, |_| {}).expect("recoverable startup");
    assert_eq!(app.language(), Language::English);
    let english = app
        .configuration_warning
        .as_ref()
        .expect("warning")
        .message(Language::English);
    assert!(english.contains("unknown command on shortcuts.conf line 2"));
    assert!(english.contains("grid.conf line 2 has 1 commands; expected 16"));
    app.language_settings.display = Language::Japanese;
    app.language_settings.next = Language::English;
    app.show_configuration_warning();
    assert!(app.configuration_warning.is_none());
    let message = app
        .status_message
        .as_ref()
        .expect("localized status")
        .0
        .clone();
    assert!(message.starts_with("以下の設定には初期設定を使用しています。"));
    for detail in [
        "shortcuts.confの2行目に不明なコマンドがあります",
        "grid.confの2行目にコマンドが1個あります。16個指定してください",
        "「ファイル」>「キーボードショートカットを再読み込み」",
    ] {
        assert!(message.contains(detail), "{message}");
    }
    assert!(message.contains(shortcut_path.to_str().expect("fixture path")));
    assert!(message.contains(grid_path.to_str().expect("fixture path")));
    let (native, buttons) =
        app.native_prompt_content(&FallbackPrompt::ConfigurationWarning(message.clone()));
    assert_eq!(native, message);
    assert!(matches!(buttons, PromptButtons::Ok));
    app.status_message = None;
    app.show_configuration_warning();
    assert!(
        app.status_message.is_none(),
        "startup warning is consumed once"
    );
    assert_eq!(
        std::fs::read(&shortcut_path).expect("preserved shortcut file"),
        shortcut_bytes.as_bytes()
    );
    assert_eq!(
        std::fs::read(&grid_path).expect("preserved grid file"),
        grid_bytes.as_bytes()
    );
    assert_eq!(
        app.shortcuts
            .get(CommandId::OpenFile)
            .expect("default")
            .to_string(),
        "Ctrl+O"
    );
    std::fs::write(&shortcut_path, "open_file = Ctrl+P\n").expect("correct shortcuts");
    app.dispatch(CommandId::ReloadShortcuts);
    assert_eq!(
        app.shortcuts
            .get(CommandId::OpenFile)
            .expect("custom")
            .to_string(),
        "Ctrl+P"
    );
    assert!(
        app.status_message
            .as_ref()
            .expect("grid error")
            .0
            .contains("grid.confの2行目にコマンドが1個あります")
    );
    std::fs::write(&grid_path, "# defaults\n").expect("correct grid");
    app.dispatch(CommandId::ReloadShortcuts);
    assert_eq!(
        app.status_message.as_ref().expect("reload success").0,
        Text::ShortcutsGridReloaded.in_language(Language::Japanese)
    );
    std::fs::write(&shortcut_path, "open_file = Ctrl+\n").expect("invalid shortcut");
    app.dispatch(CommandId::ReloadShortcuts);
    assert!(
        app.status_message
            .as_ref()
            .expect("shortcut error")
            .0
            .contains("shortcuts.confの1行目のショートカットが不正です")
    );
    assert_eq!(
        app.shortcuts
            .get(CommandId::OpenFile)
            .expect("retained")
            .to_string(),
        "Ctrl+P"
    );
}

#[test]
fn japanese_configuration_errors_keep_exact_lines_files_and_english_diagnostics() {
    let Some(root) = crate::tests::isolated_test_root(
        "configuration::tests::japanese_configuration_errors_keep_exact_lines_files_and_english_diagnostics",
    ) else {
        return;
    };
    let path = root.join("日本語 {config}.conf");
    for (grid_file, source, english, japanese) in [
        (
            false,
            "# note\nopen_file\n",
            "shortcuts.conf line 2 is missing '='",
            "shortcuts.confの2行目に「=」がありません",
        ),
        (
            false,
            "unknown = Ctrl+O\n",
            "unknown command on shortcuts.conf line 1",
            "shortcuts.confの1行目に不明なコマンドがあります",
        ),
        (
            false,
            "\nopen_file = Ctrl+\n",
            "invalid shortcut on shortcuts.conf line 2",
            "shortcuts.confの2行目のショートカットが不正です",
        ),
        (
            true,
            "# note\nimage\n",
            "grid.conf line 2 is missing '='",
            "grid.confの2行目に「=」がありません",
        ),
        (
            true,
            "unknown = open_file\n",
            "unknown media kind on grid.conf line 1",
            "grid.confの1行目のメディア種別が不明です",
        ),
        (
            true,
            "image = open_file,close_tab\n",
            "grid.conf line 1 has 2 commands; expected 16",
            "grid.confの1行目にコマンドが2個あります。16個指定してください",
        ),
        (
            true,
            "image = unknown,open_file,open_file,open_file,open_file,open_file,open_file,open_file,open_file,open_file,open_file,open_file,open_file,open_file,open_file,open_file\n",
            "unknown command on grid.conf line 1",
            "grid.confの1行目に不明なコマンドがあります",
        ),
    ] {
        std::fs::write(&path, source).expect("owned malformed configuration");
        let error = if grid_file {
            grid::load_from(&path).expect_err("invalid grid")
        } else {
            shortcuts::load_from(&path).expect_err("invalid shortcuts")
        };
        assert_eq!(error.to_string(), english);
        assert_eq!(error.message(Language::Japanese), japanese);
        assert_eq!(
            std::fs::read(&path).expect("preserved configuration"),
            source.as_bytes()
        );
    }
    let external = Error::External("external 日本語 {error}\ncode=32".into());
    assert_eq!(external.message(Language::Japanese), external.to_string());
}
