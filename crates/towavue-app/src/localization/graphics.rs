use super::{Language, Text};
use std::{fmt, sync::Arc};
use towavue_core::localization::formatted;
use towavue_runtime_windows::RenderError;

/// The host clones a failed recovery cause for every affected window.
#[derive(Clone, Debug)]
pub(crate) enum GraphicsRecoveryError {
    WindowUnavailable,
    Device(Arc<RenderError>),
    Surface(Arc<RenderError>),
    #[cfg(test)]
    Injected(String),
}

impl GraphicsRecoveryError {
    pub(crate) fn message(&self, language: Language) -> String {
        match self {
            Self::WindowUnavailable => Text::GraphicsRecoveryWindowUnavailable
                .in_language(language)
                .into(),
            Self::Device(error) => {
                formatted::graphics_device_recovery_failed(language, &error.message(language))
            }
            Self::Surface(error) => {
                formatted::graphics_surface_recovery_failed(language, &error.message(language))
            }
            #[cfg(test)]
            Self::Injected(detail) => detail.clone(),
        }
    }
}

impl fmt::Display for GraphicsRecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message(Language::English))
    }
}

#[cfg(test)]
impl From<&str> for GraphicsRecoveryError {
    fn from(detail: &str) -> Self {
        Self::Injected(detail.into())
    }
}

/// Boxed startup failures also reach the drag-to-new-window status notice.
/// Native/third-party details remain literal; recognized owned causes translate.
pub(crate) fn window_start_error(
    error: &(dyn std::error::Error + 'static),
    language: Language,
) -> String {
    if let Some(error) = error.downcast_ref::<crate::configuration::Error>() {
        error.message(language)
    } else if let Some(error) = error.downcast_ref::<towavue_runtime_windows::PreviewError>() {
        error.message(language)
    } else if let Some(error) = error.downcast_ref::<towavue_runtime_windows::update::UpdateError>()
    {
        error.message(language)
    } else if let Some(error) = error.downcast_ref::<std::io::Error>() {
        towavue_runtime_windows::io_error_message(error, language)
    } else {
        towavue_runtime_windows::native_ui_error_message(error, language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn japanese_graphics_failures_keep_diagnostics_recovery_position_and_edits() {
        let Some(root) = crate::tests::isolated_test_root(
            "localization::graphics::tests::japanese_graphics_failures_keep_diagnostics_recovery_position_and_edits",
        ) else {
            return;
        };
        for density in [1.0, 1.25, 2.0] {
            let context = localization::test_ui::japanese_context(density);
            let mut app = Application::new(None, |_| {}).expect("app");
            app.ui_context = Some(context.clone());
            app.language_settings.next = Language::English;
            let id = app
                .tabs
                .open_new(root.join("日本語{original}.png"), MediaKind::Image);
            app.edits
                .entry(id)
                .or_default()
                .push(EditOperation::FlipHorizontal, MediaKind::Image);
            let history = app.edits.clone();
            let missing = app
                .create_graphics_surface(None)
                .err()
                .expect("missing window");
            assert_eq!(
                missing.to_string(),
                "window was unavailable during graphics recovery"
            );
            for (failure, expected) in [
                (missing, "描画の復旧中にウィンドウを利用できなくなりました"),
                (
                    GraphicsRecoveryError::Device(Arc::new(RenderError::WindowHandle)),
                    "D3D11デバイスを復旧できません: ウィンドウハンドルを取得できません",
                ),
                (
                    GraphicsRecoveryError::Surface(Arc::new(RenderError::SurfaceNotSized)),
                    "D3D11の表示領域を復旧できません: D3D11の表示領域のサイズが設定されていません",
                ),
            ] {
                let position = media_time(Duration::from_secs(17));
                assert_eq!(failure.message(Language::English), failure.to_string());
                app.fail_graphics_recovery(position, PlaybackState::Paused, failure.clone());
                assert_eq!(app.playback_error.as_deref(), Some(expected));
                assert_eq!(app.current_position(), position);
                assert_eq!(app.edits, history);
                assert!(
                    matches!(&app.queued_recovery, Some(FallbackPrompt::Recovery { position: saved, state: PlaybackState::Paused, error }) if *saved == position && error == expected)
                );
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
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.text() == expected && !text.galley.elided)), "{expected}");
            }
            app.handle_render_error(RenderError::InvalidFrame);
            assert_eq!(
                app.playback_error.as_deref(),
                Some("読み込んだ動画フレームのサイズが不正です")
            );
            assert_eq!(app.edits, history);
        }
        let boxed: Box<dyn std::error::Error> = Box::new(RenderError::InvalidFrame);
        assert_eq!(
            window_start_error(boxed.as_ref(), Language::Japanese),
            "読み込んだ動画フレームのサイズが不正です"
        );
        assert_eq!(boxed.to_string(), "decoded video dimensions are invalid");
        let native = std::io::Error::other("native {detail} 日本語.png 0x80004005");
        assert_eq!(
            window_start_error(&native, Language::Japanese),
            native.to_string()
        );
    }
}
