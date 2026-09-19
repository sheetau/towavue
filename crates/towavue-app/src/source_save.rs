use crate::*;
use towavue_runtime_windows::{
    FileOperationSource, MediaInput, PreparedSourceSave, SavedSource, SourceSaveEvent,
    SourceSaveJob,
};

#[cfg(test)]
pub(crate) mod tests;

pub(super) enum Task {
    Export(ExportJob),
    Save(SourceSaveJob),
    SaveAs(towavue_runtime_windows::SaveAsJob),
    Publishing,
}
impl From<ExportJob> for Task {
    fn from(job: ExportJob) -> Self {
        Self::Export(job)
    }
}
impl Task {
    pub fn cancellable(&self) -> bool {
        !matches!(self, Self::Publishing)
    }
    pub fn is_save(&self) -> bool {
        !matches!(self, Self::Export(_))
    }
    pub fn cancel(&self) {
        match self {
            Self::Export(job) => job.cancel(),
            Self::Save(job) => job.cancel(),
            Self::SaveAs(job) => job.cancel(),
            Self::Publishing => {}
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    pub serial: u64,
    pub pending: Option<Pending>,
    pub save_as: Option<crate::save_as::Pending>,
    pub frozen: bool,
    pub deferred: Vec<AppEvent>,
}
pub(super) struct Pending {
    pub serial: u64,
    pub expected: FileOperationSource,
    pub input: MediaInput,
    pub recreating: bool,
    pub prepared: Option<PreparedSourceSave>,
}

pub(super) struct PublicationError {
    pub message: String,
    pub source_uncertain: bool,
}
impl From<String> for PublicationError {
    fn from(message: String) -> Self {
        Self {
            message,
            source_uncertain: false,
        }
    }
}
impl From<&str> for PublicationError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
impl From<towavue_runtime_windows::SourceSaveError> for PublicationError {
    fn from(error: towavue_runtime_windows::SourceSaveError) -> Self {
        let source_uncertain = matches!(
            error,
            towavue_runtime_windows::SourceSaveError::RecoveryRequired { .. }
                | towavue_runtime_windows::SourceSaveError::Source(_)
        );
        Self {
            message: error.to_string(),
            source_uncertain,
        }
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn save_source(&mut self, continuation: Option<GuardedAction>) -> bool {
        if self.modal_input_blocked()
            || self.active_export.is_some()
            || self.image_loading
            || self.image_edit_pending
            || self.image_handoff.is_some()
            || self.state == PlaybackState::Loading
        {
            return false;
        }
        let Some(tab) = self.tabs.active() else {
            return false;
        };
        let id = tab.id;
        let source = tab.target.current_path().to_owned();
        let Some(expected) = self
            .source_versions
            .get(&id)
            .and_then(Option::as_ref)
            .cloned()
        else {
            self.export_error = Some("The loaded source version is unavailable. Reopen the file before saving, or use Save as to keep the current edits.".into());
            self.request_redraw();
            return false;
        };
        let request = ExportRequest {
            source: source.clone(),
            target: source.clone(),
            kind: tab.target.media_kind(),
            operations: self
                .edits
                .get(&id)
                .map_or_else(Vec::new, |history| history.operations().to_vec()),
            hardware_encode: self.prefer_hardware_encode,
        };
        let options = ExportOptions {
            output: ExportOutput::Media,
            video_quality: self.effective_video_export_quality(request.kind, ExportOutput::Media),
            audio: self
                .audio_export_settings
                .get(&id)
                .copied()
                .unwrap_or_default(),
            metadata: self
                .metadata_export_settings
                .get(&id)
                .cloned()
                .unwrap_or_default(),
        };
        if request.operations.is_empty()
            && options.video_quality == towavue_runtime_windows::VideoExportQuality::High
            && options.audio == AudioExportOptions::default()
            && options.metadata.is_empty()
            && !self.source_backings.contains_key(&id)
            && self
                .edits
                .get(&id)
                .is_none_or(|history| !history.is_dirty())
        {
            self.set_status("No changes to save.".into());
            if let Some(action) = continuation {
                self.request_guarded(action);
            }
            return true;
        }
        let input = self.media_input_for(Some(id), &source);
        let mut worker_request = request.clone();
        worker_request.source = input.path().to_owned();
        self.source_save.serial = self.source_save.serial.wrapping_add(1);
        let serial = self.source_save.serial;
        let notify = Arc::clone(&self.notify);
        // The pending owner survives until the preparation job has been joined.
        let recreating = self.deleted_sources.contains_key(&id);
        let notify = move |event| notify(AppEvent::SourceSave(serial, event));
        let job = if recreating {
            SourceSaveJob::start_recreating(
                expected.clone(),
                self.source_backings[&id].clone(),
                worker_request,
                options.clone(),
                notify,
            )
        } else {
            SourceSaveJob::start(expected.clone(), worker_request, options.clone(), notify)
        };
        match job {
            Ok(job) => {
                self.source_save.pending = Some(Pending {
                    serial,
                    expected,
                    input,
                    recreating,
                    prepared: None,
                });
                self.active_export = Some(ActiveExport {
                    job: Task::Save(job),
                    tab: id,
                    progress: export_progress::ExportProgress::new(
                        &request,
                        &options,
                        self.media_duration,
                    ),
                    analyzing_audio: options.audio.normalization.is_enabled(),
                    request,
                    options,
                    encoded: Duration::ZERO,
                    cancelling: false,
                    continuation,
                });
                self.refresh_title();
                self.request_redraw();
                true
            }
            Err(error) => {
                self.export_error = Some(error.to_string());
                self.request_redraw();
                false
            }
        }
    }

    pub(super) fn source_save_is_current(&self) -> bool {
        let (Some(pending), Some(export)) = (&self.source_save.pending, &self.active_export) else {
            return false;
        };
        self.tabs
            .tabs()
            .iter()
            .any(|tab| tab.id == export.tab && tab.target.current_path() == export.request.source)
            && self
                .source_versions
                .get(&export.tab)
                .and_then(Option::as_ref)
                == Some(&pending.expected)
            && self.media_input_for(Some(export.tab), &export.request.source) == pending.input
            && self.deleted_sources.contains_key(&export.tab) == pending.recreating
    }

    pub(super) fn handle_source_save(&mut self, serial: u64, event: SourceSaveEvent) {
        if self
            .source_save
            .pending
            .as_ref()
            .is_none_or(|pending| pending.serial != serial)
        {
            return;
        }
        match event {
            SourceSaveEvent::Progress(time) => {
                self.handle_export_event(ExportEvent::Progress(time))
            }
            SourceSaveEvent::AnalyzingAudio(time) => {
                self.handle_export_event(ExportEvent::AnalyzingAudio(time))
            }
            SourceSaveEvent::Prepared(result) => match result {
                Ok(prepared)
                    if self.source_save_is_current()
                        && self
                            .active_export
                            .as_ref()
                            .is_some_and(|export| !export.cancelling) =>
                {
                    self.source_save
                        .pending
                        .as_mut()
                        .expect("save owner")
                        .prepared = Some(prepared);
                }
                Ok(_) => self
                    .finish_source_save(Err("Save cancelled before replacing the source.".into())),
                Err(error) => self.finish_source_save(Err(error.to_string())),
            },
        }
        self.request_redraw();
    }

    pub(super) fn finish_source_save(&mut self, result: Result<(), String>) {
        let pending = self.source_save.pending.take();
        let save_as = self.source_save.save_as.take();
        let export = self.active_export.take();
        if let Some(export) = export {
            match result {
                Ok(()) => {
                    self.set_status(format!("Saved {}", export.request.target.display()));
                    self.export_notice = self
                        .status_message
                        .as_ref()
                        .map(|(_, shown)| (*shown, export.request.target.clone()));
                    if let Some(action) = export.continuation {
                        if self.native_prompt.is_some() {
                            self.pending_guard = Some(action);
                        } else {
                            self.request_guarded(action);
                        }
                    }
                }
                Err(error) => {
                    if export.cancelling {
                        self.set_status(error);
                    } else {
                        self.export_error = Some(error);
                    }
                    self.pending_guard = export.continuation.or(self.pending_guard.take());
                }
            }
        }
        drop(pending);
        drop(save_as);
        self.refresh_title();
        self.request_redraw();
    }

    pub(super) fn quiesce_source_save(&mut self, source: &Path) {
        self.quiesce_file_relocation(source);
        self.source_save.frozen = true;
        self.image_loader.clear();
        self.image_preview_worker.clear();
        self.image_edit_worker.clear();
        self.playlist_duration_worker.clear();
        self.waveform_worker.clear();
        self.thumbnail_worker.clear();
        self.thumbnail_loading = None;
        self.waveform_loading = false;
        self.waveform_detail.restart_pending();
        for worker in self.duration_workers.values() {
            worker.clear();
        }
        if let Some(worker) = &self.metadata_worker {
            worker.clear();
        }
        self.filmstrip.clear_previews();
        self.palette.clear_preview();
        self.video_sheets.clear();
        self.tab_preview.clear();
        self.status_file_details.invalidate();
    }

    pub(super) fn source_readers_idle(&self) -> bool {
        self.image_loader.is_idle()
            && self.image_preview_worker.is_idle()
            && self.image_edit_worker.is_idle()
            && self.playlist_duration_worker.is_idle()
            && self.waveform_worker.is_idle()
            && self.thumbnail_worker.is_idle()
            && self.duration_workers.values().all(LatestTask::is_idle)
            && self
                .metadata_worker
                .as_ref()
                .is_none_or(LatestTask::is_idle)
            && self.filmstrip.is_idle()
            && self.palette.preview_is_idle()
            && self.video_sheets.is_idle()
            && self.tab_preview.is_idle()
            && self.frame_steps.is_idle()
            && self.status_file_details.is_idle()
    }

    pub(super) fn install_saved_source(
        &mut self,
        source: &Path,
        input: &MediaInput,
        saved: &SavedSource,
        request: &ExportRequest,
        options: &ExportOptions,
    ) {
        let ids: Vec<_> = self
            .tabs
            .tabs()
            .iter()
            .filter(|tab| tab.target.current_path() == source)
            .map(|tab| tab.id)
            .collect();
        for id in ids {
            if let Some(queue) = self.audio_queues.get_mut(&id) {
                queue.refresh_after_save();
            }
            // An unopened tab has no old document to retain: its first load reads
            // the newly saved disk file. Already loaded documents keep their base.
            if !self.source_versions.contains_key(&id) {
                self.retained_playback.remove(&id);
                continue;
            }
            let same_input = self.media_input_for(Some(id), source) == *input;
            self.source_backings
                .entry(id)
                .or_insert_with(|| saved.retained_source());
            self.deleted_sources.remove(&id);
            if let Some(queue) = self.audio_queues.get_mut(&id) {
                queue.clear_deleted_source();
            }
            self.source_versions
                .insert(id, Some(saved.current_source().clone()));
            let same_quality = self.effective_video_export_quality(request.kind, options.output)
                == options.video_quality;
            let history = self.edits.entry(id).or_default();
            if same_input
                && same_quality
                && self
                    .audio_export_settings
                    .get(&id)
                    .copied()
                    .unwrap_or_default()
                    == options.audio
                && self
                    .metadata_export_settings
                    .get(&id)
                    .cloned()
                    .unwrap_or_default()
                    == options.metadata
            {
                history.mark_exported(&request.operations);
            } else {
                history.invalidate_saved_source();
            }
            // Source-time resume coordinates refer to the retained original,
            // whereas a newly opened file now has the saved timeline. Do not
            // write those original coordinates under the new disk identity.
            if self.displayed_tab == Some(id) {
                self.resume_owner = None;
                self.resume_open = None;
            }
            if let Some(retained) = self.retained_playback.get_mut(&id) {
                retained.resume = None;
            }
        }
        self.folder_snapshot = None;
        self.refresh_folder_snapshot_from_disk();
        self.status_file_details.invalidate();
        self.refresh_title();
    }

    pub(super) fn thaw_source_save(&mut self, source: &Path) {
        self.source_save.frozen = false;
        self.retained_playback
            .retain(|_, tab| !tab.prepared_only || tab.duration.is_some());
        self.finish_file_relocation(source, None);
        for event in std::mem::take(&mut self.source_save.deferred) {
            self.handle_app_event(event);
        }
        if self.media_kind.is_some_and(|kind| kind != MediaKind::Image) {
            if self.media_duration.is_none()
                && let Some(path) = self.path.clone()
            {
                self.load_duration(path);
            }
            if self.timeline_open && self.waveform.is_none() {
                self.load_waveform();
            }
        }
        let durations: Vec<_> = self
            .retained_playback
            .values()
            .filter(|tab| !tab.prepared_only && tab.duration.is_none())
            .map(|tab| (tab.path.clone(), tab.instance))
            .collect();
        for (path, instance) in durations {
            self.load_duration_for(path, instance);
        }
        self.request_redraw();
    }

    pub(super) fn reject_uncertain_source(&mut self, source: &Path, message: &str) {
        let ids: Vec<_> = self
            .tabs
            .tabs()
            .iter()
            .filter(|tab| tab.target.current_path() == source)
            .map(|tab| tab.id)
            .collect();
        for id in ids {
            self.source_versions.insert(id, None);
            self.edits.entry(id).or_default().invalidate_saved_source();
            if self.source_backings.contains_key(&id) {
                continue;
            }
            if self.displayed_tab == Some(id) {
                self.session.take();
                self.file_operations.position = None;
                self.fail(message.to_owned());
            }
            if let Some(tab) = self.retained_playback.get_mut(&id) {
                tab.session = None;
                tab.recovery_position = None;
                tab.fail(message.to_owned());
            }
            self.edits.entry(id).or_default().invalidate_source();
        }
    }

    pub(super) fn document_source_available(&self, id: Option<TabId>) -> bool {
        id.and_then(|id| self.edits.get(&id))
            .is_none_or(EditHistory::source_available)
    }
}
