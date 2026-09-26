use super::*;
use crate::*;
use towavue_core::localization::formatted;
use towavue_runtime_windows::{FolderOrderError, PreviewError};

#[test]
fn preview_workers_keep_typed_causes_until_receiving_window_and_reject_stale_failures() {
    let Some(root) = crate::tests::isolated_test_root(
        "localization::preview_tests::preview_workers_keep_typed_causes_until_receiving_window_and_reject_stale_failures",
    ) else {
        return;
    };
    let (send, receive) = std::sync::mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = send.send(event);
    })
    .expect("app");
    let path = root.join("missing-{日本語}.wav");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Audio);
    app.path = Some(path.clone());
    app.media_kind = Some(MediaKind::Audio);
    let generation = app.media_generation;
    let history = app.edits.clone();
    app.load_duration_for(path.clone(), generation);
    app.load_waveform();
    // Workers began in English; only the receiving display selects the language.
    app.language_settings.display = Language::Japanese;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut delivered = (false, false);
    while !delivered.0 || !delivered.1 {
        let event = receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("preview worker");
        let expected = match &event {
            AppEvent::Duration(_, _, Err(PreviewError::Io(error))) => {
                delivered.0 = true;
                assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
                formatted::duration_failed(
                    Language::Japanese,
                    &format!("プレビューキャッシュの読み書きに失敗しました: {error}"),
                )
            }
            AppEvent::Waveform(_, _, _, Err(PreviewError::Io(error))) => {
                delivered.1 = true;
                assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
                formatted::waveform_failed(
                    Language::Japanese,
                    &format!("プレビューキャッシュの読み書きに失敗しました: {error}"),
                )
            }
            _ => panic!("unexpected preview completion"),
        };
        app.handle_app_event(event);
        assert_eq!(app.status_notice().as_deref(), Some(expected.as_str()));
    }
    assert!(!app.waveform_loading);
    let notice = app.status_notice();
    app.handle_app_event(AppEvent::Duration(
        path.clone(),
        generation.wrapping_add(1),
        Err(PreviewError::InvalidDuration),
    ));
    app.handle_app_event(AppEvent::Waveform(
        path.clone(),
        generation.wrapping_add(1),
        (app.waveform_request, app.waveform_audio_track()),
        Err(PreviewError::Message(Text::WaveformNoAudio)),
    ));
    assert_eq!(app.status_notice(), notice);
    app.handle_app_event(AppEvent::Duration(
        path.clone(),
        generation,
        Err(PreviewError::InvalidDuration),
    ));
    assert_eq!(
        app.status_notice().as_deref(),
        Some(
            formatted::duration_failed(
                Language::Japanese,
                Text::PreviewInvalidDuration.in_language(Language::Japanese)
            )
            .as_str()
        )
    );
    app.handle_app_event(AppEvent::Waveform(
        path,
        generation,
        (app.waveform_request, app.waveform_audio_track()),
        Err(PreviewError::Message(Text::WaveformNoAudio)),
    ));
    assert_eq!(
        app.status_notice().as_deref(),
        Some(
            formatted::waveform_failed(
                Language::Japanese,
                &formatted::preview_generate_failed(
                    Language::Japanese,
                    Text::WaveformNoAudio.in_language(Language::Japanese)
                )
            )
            .as_str()
        )
    );
    assert_eq!(app.edits, history);
    assert_eq!(app.tabs.active_id(), Some(tab));
    assert!(app.media_duration.is_none() && app.session.is_none());
}

#[test]
fn folder_monitor_and_boxed_shell_refusals_localize_without_changing_the_document() {
    let Some(root) = crate::tests::isolated_test_root(
        "localization::preview_tests::folder_monitor_and_boxed_shell_refusals_localize_without_changing_the_document",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.language_settings.display = Language::Japanese;
    let path = root.join("retained.png");
    std::fs::write(&path, b"owned original").expect("source");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let history = app.edits.clone();
    app.watch_folder(&root.join("missing folder"));
    let notice = app.status_notice().expect("monitor failure");
    assert!(
        notice.starts_with("Windowsのフォルダー監視に失敗しました: "),
        "{notice}"
    );
    assert!(app.folder_watcher.is_none());
    assert_eq!(app.edits, history);
    assert_eq!(
        std::fs::read(path).expect("source preserved"),
        b"owned original"
    );
    for (error, key) in [
        (FolderOrderError::WorkerStopped, Text::ShellWorkerStopped),
        (FolderOrderError::ResponseLost, Text::ShellResponseLost),
    ] {
        for language in [Language::English, Language::Japanese] {
            assert_eq!(
                window_start_error(&error, language),
                key.in_language(language)
            );
        }
        assert_eq!(
            window_start_error(&error, Language::English),
            error.to_string()
        );
    }
}
