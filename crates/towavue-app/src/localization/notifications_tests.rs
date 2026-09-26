use super::Language;
use crate::*;

#[test]
fn recent_history_failures_use_the_receiving_window_language_and_keep_media() {
    let Some(root) = crate::tests::isolated_test_root(
        "localization::notifications_tests::recent_history_failures_use_the_receiving_window_language_and_keep_media",
    ) else {
        return;
    };
    let history = root.join("owned-recent.txt");
    let commands = root.join("command-history.txt");
    std::fs::write(&history, b"unknown files").expect("files fixture");
    std::fs::write(&commands, b"unknown commands").expect("commands fixture");
    let mut app = Application::new(None, |_| {}).expect("app");
    let path = root.join("unopened-source.png");
    let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
    app.path = Some(path.clone());
    app.media_kind = Some(MediaKind::Image);
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let edits = app.edits.clone();
    let (send, receive) = std::sync::mpsc::channel();
    app.recent_files = Some(
        towavue_runtime_windows::RecentFiles::new(history.clone(), move || {
            let _ = send.send(());
        })
        .expect("worker"),
    );
    for (language, expected) in [
        (
            Language::Japanese,
            "最近使ったファイルを取得できません: 最近使ったファイルの履歴形式が不明です。既存のファイルは保持しました。; コマンド履歴を取得できません: コマンド履歴の形式が不明です。既存のファイルは保持しました。",
        ),
        (
            Language::English,
            "Recent files unavailable: Unrecognized recent files format; existing file was retained.; Command history unavailable: Unrecognized command history format; existing file was retained.",
        ),
    ] {
        receive
            .recv_timeout(Duration::from_secs(5))
            .expect("history completion");
        app.language_settings.display = language;
        app.handle_app_event(AppEvent::RecentFilesReady);
        assert_eq!(app.status_notice().as_deref(), Some(expected));
        assert_eq!(app.path.as_ref(), Some(&path));
        assert_eq!(app.tabs.active_id(), Some(tab));
        assert_eq!(app.edits, edits);
        app.recent_files.as_ref().expect("worker").refresh();
    }
    drop(app);
    assert_eq!(
        std::fs::read(history).expect("files retained"),
        b"unknown files"
    );
    assert_eq!(
        std::fs::read(commands).expect("commands retained"),
        b"unknown commands"
    );
}

