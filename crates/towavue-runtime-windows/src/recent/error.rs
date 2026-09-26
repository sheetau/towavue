use std::{fmt, io, sync::Arc};
use towavue_core::localization::{Language, Text, formatted};

#[derive(Debug, thiserror::Error)]
#[error("{}", .0.in_language(Language::English))]
struct HistoryReason(Text);

pub(super) fn invalid_reason(reason: Text) -> io::Error {
    io::Error::other(HistoryReason(reason))
}

fn io_message(error: &io::Error, language: Language) -> String {
    error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<HistoryReason>())
        .map_or_else(
            || error.to_string(),
            |reason| reason.0.in_language(language).into(),
        )
}

/// Shared immutable causes survive successive history/presence publications.
#[derive(Clone, Debug)]
pub struct RecentFailure {
    files: Option<Arc<io::Error>>,
    commands: Option<Arc<io::Error>>,
}

impl RecentFailure {
    pub(super) fn new(files: Option<io::Error>, commands: Option<io::Error>) -> Option<Self> {
        (files.is_some() || commands.is_some()).then(|| Self {
            files: files.map(Arc::new),
            commands: commands.map(Arc::new),
        })
    }

    pub fn message(&self, language: Language) -> String {
        let mut parts = Vec::with_capacity(2);
        if let Some(error) = &self.files {
            parts.push(formatted::recent_files_unavailable(
                language,
                &io_message(error, language),
            ));
        }
        if let Some(error) = &self.commands {
            parts.push(formatted::command_history_unavailable(
                language,
                &io_message(error, language),
            ));
        }
        parts.join("; ")
    }
}

impl fmt::Display for RecentFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message(Language::English))
    }
}

impl std::error::Error for RecentFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.files
            .as_deref()
            .or(self.commands.as_deref())
            .map(|error| error as &(dyn std::error::Error + 'static))
    }
}
