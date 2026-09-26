use std::io;
use towavue_core::localization::{Language, Text};

#[derive(Debug, thiserror::Error)]
pub enum LanguagePreferenceError {
    #[error("{}", .0.in_language(Language::English))]
    Message(Text),
    #[error("{0}")]
    Io(
        #[from]
        #[source]
        io::Error,
    ),
    #[error("{0}")]
    Diagnostic(String),
}

impl LanguagePreferenceError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Message(reason) => reason.in_language(language).into(),
            Self::Io(error) => Self::io_message(error, language),
            Self::Diagnostic(detail) => detail.clone(),
        }
    }

    pub fn io_message(error: &io::Error, language: Language) -> String {
        error
            .get_ref()
            .and_then(|cause| cause.downcast_ref::<Self>())
            .map_or_else(|| error.to_string(), |cause| cause.message(language))
    }
}

impl From<String> for LanguagePreferenceError {
    fn from(detail: String) -> Self {
        Self::Diagnostic(detail)
    }
}

impl From<&str> for LanguagePreferenceError {
    fn from(detail: &str) -> Self {
        Self::Diagnostic(detail.into())
    }
}
