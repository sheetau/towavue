#[cfg(test)]
use super::Language;
use super::Text;
use crate::{Application, FallbackPrompt, GuardedAction, PromptButtons, display_name, image_paste};
use towavue_core::localization::formatted;

impl<N: Fn(crate::AppEvent) + Send + Sync + 'static> Application<N> {
    pub(crate) fn native_prompt_content(&self, prompt: &FallbackPrompt) -> (String, PromptButtons) {
        let language = self.language();
        let text = |key: Text| key.in_language(language);
        match prompt {
            FallbackPrompt::LanguageNotice(message) => {
                (message.clone(), PromptButtons::Information)
            }
            FallbackPrompt::LanguageRestart(_, message) => {
                (message.clone(), PromptButtons::RestartApplication)
            }
            FallbackPrompt::ConfigurationWarning(message) => (message.clone(), PromptButtons::Ok),
            FallbackPrompt::UpdateNotice(notice) => (
                formatted::native_update_notice(
                    language,
                    &notice.version.to_string(),
                    if notice.failed {
                        text(Text::NativePreviousInstallFailed)
                    } else {
                        ""
                    },
                ),
                PromptButtons::InstallUpdate,
            ),
            FallbackPrompt::Recovery { error, .. } => (
                formatted::native_recovery(language, error),
                PromptButtons::RetryCancel,
            ),
            FallbackPrompt::Guard => {
                let name = self
                    .path
                    .as_deref()
                    .map(display_name)
                    .unwrap_or_else(|| image_paste::DEFAULT_NAME.into());
                let save = text(if self.current_document_untitled() {
                    Text::NativeSaveChooseLocation
                } else {
                    Text::NativeSaveReplacesSource
                });
                (
                    formatted::native_save_guard(language, &name, save),
                    PromptButtons::SaveDiscardCancel {
                        discard_all: matches!(
                            self.pending_guard,
                            Some(GuardedAction::Exit | GuardedAction::CoordinatedExit(_))
                        ),
                    },
                )
            }
            FallbackPrompt::ExportError => (
                formatted::native_export_error(
                    language,
                    self.export_error.as_deref().unwrap_or_default(),
                ),
                PromptButtons::Ok,
            ),
            FallbackPrompt::ExportBusy => (
                text(Text::NativeExportBusy).into(),
                PromptButtons::YesNoCancel,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MediaTime, PlaybackState, updates};

    #[test]
    fn japanese_native_prompt_content_preserves_actions_and_literal_user_values() {
        let Some(root) = crate::tests::isolated_test_root(
            "localization::native_prompt::tests::japanese_native_prompt_content_preserves_actions_and_literal_user_values",
        ) else {
            return;
        };
        let mut app = Application::new(None, |_| {}).expect("app");
        app.language_settings.display = Language::Japanese;
        app.path = Some(root.join("日本語 {source}.png"));
        app.pending_guard = Some(GuardedAction::Exit);
        let (message, buttons) = app.native_prompt_content(&FallbackPrompt::Guard);
        assert!(message.contains("日本語 {source}.png"));
        assert!(message.contains("元のファイルに上書き保存"));
        assert!(matches!(
            buttons,
            PromptButtons::SaveDiscardCancel { discard_all: true }
        ));
        assert!(!app.exit_requested);
        let (message, buttons) =
            app.native_prompt_content(&FallbackPrompt::UpdateNotice(updates::Notice {
                version: "1.0.3".parse().expect("version"),
                failed: true,
            }));
        assert!(message.contains("バージョン1.0.3") && message.contains("前回のインストール"));
        assert!(matches!(buttons, PromptButtons::InstallUpdate));
        let error = "external {error} 0x80004005";
        let (message, buttons) = app.native_prompt_content(&FallbackPrompt::Recovery {
            position: MediaTime::ZERO,
            state: PlaybackState::Paused,
            error: error.into(),
        });
        assert!(message.contains(error) && message.contains("再試行"));
        assert!(matches!(buttons, PromptButtons::RetryCancel));
        app.export_error = Some(error.into());
        let (message, buttons) = app.native_prompt_content(&FallbackPrompt::ExportError);
        assert!(message.contains(error) && message.starts_with("書き出しに失敗"));
        assert!(matches!(buttons, PromptButtons::Ok));
        let (_, buttons) = app.native_prompt_content(&FallbackPrompt::ExportBusy);
        assert!(matches!(buttons, PromptButtons::YesNoCancel));
    }
}
