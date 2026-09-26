//! Keep owned clipboard-image failures typed until the receiving window displays them.
use towavue_core::localization::{Language, Text};

#[derive(Debug, thiserror::Error)]
pub enum ClipboardImageError {
    #[error("{}", .0.in_language(Language::English))]
    Message(Text),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Png(#[from] png::EncodingError),
    #[error(transparent)]
    FileOperation(#[from] crate::FileOperationError),
    #[error("{0}")]
    Diagnostic(String),
}

impl ClipboardImageError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Message(text) => text.in_language(language).into(),
            Self::FileOperation(error) => error.message(language),
            // Native clipboard, filesystem and PNG details remain literal.
            _ => self.to_string(),
        }
    }
}

impl From<String> for ClipboardImageError {
    fn from(detail: String) -> Self {
        Self::Diagnostic(detail)
    }
}

impl From<&str> for ClipboardImageError {
    fn from(detail: &str) -> Self {
        Self::Diagnostic(detail.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_input_errors_keep_typed_causes_and_native_diagnostics() {
        let error = ClipboardImageError::from(crate::FileOperationError::SourceChanged);
        assert_eq!(
            error.to_string(),
            "the source changed; reopen it before changing the file"
        );
        assert_eq!(
            error.message(Language::Japanese),
            crate::FileOperationError::SourceChanged.message(Language::Japanese)
        );
        let detail = "native {detail}: Ω / 0x80004005";
        let error = ClipboardImageError::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            detail,
        ));
        assert_eq!(error.to_string(), detail);
        assert_eq!(error.message(Language::Japanese), detail);
        assert!(
            matches!(error, ClipboardImageError::Io(ref io) if io.kind() == std::io::ErrorKind::PermissionDenied)
        );
        // An external string matching a catalog message is still external text.
        let error = ClipboardImageError::from("Image paste cancelled");
        assert_eq!(error.message(Language::Japanese), "Image paste cancelled");
    }
}
