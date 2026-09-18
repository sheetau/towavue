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
        let Some(prepared) = app
            .source_save
            .pending
            .as_mut()
            .and_then(|pending| pending.prepared.take())
        else {
            continue;
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
