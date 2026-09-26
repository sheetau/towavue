//! Owned native-window helper advice, separate from literal platform diagnostics.
use towavue_core::localization::{Language, Text};

#[derive(Debug, thiserror::Error)]
pub enum NativeUiError {
    #[error("{}", .0.in_language(Language::English))]
    Message(Text),
    #[error("{0}")]
    Diagnostic(String),
}

impl NativeUiError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Message(reason) => reason.in_language(language).into(),
            Self::Diagnostic(detail) => detail.clone(),
        }
    }
}

/// Localize known owned startup/decoration causes before a boxed error is flattened.
/// Unknown OS/library details keep their complete diagnostic text.
pub fn native_ui_error_message(
    error: &(dyn std::error::Error + 'static),
    language: Language,
) -> String {
    if let Some(error) = error.downcast_ref::<NativeUiError>() {
        error.message(language)
    } else if let Some(error) = error.downcast_ref::<crate::RenderError>() {
        error.message(language)
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxed_window_helpers_localize_owned_reasons_and_keep_unknown_details() {
        let error: Box<dyn std::error::Error> =
            NativeUiError::Message(Text::NativeCaptionInstallFailed).into();
        assert_eq!(
            native_ui_error_message(error.as_ref(), Language::English),
            "Could not install the native caption"
        );
        assert_eq!(
            native_ui_error_message(error.as_ref(), Language::Japanese),
            "ネイティブタイトルバーを設定できませんでした"
        );
        let error: Box<dyn std::error::Error> = crate::RenderError::SurfaceNotSized.into();
        assert_eq!(
            native_ui_error_message(error.as_ref(), Language::Japanese),
            crate::RenderError::SurfaceNotSized.message(Language::Japanese)
        );
        let error: Box<dyn std::error::Error> =
            std::io::Error::other("native {detail}: Ω / 0x80004005").into();
        assert_eq!(
            native_ui_error_message(error.as_ref(), Language::Japanese),
            "native {detail}: Ω / 0x80004005"
        );
        let error = NativeUiError::Diagnostic("Could not install the native caption".into());
        assert_eq!(
            error.message(Language::Japanese),
            "Could not install the native caption"
        );
        let error = NativeUiError::Message(Text::NativeCursorLockPosition);
        assert_eq!(
            error.to_string(),
            "Cursor locking requires a focused window and an interior position"
        );
        assert_eq!(
            error.message(Language::Japanese),
            "カーソルを固定するには、ウィンドウがアクティブで、位置がウィンドウ内にある必要があります"
        );
    }
}
