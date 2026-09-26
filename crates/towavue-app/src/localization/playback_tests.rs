use super::*;
use crate::*;
use towavue_runtime_windows::AudioOutputError;

#[test]
fn audio_failures_paint_in_the_captured_language_without_changing_edits() {
    let Some(root) = crate::tests::isolated_test_root(
        "localization::playback_tests::audio_failures_paint_in_the_captured_language_without_changing_edits",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let context = test_ui::japanese_context(density);
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context.clone());
        app.language_settings.next = Language::English;
        let id = app
            .tabs
            .open_new(root.join("日本語{original}.wav"), MediaKind::Audio);
        app.edits
            .entry(id)
            .or_default()
            .push(EditOperation::SetVolume(0.5), MediaKind::Audio);
        let history = app.edits.clone();
        let position = media_time(Duration::from_secs(17));
        app.clock = Some(PlaybackClock::paused(position, 1.0));
        for (failure, expected) in [
            (AudioOutputError::Closed, "音声出力の処理が停止しました"),
            (
                AudioOutputError::UnsupportedFormat(44100, 6),
                "未対応の音声形式です: 44100 Hz、6チャンネル",
            ),
            (
                AudioOutputError::Wasapi("native {detail} 日本語 0x80004005".into()),
                "WASAPI音声出力に失敗しました: native {detail} 日本語 0x80004005",
            ),
        ] {
            app.handle_audio_event(AudioOutputEvent::Failed(failure.into()));
            assert_eq!(app.playback_error.as_deref(), Some(expected));
            assert_eq!(app.state, PlaybackState::Faulted);
            assert_eq!(app.current_position(), position);
            assert_eq!(app.edits, history);
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1100.0, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
                    },
                );
            }
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == expected && !text.galley.elided)),
                "{expected}"
            );
        }
    }
}
