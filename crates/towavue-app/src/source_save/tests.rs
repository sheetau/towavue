use super::*;

// Single-application fixture driver for existing format/guard controls. The
// production multi-window coordinator is exercised in window_host::source_save.
pub(crate) fn finish<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: &std::sync::mpsc::Receiver<AppEvent>,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.active_export.is_some() {
        assert!(Instant::now() < deadline, "save fixture completion");
        if let Ok(event) = events.recv_timeout(Duration::from_millis(2)) {
            app.handle_app_event(event);
        }
        publish_ready(app);
    }
}

pub(crate) fn publish_ready<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>) {
    let deadline = Instant::now() + Duration::from_secs(15);
    if let Some(prepared) = app
        .source_save
        .save_as
        .as_mut()
        .and_then(|pending| pending.prepared.take())
    {
        assert!(app.save_as_is_current());
        let export = app.active_export.as_mut().expect("Save as");
        let request = export.request.clone();
        export.job = Task::Publishing;
        app.quiesce_source_save(&request.target);
        app.quiesce_file_relocation(&request.source);
        while !app.source_readers_idle() {
            assert!(Instant::now() < deadline, "Save as reader drain");
            std::thread::sleep(Duration::from_millis(2));
        }
        let (send, result) = std::sync::mpsc::channel();
        towavue_runtime_windows::commit_save_as(prepared, move |result| {
            let _ = send.send(result);
        })
        .expect("Save as publication worker");
        let result = result
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("Save as publication completion");
        if let Ok(saved) = &result {
            app.install_save_as_peers(&request.target, saved);
            app.adopt_save_as(saved);
        }
        app.finish_file_relocation(&request.source, None);
        app.thaw_source_save(&request.target);
        app.finish_source_save(result.map(|_| ()).map_err(|error| error.to_string()));
        return;
    }
    let Some(prepared) = app
        .source_save
        .pending
        .as_mut()
        .and_then(|pending| pending.prepared.take())
    else {
        return;
    };
    assert!(app.source_save_is_current());
    let input = app
        .source_save
        .pending
        .as_ref()
        .expect("input")
        .input
        .clone();
    let export = app.active_export.as_mut().expect("save");
    let request = export.request.clone();
    let options = export.options.clone();
    export.job = Task::Publishing;
    app.quiesce_source_save(&request.source);
    while !app.source_readers_idle() {
        assert!(Instant::now() < deadline, "reader drain");
        std::thread::sleep(Duration::from_millis(2));
    }
    let (send, result) = std::sync::mpsc::channel();
    towavue_runtime_windows::commit_source_save(prepared, move |result| {
        let _ = send.send(result);
    })
    .expect("publication worker");
    let result = result
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .expect("publication completion");
    if let Ok(saved) = &result {
        app.install_saved_source(&request.source, &input, saved, &request, &options);
    }
    app.thaw_source_save(&request.source);
    app.finish_source_save(result.map(|_| ()).map_err(|error| error.to_string()));
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    /// Injects a native picker result with the same selected-target snapshot as
    /// the production Save as callback. No visible dialog or input is claimed.
    pub(crate) fn finish_test_dialog(&mut self, result: Result<Option<PathBuf>, DialogError>) {
        if matches!(
            self.pending_dialog,
            Some(DialogIntent::Export {
                output: ExportOutput::Media,
                ..
            })
        ) {
            self.finish_save_as_dialog(result.and_then(|path| {
                path.map(|path| {
                    towavue_runtime_windows::SaveAsTarget::capture(&path)
                        .map_err(|error| DialogError::InvalidExportChoice(error.to_string()))
                })
                .transpose()
            }));
        } else {
            self.finish_dialog(result);
        }
    }
    pub(crate) fn start_test_save_as(
        &mut self,
        target: PathBuf,
        continuation: Option<GuardedAction>,
    ) -> bool {
        let tab = self.tabs.active().expect("fixture document");
        self.start_save_as(
            tab.id,
            tab.target.current_path().to_owned(),
            tab.target.media_kind(),
            towavue_runtime_windows::SaveAsTarget::capture(&target)
                .expect("injected destination choice"),
            continuation,
        )
    }
}

pub(crate) fn loaded<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    tab: TabId,
    source: &Path,
) {
    app.source_versions.insert(
        tab,
        Some(FileOperationSource::capture(source).expect("loaded fixture identity")),
    );
}
