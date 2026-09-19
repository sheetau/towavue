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
    app.source_backings.insert(id, saved.into());
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

fn deletion_snapshot(folder: &Path, paths: &[PathBuf]) -> FolderSnapshot {
    FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![0]),
        folder_path: folder.to_owned(),
        items: paths
            .iter()
            .enumerate()
            .map(|(i, path)| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![i as u8 + 1]),
                path: path.clone(),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: Vec::new(),
        source: FolderSnapshotSource::PersistedShellView,
        generation: 1,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    }
}

#[test]
fn deleted_image_keeps_reading_pixels_edits_export_and_transfer_until_closed() {
    let Some(root) = crate::tests::isolated_test_root(
        "source_backing::tests::deleted_image_keeps_reading_pixels_edits_export_and_transfer_until_closed",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    let next = root.join("next.bmp");
    for path in [&source, &next] {
        crate::tab_transfer::tests::bitmap(path);
    }
    let (mut app, events) = self::app();
    app.open_external(source.clone(), true);
    finish(&mut app, &events);
    let id = app.displayed_tab.expect("tab");
    let before = deletion_snapshot(&root, &[source.clone(), next.clone()]);
    app.apply_folder_snapshot(before.clone());
    app.set_reading_layout(true, app.reading_settings);
    finish(&mut app, &events);
    let held_paths = app.reading_request_paths();
    let held_pages = app.reading_page_views().len();
    let original = app.image.as_ref().expect("image").decoded.frames[0]
        .rgba
        .clone();
    let expected = app.source_versions[&id].clone().expect("loaded version");
    app.quiesce_source_save(&source);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.source_readers_idle() {
        assert!(Instant::now() < deadline, "readers drained");
        std::thread::sleep(Duration::from_millis(2));
    }
    let retained =
        towavue_runtime_windows::RetainedSource::capture(&expected).expect("retained original");
    let backup = retained.original_path().to_owned();
    std::fs::remove_file(&source).expect("simulate accepted deletion");
    app.finish_file_recycling(
        &expected,
        &towavue_runtime_windows::FileRecycleReport {
            retained_source: Some(retained),
            before,
            after: Some(deletion_snapshot(&root, std::slice::from_ref(&next))),
        },
    );
    app.thaw_source_save(&source);
    assert_eq!(app.path.as_ref(), Some(&source));
    assert_eq!(app.reading_request_paths(), held_paths);
    assert_eq!(app.reading_page_views().len(), held_pages);
    assert_eq!(
        app.image.as_ref().expect("held image").decoded.frames[0].rgba,
        original
    );
    app.set_reading_layout(false, app.reading_settings);
    app.edits.entry(id).or_default().push(
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
    let export = root.join("export.bmp");
    assert!(app.start_export(
        id,
        source.clone(),
        MediaKind::Image,
        export.clone(),
        None,
        ExportOutput::Media
    ));
    finish(&mut app, &events);
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    assert!(app.current_source_deleted());
    assert!(!source.exists());
    assert_eq!(
        towavue_runtime_windows::decode_image(&export)
            .expect("derivative")
            .frames[0]
            .rgba,
        edited
    );
    app.open_external(next.clone(), true);
    finish(&mut app, &events);
    app.activate_tab(id);
    finish(&mut app, &events);
    assert!(app.current_source_deleted());
    assert_eq!(
        app.image.as_ref().expect("reactivated").decoded.frames[0].rgba,
        edited
    );
    let (mut target, target_events) = self::app();
    let moved = crate::tab_transfer::tests::transfer(&mut app, &mut target, id);
    finish(&mut target, &target_events);
    assert!(!app.deleted_sources.contains_key(&id));
    assert!(target.current_source_deleted());
    assert_eq!(target.media_input(&source).path(), backup);
    target.edits.get_mut(&moved).expect("history").undo();
    target.refresh_image_edits();
    finish(&mut target, &target_events);
    assert_eq!(
        target
            .image
            .as_ref()
            .expect("undo after transfer")
            .decoded
            .frames[0]
            .rgba,
        original
    );
    target.qualify_current_history();
    assert!(target.viewed_media.take_pending().is_empty());
    target.close_tab_unchecked(moved);
    assert!(target.deleted_sources.is_empty());
    assert!(
        target.tabs.gallery().is_some(),
        "closing the last deleted tab preserves normal Gallery behavior"
    );
    assert!(target.source_backings.is_empty());
    assert!(
        !target.closed_tabs.iter().any(
            |closed| matches!(closed, closed_tabs::ClosedTab::Media(path, _) if path == &source)
        )
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while backup.exists() {
        assert!(
            Instant::now() < deadline,
            "last deleted document releases original"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn deleted_navigation_uses_surviving_shell_neighbors_and_releases_the_old_document() {
    let Some(root) = crate::tests::isolated_test_root(
        "source_backing::tests::deleted_navigation_uses_surviving_shell_neighbors_and_releases_the_old_document",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    let first = root.join("first.bmp");
    let last = root.join("last.bmp");
    for path in [&source, &first, &last] {
        crate::tab_transfer::tests::bitmap(path);
    }
    let expected = FileOperationSource::capture(&source).expect("source");
    let retained = towavue_runtime_windows::RetainedSource::capture(&expected).expect("copy");
    let (mut app, events) = self::app();
    app.open_external(source.clone(), true);
    finish(&mut app, &events);
    let id = app.displayed_tab.expect("tab");
    std::fs::remove_file(&source).expect("owned removal");
    app.finish_file_recycling(
        &expected,
        &towavue_runtime_windows::FileRecycleReport {
            retained_source: Some(retained),
            before: deletion_snapshot(&root, &[first.clone(), source.clone(), last.clone()]),
            after: Some(deletion_snapshot(&root, &[first.clone(), last.clone()])),
        },
    );
    let snapshot = app.navigation_snapshot().expect("position");
    assert_eq!(
        snapshot
            .items
            .iter()
            .map(|item| &item.path)
            .collect::<Vec<_>>(),
        vec![&first, &source, &last]
    );
    app.jump_images(1);
    finish(&mut app, &events);
    assert_eq!(app.path.as_ref(), Some(&last));
    assert_eq!(app.displayed_tab, Some(id));
    assert!(app.deleted_sources.is_empty());
    assert!(app.source_backings.is_empty());
}

#[test]
fn deleted_filmstrip_keeps_a_local_held_entry_and_can_navigate_to_real_media() {
    let Some(root) = crate::tests::isolated_test_root(
        "source_backing::tests::deleted_filmstrip_keeps_a_local_held_entry_and_can_navigate_to_real_media",
    ) else {
        return;
    };
    let source = root.join("source.bmp");
    let next = root.join("next.bmp");
    for path in [&source, &next] {
        crate::tab_transfer::tests::bitmap(path);
    }
    let (mut app, events) = self::app();
    app.open_external(source.clone(), true);
    finish(&mut app, &events);
    let id = app.displayed_tab.expect("tab");
    let before = deletion_snapshot(&root, &[source.clone(), next.clone()]);
    app.apply_folder_snapshot(before.clone());
    let context = app.ui_context.clone().expect("context");
    context.enable_accesskit();
    app.dispatch(CommandId::ToggleFilmstrip);
    let draw = |app: &mut App, events| {
        let mut actions = Vec::new();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 576.0),
                )),
                events,
                ..Default::default()
            },
            |_| app.draw_filmstrip(&context, context.content_rect(), &mut actions),
        );
        (output, actions)
    };
    for _ in 0..3 {
        draw(&mut app, vec![]);
    }
    let expected = FileOperationSource::capture(&source).expect("source");
    app.quiesce_source_save(&source);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.source_readers_idle() {
        assert!(Instant::now() < deadline, "readers drained");
        std::thread::sleep(Duration::from_millis(2));
    }
    let retained = towavue_runtime_windows::RetainedSource::capture(&expected).expect("retained");
    std::fs::remove_file(&source).expect("owned deletion");
    app.finish_file_recycling(
        &expected,
        &towavue_runtime_windows::FileRecycleReport {
            retained_source: Some(retained),
            before,
            after: Some(deletion_snapshot(&root, std::slice::from_ref(&next))),
        },
    );
    app.thaw_source_save(&source);
    app.folder_order.request(None);
    app.pending_folder = None;
    assert!(app.filmstrip_open);
    assert!(
        app.folder_snapshot
            .as_ref()
            .expect("real Shell view")
            .items
            .iter()
            .all(|item| item.path != source)
    );
    for density in [1.0, 1.25, 2.0] {
        context.set_pixels_per_point(density);
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        let (output, actions) = draw(&mut app, vec![]);
        assert!(actions.is_empty());
        let tree = output.platform_output.accesskit_update.expect("tree");
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("(deleted) source.bmp"))
        );
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("next.bmp"))
        );
    }
    let count = app.tabs.tabs().len();
    app.handle_ui_action(UiAction::OpenFilmstripMedia(source.clone(), true));
    app.handle_ui_action(UiAction::OpenFilmstripWindow(source.clone()));
    assert_eq!(
        app.tabs.tabs().len(),
        count,
        "held path cannot open a new file-backed tab"
    );
    assert!(app.pending_window_launches.is_empty());
    app.handle_ui_action(UiAction::OpenFilmstripMedia(source.clone(), false));
    assert!(!app.filmstrip_open);
    assert!(app.current_source_deleted());
    assert_eq!(app.path.as_ref(), Some(&source));
    app.dispatch(CommandId::ToggleFilmstrip);
    for _ in 0..3 {
        draw(&mut app, vec![]);
    }
    let output = draw(&mut app, vec![]).0;
    let node = output
        .platform_output
        .accesskit_update
        .expect("tree")
        .nodes
        .into_iter()
        .find(|(_, node)| node.label() == Some("next.bmp"))
        .expect("next card")
        .0;
    let (_, actions) = draw(
        &mut app,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: node,
                data: None,
            },
        )],
    );
    assert!(
        actions.iter().any(
            |action| matches!(action, UiAction::OpenFilmstripMedia(path, false) if path==&next)
        )
    );
    for action in actions {
        app.handle_ui_action(action);
    }
    finish(&mut app, &events);
    assert_eq!(app.path.as_ref(), Some(&next));
    assert_eq!(app.displayed_tab, Some(id));
    assert!(app.deleted_sources.is_empty());
    assert!(app.source_backings.is_empty());
    assert!(!source.exists());
}
