use crate::*;
use towavue_runtime_windows::VideoFrameSnapshot;

pub(super) struct PendingFrameExport {
    tab: TabId,
    generation: u64,
    frame: VideoFrameSnapshot,
    input: towavue_runtime_windows::MediaInput,
    operations: Vec<EditOperation>,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn capture_frame_export(&self) -> Option<PendingFrameExport> {
        if self.media_kind != Some(MediaKind::Video)
            || self.active_export.is_some()
            || self.modal_input_blocked()
        {
            return None;
        }
        let tab = self.tabs.active()?;
        let frame = self.session.as_ref()?.current_video_snapshot()?;
        let input = self.media_input_for(Some(tab.id), tab.target.current_path()?);
        if input.path() != frame.source_path() {
            return None;
        }
        Some(PendingFrameExport {
            tab: tab.id,
            generation: self.media_generation,
            frame,
            input,
            operations: self
                .edits
                .get(&tab.id)
                .map(|history| history.operations().to_vec())
                .unwrap_or_default(),
        })
    }

    pub(super) fn export_current_frame(&mut self) {
        let Some(intent) = self.capture_frame_export() else {
            self.set_status(
                "No current frame is available for export, or another dialog/export is active."
                    .into(),
            );
            return;
        };
        let stem = intent
            .input
            .logical_path()
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let suggested_name = format!(
            "{stem}-frame-{}ns.png",
            intent.frame.source_time().as_nanoseconds()
        );
        self.begin_dialog(
            FileDialogKind::SaveFrame { suggested_name },
            DialogIntent::ExportFrame(intent),
        );
    }
}

impl PendingFrameExport {
    pub(super) fn finish<N: Fn(AppEvent) + Send + Sync + 'static>(
        self,
        app: &mut Application<N>,
        result: Result<Option<PathBuf>, DialogError>,
    ) {
        let target = match result {
            Ok(Some(target)) => target,
            Ok(None) => return,
            Err(error) => {
                app.set_status(error.to_string());
                return;
            }
        };
        if self.generation != app.media_generation
            || app.active_export.is_some()
            || !app.tabs.active().is_some_and(|tab| {
                tab.id == self.tab
                    && tab.target.current_path() == Some(self.input.logical_path())
                    && tab
                        .target
                        .current_path()
                        .is_some_and(|path| app.media_input_for(Some(tab.id), path) == self.input)
            })
        {
            app.set_status(
                "The source changed while choosing a frame export path; nothing was exported."
                    .into(),
            );
            return;
        }
        let request = ExportRequest {
            source: self.input.logical_path().to_owned(),
            target: target.clone(),
            kind: MediaKind::Video,
            operations: self.operations,
            hardware_encode: false,
        };
        let notify = Arc::clone(&app.notify);
        match ExportJob::start_video_frame_input(
            self.frame,
            self.input,
            target,
            request.operations.clone(),
            move |event| notify(AppEvent::Export(event)),
        ) {
            Ok(job) => {
                let options = ExportOptions {
                    output: ExportOutput::VideoFrame,
                    ..Default::default()
                };
                app.active_export = Some(ActiveExport {
                    progress: export_progress::ExportProgress::new(&request, &options, None),
                    job: job.into(),
                    tab: self.tab,
                    request,
                    options,
                    encoded: Duration::ZERO,
                    analyzing_audio: false,
                    cancelling: false,
                    continuation: None,
                });
                app.refresh_title();
                app.request_redraw();
            }
            Err(error) => {
                app.export_error = Some(error.to_string());
                app.request_redraw();
            }
        }
    }
}

#[cfg(test)]
mod tests;
