use super::*;
use towavue_runtime_windows::DecodedImageFrame;

#[test]
fn frozen_file_operation_keeps_image_and_chrome_without_restarting_readers() {
    let Some(root) = crate::tests::isolated_test_root(
        "source_save::tests::frozen_file_operation_keeps_image_and_chrome_without_restarting_readers",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(crate::chrome::style);
        let path = root.join("retained-image.bmp");
        crate::tab_transfer::tests::bitmap(&path);
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context.clone());
        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
        app.displayed_tab = Some(tab);
        app.path = Some(path.clone());
        app.media_kind = Some(MediaKind::Image);
        app.state = PlaybackState::Paused;
        app.image = Some(
            ImagePresentation::from_decoded(
                &context,
                &path,
                Arc::new(DecodedImage {
                    animation_plays: 0,
                    format: "test",
                    frames: vec![DecodedImageFrame {
                        width: 2,
                        height: 2,
                        rgba: vec![255; 16],
                        delay: Duration::ZERO,
                    }],
                }),
            )
            .expect("generated image"),
        );
        let texture = app.image.as_ref().expect("image").texture.id();
        let paint = |app: &mut Application<_>| {
            let mut actions = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut actions),
            );
            assert!(actions.is_empty());
            let image = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.clone()),
                    _ => None,
                })
                .expect("held image remains in the rendered scene");
            (output, image)
        };
        paint(&mut app);
        let (_, before) = paint(&mut app);
        app.quiesce_source_save(&path);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !app.source_readers_idle() {
            assert!(Instant::now() < deadline, "reader drain");
            std::thread::sleep(Duration::from_millis(2));
        }
        for _ in 0..3 {
            let (output, held) = paint(&mut app);
            assert_eq!(
                held, before,
                "image geometry, pixels and opacity remain unchanged"
            );
            assert!(
                app.source_readers_idle(),
                "painting must not reopen the source"
            );
            let notice = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text().contains("Saving a file") => {
                        Some(text.visual_bounding_rect())
                    }
                    _ => None,
                })
                .expect("operation status");
            assert!(
                notice.top() > 500.0,
                "notice belongs to the bottom status bar"
            );
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text().contains("retained-image"))),
                "tab chrome remains visible"
            );
        }
        app.thaw_source_save(&path);
        assert!(!app.modal_input_blocked());
    }
}

#[test]
fn publication_error_keeps_uncertain_source_classification_after_localization() {
    use towavue_runtime_windows::{FileOperationError, SourceSaveError};
    for (error, uncertain) in [
        (
            SourceSaveError::Source(FileOperationError::SourceChanged),
            true,
        ),
        (
            SourceSaveError::Source(FileOperationError::Io(std::io::Error::other(
                "source detail",
            ))),
            true,
        ),
        (
            SourceSaveError::RecoveryRequired {
                message: "native 日本語 {error}".into(),
                directory: PathBuf::from(r"D:\日本語 {retained}"),
            },
            true,
        ),
        (SourceSaveError::Io(std::io::Error::other("detail")), false),
        (SourceSaveError::Export(ExportError::Cancelled), false),
        (SourceSaveError::InvalidRequest, false),
    ] {
        let english = error.to_string();
        let japanese = error.message(localization::Language::Japanese);
        let publication = PublicationError::from(error);
        assert_eq!(publication.source_uncertain(), uncertain);
        assert_eq!(
            publication.message(localization::Language::English),
            english
        );
        assert_eq!(
            publication.message(localization::Language::Japanese),
            japanese
        );
        assert_eq!(publication.source_uncertain(), uncertain);
    }
    for error in [
        PublicationError::from("external 日本語 {error}"),
        localization::Text::SaveReaderTimeout.into(),
    ] {
        assert!(!error.source_uncertain());
        assert!(!error.message(localization::Language::Japanese).is_empty());
        assert!(!error.source_uncertain());
    }
}

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
            self.document_input(tab.id)
                .expect("fixture input")
                .logical_path()
                .to_owned(),
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
