//! Preserve history validation and I/O classification until window-local display.
use std::io;
use towavue_core::localization::{Language, Text};

#[derive(Debug, thiserror::Error)]
pub enum VideoResumeError {
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

impl VideoResumeError {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Message(reason) => reason.in_language(language).into(),
            Self::Io(error) => error
                .get_ref()
                .and_then(|cause| cause.downcast_ref::<Self>())
                .map_or_else(|| error.to_string(), |cause| cause.message(language)),
            Self::Diagnostic(detail) => detail.clone(),
        }
    }
}

impl From<String> for VideoResumeError {
    fn from(detail: String) -> Self {
        Self::Diagnostic(detail)
    }
}

impl From<&str> for VideoResumeError {
    fn from(detail: &str) -> Self {
        Self::Diagnostic(detail.into())
    }
}
