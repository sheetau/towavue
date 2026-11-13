/// A stream index scoped to one media source, never a native handle. Validate it
/// against the current source before decoding; it is not a global track identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AudioTrackId(usize);

impl AudioTrackId {
    pub const fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioTrack {
    pub id: AudioTrackId,
    pub title: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AudioTrackCatalog {
    pub tracks: Vec<AudioTrack>,
    /// The source's preferred stream, including the container's default policy.
    pub preferred: Option<AudioTrackId>,
}

/// Listening preference only. Export retention has its own independent choice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AudioTrackSelection {
    #[default]
    Default,
    Track(AudioTrackId),
    All,
}

/// Source-scoped output retention, independent of the listening selection.
/// An empty explicit selection creates a video with no audio tracks.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum AudioTrackRetention {
    #[default]
    All,
    Selected(Vec<AudioTrackId>),
}

impl AudioTrackCatalog {
    /// Cycle concrete tracks in source order; All starts at the first track.
    pub fn next_track(&self, selection: AudioTrackSelection) -> Option<AudioTrackId> {
        if self.tracks.is_empty() {
            return None;
        }
        let current = match selection {
            AudioTrackSelection::Default => self.preferred,
            AudioTrackSelection::Track(track) => Some(track),
            AudioTrackSelection::All => None,
        };
        let next = current
            .and_then(|id| self.tracks.iter().position(|track| track.id == id))
            .map_or(0, |index| (index + 1) % self.tracks.len());
        Some(self.tracks[next].id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycling_uses_source_order_preferred_track_and_wraps_without_all() {
        let ids = [2, 5, 8].map(AudioTrackId::from_index);
        let mut catalog = AudioTrackCatalog {
            tracks: ids
                .iter()
                .map(|id| AudioTrack {
                    id: *id,
                    title: None,
                    language: None,
                })
                .collect(),
            preferred: Some(ids[1]),
        };
        assert_eq!(
            catalog.next_track(AudioTrackSelection::Default),
            Some(ids[2])
        );
        assert_eq!(catalog.next_track(AudioTrackSelection::All), Some(ids[0]));
        assert_eq!(
            catalog.next_track(AudioTrackSelection::Track(ids[2])),
            Some(ids[0])
        );
        assert_eq!(
            catalog.next_track(AudioTrackSelection::Track(ids[0])),
            Some(ids[1])
        );
        assert_eq!(
            catalog.next_track(AudioTrackSelection::Track(AudioTrackId::from_index(77))),
            Some(ids[0])
        );
        catalog.tracks.clear();
        assert_eq!(catalog.next_track(AudioTrackSelection::Default), None);
        assert_eq!(catalog.next_track(AudioTrackSelection::All), None);
    }
}

/// The target of an output edit, captured independently of the listening selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioTrackTarget {
    All,
    Track(AudioTrackId),
}

/// Undoable track inclusion. It never changes playback volume or waveform samples.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioTrackOutput {
    enabled_by_default: bool,
    exceptions: Vec<AudioTrackId>,
}

impl Default for AudioTrackOutput {
    fn default() -> Self {
        Self {
            enabled_by_default: true,
            exceptions: Vec::new(),
        }
    }
}

impl AudioTrackOutput {
    pub fn is_enabled(&self, track: AudioTrackId) -> bool {
        self.enabled_by_default != self.exceptions.contains(&track)
    }

    pub fn set_enabled(&mut self, target: AudioTrackTarget, enabled: bool) {
        match target {
            AudioTrackTarget::All => {
                self.enabled_by_default = enabled;
                self.exceptions.clear();
            }
            AudioTrackTarget::Track(track) => {
                self.exceptions.retain(|id| *id != track);
                if enabled != self.enabled_by_default {
                    self.exceptions.push(track);
                    self.exceptions.sort_by_key(|id| id.index());
                }
            }
        }
    }
}
