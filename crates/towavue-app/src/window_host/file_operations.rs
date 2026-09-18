use super::*;
use crate::file_operations::Completed;
use towavue_runtime_windows::{FileOperationOutcome, VideoResumeSource};

pub(super) struct Transaction {
    owner: WindowKey,
    serial: u64,
    source: PathBuf,
}

impl WindowHost {
    pub(super) fn start_pending_file_operation(&mut self) {
        if self.file_operation.is_some() {
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
        let path = source.path().to_owned();
        let notify = Arc::clone(&app.notify);
        self.file_operation = Some(Transaction {
            owner,
            serial,
            source: path.clone(),
        });
        for app in self.windows.values_mut() {
            app.quiesce_file_relocation(&path);
        }
        if let Err(error) =
            towavue_runtime_windows::start_file_operation(source, action, move |result| {
                let result = result
                    .map(|outcome| {
                        let resume = match &outcome {
                            FileOperationOutcome::Moved(path) => {
                                VideoResumeSource::capture(path).ok()
                            }
                            _ => None,
                        };
                        Completed { outcome, resume }
                    })
                    .map_err(|error| error.to_string());
                notify(AppEvent::FileOperationFinished(serial, result));
            })
        {
            self.finish_host_file_operation(owner, serial, Err(error.to_string()));
        }
    }

    pub(super) fn finish_host_file_operation(
        &mut self,
        owner: WindowKey,
        serial: u64,
        result: Result<Completed, String>,
    ) {
        if self
            .file_operation
            .as_ref()
            .is_none_or(|pending| pending.owner != owner || pending.serial != serial)
        {
            return;
        }
        let pending = self.file_operation.take().expect("matching transaction");
        for app in self.windows.values_mut() {
            app.finish_file_relocation(&pending.source, result.as_ref().ok());
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
                }) => {
                    app.set_status(format!("A copy was created at {}; the original could not be removed and remains open.", path.display()));
                }
                Ok(Completed {
                    outcome: FileOperationOutcome::Recycled,
                    ..
                }) => {
                    app.set_status("File recycled.".into());
                }
                Err(error) => app.set_status(error),
            }
            app.request_redraw();
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
