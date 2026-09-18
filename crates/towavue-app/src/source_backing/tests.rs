use super::*;
use std::sync::{atomic::AtomicBool, mpsc};
use towavue_runtime_windows::{FileOperationSource, SavedSource, SourceSaveError};

type App = Application<Box<dyn Fn(AppEvent) + Send + Sync>>;
fn app() -> (App, mpsc::Receiver<AppEvent>) {
    let (sender, receiver) = mpsc::channel();
    let notify: Box<dyn Fn(AppEvent) + Send + Sync> = Box::new(move |event| {
        sender.send(event).ok();
    });
    let mut app = Application::new(None, notify).expect("app");
    app.ui_context = Some(fonts::test_context());
    (app, receiver)
}
fn finish(app: &mut App, receiver: &mpsc::Receiver<AppEvent>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        while let Ok(event) = receiver.try_recv() {
            app.handle_app_event(event);
        }
        app.finish_image_load();
        assert!(app.image_error.is_none(), "{:?}", app.image_error);
        if !app.image_loading
            && !app.image_edit_pending
            && app.active_export.is_none()
            && app.image_loader.is_idle()
        {
            break;
        }
        assert!(Instant::now() < deadline, "owned image/export completion");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn publish(path: &Path, operations: Vec<EditOperation>) -> Result<SavedSource, SourceSaveError> {
    let expected = FileOperationSource::capture(path).expect("source");
    let prepared = towavue_runtime_windows::prepare_source_save(
        expected,
        ExportRequest {
            source: path.to_owned(),
            target: path.to_owned(),
            kind: MediaKind::Image,
            operations,
            hardware_encode: false,
        },
        ExportOptions::default(),
        &AtomicBool::new(false),
        &|_| {},
        &|_| {},
    )?;
    let (sender, receiver) = mpsc::channel();
    towavue_runtime_windows::commit_source_save(prepared, move |result| {
        sender.send(result).ok();
    })
    .expect("publication worker");
    receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("publication result")
}
#[test]
fn retained_source_backing_reloads_undoes_exports_and_transfers_without_double_applying_edits() {
    let Some(root) = crate::tests::isolated_test_root(
        "source_backing::tests::retained_source_backing_reloads_undoes_exports_and_transfers_without_double_applying_edits",
    ) else {
        return;
    };
    let path = root.join("image.bmp");
    crate::tab_transfer::tests::bitmap(&path);
    let original = towavue_runtime_windows::decode_image(&path)
        .expect("original pixels")
        .frames[0]
        .rgba
        .clone();
    let (mut app, events) = app();
    app.open_external(path.clone(), true);
    finish(&mut app, &events);
    let id = app.tabs.active_id().expect("tab");
    app.edits
        .entry(id)
        .or_default()
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    app.edits.get_mut(&id).expect("history").push(
        EditOperation::Resize(
            towavue_core::ImageResize::new(4, 2, towavue_core::ResampleFilter::Nearest)
                .expect("resize"),
        ),
        MediaKind::Image,
    );
    app.refresh_image_edits();
    finish(&mut app, &events);
    let edited = app.image.as_ref().expect("edited image").decoded.frames[0]
        .rgba
        .clone();
    assert_ne!(edited, original);
    let saved =
        publish(&path, app.edits[&id].operations().to_vec()).expect("real source publication");
    let backup = saved.original_path().to_owned();
    app.source_versions
        .insert(id, Some(saved.current_source().clone()));
    app.source_backings.insert(id, saved);
    app.edits.get_mut(&id).expect("history").mark_saved();
    app.request_image_paths(vec![path.clone()], 0);
    finish(&mut app, &events);
    assert_eq!(app.path.as_ref(), Some(&path));
    assert_eq!(
        app.image.as_ref().expect("reloaded edits").decoded.frames[0].rgba,
        edited,
        "reloading reads the original and applies edits once"
    );
    app.edits.get_mut(&id).expect("history").undo();
    app.edits.get_mut(&id).expect("history").undo();
    app.refresh_image_edits();
    finish(&mut app, &events);
    assert_eq!(
        app.image.as_ref().expect("undo").decoded.frames[0].rgba,
        original
    );
    app.edits.get_mut(&id).expect("history").redo();
    app.edits.get_mut(&id).expect("history").redo();
    app.refresh_image_edits();
    finish(&mut app, &events);
    let export = root.join("export.bmp");
    assert!(app.start_export(
        id,
        path.clone(),
        MediaKind::Image,
        export.clone(),
        None,
        ExportOutput::Media
    ));
    finish(&mut app, &events);
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    assert_eq!(
        app.export_paths.get(&id),
        Some(&export),
        "logical export ownership remains intact"
    );
    assert_eq!(
        towavue_runtime_windows::decode_image(&export)
            .expect("derivative")
            .frames[0]
            .rgba,
        edited
    );
    assert!(app.start_export(
        id,
        path.clone(),
        MediaKind::Image,
        path.clone(),
        None,
        ExportOutput::Media
    ));
    finish(&mut app, &events);
    assert!(
        app.export_error.take().is_some(),
        "Export cannot bypass the separate source Save transaction"
    );
    assert_eq!(
        towavue_runtime_windows::decode_image(&path)
            .expect("source protected")
            .frames[0]
            .rgba,
        edited
    );
    let (mut target, target_events) = self::app();
    let moved = crate::tab_transfer::tests::transfer(&mut app, &mut target, id);
    finish(&mut target, &target_events);
    assert!(!app.source_backings.contains_key(&id));
    assert_eq!(target.media_input(&path).path(), backup);
    assert_eq!(target.path.as_ref(), Some(&path));
    target
        .edits
        .get_mut(&moved)
        .expect("transferred history")
        .undo();
    target
        .edits
        .get_mut(&moved)
        .expect("transferred history")
        .undo();
    target.refresh_image_edits();
    finish(&mut target, &target_events);
    assert_eq!(
        target
            .image
            .as_ref()
            .expect("transferred undo")
            .decoded
            .frames[0]
            .rgba,
        original
    );
    target.close_tab_unchecked(moved);
    assert!(!target.source_backings.contains_key(&moved));
    let deadline = Instant::now() + Duration::from_secs(5);
    while backup.exists() {
        assert!(
            Instant::now() < deadline,
            "last document releases retained source"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
