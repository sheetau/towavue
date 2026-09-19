use super::*;
use crate::file_operations::Completed;
use towavue_runtime_windows::{
    FileOperationAction, FileOperationOutcome, FileOperationSource, VideoResumeSource,
};

pub(super) struct Transaction {
    owner: WindowKey,
    serial: u64,
    source: PathBuf,
    action: Option<(FileOperationSource, FileOperationAction)>,
    suppress_confirmation: bool,
    expected: FileOperationSource,
    waiting_since: Option<Instant>,
    retain_copy: bool,
}

impl WindowHost {
    pub(super) fn start_pending_file_operation(&mut self) {
        if self.file_operation.is_some() || self.source_save.is_some() {
            return;
        }
        let Some(owner) = self
            .windows
            .iter()
            .find_map(|(key, app)| app.file_operations.ready.is_some().then_some(*key))
        else {
            return;
        };
        let current = self.windows[&owner].file_relocation_is_current();
        let blocked = self.windows.iter().any(|(key, app)| {
            app.active_export.is_some()
                || (*key != owner && app.modal_input_blocked())
                || app.image_loading
                || app.image_edit_pending
                || app.state == PlaybackState::Loading
        });
        if !current || blocked {
            let app = self.windows.get_mut(&owner).expect("request owner");
            app.file_operations.ready = None;
            app.file_operations.pending = None;
            app.set_status(
                "Finish loading, exporting or the other open dialog before changing the file."
                    .into(),
            );
            return;
        }
        let app = self.windows.get_mut(&owner).expect("request owner");
        let serial = app
            .file_operations
            .pending
            .as_ref()
            .expect("request")
            .serial;
        let (source, action) = app.file_operations.ready.take().expect("ready request");
        let deleting = matches!(action, FileOperationAction::Recycle);
        let path = source.path().to_owned();
        let dirty = self.windows.values().any(|app| {
            app.tabs.tabs().iter().any(|tab| {
                tab.target.current_path() == Some(path.as_ref())
                    && app.edits.get(&tab.id).is_some_and(EditHistory::is_dirty)
            })
        });
        self.file_operation = Some(Transaction {
            owner,
            serial,
            source: path.clone(),
            expected: source.clone(),
            waiting_since: None,
            retain_copy: false,
            action: Some((source, action)),
            suppress_confirmation: false,
        });
        // Lock every hosted owner while the prompt is open: its dirty summary and
        // source identity must remain valid until the accepted mutation starts.
        for app in self.windows.values_mut() {
            app.file_operations.locked = true;
            app.cancel_hold_speed();
            app.cancel_view_drag();
            app.cancel_frame_steps();
            app.cancel_shortcut_prefix();
            app.request_redraw();
        }
        if deleting && (!self.delete_confirmation_suppressed || dirty) {
            let app = &self.windows[&owner];
            let Some(window) = app.window.clone() else {
                self.cancel_file_operation(
                    "The delete dialog's owner window is unavailable.".into(),
                );
                return;
            };
            let notify = Arc::clone(&app.notify);
            if let Err(error) =
                towavue_runtime_windows::confirm_file_delete(window, path, dirty, move |result| {
                    notify(AppEvent::FileDeleteConfirmed(
                        serial,
                        result.map_err(|error| error.to_string()),
                    ));
                })
            {
                self.cancel_file_operation(error.to_string());
            }
        } else {
            self.run_file_operation();
        }
    }

    fn cancel_file_operation(&mut self, message: String) {
        let Some(pending) = self.file_operation.take() else {
            return;
        };
        for app in self.windows.values_mut() {
            if pending.waiting_since.is_some() {
                app.thaw_source_save(&pending.source);
            }
            app.file_operations.locked = false;
            app.finish_folder_load();
            app.finish_audio_folder_loads();
            app.request_redraw();
        }
        if let Some(app) = self.windows.get_mut(&pending.owner) {
            app.file_operations.pending = None;
            app.set_status(message);
        }
    }

