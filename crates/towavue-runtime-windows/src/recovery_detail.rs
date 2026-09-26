//! Keep recovery causes and every preserved path until the UI selects a language.
use std::{io, path::PathBuf};
use towavue_core::localization::{Language, Text, formatted};

#[derive(Debug, thiserror::Error)]
pub enum RecoveryDetail {
    #[error("{}", .0.in_language(Language::English))]
    Message(Text),
    #[error("{0}")]
    Io(#[source] io::Error),
    #[error("{0}")]
    File(#[source] Box<crate::FileOperationError>),
    #[error("{0}")]
    Save(#[source] Box<crate::SourceSaveError>),
    #[error("{source}; document original retained in {directory}")]
    RetainedOriginal {
        #[source]
        source: Box<Self>,
        directory: PathBuf,
    },
    #[error("{0}")]
    Detail(String),
}

impl RecoveryDetail {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Message(text) => text.in_language(language).into(),
            Self::Io(error) => Self::io_message(error, language),
            Self::File(error) => error.message(language),
            Self::Save(error) => error.message(language),
            Self::RetainedOriginal { source, directory } => formatted::recovery_original_retained(
                language,
                &source.message(language),
                &directory.display().to_string(),
            ),
            Self::Detail(detail) => detail.clone(),
        }
    }

    pub(crate) fn io_message(error: &io::Error, language: Language) -> String {
        crate::io_error_message(error, language)
    }

    pub(crate) fn with_original(self, directory: PathBuf) -> Self {
        Self::RetainedOriginal {
            source: Box::new(self),
            directory,
        }
    }
}

impl From<io::Error> for RecoveryDetail {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<crate::FileOperationError> for RecoveryDetail {
    fn from(error: crate::FileOperationError) -> Self {
        Self::File(Box::new(error))
    }
}

impl From<crate::SourceSaveError> for RecoveryDetail {
    fn from(error: crate::SourceSaveError) -> Self {
        Self::Save(Box::new(error))
    }
}

impl From<Text> for RecoveryDetail {
    fn from(text: Text) -> Self {
        Self::Message(text)
    }
}

impl From<String> for RecoveryDetail {
    fn from(detail: String) -> Self {
        Self::Detail(detail)
    }
}

impl From<&str> for RecoveryDetail {
    fn from(detail: &str) -> Self {
        Self::Detail(detail.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileOperationError, SourceSaveError};
    use std::error::Error;

    #[test]
    fn nested_recovery_keeps_native_causes_and_both_original_paths() {
        let original = PathBuf::from("日本語{original}/retained");
        let destination = PathBuf::from("日本語{destination}/backup");
        let native = io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native {detail} 日本語.png",
        );
        let detail = RecoveryDetail::from(SourceSaveError::Source(FileOperationError::Io(native)))
            .with_original(original.clone());
        let error = SourceSaveError::RecoveryRequired {
            message: detail,
            directory: destination.clone(),
        };
        let english = format!(
            "source replacement needs recovery: file operation failed: native {{detail}} 日本語.png; document original retained in {}; preserved files: {}",
            original.display(),
            destination.display()
        );
        assert_eq!(error.to_string(), english);
        assert_eq!(error.message(Language::English), english);
        let japanese = error.message(Language::Japanese);
        assert!(japanese.contains("native {detail} 日本語.png"));
        assert!(japanese.contains(&original.display().to_string()));
        assert!(japanese.contains(&destination.display().to_string()));
        assert!(
            !japanese.contains("document original retained")
                && !japanese.contains("file operation failed")
        );
        let mut cause = error.source();
        let mut native_cause = None;
        while let Some(current) = cause {
            if let Some(error) = current.downcast_ref::<io::Error>() {
                native_cause = Some(error);
                break;
            }
            cause = current.source();
        }
        assert_eq!(
            native_cause.expect("original native cause").kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn owned_recovery_reasons_and_recycle_causes_translate_across_worker_delivery() {
        for (text, english) in [
            (
                Text::PublicationWorkerStopped,
                "publication worker stopped unexpectedly",
            ),
            (
                Text::SaveAsPublicationStopped,
                "Save as publication stopped unexpectedly",
            ),
            (
                Text::ReplacementIdentitiesChanged,
                "replacement identities changed",
            ),
        ] {
            let error = io::Error::other(RecoveryDetail::from(text));
            assert_eq!(error.kind(), io::ErrorKind::Other);
            let error = SourceSaveError::Io(error);
            assert_eq!(error.to_string(), format!("source save failed: {english}"));
            assert!(
                error
                    .message(Language::Japanese)
                    .contains(text.in_language(Language::Japanese))
            );
        }
        let error = FileOperationError::RecoveryRequired {
            message: FileOperationError::SourceChanged.into(),
            directory: PathBuf::from("日本語{recovery}"),
        };
        let expected = error.message(Language::Japanese);
        assert!(expected.contains(Text::FileSourceChanged.in_language(Language::Japanese)));
        assert!(!expected.contains("reopen it"));
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || sender.send(error).expect("worker delivery"))
            .join()
            .expect("worker");
        assert_eq!(
            receiver
                .recv()
                .expect("typed recovery")
                .message(Language::Japanese),
            expected
        );
    }
}
