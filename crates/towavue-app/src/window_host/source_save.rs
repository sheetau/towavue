use super::*;
use towavue_runtime_windows::{PreparedSourceSave, SavedSource};

#[cfg(test)]
mod tests;

pub(super) struct Publication {
    owner: WindowKey,
    serial: u64,
    source: PathBuf,
    prepared: Option<PreparedSourceSave>,
    waiting_since: Instant,
}

impl WindowHost {
    pub(super) fn advance_source_save(&mut self) {
        if self.source_save.is_none() && self.file_operation.is_none() {
            let owner = self.windows.iter().find_map(|(key, app)| {
                app.source_save
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.prepared.is_some())
                    .then_some(*key)
            });
            if let Some(owner) = owner {
                self.begin_source_publication(owner);
            }
        }
        let Some(pending) = &self.source_save else {
            return;
        };
        if pending.prepared.is_none() {
            return;
        }
        if !self.windows.values().all(|app| app.source_readers_idle()) {
            if pending.waiting_since.elapsed() >= Duration::from_secs(10) {
                let owner = pending.owner;
                let serial = pending.serial;
                self.finish_source_publication(owner, serial, Err("Save cancelled because a media reader did not stop. The source was not replaced.".into()));
            }
            return;
        }
        let pending = self.source_save.as_mut().expect("save publication");
        let owner = pending.owner;
        let serial = pending.serial;
        let prepared = pending.prepared.take().expect("unpublished candidate");
        let notify = Arc::clone(&self.windows[&owner].notify);
        if let Err(error) = towavue_runtime_windows::commit_source_save(prepared, move |result| {
            notify(AppEvent::SourceSavePublished(
                serial,
                result.map_err(crate::source_save::PublicationError::from),
            ));
        }) {
            self.finish_source_publication(owner, serial, Err(error.to_string().into()));
        }
    }

    fn begin_source_publication(&mut self, owner: WindowKey) {
        let app = &self.windows[&owner];
        let pending = app.source_save.pending.as_ref().expect("prepared save");
        let source = pending.expected.path().to_owned();
        let blocked = !app.source_save_is_current()
            || app.native_prompt.is_some()
            || app.pending_dialog.is_some()
            || app
                .active_export
                .as_ref()
                .is_none_or(|export| export.cancelling)
            || self.windows.iter().any(|(key, app)| {
                app.exit_requested
                    || app.image_loading
                    || app.image_edit_pending
                    || app.state == PlaybackState::Loading
                    || (*key != owner && (app.active_export.is_some() || app.modal_input_blocked()))
                    || app.tabs.tabs().iter().any(|tab| {
                        tab.target.current_path() == source
                            && !app.source_backings.contains_key(&tab.id)
                            && app
                                .source_versions
                                .get(&tab.id)
                                .is_some_and(|version| version.as_ref() != Some(&pending.expected))
                    })
            });
        if blocked {
            self.windows.get_mut(&owner).expect("save owner").finish_source_save(Err(
                "Save cancelled before replacing the source. Finish other loading/dialogs, or reopen another tab with an outdated source version.".into()
            ));
            return;
        }
        let app = self.windows.get_mut(&owner).expect("save owner");
        let pending = app.source_save.pending.as_mut().expect("save owner");
        self.source_save = Some(Publication {
            owner,
            serial: pending.serial,
            source: source.clone(),
            prepared: pending.prepared.take(),
            waiting_since: Instant::now(),
        });
        // The preparation notification has returned through the event loop; its
        // worker is joined here before accepting the non-cancellable mutation.
        app.active_export.as_mut().expect("save progress").job =
            crate::source_save::Task::Publishing;
        for app in self.windows.values_mut() {
            app.quiesce_source_save(&source);
        }
    }

    pub(super) fn finish_source_publication(
        &mut self,
        owner: WindowKey,
        serial: u64,
        result: Result<SavedSource, crate::source_save::PublicationError>,
    ) {
        if self
            .source_save
            .as_ref()
            .is_none_or(|pending| pending.owner != owner || pending.serial != serial)
        {
            return;
        }
        let publication = self.source_save.take().expect("matching publication");
        if let Ok(saved) = &result {
            let app = &self.windows[&owner];
            let input = app
                .source_save
                .pending
                .as_ref()
                .expect("save input")
                .input
                .clone();
            let export = app.active_export.as_ref().expect("save progress");
            let request = export.request.clone();
            let options = export.options.clone();
            for app in self.windows.values_mut() {
                app.install_saved_source(&publication.source, &input, saved, &request, &options);
            }
        }
        if let Err(error) = &result
            && error.source_uncertain
        {
            for app in self.windows.values_mut() {
                app.reject_uncertain_source(&publication.source, &error.message);
            }
        }
        // Every accepted result, including recovery-required failures, reaches
        // the retained owner. No close/navigation event may discard it.
        for app in self.windows.values_mut() {
            app.thaw_source_save(&publication.source);
        }
        self.windows
            .get_mut(&owner)
            .expect("retained save owner")
            .finish_source_save(result.map(|_| ()).map_err(|error| error.message));
    }
}
