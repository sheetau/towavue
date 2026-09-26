use towavue_core::localization::{Language, Text, formatted};

#[derive(Debug, thiserror::Error)]
pub enum FileSearchFailure {
    #[error("{}", Text::FileSearchInvalidRequest.in_language(Language::English))]
    InvalidRequest,
    #[error("Cannot search this folder: {0}")]
    Folder(#[source] std::io::Error),
    #[error("{0}")]
    Diagnostic(String),
}

impl FileSearchFailure {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::InvalidRequest => Text::FileSearchInvalidRequest.in_language(language).into(),
            Self::Folder(error) => {
                formatted::file_search_folder_failed(language, &error.to_string())
            }
            Self::Diagnostic(detail) => detail.clone(),
        }
    }
}

impl From<String> for FileSearchFailure {
    fn from(detail: String) -> Self {
        Self::Diagnostic(detail)
    }
}

impl From<&str> for FileSearchFailure {
    fn from(detail: &str) -> Self {
        Self::Diagnostic(detail.into())
    }
}
