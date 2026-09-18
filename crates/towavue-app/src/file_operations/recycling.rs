use super::*;
use towavue_runtime_windows::FileRecycleReport;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(crate) fn finish_file_recycling(&mut self, source: &Path, report: &FileRecycleReport) {
        let affected: Vec<_> = self
            .tabs
            .tabs()
            .iter()
            .filter(|tab| tab.target.current_path() == source)
            .map(|tab| (tab.id, tab.target.media_kind()))
            .collect();
        let active_affected = self.path.as_deref() == Some(source);
        self.file_operations.position = None;
        self.file_operations.locked = false;
        // Do not restart deleted readers, or retain a closed-tab/history route to
        // the removed file. Dirty state is discarded only after confirmed success.
        if active_affected {
            self.duration_workers.remove(&self.media_generation);
            self.resume_owner = None;
            self.clear_active_media();
        }
        for (id, kind) in affected {
            let replacement = report
                .after
                .as_ref()
                .and_then(|after| after.replacement_after_removal(&report.before, source, kind));
            self.edits.remove(&id);
            self.source_versions.remove(&id);
            self.export_paths.remove(&id);
            self.audio_export_settings.remove(&id);
            self.metadata_export_settings.remove(&id);
            if replacement.is_none() {
                self.audio_queues.remove(&id);
            } else if let (Some(queue), Some(after)) =
                (self.audio_queues.get_mut(&id), report.after.as_ref())
            {
                queue.accept_after_recycling(after.clone());
            }
            self.retained_images.remove(&id);
            if let Some(saved) = self.retained_playback.remove(&id) {
                self.duration_workers.remove(&saved.instance);
            }
            if let Some(context) = &self.ui_context {
                tab_focus::forget(context, id);
            }
            if let Some(path) = replacement {
                self.tabs
                    .get_mut(id)
                    .expect("existing deleted-file tab")
                    .target
                    .set_current_path(path.clone(), kind);
                self.viewed_media.begin(id, &path);
            } else {
                self.tabs.take(id);
                self.playback_volumes.remove(&id);
            }
        }
        self.closed_tabs.retain(
            |closed| !matches!(closed, closed_tabs::ClosedTab::Media(path, _) if path == source),
        );
        self.viewed_media.forget(Some(source));
        self.tab_preview.clear();
        self.image_previews.clear();
        self.folder_snapshot = None;
        for queue in self.audio_queues.values_mut() {
            queue.refresh_after_relocation();
        }
        if active_affected {
            if let Some(tab) = self.tabs.active() {
                self.load_path(
                    tab.target.current_path().to_owned(),
                    tab.target.media_kind(),
                );
            } else {
                // A deleted last item leaves the existing window open and empty.
                self.clear_active_media();
            }
        } else {
            self.refresh_folder_snapshot();
        }
        if report.after.is_none() {
            self.set_status("File recycled; the folder could not be refreshed.".into());
        }
        self.refresh_title();
        self.request_redraw();
    }
}
