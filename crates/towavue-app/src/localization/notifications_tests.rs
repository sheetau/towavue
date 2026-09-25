use super::Language;
use crate::*;

#[test]
fn japanese_export_failures_keep_edits_and_guarded_continuations() {
    let Some(root) = crate::tests::isolated_test_root(
        "localization::notifications_tests::japanese_export_failures_keep_edits_and_guarded_continuations",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.language_settings.display = Language::Japanese;
    let source = root.join("日本語{source}.bmp");
    let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
    app.path = Some(source.clone());
    app.media_kind = Some(MediaKind::Image);
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let history = app.edits.clone();
    for (error, cancelling, message) in [
        (
            ExportError::Cancelled,
            true,
            "書き出しをキャンセルしました。既存のファイルは変更していません",
        ),
        (
            ExportError::InvalidTimeline,
            false,
            "タイムライン編集にはメディアの長さと、空ではない有効な範囲が必要です",
        ),
        (
            ExportError::Failed("native 日本語 {error}\ncode=32".into()),
            false,
            "FFmpegの書き出しに失敗しました: native 日本語 {error}\ncode=32",
        ),
        (
            ExportError::Message(localization::Text::ExportSourceChangedBeforePublish),
            false,
            "FFmpegの書き出しに失敗しました: 書き出し中に元ファイルが変更されました。保存先への書き込みは行っていません",
        ),
    ] {
        let request = ExportRequest {
            source: source.clone(),
            target: source.clone(),
            kind: MediaKind::Image,
            operations: vec![],
            hardware_encode: false,
        };
        let options = ExportOptions::default();
        app.active_export = Some(ActiveExport {
            progress: export_progress::ExportProgress::new(&request, &options, None),
            // This worker rejects a same-source request; the tested outcome is injected.
            job: ExportJob::start(request.clone(), |_| {})
                .expect("fixture worker")
                .into(),
            tab,
            request,
            options,
            encoded: Duration::ZERO,
            analyzing_audio: false,
            cancelling,
            continuation: Some(GuardedAction::Exit),
        });
        app.export_error = None;
        app.status_message = None;
        app.handle_export_event(ExportEvent::Finished(Err(error)));
        assert!(app.active_export.is_none());
        assert_eq!(app.edits, history);
        assert!(matches!(app.pending_guard, Some(GuardedAction::Exit)));
        assert!(!app.exit_requested);
        if cancelling {
            assert_eq!(app.status_notice().as_deref(), Some(message));
            assert!(app.export_error.is_none());
        } else {
            assert_eq!(app.export_error.as_deref(), Some(message));
            assert!(
                app.native_prompt_content(&FallbackPrompt::ExportError)
                    .0
                    .contains(message)
            );
        }
        assert!(!source.exists(), "failed export cannot create its target");
    }
}

#[test]
fn japanese_video_quality_and_resume_notices_keep_settings_and_error_details() {
    use towavue_runtime_windows::{VideoExportQuality, VideoResumeEvent};
    let Some(_root) = crate::tests::isolated_test_root(
        "localization::notifications_tests::japanese_video_quality_and_resume_notices_keep_settings_and_error_details",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let mut peer = Application::new(None, |_| {}).expect("peer");
    peer.video_export_quality = Arc::clone(&app.video_export_quality);
    app.language_settings.display = Language::Japanese;
    let history = app.edits.clone();
    for (quality, english, japanese) in [
        (VideoExportQuality::High, "High quality", "高品質"),
        (VideoExportQuality::Balanced, "Balanced", "バランス"),
        (
            VideoExportQuality::Smaller,
            "Smaller file",
            "ファイルサイズ優先",
        ),
    ] {
        assert_eq!(quality.label(), english);
        app.set_video_export_quality(quality);
        assert_eq!(
            app.status_notice(),
            Some(format!("動画の書き出し品質: {japanese}"))
        );
        assert_eq!(peer.video_export_quality(), quality);
        assert_eq!(app.edits, history);
    }
    let detail = "Windows {detail}: 日本語.mp4 / 0x80004005";
    app.handle_video_resume(VideoResumeEvent::SaveFailed(detail.into()));
    assert_eq!(
        app.status_notice(),
        Some(format!("動画の再生位置を保存できませんでした: {detail}"))
    );
    app.handle_video_resume(VideoResumeEvent::ClearFailed(detail.into()));
    assert_eq!(
        app.status_notice(),
        Some(format!("動画の再生位置を消去できませんでした: {detail}"))
    );
    assert_eq!(app.edits, history);
}

#[test]
fn japanese_operation_notices_keep_edits_and_external_values() {
    let Some(root) = crate::tests::isolated_test_root(
        "localization::notifications_tests::japanese_operation_notices_keep_edits_and_external_values",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.language_settings.display = Language::Japanese;
    app.state = PlaybackState::Paused;
    let path = root.join("日本語{original}.png");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
    app.path = Some(path.clone());
    app.media_kind = Some(MediaKind::Image);
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    let edits = app.edits.clone();
    for (command, expected) in [
        (
            CommandId::ToggleImageInterpolation,
            "画像表示：最近傍補間（画像データと編集内容は変更しません）",
        ),
        (
            CommandId::ToggleImageInterpolation,
            "画像表示：なめらかな補間（画像データと編集内容は変更しません）",
        ),
        (
            CommandId::ToggleImageMinification,
            "画像の縮小表示：高速（画像データと編集内容は変更しません）",
        ),
        (
            CommandId::ToggleImageMinification,
            "画像の縮小表示：高品質（画像データと編集内容は変更しません）",
        ),
    ] {
        app.dispatch(command);
        assert_eq!(app.status_notice().as_deref(), Some(expected));
        assert_eq!(app.edits, edits);
        assert_eq!(app.path.as_ref(), Some(&path));
        assert_eq!(app.tabs.active_id(), Some(tab));
    }
    assert!(!app.nearest_images && app.high_quality_minification);
    app.handle_app_event(AppEvent::ImageCopied(Ok((321, 123))));
    assert_eq!(
        app.status_notice().as_deref(),
        Some("画像をコピーしました 321 × 123 px")
    );
    let detail = "Windows {detail}: 日本語.png / 0x80004005";
    app.handle_app_event(AppEvent::ImageCopied(Err(detail.into())));
    assert_eq!(
        app.status_notice(),
        Some(format!("画像をコピーできませんでした: {detail}"))
    );
    app.handle_app_event(AppEvent::FileRevealed(Ok(path.clone())));
    assert_eq!(
        app.status_notice(),
        Some(format!(
            "エクスプローラーで選択しました: {}",
            path.display()
        ))
    );
    app.handle_app_event(AppEvent::FileRevealed(Err(std::io::Error::other(detail))));
    assert_eq!(
        app.status_notice(),
        Some(format!(
            "エクスプローラーでファイルを表示できませんでした: {detail}"
        ))
    );
    assert_eq!(app.edits, edits);
    app.set_fullscreen(true);
    assert!(app.fullscreen);
    assert_eq!(
        app.status_notice().as_deref(),
        Some("全画面表示 — 下端で操作ボタンを表示 · Escape／Enterで戻る")
    );
    app.set_fullscreen(false);
    assert!(
        app.status_message.is_none(),
        "only the matching translated hint is cleared"
    );
    app.set_fullscreen(true);
    app.set_status("unrelated notification".into());
    app.set_fullscreen(false);
    assert_eq!(
        app.status_notice().as_deref(),
        Some("unrelated notification")
    );
}

#[test]
fn japanese_loading_and_error_notices_preserve_priority_and_stale_completion_guards() {
    let Some(_root) = crate::tests::isolated_test_root(
        "localization::notifications_tests::japanese_loading_and_error_notices_preserve_priority_and_stale_completion_guards",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.language_settings.display = Language::Japanese;
    app.media_kind = Some(MediaKind::Image);
    app.image_edit_pending = true;
    app.image_edit_generation = 7;
    app.image_loading = true;
    assert_eq!(
        app.status_notice().as_deref(),
        Some("画像を再サンプリング中…")
    );
    app.finish_image_edits(6, Err("stale failure".into()));
    assert!(app.image_edit_pending && app.image_error.is_none());
    let detail = "decoder {detail} 日本語.png";
    app.finish_image_edits(7, Err(detail.into()));
    assert!(!app.image_edit_pending);
    assert_eq!(app.image_error.as_deref(), Some(detail));
    assert_eq!(
        app.status_notice(),
        Some(format!(
            "画像を再サンプリングできませんでした: {detail}。「元に戻す」で前の編集状態に戻せます。"
        ))
    );
    app.status_message = None;
    assert_eq!(
        app.status_notice(),
        Some(format!("画像を読み込めませんでした: {detail}"))
    );
    app.image_error = None;
    assert_eq!(app.status_notice().as_deref(), Some("画像を読み込み中…"));
    app.reading_mode = true;
    app.reading_pages = vec![Err("first {page}".into()), Err("second 日本語".into())];
    assert_eq!(
        app.status_notice().as_deref(),
        Some("読書ページを2件読み込めませんでした: first {page}; second 日本語")
    );
    app.reading_pages.clear();
    app.image_loading = false;
    assert_eq!(
        app.status_notice().as_deref(),
        Some("表示できる画像ページがありません")
    );
    app.reading_mode = false;
    app.media_kind = Some(MediaKind::Video);
    app.state = PlaybackState::Faulted;
    app.playback_error = Some(detail.into());
    assert_eq!(
        app.status_notice(),
        Some(format!("メディアを再生できませんでした: {detail}"))
    );
    app.state = PlaybackState::Loading;
    assert_eq!(
        app.status_notice().as_deref(),
        Some("メディアを読み込み中…")
    );
    app.set_status("higher-priority {notice}".into());
    assert_eq!(
        app.status_notice().as_deref(),
        Some("higher-priority {notice}")
    );
}

#[test]
fn japanese_leave_notices_keep_dialogs_edits_and_pending_navigation() {
    let Some(root) = crate::tests::isolated_test_root(
        "localization::notifications_tests::japanese_leave_notices_keep_dialogs_edits_and_pending_navigation",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    app.language_settings.display = Language::Japanese;
    let path = root.join("日本語{original}.mp4");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Video);
    app.path = Some(path);
    app.media_kind = Some(MediaKind::Video);
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Video);
    let edits = app.edits.clone();
    app.open_audio_export_options();
    assert!(app.audio_export_dialog.is_some());
    app.request_guarded(GuardedAction::Exit);
    assert_eq!(
        app.status_notice().as_deref(),
        Some("音声の書き出し設定を適用するかキャンセルしてから移動・終了してください。")
    );
    assert!(app.audio_export_dialog.is_some());
    assert!(!app.exit_requested && app.pending_guard.is_none());
    app.audio_export_dialog = None;
    app.resize_dialog = Some(resize::ResizeDialog::new((321, 123), 1));
    app.request_guarded(GuardedAction::CloseTab(tab));
    assert_eq!(
        app.status_notice().as_deref(),
        Some("画像のサイズ変更を適用するかキャンセルしてから移動・終了してください。")
    );
    assert!(app.resize_dialog.is_some());
    assert!(app.pending_guard.is_none());
    app.resize_dialog = None;
    app.about_open = true;
    app.request_guarded(GuardedAction::Navigate(root.join("other.mp4")));
    assert_eq!(
        app.status_notice().as_deref(),
        Some("ダイアログを閉じてから移動・終了してください。")
    );
    assert!(app.about_open && app.pending_guard.is_none());
    assert!(!app.exit_requested);
    assert_eq!(app.tabs.active_id(), Some(tab));
    assert_eq!(app.edits, edits);
}
