use crate::*;
use towavue_runtime_windows::{
    FileOperationSource, MediaInput, PreparedSaveAs, SaveAsEvent, SaveAsJob, SaveAsRequest,
    SaveAsTarget, SavedAsSource,
};

pub(super) struct Pending {
    pub serial: u64,
    pub input: MediaInput,
    pub loaded_version: Option<FileOperationSource>,
    pub selected: SaveAsTarget,
    pub prepared: Option<PreparedSaveAs>,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn start_save_as(
        &mut self,
        id: TabId,
        source: PathBuf,
        kind: MediaKind,
        selected: SaveAsTarget,
        continuation: Option<GuardedAction>,
    ) -> bool {
        if self.active_export.is_some() || !self.document_source_available(Some(id)) {
            return false;
        }
        if !self
            .tabs
            .tabs()
            .iter()
            .any(|tab| tab.id == id && tab.target.current_path() == source)
        {
            return false;
        }
        if selected.path() == source {
            // This is still the same document. Source Save uses its loaded
            // version rather than authorizing external replacement via a picker.
            return self.tabs.active().is_some_and(|tab| tab.id == id)
                && self.save_source(continuation);
        }
        let request = ExportRequest {
            source: source.clone(),
            target: selected.path().to_owned(),
            kind,
            operations: self
                .edits
                .get(&id)
                .map_or_else(Vec::new, |history| history.operations().to_vec()),
            hardware_encode: self.prefer_hardware_encode,
        };
        let options = ExportOptions {
            output: ExportOutput::Media,
            video_quality: self.effective_video_export_quality(kind, ExportOutput::Media),
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
        let input = self.media_input_for(Some(id), &source);
        let loaded_version = self
            .source_versions
            .get(&id)
            .and_then(Option::as_ref)
            .cloned();
        self.source_save.serial = self.source_save.serial.wrapping_add(1);
        let serial = self.source_save.serial;
        let notify = Arc::clone(&self.notify);
        let job = SaveAsJob::start(
            SaveAsRequest {
                input: input.clone(),
                source_version: loaded_version.clone(),
                target: selected.clone(),
                export: request.clone(),
                options: options.clone(),
            },
            move |event| notify(AppEvent::SaveAs(serial, event)),
        );
        match job {
            Ok(job) => {
                self.source_save.save_as = Some(Pending {
                    serial,
                    input,
                    loaded_version,
                    selected,
                    prepared: None,
                });
                self.active_export = Some(ActiveExport {
                    job: source_save::Task::SaveAs(job),
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
                self.pending_guard = continuation;
                self.request_redraw();
                false
            }
        }
    }

    pub(super) fn save_as_is_current(&self) -> bool {
        let (Some(pending), Some(export)) = (&self.source_save.save_as, &self.active_export) else {
            return false;
        };
        self.tabs
            .tabs()
            .iter()
            .any(|tab| tab.id == export.tab && tab.target.current_path() == export.request.source)
            && export.request.target == pending.selected.path()
            && self.media_input_for(Some(export.tab), &export.request.source) == pending.input
            && self
                .source_versions
                .get(&export.tab)
                .and_then(Option::as_ref)
                == pending.loaded_version.as_ref()
    }

    pub(super) fn handle_save_as(&mut self, serial: u64, event: SaveAsEvent) {
        if self
            .source_save
            .save_as
            .as_ref()
            .is_none_or(|pending| pending.serial != serial)
        {
            return;
        }
        match event {
            SaveAsEvent::Progress(time) => self.handle_export_event(ExportEvent::Progress(time)),
            SaveAsEvent::AnalyzingAudio(time) => {
                self.handle_export_event(ExportEvent::AnalyzingAudio(time))
            }
            SaveAsEvent::Prepared(result) => match result {
                Ok(prepared)
                    if self.save_as_is_current()
                        && self
                            .active_export
                            .as_ref()
                            .is_some_and(|export| !export.cancelling) =>
                {
                    self.source_save
                        .save_as
                        .as_mut()
                        .expect("Save as owner")
                        .prepared = Some(prepared);
                }
                Ok(_) => self.finish_source_save(Err(
                    "Save as cancelled before publishing the destination.".into(),
                )),
                Err(error) => self.finish_source_save(Err(error.to_string())),
            },
        }
        self.request_redraw();
    }

    pub(super) fn install_save_as_peers(&mut self, target: &Path, saved: &SavedAsSource) {
        let ids: Vec<_> = self
            .tabs
            .tabs()
            .iter()
            .filter(|tab| tab.target.current_path() == target)
            .map(|tab| tab.id)
            .collect();
        for id in ids {
            if let Some(queue) = self.audio_queues.get_mut(&id) {
                queue.refresh_after_save();
            }
            if !self.source_versions.contains_key(&id) {
                self.retained_playback.remove(&id);
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(entry) =
                self.source_backings.entry(id)
            {
                if let Some(replaced) = saved.replaced_source() {
                    entry.insert(replaced.retained_source());
                } else {
                    // The host rejects loaded unbacked owners of a missing
                    // target. Never invent an original from the newly saved file.
                    continue;
                }
            }
            self.source_versions
                .insert(id, Some(saved.current_source().clone()));
            self.deleted_sources.remove(&id);
            if let Some(queue) = self.audio_queues.get_mut(&id) {
                queue.clear_deleted_source();
            }
            self.edits.entry(id).or_default().invalidate_saved_source();
            if self.displayed_tab == Some(id) {
                self.resume_owner = None;
                self.resume_open = None;
            }
            if let Some(retained) = self.retained_playback.get_mut(&id) {
                retained.resume = None;
            }
        }
        self.refresh_folder_snapshot_from_disk();
        self.status_file_details.invalidate();
        self.refresh_title();
    }

    pub(super) fn adopt_save_as(&mut self, saved: &SavedAsSource) {
        let export = self.active_export.as_ref().expect("Save as owner");
        let id = export.tab;
        let request = export.request.clone();
        let options = export.options.clone();
        let source = &request.source;
        let target = saved.current_source().path();
        let tab = self.tabs.get_mut(id).expect("retained Save as tab");
        tab.target.set_current_path(target.to_owned(), request.kind);
        self.source_backings.insert(id, saved.retained_source());
        self.source_versions
            .insert(id, Some(saved.current_source().clone()));
        self.deleted_sources.remove(&id);
        self.export_paths.remove(&id);
        let same_options = self.effective_video_export_quality(request.kind, options.output)
            == options.video_quality
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
                == options.metadata;
        let history = self.edits.entry(id).or_default();
        if same_options {
            history.mark_exported(&request.operations);
        } else {
            history.invalidate_saved_source();
        }
        self.media_sequence = self
            .media_sequence
            .max(self.media_generation)
            .wrapping_add(1);
        let instance = self.media_sequence;
        self.viewed_media.begin(id, target);
        if let Some(retained) = self.retained_images.get_mut(&id) {
            self.duration_workers.remove(&retained.instance);
            retained.instance = instance;
            retained.path = target.to_owned();
            retained.filmstrip_view.relocate(source, target);
            retained.folder_snapshot = None;
            retained.previews.clear();
            if let Some(focus) = &mut retained.reading_focus
                && focus.path == *source
            {
                focus.path = target.to_owned();
            }
        }
        if let Some(retained) = self.retained_playback.get_mut(&id) {
            self.duration_workers.remove(&retained.instance);
            retained.instance = instance;
            retained.path = target.to_owned();
            retained.filmstrip_view.relocate(source, target);
            retained.folder_snapshot = None;
            retained.resume = None;
        }
        if let Some(queue) = self.audio_queues.get_mut(&id) {
            queue.relocate(source, target);
            queue.clear_deleted_source();
            queue.refresh_after_save();
        }
        if self.displayed_tab == Some(id) {
            self.duration_workers.remove(&self.media_generation);
            self.media_generation = instance;
            self.path = Some(target.to_owned());
            self.resume_owner = None;
            self.resume_open = None;
            self.image_previews.clear();
            if let Some(focus) = &mut self.reading_focus
                && focus.path == *source
            {
                focus.path = target.to_owned();
            }
            self.filmstrip.preserve_after_file_operation(
                self.ui_context.as_ref(),
                source,
                Some(target),
            );
            self.folder_snapshot = None;
            self.refresh_folder_snapshot_from_disk();
            self.refresh_status_file_details();
        }
        self.tab_preview.clear();
        self.refresh_title();
        self.request_redraw();
    }
}
