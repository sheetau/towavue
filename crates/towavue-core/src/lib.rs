//! Platform-independent state and interface definitions for towavue.

#![forbid(unsafe_code)]

use std::fmt;

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

#[cfg(test)]
mod tests {
    use super::{MediaTime, PlaybackState};

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
}