    pub(super) fn finish_delete_confirmation(
        &mut self,
        owner: WindowKey,
        serial: u64,
        result: Result<towavue_runtime_windows::DeleteConfirmation, String>,
    ) {
        let Some(pending) = self.file_operation.as_mut().filter(|pending| {
            pending.owner == owner
                && pending.serial == serial
                && matches!(pending.action, Some((_, FileOperationAction::Recycle)))
        }) else {
            return;
        };
        match result {
            Ok(choice) if choice.confirmed => {
                pending.suppress_confirmation = choice.dont_ask_again;
                self.run_file_operation();
            }
            Ok(_) => self.cancel_file_operation("File deletion cancelled.".into()),
            Err(error) => self.cancel_file_operation(error),
        }
    }

    fn run_file_operation(&mut self) {
        let pending = self.file_operation.as_ref().expect("accepted transaction");
        if pending.waiting_since.is_some() || pending.action.is_none() {
            return;
        }
        let deleting = matches!(pending.action, Some((_, FileOperationAction::Recycle)));
        if !deleting {
            self.launch_file_operation();
            return;
        }
        let path = pending.source.clone();
        let mut retain_copy = false;
        for app in self.windows.values() {
            for tab in app
                .tabs
                .tabs()
                .iter()
                .filter(|tab| tab.target.current_path() == Some(path.as_ref()))
            {
                if app.source_backings.contains_key(&tab.id) {
                    continue;
                }
                let version = app.source_versions.get(&tab.id);
                let loaded = (app.displayed_tab == Some(tab.id)
                    && (app.image.is_some() || app.session.is_some()))
                    || app
                        .retained_images
                        .get(&tab.id)
                        .is_some_and(|saved| saved.image.is_some())
                    || app
                        .retained_playback
                        .get(&tab.id)
                        .is_some_and(|saved| saved.session.is_some());
                if version.is_some_and(|version| version.as_ref() != Some(&pending.expected))
                    || (version.is_none() && loaded)
                {
                    self.cancel_file_operation("Deletion cancelled because an open document has an outdated or unavailable source version. Export or reopen that document first.".into());
                    return;
                }
                retain_copy = true;
            }
        }
        let pending = self.file_operation.as_mut().expect("accepted transaction");
        pending.retain_copy = retain_copy;
        pending.waiting_since = Some(Instant::now());
        for app in self.windows.values_mut() {
            app.quiesce_source_save(&path);
        }
        self.advance_file_operation();
    }

    pub(super) fn advance_file_operation(&mut self) {
        let Some(pending) = self.file_operation.as_ref() else {
            return;
        };
        let Some(waiting_since) = pending.waiting_since else {
            return;
        };
        if pending.action.is_none() {
            return;
        }
        if !self.windows.values().all(|app| app.source_readers_idle()) {
            if waiting_since.elapsed() >= Duration::from_secs(10) {
                self.cancel_file_operation("Deletion cancelled because a media reader did not stop. The file was not deleted.".into());
            }
            return;
        }
        self.launch_file_operation();
    }

