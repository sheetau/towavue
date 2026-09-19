use super::*;
use towavue_runtime_windows::FileRecycleReport;

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(crate) fn finish_file_recycling(
        &mut self,
        source: &FileOperationSource,
        report: &FileRecycleReport,
    ) {
        let path = source.path();
        let affected: Vec<_> = self
            .tabs
            .tabs()
            .iter()
            .filter(|tab| tab.target.current_path() == Some(path))
            .map(|tab| tab.id)
            .collect();
        let deleted = source_backing::DeletedSource {
            path: path.to_owned(),
            before: Arc::new(report.before.clone()),
        };
        for id in affected {
            // Keep each loaded document's earliest original, including edits
            // predating a previous Save. The host validated unbacked versions.
            if let std::collections::btree_map::Entry::Vacant(entry) =
                self.source_backings.entry(id)
            {
                entry.insert(
                    report
                        .retained_source
                        .as_ref()
                        .expect("host retained every unbacked document")
                        .clone(),
                );
            }
            self.source_versions
                .entry(id)
                .or_insert_with(|| Some(source.clone()));
            self.deleted_sources.insert(id, deleted.clone());
            if let Some(queue) = self.audio_queues.get_mut(&id) {
                queue.retain_deleted_source(deleted.clone());
            }
            if self.displayed_tab == Some(id) {
                self.resume_owner = None;
                self.resume_open = None;
            }
            if let Some(saved) = self.retained_playback.get_mut(&id) {
                saved.resume = None;
            }
        }
        self.closed_tabs.retain(
            |closed| !matches!(closed, closed_tabs::ClosedTab::Media(closed, _) if closed == path),
        );
        self.viewed_media.forget(Some(path));
        self.tab_preview.clear();
        self.image_previews.clear();
        self.status_file_details.invalidate();
        self.filmstrip
            .preserve_after_file_operation(self.ui_context.as_ref(), path, None);
        self.filmstrip.set_held_deleted(
            self.current_source_deleted()
                .then_some(self.path.as_deref())
                .flatten(),
        );
        if let Some(after) = &report.after
            && self.path.as_deref().and_then(Path::parent) == Some(after.folder_path.as_path())
        {
            self.apply_folder_snapshot(after.clone());
        }
        for queue in self.audio_queues.values_mut() {
            queue.refresh_after_relocation();
        }
        self.refresh_folder_snapshot();
        self.refresh_title();
        self.request_redraw();
    }
}
