use super::*;
use towavue_runtime_windows::{PreparedSaveAs, PreparedSourceSave, SavedAsSource, SavedSource};

#[cfg(test)]
mod tests;

enum Candidate {
    Source(PreparedSourceSave),
    SaveAs(PreparedSaveAs),
}

pub(super) struct Publication {
    owner: WindowKey,
    serial: u64,
    source: PathBuf,
    prepared: Option<Candidate>,
    document_source: Option<PathBuf>,
    waiting_since: Instant,
}

impl WindowHost {
    pub(super) fn advance_source_save(&mut self) {
        if self.source_save.is_none() && self.file_operation.is_none() {
            let owner = self.windows.iter().find_map(|(key, app)| {
                (app.source_save
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.prepared.is_some())
                    || app
                        .source_save
                        .save_as
                        .as_ref()
                        .is_some_and(|pending| pending.prepared.is_some()))
                .then_some(*key)
            });
            if let Some(owner) = owner {
                if self.windows[&owner].source_save.save_as.is_some() {
                    self.begin_save_as_publication(owner);
                } else {
                    self.begin_source_publication(owner);
                }
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
                self.abort_save_publication(owner, serial, "Save cancelled because a media reader did not stop. No destination was published.".into());
            }
            return;
        }
        let pending = self.source_save.as_mut().expect("save publication");
        let owner = pending.owner;
        let serial = pending.serial;
        let prepared = pending.prepared.take().expect("unpublished candidate");
        let notify = Arc::clone(&self.windows[&owner].notify);
        let started = match prepared {
            Candidate::Source(prepared) => {
                towavue_runtime_windows::commit_source_save(prepared, move |result| {
                    notify(AppEvent::SourceSavePublished(
                        serial,
                        result.map_err(crate::source_save::PublicationError::from),
                    ));
                })
            }
            Candidate::SaveAs(prepared) => {
                towavue_runtime_windows::commit_save_as(prepared, move |result| {
                    notify(AppEvent::SaveAsPublished(
                        serial,
                        result.map_err(crate::source_save::PublicationError::from),
                    ));
                })
            }
        };
        if let Err(error) = started {
            self.abort_save_publication(owner, serial, error.to_string().into());
        }
    }

    fn abort_save_publication(
        &mut self,
        owner: WindowKey,
        serial: u64,
        error: crate::source_save::PublicationError,
    ) {
        if self
            .source_save
            .as_ref()
            .is_some_and(|pending| pending.document_source.is_some())
        {
            self.finish_save_as_publication(owner, serial, Err(error));
        } else {
            self.finish_source_publication(owner, serial, Err(error));
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
            prepared: pending.prepared.take().map(Candidate::Source),
            document_source: None,
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

impl WindowHost {
    fn begin_save_as_publication(&mut self, owner: WindowKey) {
        let app = &self.windows[&owner];
        let pending = app.source_save.save_as.as_ref().expect("prepared Save as");
        let export = app.active_export.as_ref().expect("Save as progress");
        let source = export.request.target.clone();
        let document_source = export.request.source.clone();
        let expected = pending
            .prepared
            .as_ref()
            .expect("Save as candidate")
            .expected_target();
        let blocked = !app.save_as_is_current()
            || export.cancelling
            || app.native_prompt.is_some()
            || app.pending_dialog.is_some()
            || self.windows.iter().any(|(key, app)| {
                app.exit_requested
                    || app.image_loading
                    || app.image_edit_pending
                    || app.state == PlaybackState::Loading
                    || (*key != owner && (app.active_export.is_some() || app.modal_input_blocked()))
                    || app.tabs.tabs().iter().any(|tab| {
                        tab.target.current_path() == source
                            && !app.source_backings.contains_key(&tab.id)
                            && app.source_versions.get(&tab.id).is_some_and(|version| {
                                expected.is_none() || version.as_ref() != expected
                            })
                    })
            });
        if blocked {
            self.windows.get_mut(&owner).expect("Save as owner").finish_source_save(Err(
                "Save as cancelled before publishing. Finish other loading/dialogs, or reopen a destination tab with an outdated source version.".into()));
            return;
        }
        let app = self.windows.get_mut(&owner).expect("Save as owner");
        let pending = app.source_save.save_as.as_mut().expect("Save as owner");
        self.source_save = Some(Publication {
            owner,
            serial: pending.serial,
            source: source.clone(),
            document_source: Some(document_source.clone()),
            prepared: pending.prepared.take().map(Candidate::SaveAs),
            waiting_since: Instant::now(),
        });
        app.active_export.as_mut().expect("Save as progress").job =
            crate::source_save::Task::Publishing;
        for app in self.windows.values_mut() {
            app.quiesce_source_save(&source);
        }
        // The exporting document changes input after publication. Suspend its
        // active/retained sessions too; unrelated source tabs keep their paths.
        self.windows
            .get_mut(&owner)
            .expect("Save as owner")
            .quiesce_file_relocation(&document_source);
    }

    pub(super) fn finish_save_as_publication(
        &mut self,
        owner: WindowKey,
        serial: u64,
        result: Result<SavedAsSource, crate::source_save::PublicationError>,
    ) {
        if self.source_save.as_ref().is_none_or(|pending| {
            pending.owner != owner || pending.serial != serial || pending.document_source.is_none()
        }) {
            return;
        }
        let publication = self
            .source_save
            .take()
            .expect("matching Save as publication");
        if let Ok(saved) = &result {
            for app in self.windows.values_mut() {
                app.install_save_as_peers(&publication.source, saved);
            }
            self.windows
                .get_mut(&owner)
                .expect("Save as owner")
                .adopt_save_as(saved);
        }
        if let Err(error) = &result
            && error.source_uncertain
        {
            for app in self.windows.values_mut() {
                app.reject_uncertain_source(&publication.source, &error.message);
            }
        }
        let app = self.windows.get_mut(&owner).expect("Save as owner");
        // Resume the foreground using its current logical identity, and original
        // peers using theirs, before releasing deferred UI events. The exporter
        // may now be a background tab rather than this window's displayed media.
        app.finish_file_relocation(
            publication
                .document_source
                .as_deref()
                .expect("document source"),
            None,
        );
        for app in self.windows.values_mut() {
            app.thaw_source_save(&publication.source);
        }
        self.windows
            .get_mut(&owner)
            .expect("retained Save as owner")
            .finish_source_save(result.map(|_| ()).map_err(|error| error.message));
    }
}