    fn launch_file_operation(&mut self) {
        let pending = self.file_operation.as_mut().expect("accepted transaction");
        let Some((source, action)) = pending.action.take() else {
            return;
        };
        let owner = pending.owner;
        let serial = pending.serial;
        let path = pending.source.clone();
        let frozen = pending.waiting_since.is_some();
        let retain_copy = pending.retain_copy;
        let preference = pending
            .suppress_confirmation
            .then(|| self.delete_preference_path.clone());
        let notify = Arc::clone(&self.windows[&owner].notify);
        if !frozen {
            for app in self.windows.values_mut() {
                app.quiesce_file_relocation(&path);
            }
        }
        let result = if matches!(action, FileOperationAction::Recycle) {
            let complete = move |result: Result<
                towavue_runtime_windows::FileRecycleReport,
                towavue_runtime_windows::FileOperationError,
            >| {
                let result = result
                    .map(|recycle| {
                        let preference_warning = preference.and_then(|path| {
                            path.ok_or_else(|| "APPDATA is unavailable".to_owned())
                                .and_then(|path| {
                                    crate::file_operations::preferences::save_suppressed(&path)
                                        .map_err(|error| error.to_string())
                                })
                                .err()
                        });
                        Completed {
                            outcome: FileOperationOutcome::Recycled,
                            versions: None,
                            resume: None,
                            recycle: Some(Box::new(recycle)),
                            preference_warning,
                        }
                    })
                    .map_err(|error| error.to_string());
                notify(AppEvent::FileOperationFinished(serial, result));
            };
            if retain_copy {
                towavue_runtime_windows::start_file_recycling_retaining_source(source, complete)
            } else {
                towavue_runtime_windows::start_file_recycling(source, complete)
            }
        } else {
            let original = source.clone();
            towavue_runtime_windows::start_file_operation(source, action, move |result| {
                let result = result
                    .map(|outcome| {
                        let resume = match &outcome {
                            FileOperationOutcome::Moved(path) => {
                                VideoResumeSource::capture(path).ok()
                            }
                            _ => None,
                        };
                        let source = match &outcome {
                            FileOperationOutcome::Moved(path) => original.after_move(path).ok(),
                            _ => None,
                        };
                        Completed {
                            versions: Some(Box::new(crate::file_operations::RelocatedVersions {
                                original,
                                current: source,
                            })),
                            outcome,
                            resume,
                            recycle: None,
                            preference_warning: None,
                        }
                    })
                    .map_err(|error| error.to_string());
                notify(AppEvent::FileOperationFinished(serial, result));
            })
        };
        if let Err(error) = result {
            self.finish_host_file_operation(owner, serial, Err(error.to_string()));
        }
    }

    pub(super) fn finish_host_file_operation(
        &mut self,
        owner: WindowKey,
        serial: u64,
        result: Result<Completed, String>,
    ) {
        if self.file_operation.as_ref().is_none_or(|pending| {
            pending.owner != owner || pending.serial != serial || pending.action.is_some()
        }) {
            return;
        }
        let pending = self.file_operation.take().expect("matching transaction");
        for app in self.windows.values_mut() {
            if let Ok(completed) = &result
                && let Some(recycle) = &completed.recycle
            {
                app.finish_file_recycling(&pending.expected, recycle);
            }
            if pending.waiting_since.is_some() {
                app.thaw_source_save(&pending.source);
            } else {
                app.finish_file_relocation(&pending.source, result.as_ref().ok());
            }
        }
        if let Some(app) = self.windows.get_mut(&owner) {
            app.file_operations.pending = None;
            match result {
                Ok(Completed {
                    outcome: FileOperationOutcome::Moved(path),
                    ..
                }) => {
                    if let Some(recent) = &app.recent_files {
                        recent.remove(pending.source, towavue_runtime_windows::RecentKind::File);
                        recent.record(path.clone());
                    }
                    app.set_status(format!("File moved: {}", path.display()));
                }
                Ok(Completed {
                    outcome: FileOperationOutcome::CopiedButSourceRetained(path),
                    ..
                }) => app.set_status(format!(
                    "A copy was created at {}; the original could not be removed and remains open.",
                    path.display()
                )),
                Ok(Completed {
                    outcome: FileOperationOutcome::Recycled,
                    preference_warning,
                    recycle,
                    ..
                }) => {
                    self.delete_confirmation_suppressed |= pending.suppress_confirmation;
                    if let Some(recent) = &app.recent_files {
                        recent.remove(pending.source, towavue_runtime_windows::RecentKind::File);
                    }
                    let message = if recycle
                        .as_ref()
                        .is_some_and(|report| report.after.is_none())
                    {
                        "File recycled; the folder could not be refreshed."
                    } else {
                        "File moved to the Recycle Bin."
                    };
                    app.set_status(preference_warning.map_or_else(
                        || message.into(),
                        |error| {
                            format!(
                                "{message} The confirmation preference could not be saved: {error}"
                            )
                        },
                    ));
                }
                Err(error) => app.set_status(error),
            }
            app.request_redraw();
        }
    }
}

#[cfg(test)]
pub(super) mod tests;

#[cfg(test)]
mod recycle_tests;
