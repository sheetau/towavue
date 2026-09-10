use super::*;
use std::os::windows::process::CommandExt;

fn image_fixture(root: &Path, extension: &str) -> PathBuf {
    let raw = root.join(format!("raw.{extension}"));
    let source = root.join(format!("source.{}", extension.to_ascii_uppercase()));
    let output = std::process::Command::new(
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe"),
    )
    .creation_flags(0x08000000)
    .args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=32x24",
        "-frames:v",
        "1",
    ])
    .arg(&raw)
    .output()
    .expect("generated image");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if extension == "jpeg" {
        let packet = r#"<r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:d="http://purl.org/dc/elements/1.1/"><d:title><r:Alt><r:li xml:lang="x-default">Original title</r:li><r:li xml:lang="ja">元の題名</r:li></r:Alt></d:title><d:creator><r:Seq><r:li>First author</r:li><r:li>Second author</r:li></r:Seq></d:creator><d:rights><r:Alt><r:li xml:lang="x-default">Original copyright</r:li></r:Alt></d:rights></r:Description></r:RDF>"#;
        let header = b"http://ns.adobe.com/xap/1.0/\0";
        let bytes = std::fs::read(&raw).expect("JPEG bytes");
        let mut tagged = bytes[..2].to_vec();
        tagged.extend_from_slice(&[0xff, 0xe1]);
        tagged.extend_from_slice(&((header.len() + packet.len() + 2) as u16).to_be_bytes());
        tagged.extend_from_slice(header);
        tagged.extend_from_slice(packet.as_bytes());
        tagged.extend_from_slice(&bytes[2..]);
        std::fs::write(&source, tagged).expect("JPEG XMP fixture");
        return source;
    }
    let mut metadata = MetadataExportOptions::default();
    for (field, value) in [
        (MetadataField::Title, "元の題名"),
        (MetadataField::Artist, "Original author"),
        (MetadataField::Copyright, "Original copyright"),
    ] {
        metadata
            .set(field, Some(value.into()))
            .expect("fixture metadata");
    }
    towavue_runtime_windows::export_media_with_options(
        &ExportRequest {
            source: raw,
            target: source.clone(),
            kind: MediaKind::Image,
            operations: vec![],
            hardware_encode: false,
        },
        ExportOptions {
            metadata,
            ..Default::default()
        },
    )
    .expect("tagged source");
    source
}

fn apply_ready<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: &std::sync::mpsc::Receiver<AppEvent>,
    options: MetadataExportOptions,
) {
    app.dispatch(CommandId::MetadataExportOptions);
    read_ready(app, events);
    let token = app.metadata_dialog.as_ref().expect("dialog").token;
    app.handle_ui_action(UiAction::FinishMetadataOptions(token, Some(options)));
    assert!(app.metadata_dialog.is_none());
}

fn values(path: &Path) -> Vec<MetadataSourceValue> {
    towavue_runtime_windows::read_export_metadata(path, MediaKind::Image).expect("image values")
}

#[test]
fn png_metadata_ui_reads_source_blocks_unsupported_or_failed_reads_and_explains_scope() {
    let Some(root) = crate::tests::isolated_test_root(
        "metadata_export::tests::image::png_metadata_ui_reads_source_blocks_unsupported_or_failed_reads_and_explains_scope",
    ) else {
        return;
    };
    metadata_ui_reads_source_blocks_unsupported_or_failed_reads_and_explains_scope(&root, "png");
}

#[test]
fn jpeg_metadata_ui_reads_source_blocks_unsupported_or_failed_reads_and_explains_scope() {
    let Some(root) = crate::tests::isolated_test_root(
        "metadata_export::tests::image::jpeg_metadata_ui_reads_source_blocks_unsupported_or_failed_reads_and_explains_scope",
    ) else {
        return;
    };
    metadata_ui_reads_source_blocks_unsupported_or_failed_reads_and_explains_scope(&root, "jpeg");
}

