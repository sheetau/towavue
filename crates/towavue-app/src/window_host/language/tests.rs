use super::*;

#[test]
fn language_selection_is_durable_shared_and_does_not_restart_or_change_live_windows() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::language::tests::language_selection_is_durable_shared_and_does_not_restart_or_change_live_windows",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let path = root.join("config/towavue/language.conf");
    let (send, receive) = std::sync::mpsc::channel();
    host.language.store = Some(
        LanguagePreferences::open(path.clone(), move |result| {
            send.send(result).expect("result");
        })
        .expect("writer"),
    );
    let first = *host.windows.keys().next().expect("first");
    let second = host.add_application(None).expect("second");
    let app = host.windows.get_mut(&first).expect("first");
    let tab = app
        .tabs
        .open_new(root.join("memory-only.png"), MediaKind::Image);
    let mut history = EditHistory::default();
    history.push(EditOperation::FlipHorizontal, MediaKind::Image);
    app.edits.insert(tab, history.clone());
    host.route(Event::Window(first, AppEvent::Language(Language::Japanese)));
    assert!(
        host.windows
            .values()
            .all(|app| app.language_settings.saving)
    );
    assert!(
        host.windows
            .values()
            .all(|app| app.language_settings.next == Language::English)
    );
    // Another window cannot enqueue a stale choice while publication is pending.
    host.route(Event::Window(
        second,
        AppEvent::Language(Language::Japanese),
    ));
    let result = receive.recv_timeout(Duration::from_secs(5)).expect("saved");
    host.windows.get_mut(&first).expect("first").about_open = true;
    host.route(Event::LanguageSaved(result));
    assert!(host.language.notice.is_some(), "wait for an existing modal");
    assert!(host.windows.values().all(
        |app| app.language_settings.next == Language::Japanese && !app.language_settings.saving
    ));
    let third = host.add_application(None).expect("new window");
    assert_eq!(
        host.windows[&third].language_settings.display,
        Language::English
    );
    assert_eq!(
        host.windows[&third].language_settings.next,
        Language::Japanese
    );
    host.windows.get_mut(&first).expect("first").about_open = false;
    host.show_language_notice();
    assert!(matches!(
        host.windows[&first].native_prompt,
        Some(FallbackPrompt::LanguageRestart(Language::Japanese, _))
    ));
    host.windows
        .get_mut(&first)
        .expect("first")
        .finish_native_prompt(Ok(PromptResponse::No));
    for app in host.windows.values() {
        assert!(!app.exit_requested);
        assert!(app.pending_guard.is_none());
        assert_eq!(app.language(), Language::English);
    }
    let restarted = WindowHost::new(None, None).expect("next host");
    assert_eq!(
        host.windows[&first].edits[&tab], history,
        "unsaved history retained exactly"
    );
    assert_eq!(restarted.language.settings.display, Language::Japanese);
    assert!(
        restarted
            .windows
            .values()
            .all(|app| app.language() == Language::Japanese)
    );
    assert!(receive.try_recv().is_err(), "only one admitted write");
    host.select_language(second, Language::English);
    host.route(Event::LanguageSaved(
        receive
            .recv_timeout(Duration::from_secs(5))
            .expect("reverted choice"),
    ));
    assert_eq!(host.language.settings.next, Language::English);
    match &host.windows[&second].native_prompt {
        Some(FallbackPrompt::LanguageNotice(message)) => assert_eq!(
            message,
            Text::LanguageUnchanged.in_language(Language::English)
        ),
        _ => panic!("saved current-language notice"),
    }
    assert_eq!(host.windows[&first].edits[&tab], history);
}

#[test]
fn language_save_failure_preserves_selection_and_closed_owner_notice_moves_to_a_survivor() {
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::language::tests::language_save_failure_preserves_selection_and_closed_owner_notice_moves_to_a_survivor",
    ) else {
        return;
    };
    let mut host = WindowHost::new(None, None).expect("host");
    let first = *host.windows.keys().next().expect("first");
    let second = host.add_application(None).expect("second");
    let path = root.join("config/towavue/language.conf");
    let (send, receive) = std::sync::mpsc::channel();
    host.language.store = Some(
        LanguagePreferences::open(path.clone(), move |result| {
            send.send(result).expect("result");
        })
        .expect("writer"),
    );
    std::fs::write(&path, "future version").expect("foreign setting");
    host.select_language(first, Language::Japanese);
    let failure = receive
        .recv_timeout(Duration::from_secs(5))
        .expect("failure");
    host.language.settings.display = Language::Japanese;
    host.broadcast_language_settings();
    host.route(Event::LanguageSaved(failure));
    assert_eq!(
        host.windows[&first].status_notice().as_deref(),
        Some("表示言語を保存できませんでした: 表示言語の設定が不正です")
    );
    host.language.settings.display = Language::English;
    host.broadcast_language_settings();
    assert_eq!(host.language.settings.next, Language::English);
    assert!(!host.language.settings.saving);
    assert!(host.language.notice.is_none());
    assert_eq!(
        std::fs::read_to_string(&path).expect("preserved"),
        "future version"
    );
    std::fs::remove_file(&path).expect("remove owned fixture");
    host.select_language(first, Language::Japanese);
    host.windows.get_mut(&first).expect("first").exit_requested = true;
    host.route(Event::LanguageSaved(
        receive.recv_timeout(Duration::from_secs(5)).expect("saved"),
    ));
    assert!(matches!(
        host.windows[&second].native_prompt,
        Some(FallbackPrompt::LanguageRestart(Language::Japanese, _))
    ));
    assert!(!host.windows[&second].exit_requested);
}
