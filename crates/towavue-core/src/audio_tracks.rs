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