fn metadata_ui_reads_source_blocks_unsupported_or_failed_reads_and_explains_scope(
    root: &Path,
    extension: &str,
) {
    let source = image_fixture(root, extension);
    let original = std::fs::read(&source).expect("source");
    let (send, events) = std::sync::mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = send.send(event);
    })
    .expect("app");
    let context = fonts::test_context();
    context.enable_accesskit();
    app.ui_context = Some(context.clone());
    let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
    app.path = Some(source.clone());
    app.media_kind = Some(MediaKind::Image);
    let size = egui::vec2(640.0, 900.0);
    app.open_metadata_export_options();
    let token = app.metadata_dialog.as_ref().expect("dialog").token;
    app.handle_ui_action(UiAction::FinishMetadataOptions(token, Some(setting())));
    assert!(
        app.metadata_dialog.is_some() && app.metadata_export_settings.is_empty(),
        "queued Apply cannot bypass pending read"
    );
    read_ready(&mut app, &events);
    for _ in 0..3 {
        frame(&mut app, size, vec![]);
    }
    let output = frame(&mut app, size, vec![]);
    let tree = output.platform_output.accesskit_update.expect("tree");
    let labels: Vec<_> = tree
        .nodes
        .iter()
        .filter_map(|(_, node)| node.label().or_else(|| node.value()))
        .collect();
    let expected = if extension == "png" {
        vec![
            "PNG text: 元の題名",
            "PNG keyword: Title",
            "PNG input and PNG output only.",
            "EXIF, XMP",
            "including when all fields are Keep",
            "Choose a .png export path",
        ]
    } else {
        vec![
            "JPEG XMP (x-default): Original title",
            "JPEG XMP (ja): 元の題名",
            "JPEG XMP property: dc:title (language alternatives)",
            "JPEG input and JPEG output only.",
            "EXIF, IPTC and JPEG comments (COM) are not synchronized",
            "including when all fields are Keep",
            "Set replaces all values of the field with one",
            "Remove deletes all values",
            "Choose a .jpg or .jpeg export path",
        ]
    };
    for expected in expected {
        assert!(
            labels.iter().any(|label| label.contains(expected)),
            "missing explanation: {expected}"
        );
    }
    assert!(
        !tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Apply metadata"))
            .expect("Apply")
            .1
            .is_disabled()
    );
    if extension == "jpeg" {
        click(&mut app, "Title");
        let tree = frame(&mut app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("selector");
        for unsupported in [
            "Album",
            "Album artist",
            "Composer",
            "Genre",
            "Date",
            "Track",
        ] {
            assert!(
                !tree
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some(unsupported)),
                "hidden JPEG field {unsupported}"
            );
        }
        click(&mut app, "Artist");
        let tree = frame(&mut app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("creators");
        for expected in [
            "JPEG XMP (creator 1): First author",
            "JPEG XMP (creator 2): Second author",
        ] {
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, node)| node.value() == Some(expected)),
                "{expected}"
            );
        }
        click(&mut app, "Artist");
        click(&mut app, "Title");
        let mut unsupported = MetadataExportOptions::default();
        unsupported
            .set(MetadataField::Album, Some("hidden field".into()))
            .expect("typed text");
        app.handle_ui_action(UiAction::FinishMetadataOptions(token, Some(unsupported)));
        assert!(app.metadata_dialog.is_some() && app.metadata_export_settings.is_empty());
        let mut invalid = MetadataExportOptions::default();
        invalid
            .set(MetadataField::Title, Some("invalid XML\u{1}".into()))
            .expect("typed text");
        app.handle_ui_action(UiAction::FinishMetadataOptions(token, Some(invalid)));
        assert!(app.metadata_dialog.is_some() && app.metadata_export_settings.is_empty());
        click(&mut app, "Set value");
        set_value(&mut app, "invalid XML\u{1}");
        let tree = frame(&mut app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("XML validation");
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Apply metadata") && node.is_disabled())
        );
        app.handle_ui_action(UiAction::FinishMetadataOptions(token, Some(setting())));
        assert!(app.metadata_dialog.is_some() && app.metadata_export_settings.is_empty());
    }
    click(&mut app, "Set value");
    set_value(&mut app, "日本語 UI title");
    click(&mut app, "Apply metadata");
    assert_eq!(
        app.metadata_export_settings[&tab].get(MetadataField::Title),
        Some("日本語 UI title")
    );
    assert!(app.edits.is_empty());
    let retained = app.metadata_export_settings[&tab].clone();
    app.open_metadata_export_options();
    let old_token = app.metadata_dialog.as_ref().expect("old dialog").token;
    app.media_generation += 1;
    app.finish_metadata_read(old_token, Ok(vec![]));
    assert!(
        app.metadata_dialog
            .as_ref()
            .expect("stale dialog")
            .current
            .is_none()
    );
    app.cancel_stale_metadata_dialog();
    assert!(app.metadata_dialog.is_none());
    app.open_metadata_export_options();
    app.finish_metadata_read(old_token, Err("late result".into()));
    assert!(
        app.metadata_dialog
            .as_ref()
            .expect("new dialog")
            .current
            .is_none()
    );
    read_ready(&mut app, &events);
    assert!(
        app.metadata_dialog
            .as_ref()
            .expect("current")
            .current
            .as_ref()
            .expect("read")
            .is_ok()
    );
    click(&mut app, "Cancel");
    assert_eq!(app.metadata_export_settings[&tab], retained);
    for extension in ["jpg", "webp", "tiff", "gif", "avif", "bmp", "png"] {
        let other = root.join(format!("unsupported.{extension}"));
        std::fs::write(&other, b"not PNG").expect("invalid fixture");
        app.tabs
            .active_mut()
            .expect("tab")
            .target
            .set_current_path(other.clone(), MediaKind::Image);
        app.path = Some(other);
        app.open_metadata_export_options();
        read_ready(&mut app, &events);
        let token = app.metadata_dialog.as_ref().expect("dialog").token;
        assert!(
            app.metadata_dialog
                .as_ref()
                .expect("dialog")
                .current
                .as_ref()
                .expect("read")
                .is_err()
        );
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        let tree = frame(&mut app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("tree");
        assert!(
            tree.nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Apply metadata"))
                .expect("Apply")
                .1
                .is_disabled()
        );
        app.handle_ui_action(UiAction::FinishMetadataOptions(
            token,
            Some(MetadataExportOptions::default()),
        ));
        assert!(app.metadata_dialog.is_some());
        assert_eq!(app.metadata_export_settings[&tab], retained);
        click(&mut app, "Cancel");
        assert!(app.metadata_dialog.is_none());
    }
    assert_eq!(std::fs::read(&source).expect("source unchanged"), original);
}

