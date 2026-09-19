use super::*;
use towavue_runtime_windows::VideoExportQuality;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn video_export_quality(&self) -> VideoExportQuality {
        *self
            .video_export_quality
            .lock()
            .expect("video export quality")
    }

    pub(super) fn effective_video_export_quality(
        &self,
        kind: MediaKind,
        output: ExportOutput,
    ) -> VideoExportQuality {
        if kind == MediaKind::Video && output == ExportOutput::Media {
            self.video_export_quality()
        } else {
            VideoExportQuality::High
        }
    }

    pub(super) fn set_video_export_quality(&mut self, quality: VideoExportQuality) {
        // All hosted windows share this session setting. Workers own copied
        // ExportOptions so changing it never retunes an in-flight export.
        *self
            .video_export_quality
            .lock()
            .expect("video export quality") = quality;
        (self.notify)(AppEvent::VideoExportQualityChanged);
        self.set_status(format!("Video export quality: {}", quality.label()));
    }
}
