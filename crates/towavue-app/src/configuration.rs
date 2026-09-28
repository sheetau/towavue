//! Keep configuration failures as data until the host's display language is known.
use crate::localization::{Language, Text};
use std::path::PathBuf;
use towavue_core::localization::formatted;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub enum Error {
    Text(Text),
    External(String),
    MissingEquals { file: &'static str, line: usize },
    UnknownCommand { file: &'static str, line: usize },
    InvalidShortcut { line: usize },
}

impl Error {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::Text(text) => text.in_language(language).into(),
            Self::External(error) => error.clone(),
            Self::MissingEquals { file, line } => {
                formatted::configuration_missing_equals(language, file, *line)
            }
            Self::UnknownCommand { file, line } => {
                formatted::configuration_unknown_command(language, file, *line)
            }
            Self::InvalidShortcut { line } => {
                formatted::configuration_invalid_shortcut(language, *line)
            }
        }
    }
}

// Diagnostics stay English; UI callers explicitly select the captured host language.
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message(Language::English))
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::External(error.to_string())
    }
}

pub(super) fn path(root: Option<std::ffi::OsString>, name: &str) -> Result<PathBuf, Error> {
    let root = root.ok_or(Error::Text(Text::AppDataUnavailable))?;
    Ok(PathBuf::from(root).join("towavue").join(name))
}

pub struct Warning {
    entries: Vec<(PathBuf, Error)>,
}

impl Warning {
    pub fn new(entries: Vec<(PathBuf, Error)>) -> Option<Self> {
        (!entries.is_empty()).then_some(Self { entries })
    }

    pub fn message(&self, language: Language) -> String {
        let details = self
            .entries
            .iter()
            .map(|(path, error)| format!("{}\n{}", path.display(), error.message(language)))
            .collect::<Vec<_>>()
            .join("\n\n");
        formatted::configuration_fallback(
            language,
            &details,
            Text::MenuFile.in_language(language),
            Text::CommandReloadShortcuts.in_language(language),
        )
    }
}

impl<N: Fn(crate::AppEvent) + Send + Sync + 'static> crate::Application<N> {
    pub(super) fn show_configuration_warning(&mut self) {
        if let Some(warning) = self.configuration_warning.take() {
            let message = warning.message(self.language());
            self.set_status(message.clone());
            self.open_native_prompt(crate::FallbackPrompt::ConfigurationWarning(message));
        }
    }
}