#[test]
fn japanese_image_failures_preserve_owners_reading_pages_and_external_details() {
    use towavue_runtime_windows::{DecodeError, DecodedImageFrame, ImageDecodeError, LoadedImages};
    let Some(root) = crate::tests::isolated_test_root(
        "localization::notifications_tests::japanese_image_failures_preserve_owners_reading_pages_and_external_details",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let context = super::test_ui::japanese_context(density);
        let path = root.join("日本語{original}.png");
        let other = root.join("日本語{other}.png");
        let limit = context.input(|input| input.max_texture_side);
        let image = |width: u32| {
            Arc::new(DecodedImage {
                animation_plays: 1,
                format: "PNG",
                frames: vec![DecodedImageFrame {
                    width,
                    height: 1,
                    rgba: vec![255; 4],
                    delay: Duration::ZERO,
                }],
            })
        };
        let empty = Arc::new(DecodedImage {
            animation_plays: 1,
            format: "PNG",
            frames: Vec::new(),
        });
        let detail = "native {detail}: 日本語.png";
        for (result, expected) in [
            (
                Err(ImageDecodeError::UnknownFormat),
                "画像形式を判別できません".to_owned(),
            ),
            (
                Err(ImageDecodeError::Ffmpeg(DecodeError::NoMediaStream)),
                "FFmpegで画像を読み込めません: ファイルに再生可能な音声や動画がありません".into(),
            ),
            (
                Err(ImageDecodeError::Open(std::io::Error::other(detail))),
                format!("画像を開けません: {detail}"),
            ),
            (
                Ok(empty.clone()),
                "読み込んだ画像に指定したフレームがありません".into(),
            ),
            (
                Ok(image(limit as u32 + 1)),
                format!("画像サイズがこのGPUの画像サイズ上限（{limit}px）を超えています"),
            ),
        ] {
            let mut app = Application::new(None, |_| {}).expect("app");
            app.ui_context = Some(context.clone());
            app.language_settings.display = Language::Japanese;
            app.language_settings.next = Language::English;
            let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
            app.displayed_tab = Some(tab);
            app.path = Some(path.clone());
            app.media_kind = Some(MediaKind::Image);
            app.image_generation = 42;
            app.image_loading = true;
            let history = app.edits.clone();
            app.apply_loaded_images(LoadedImages {
                source: None,
                generation: 42,
                first_index: 0,
                total: 1,
                images: vec![(path.clone(), result)],
            });
            assert_eq!(app.image_error.as_deref(), Some(expected.as_str()));
            assert_eq!(app.status_notice().as_deref(), Some(expected.as_str()));
            assert_eq!(app.state, PlaybackState::Faulted);
            assert!(app.image.is_none() && !app.image_loading);
            assert_eq!(app.edits, history);
            assert_eq!(app.tabs.active_id(), Some(tab));
            assert_eq!(app.path.as_ref(), Some(&path));
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1000.0, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
                    },
                );
            }
            assert_eq!(output.pixels_per_point, density);
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == expected && !text.galley.elided)),
                "complete image error reaches the painted status: {expected}"
            );
            // A stale failure must not replace the current error or owner.
            app.apply_loaded_images(LoadedImages {
                source: None,
                generation: 41,
                first_index: 0,
                total: 1,
                images: vec![(other.clone(), Err(ImageDecodeError::TooLarge))],
            });
            assert_eq!(app.image_error.as_deref(), Some(expected.as_str()));
            app.reading_mode = true;
            app.image_loading = true;
            app.apply_loaded_images(LoadedImages {
                source: None,
                generation: 42,
                first_index: 0,
                total: 2,
                images: vec![(path.clone(), Ok(image(1)))],
            });
            let texture = app.image.as_ref().expect("first reading page").texture.id();
            app.apply_loaded_images(LoadedImages {
                source: None,
                generation: 42,
                first_index: 1,
                total: 2,
                images: vec![(other.clone(), Err(ImageDecodeError::UnknownFormat))],
            });
            assert_eq!(
                app.image
                    .as_ref()
                    .expect("first page retained")
                    .texture
                    .id(),
                texture
            );
            assert!(app.image_error.is_none());
            assert_eq!(app.state, PlaybackState::Paused);
            assert_eq!(
                app.reading_pages[0].as_ref().err().map(String::as_str),
                Some("日本語{other}.png: 画像形式を判別できません")
            );
            assert_eq!(app.edits, history);
        }
    }
}

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
    app.handle_app_event(AppEvent::ImageCopied(Err(
        towavue_runtime_windows::ClipboardImageError::Message(
            localization::Text::ClipboardImageFrameUnavailable,
        ),
    )));
    assert_eq!(
        app.status_notice().as_deref(),
        Some("画像をコピーできませんでした: 画像のフレームを利用できません")
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

#[test]
fn prepared_tab_and_timeline_failures_keep_captured_language_external_details_and_history() {
    let Some(root) = crate::tests::isolated_test_root(
        "localization::notifications_tests::prepared_tab_and_timeline_failures_keep_captured_language_external_details_and_history",
    ) else {
        return;
    };
    for language in [Language::English, Language::Japanese] {
        let mut app = Application::new(None, |_| {}).expect("app");
        app.language_settings.display = language;
        app.language_settings.next = if language == Language::English {
            Language::Japanese
        } else {
            Language::English
        };
        let path = root.join("missing-{source}.mp4");
        let tab = app.tabs.open_new(path.clone(), MediaKind::Video);
        app.prepare_playback_tab(tab);
        let saved = &app.retained_playback[&tab];
        assert_eq!(saved.language, language);
        let instance = saved.instance;
        let detail = "external \u{65e5}\u{672c}\u{8a9e}{path}\n0x80004005";
        app.finish_playback_tab_preparation(
            tab,
            instance,
            path.clone(),
            Err(towavue_runtime_windows::PreviewError::Generate(
                detail.into(),
            )),
            None,
        );
        let expected = towavue_core::localization::formatted::tab_metadata_unavailable(
            language,
            &towavue_core::localization::formatted::preview_generate_failed(language, detail),
        );
        assert_eq!(
            app.retained_playback[&tab]
                .status
                .as_ref()
                .expect("notice")
                .0,
            expected
        );
        app.finish_playback_tab_preparation(
            tab,
            instance.wrapping_add(1),
            path,
            Err(towavue_runtime_windows::PreviewError::InvalidDuration),
            None,
        );
        assert_eq!(
            app.retained_playback[&tab]
                .status
                .as_ref()
                .expect("notice")
                .0,
            expected
        );
        app.media_kind = Some(MediaKind::Video);
        app.edits.entry(tab).or_default().push(
            EditOperation::Timeline(towavue_core::TimelineEdit::ScaleVolume(
                towavue_core::TimeRange::new(MediaTime::ZERO, media_time(Duration::from_secs(1)))
                    .expect("range"),
                0.5,
            )),
            MediaKind::Video,
        );
        let history = app.edits.clone();
        assert_eq!(
            app.history_timeline(),
            Err(localization::Text::WaitForTimelineDuration)
        );
        app.sync_playback_edits();
        assert_eq!(
            app.status_notice(),
            Some(
                localization::Text::WaitForTimelineDuration
                    .in_language(language)
                    .into()
            )
        );
        app.media_duration = Some(Duration::ZERO);
        assert_eq!(
            app.history_timeline(),
            Err(localization::Text::InvalidTimelineHistory)
        );
        app.sync_playback_edits();
        assert_eq!(
            app.status_notice(),
            Some(
                localization::Text::InvalidTimelineHistory
                    .in_language(language)
                    .into()
            )
        );
        for key in [
            localization::Text::RecoverBeforePlayback,
            localization::Text::RendererUnavailable,
        ] {
            app.fail_text(key);
            assert_eq!(
                app.playback_error.as_deref(),
                Some(key.in_language(language))
            );
            assert_eq!(app.state, PlaybackState::Faulted);
        }
        assert_eq!(app.edits, history);
        assert!(app.retained_playback[&tab].session.is_none());
    }
}
