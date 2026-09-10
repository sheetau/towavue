use crate::*;
use towavue_runtime_windows::{VideoOrientation, video_edit_geometry};

pub(super) struct VideoEditSnapshot {
    pub tab: TabId,
    pub path: PathBuf,
    pub media_generation: u64,
    pub generation: PlaybackGeneration,
    pub source: (u32, u32, f32),
    pub orientation: VideoOrientation,
    pub max_side: usize,
    pub operations: Vec<EditOperation>,
    pub geometry: (u32, u32, f32),
}

impl VideoEditSnapshot {
    pub fn validate(&self, operation: EditOperation) -> Result<(), String> {
        let mut edits = self.operations.clone();
        edits.push(operation);
        video_edit_geometry(
            (self.source.0, self.source.1),
            self.source.2,
            self.orientation,
            &edits,
            self.max_side,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn capture_video_edit(&self) -> Result<VideoEditSnapshot, String> {
        let geometry = self.validate_video_operations(self.video_operations())?;
        let tab = self.tabs.active().ok_or("Video tab is unavailable")?;
        let path = self.path.as_ref().ok_or("Video path is unavailable")?;
        let session = self.session.as_ref().expect("validated video session");
        Ok(VideoEditSnapshot {
            tab: tab.id,
            path: path.clone(),
            media_generation: self.media_generation,
            generation: self.generation,
            source: session.video_geometry().expect("validated frame"),
            orientation: session.video_orientation().unwrap_or_default(),
            max_side: self
                .renderer
                .as_ref()
                .expect("validated renderer")
                .max_texture_side(),
            operations: self.video_operations().to_vec(),
            geometry,
        })
    }

    pub(super) fn video_edit_is_current(&self, snapshot: &VideoEditSnapshot) -> bool {
        self.media_kind == Some(MediaKind::Video)
            && self.timeline_open
            && !self.fullscreen
            && self.tabs.active().is_some_and(|tab| tab.id == snapshot.tab)
            && self.path.as_ref() == Some(&snapshot.path)
            && self.media_generation == snapshot.media_generation
            && self.generation == snapshot.generation
            && self.video_operations() == snapshot.operations
            && self
                .session
                .as_ref()
                .and_then(PlaybackSession::video_geometry)
                == Some(snapshot.source)
            && self
                .session
                .as_ref()
                .and_then(PlaybackSession::video_orientation)
                == Some(snapshot.orientation)
            && self
                .renderer
                .as_ref()
                .is_some_and(|renderer| renderer.max_texture_side() == snapshot.max_side)
    }
}
