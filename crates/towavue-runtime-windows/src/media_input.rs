use crate::RetainedSource;
use std::path::{Path, PathBuf};

/// A logical media path plus an optional retained, immutable original input.
/// Clone this into every worker/session that can outlive its tab's current state.
#[derive(Clone)]
pub struct MediaInput {
    logical: PathBuf,
    original: Option<RetainedSource>,
}
impl MediaInput {
    pub fn new(path: PathBuf) -> Self {
        Self {
            logical: path,
            original: None,
        }
    }
    pub fn retained(path: PathBuf, original: impl Into<RetainedSource>) -> Self {
        Self {
            logical: path,
            original: Some(original.into()),
        }
    }
    pub fn logical_path(&self) -> &Path {
        &self.logical
    }
    pub fn path(&self) -> &Path {
        self.original
            .as_ref()
            .map_or(self.logical.as_path(), RetainedSource::original_path)
    }
}
impl std::fmt::Debug for MediaInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaInput")
            .field("logical", &self.logical)
            .field("input", &self.path())
            .finish()
    }
}
impl PartialEq for MediaInput {
    fn eq(&self, other: &Self) -> bool {
        self.logical == other.logical && self.path() == other.path()
    }
}
impl Eq for MediaInput {}
