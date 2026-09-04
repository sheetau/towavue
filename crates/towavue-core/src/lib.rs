//! Platform-independent state and interface definitions for towavue.

#![forbid(unsafe_code)]

mod commands;
mod media;
mod navigation;
mod tabs;

use std::fmt;
use std::time::Duration;

pub use commands::{
    CommandContext, CommandDefinition, CommandId, Key, KeySequence, KeyStroke, Modifiers,
    ShortcutBindings, ShortcutMatch, command_definitions,
};
pub use media::MediaKind;
pub use navigation::{
    FolderMediaItem, FolderSnapshot, FolderSnapshotSource, PropertyKey, ShellIdentity, SortColumn,
    SortDirection,
};
pub use tabs::{Tab, TabId, TabSet, TabTarget};

/// A signed media timestamp stored as nanoseconds.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct MediaTime(i64);

impl MediaTime {
    /// The zero timestamp.
    pub const ZERO: Self = Self(0);

    /// Creates a timestamp from nanoseconds.
    pub const fn from_nanoseconds(value: i64) -> Self {
        Self(value)
    }

    /// Returns the timestamp in nanoseconds.
    pub const fn as_nanoseconds(self) -> i64 {
        self.0
    }

    /// Returns the timestamp in seconds for presentation timing.
    pub fn as_seconds_f64(self) -> f64 {
        self.0 as f64 / 1_000_000_000.0
    }

    pub fn saturating_add(self, duration: Duration) -> Self {
        let nanoseconds = duration.as_nanos().min(i64::MAX as u128) as i64;
        Self(self.0.saturating_add(nanoseconds))
    }

    pub fn saturating_sub(self, duration: Duration) -> Self {
        let nanoseconds = duration.as_nanos().min(i64::MAX as u128) as i64;
        Self(self.0.saturating_sub(nanoseconds))
    }
}

impl fmt::Display for MediaTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:.3}s", self.as_seconds_f64())
    }
}

/// The user-visible lifecycle of the single M1 playback session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackState {
    #[default]
    Loading,
    Playing,
    Paused,
    Ended,
    Faulted,
}

/// Identifies results belonging to one open or seek transaction.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlaybackGeneration(u64);

impl PlaybackGeneration {
    pub const INITIAL: Self = Self(0);

    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{MediaTime, PlaybackGeneration, PlaybackState};

    #[test]
    fn media_time_preserves_signed_nanoseconds() {
        let time = MediaTime::from_nanoseconds(-1_250_000_000);

        assert_eq!(time.as_nanoseconds(), -1_250_000_000);
        assert_eq!(time.as_seconds_f64(), -1.25);
        assert_eq!(time.to_string(), "-1.250s");
    }

    #[test]
    fn playback_starts_in_loading_state() {
        assert_eq!(PlaybackState::default(), PlaybackState::Loading);
    }

    #[test]
    fn media_time_arithmetic_saturates() {
        assert_eq!(
            MediaTime::from_nanoseconds(i64::MAX - 1)
                .saturating_add(Duration::from_nanos(2))
                .as_nanoseconds(),
            i64::MAX
        );
        assert_eq!(
            MediaTime::ZERO
                .saturating_sub(Duration::from_secs(5))
                .as_nanoseconds(),
            -5_000_000_000
        );
    }

    #[test]
    fn playback_generation_advances_with_wrapping_identity() {
        let generation = PlaybackGeneration::INITIAL.next();

        assert_eq!(generation.value(), 1);
        assert_ne!(generation, PlaybackGeneration::INITIAL);
    }
}