#[test]
fn png_metadata_save_resave_all_keep_remove_format_failure_guard_and_source_lifecycle() {
    let Some(root) = crate::tests::isolated_test_root(
        "metadata_export::tests::image::png_metadata_save_resave_all_keep_remove_format_failure_guard_and_source_lifecycle",
    ) else {
        return;
    };
    metadata_save_resave_all_keep_remove_format_failure_guard_and_source_lifecycle(&root, "png");
}

#[test]
fn jpeg_metadata_save_resave_all_keep_remove_format_failure_guard_and_source_lifecycle() {
    let Some(root) = crate::tests::isolated_test_root(
        "metadata_export::tests::image::jpeg_metadata_save_resave_all_keep_remove_format_failure_guard_and_source_lifecycle",
    ) else {
        return;
    };
    metadata_save_resave_all_keep_remove_format_failure_guard_and_source_lifecycle(&root, "jpeg");
}

fn metadata_save_resave_all_keep_remove_format_failure_guard_and_source_lifecycle(
    root: &Path,
    extension: &str,
) {
    let source = image_fixture(root, extension);
    let original = std::fs::read(&source).expect("original");
    let source_values = values(&source);
    let (send, events) = std::sync::mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = send.send(event);
    })
    .expect("app");
    let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
    app.path = Some(source.clone());
    app.media_kind = Some(MediaKind::Image);
    app.state = PlaybackState::Paused;
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Image);
    let history = app.edits.clone();
    let generation = app.media_generation;
    let target = root.join(format!("saved.{extension}"));
    let intent = || DialogIntent::Export {
        tab,
        source: source.clone(),
        kind: MediaKind::Image,
        generation,
        output: ExportOutput::Media,
        continuation: None,
    };
    apply_ready(&mut app, &events, setting());
    assert_eq!(app.edits, history);
    assert_eq!(app.state, PlaybackState::Paused);
    assert_eq!(app.media_generation, generation);
    app.pending_dialog = Some(intent());
    app.finish_dialog(Ok(None));
    assert_eq!(app.metadata_export_settings.get(&tab), Some(&setting()));
    app.pending_dialog = Some(intent());
    app.finish_dialog(Ok(Some(target.clone())));
    assert_eq!(
        app.active_export.as_ref().expect("job").options.metadata,
        setting()
    );
    app.open_metadata_export_options();
    assert!(app.metadata_dialog.is_none());
    drain_export(&mut app, &events);
    assert!(app.export_error.is_none(), "{:?}", app.export_error);
    assert!(!app.edits[&tab].is_dirty());
    let pixels = towavue_runtime_windows::decode_image(&target).expect("pixels");
    assert_eq!((pixels.frames[0].width, pixels.frames[0].height), (24, 32));
    assert!(
        values(&target)
            .iter()
            .any(|value| value.field == MetadataField::Title
                && value.value == "日本語 exported title")
    );
    app.dispatch(CommandId::Save);
    drain_export(&mut app, &events);
    assert!(app.export_error.is_none());
    assert_eq!(app.export_paths.get(&tab), Some(&target));
    let mut remove = MetadataExportOptions::default();
    remove
        .set(MetadataField::Title, Some(String::new()))
        .expect("remove");
    apply_ready(&mut app, &events, remove);
    app.dispatch(CommandId::Save);
    drain_export(&mut app, &events);
    assert!(app.export_error.is_none());
    assert!(
        values(&target)
            .iter()
            .all(|value| value.field != MetadataField::Title)
    );
    apply_ready(&mut app, &events, MetadataExportOptions::default());
    assert!(!app.metadata_export_settings.contains_key(&tab));
    app.dispatch(CommandId::Save);
    drain_export(&mut app, &events);
    assert!(app.export_error.is_none());
    assert_eq!(
        values(&target),
        source_values,
        "all Keep restores source, not previous output tags"
    );
    assert_eq!(
        towavue_runtime_windows::decode_image(&target)
            .expect("Keep pixels")
            .frames[0]
            .rgba,
        pixels.frames[0].rgba
    );
    apply_ready(&mut app, &events, setting());
    app.edits
        .get_mut(&tab)
        .expect("history")
        .push(EditOperation::FlipHorizontal, MediaKind::Image);
    let bad = root.join(if extension == "png" {
        "unsupported.jpg"
    } else {
        "unsupported.png"
    });
    std::fs::write(&bad, b"existing target").expect("sentinel");
    app.pending_dialog = Some(intent());
    app.finish_dialog(Ok(Some(bad.clone())));
    drain_export(&mut app, &events);
    assert!(
        app.export_error
            .as_ref()
            .expect("format failure")
            .contains(if extension == "png" {
                "PNG input and PNG output"
            } else {
                "JPEG input and JPEG output"
            })
    );
    assert_eq!(std::fs::read(&bad).expect("protected"), b"existing target");
    assert_eq!(app.export_paths.get(&tab), Some(&target));
    assert!(app.edits[&tab].is_dirty());
    assert_eq!(app.metadata_export_settings.get(&tab), Some(&setting()));
    app.handle_ui_action(UiAction::DismissExportError);
    app.request_guarded(GuardedAction::Exit);
    app.handle_ui_action(UiAction::ResolveGuard(GuardDecision::Save));
    assert!(!app.exit_requested);
    drain_export(&mut app, &events);
    assert!(app.exit_requested && app.export_error.is_none());
    app.exit_requested = false;
    let other_path = root.join(format!("other.{extension}"));
    let other = app.tabs.open_new(other_path.clone(), MediaKind::Image);
    std::fs::copy(&source, &other_path).expect("other image");
    app.tabs.activate(tab);
    app.displayed_tab = Some(tab);
    app.activate_tab(other);
    assert_eq!(app.path.as_deref(), Some(other_path.as_path()));
    assert_eq!(app.metadata_export_settings.get(&tab), Some(&setting()));
    assert!(!app.metadata_export_settings.contains_key(&other));
    app.activate_tab(tab);
    assert_eq!(app.path.as_deref(), Some(source.as_path()));
    assert_eq!(app.metadata_export_settings.get(&tab), Some(&setting()));
    app.navigate_to_unchecked(source.clone());
    assert!(!app.metadata_export_settings.contains_key(&tab));
    apply_ready(&mut app, &events, setting());
    app.request_guarded(GuardedAction::CloseTab(tab));
    assert!(!app.metadata_export_settings.contains_key(&tab));
    assert_eq!(std::fs::read(&source).expect("source retained"), original);
}
