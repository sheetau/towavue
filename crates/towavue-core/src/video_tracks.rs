/// A stream index scoped to one media source. Validate it before decoding.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct VideoTrackId(usize);

impl VideoTrackId {
    pub const fn from_index(index: usize) -> Self {
        Self(index)
    }
    pub const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoTrack {
    pub id: VideoTrackId,
    pub title: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VideoTrackCatalog {
    pub tracks: Vec<VideoTrack>,
    /// Timed video only; attached cover images are not playback choices.
    pub preferred: Option<VideoTrackId>,
}

impl VideoTrackCatalog {
    pub fn next_track(&self, selection: Option<VideoTrackId>) -> Option<VideoTrackId> {
        if self.tracks.is_empty() {
            return None;
        }
        let current = selection.or(self.preferred);
        let next = current
            .and_then(|id| self.tracks.iter().position(|track| track.id == id))
            .map_or(0, |index| (index + 1) % self.tracks.len());
        Some(self.tracks[next].id)
    }
}
