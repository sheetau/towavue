use super::*;
use crate::audio_export::tests::frame;
use crate::audio_export_tests::{drain_export, fixture};
use crate::video_rotation::tests::{access, node};

mod png;

fn read_ready<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: &std::sync::mpsc::Receiver<AppEvent>,
) {
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while app
        .metadata_dialog
        .as_ref()
        .expect("reading dialog")
        .current
        .is_none()
    {
        assert!(std::time::Instant::now() < until, "metadata read deadline");
        if let Ok(event) = events.recv_timeout(Duration::from_millis(20)) {
            app.handle_app_event(event);
        }
    }
}

fn setting() -> MetadataExportOptions {
    let mut value = MetadataExportOptions::default();
    value
        .set(MetadataField::Title, Some("日本語 exported title".into()))
        .expect("valid title");
    value
}

fn apply<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    options: MetadataExportOptions,
) {
    app.dispatch(CommandId::MetadataExportOptions);
    let token = app.metadata_dialog.as_ref().expect("metadata dialog").token;
    app.handle_ui_action(UiAction::FinishMetadataOptions(token, Some(options)));
}

fn click<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>, label: &str) {
    let tree = frame(app, egui::vec2(640.0, 600.0), vec![])
        .platform_output
        .accesskit_update
        .expect("tree");
    let target = tree
        .nodes
        .iter()
        .find(|(_, node)| {
            node.role() == egui::accesskit::Role::ComboBox && node.value() == Some(label)
        })
        .map(|(id, _)| *id)
        .unwrap_or_else(|| node(&tree, label));
    frame(app, egui::vec2(640.0, 600.0), vec![access(target, None)]);
}

fn set_value<N: Fn(AppEvent) + Send + Sync + 'static>(app: &mut Application<N>, value: &str) {
    let tree = frame(app, egui::vec2(640.0, 600.0), vec![])
        .platform_output
        .accesskit_update
        .expect("tree");
    let inputs: Vec<_> = tree
        .nodes
        .iter()
        .filter(|(_, node)| {
            node.supports_action(egui::accesskit::Action::SetValue) && !node.is_disabled()
        })
        .collect();
    assert_eq!(inputs.len(), 1, "one enabled metadata text input");
    assert_eq!(
        inputs[0].1.role(),
        egui::accesskit::Role::MultilineTextInput
    );
    frame(
        app,
        egui::vec2(640.0, 600.0),
        vec![access(inputs[0].0, Some(value))],
    );
}

#[test]
fn metadata_modal_rejects_stale_input_reads_and_leaving_without_changing_history() {
    let Some(root) = crate::tests::isolated_test_root(
        "metadata_export::tests::metadata_modal_rejects_stale_input_reads_and_leaving_without_changing_history",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    let source = root.join("source.mkv");
    let tab = app.tabs.open_new(source.clone(), MediaKind::Video);
    app.path = Some(source);
    app.media_kind = Some(MediaKind::Video);
    app.state = PlaybackState::Playing;
    app.edits
        .entry(tab)
        .or_default()
        .push(EditOperation::RotateClockwise, MediaKind::Video);
    let history = app.edits.clone();
    app.dispatch(CommandId::MetadataExportOptions);
    let token = app.metadata_dialog.as_ref().expect("viewing options").token;
    app.dispatch(CommandId::Undo);
    app.dispatch(CommandId::AudioExportOptions);
    app.request_guarded(GuardedAction::Exit);
    assert!(
        app.audio_export_dialog.is_none() && app.pending_guard.is_none() && !app.exit_requested
    );
    app.finish_metadata_read(token.wrapping_add(1), Ok(vec![]));
    assert!(
        app.metadata_dialog
            .as_ref()
            .expect("draft")
            .current
            .is_none()
    );
    app.finish_metadata_read(token, Err("read failure".into()));
    assert!(
        app.metadata_dialog
            .as_ref()
            .expect("draft")
            .current
            .as_ref()
            .expect("read result")
            .is_err()
    );
    app.handle_ui_action(UiAction::FinishMetadataOptions(
        token.wrapping_add(1),
        Some(setting()),
    ));
    assert!(app.metadata_dialog.is_some() && app.metadata_export_settings.is_empty());
    app.handle_ui_action(UiAction::FinishMetadataOptions(token, None));
    apply(&mut app, setting());
    assert_eq!(app.metadata_export_settings.get(&tab), Some(&setting()));
    assert_eq!(app.edits, history);
    assert_eq!(app.state, PlaybackState::Playing);
    for stale in 0..4 {
        app.open_metadata_export_options();
        let dialog = app.metadata_dialog.as_mut().expect("modal");
        let token = dialog.token;
        match stale {
            0 => dialog.generation += 1,
            1 => dialog.source = root.join("other.mkv"),
            2 => dialog.kind = MediaKind::Audio,
            _ => {
                dialog.tab = app.tabs.open_new(root.join("other.wav"), MediaKind::Audio);
                app.tabs.activate(tab);
            }
        }
        app.finish_metadata_read(token, Ok(vec![]));
        assert!(
            app.metadata_dialog
                .as_ref()
                .expect("stale")
                .current
                .is_none()
        );
        app.handle_ui_action(UiAction::FinishMetadataOptions(
            token,
            Some(MetadataExportOptions::default()),
        ));
        assert_eq!(app.metadata_export_settings.get(&tab), Some(&setting()));
    }
    apply(&mut app, MetadataExportOptions::default());
    assert!(app.metadata_export_settings.is_empty());
    for kind in [None, Some(MediaKind::Image)] {
        app.media_kind = kind;
        app.dispatch(CommandId::MetadataExportOptions);
        assert!(app.metadata_dialog.is_none());
    }
}

#[test]
fn metadata_ui_all_fields_modes_invalid_text_cancel_focus_and_compact_layout() {
    let Some(root) = crate::tests::isolated_test_root(
        "metadata_export::tests::metadata_ui_all_fields_modes_invalid_text_cancel_focus_and_compact_layout",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Image] {
        let context = fonts::test_context();
        context.enable_accesskit();
        let mut app = Application::new(None, |_| {}).expect("app");
        app.ui_context = Some(context.clone());
        let source = root.join(if kind == MediaKind::Image {
            "image.PNG"
        } else {
            "audio.wav"
        });
        let tab = app.tabs.open_new(source.clone(), kind);
        app.path = Some(source);
        app.media_kind = Some(kind);
        app.state = PlaybackState::Paused;
        let size = egui::vec2(640.0, 600.0);
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        let menu = node(
            &frame(&mut app, size, vec![])
                .platform_output
                .accesskit_update
                .expect("tree"),
            "towavue menu",
        );
        frame(
            &mut app,
            size,
            vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: menu,
                    data: None,
                },
            )],
        );
        app.dispatch(CommandId::MetadataExportOptions);
        let token = app.metadata_dialog.as_ref().expect("dialog").token;
        app.finish_metadata_read(
            token,
            Ok(vec![MetadataSourceValue {
                field: MetadataField::Title,
                scope: "File",
                value: "Original title".into(),
                truncated: false,
            }]),
        );
        for (index, field) in MetadataField::ALL.into_iter().enumerate() {
            if index > 0 {
                click(&mut app, MetadataField::ALL[index - 1].label());
                click(&mut app, field.label());
            }
            click(&mut app, "Set value");
            let value = format!("{} 日本語\nsecond line", field.label());
            set_value(&mut app, &value);
            app.finish_metadata_read(token, Ok(vec![]));
            assert_eq!(
                app.metadata_dialog.as_ref().expect("draft").fields[index].text,
                value
            );
            click(&mut app, "Remove value");
            assert_eq!(
                app.metadata_dialog
                    .as_ref()
                    .expect("draft")
                    .options()
                    .expect("valid")
                    .get(field),
                Some("")
            );
            click(&mut app, "Keep source value");
            assert_eq!(
                app.metadata_dialog
                    .as_ref()
                    .expect("draft")
                    .options()
                    .expect("valid")
                    .get(field),
                None
            );
            click(&mut app, "Set value");
        }
        assert!(app.metadata_export_settings.is_empty());
        set_value(&mut app, &"音".repeat(342));
        let tree = frame(&mut app, size, vec![])
            .platform_output
            .accesskit_update
            .expect("tree");
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Apply metadata") && node.is_disabled())
        );
        set_value(&mut app, "Copyright text");
        let expected = app
            .metadata_dialog
            .as_ref()
            .expect("draft")
            .options()
            .expect("valid");
        click(&mut app, "Apply metadata");
        assert_eq!(app.metadata_export_settings.get(&tab), Some(&expected));
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        assert_eq!(
            frame(&mut app, size, vec![])
                .platform_output
                .accesskit_update
                .expect("tree")
                .focus,
            menu
        );
        for command in [CommandId::ToggleCommandPalette, CommandId::ToggleGridMenu] {
            app.dispatch(command);
            frame(&mut app, size, vec![]);
            app.dispatch(CommandId::MetadataExportOptions);
            assert!(!app.palette_open && !app.grid_open);
            click(&mut app, "Keep source value");
            click(&mut app, "Cancel");
            assert_eq!(app.metadata_export_settings.get(&tab), Some(&expected));
            for _ in 0..3 {
                frame(&mut app, size, vec![]);
            }
            assert_eq!(
                frame(&mut app, size, vec![])
                    .platform_output
                    .accesskit_update
                    .expect("tree")
                    .focus,
                menu
            );
        }
        for size in [
            egui::vec2(480.0, 360.0),
            egui::vec2(320.0, 200.0),
            egui::vec2(240.0, 150.0),
        ] {
            app.open_metadata_export_options();
            for _ in 0..4 {
                frame(&mut app, size, vec![]);
            }
            let output = frame(&mut app, size, vec![]);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for label in ["Apply metadata", "Cancel"] {
                let (clip, text) = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == label => {
                            Some((shape.clip_rect, text))
                        }
                        _ => None,
                    })
                    .expect("footer text remains drawn");
                assert!(screen.contains_rect(text.visual_bounding_rect()));
                assert!(clip.contains_rect(text.visual_bounding_rect()));
            }
            frame(
                &mut app,
                size,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(app.metadata_dialog.is_none());
            assert_eq!(app.metadata_export_settings.get(&tab), Some(&expected));
        }
        assert!(
            app.edits
                .get(&tab)
                .is_none_or(|history| !history.is_dirty())
        );
    }
}

#[test]
fn metadata_settings_read_save_resave_derivative_guard_and_reset_with_source() {
    let Some(root) = crate::tests::isolated_test_root(
        "metadata_export::tests::metadata_settings_read_save_resave_derivative_guard_and_reset_with_source",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        let source = root.join(if kind == MediaKind::Audio {
            "audio-source.mkv"
        } else {
            "video-source.mkv"
        });
        fixture(&source);
        let original = std::fs::read(&source).expect("source");
        let (sender, events) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            let _ = sender.send(event);
        })
        .expect("app");
        let tab = app.tabs.open_new(source.clone(), kind);
        app.path = Some(source.clone());
        app.media_kind = Some(kind);
        app.state = PlaybackState::Paused;
        app.edits
            .entry(tab)
            .or_default()
            .push(EditOperation::SetVolume(0.5), kind);
        app.dispatch(CommandId::MetadataExportOptions);
        read_ready(&mut app, &events);
        assert!(
            app.metadata_dialog
                .as_ref()
                .expect("dialog")
                .current
                .as_ref()
                .expect("read")
                .is_ok()
        );
        let token = app.metadata_dialog.as_ref().expect("dialog").token;
        app.handle_ui_action(UiAction::FinishMetadataOptions(token, Some(setting())));
        let target = root.join(if kind == MediaKind::Audio {
            "audio.wav"
        } else {
            "video.avi"
        });
        let generation = app.media_generation;
        let intent = |output| DialogIntent::Export {
            tab,
            source: source.clone(),
            kind,
            generation,
            output,
            continuation: None,
        };
        app.pending_dialog = Some(intent(ExportOutput::Media));
        app.finish_dialog(Ok(Some(target.clone())));
        assert_eq!(
            app.active_export.as_ref().expect("job").options.metadata,
            setting()
        );
        app.open_metadata_export_options();
        assert!(
            app.metadata_dialog.is_none(),
            "running export blocks settings"
        );
        drain_export(&mut app, &events);
        assert!(app.export_error.is_none(), "{:?}", app.export_error);
        assert!(!app.edits[&tab].is_dirty());
        let tags =
            towavue_runtime_windows::read_export_metadata(&target, kind).expect("saved metadata");
        assert!(tags.iter().any(|value| value.field == MetadataField::Title
            && value.value == setting().get(MetadataField::Title).expect("title")));
        app.dispatch(CommandId::Save);
        drain_export(&mut app, &events);
        assert!(app.export_error.is_none());
        assert_eq!(app.export_paths.get(&tab), Some(&target));
        let mut removed = MetadataExportOptions::default();
        removed
            .set(MetadataField::Title, Some(String::new()))
            .expect("remove title");
        apply(&mut app, removed.clone());
        app.dispatch(CommandId::Save);
        drain_export(&mut app, &events);
        assert!(app.export_error.is_none());
        assert!(
            !towavue_runtime_windows::read_export_metadata(&target, kind)
                .expect("removed metadata")
                .iter()
                .any(|value| value.field == MetadataField::Title && !value.value.is_empty())
        );
        apply(&mut app, setting());
        app.edits
            .get_mut(&tab)
            .expect("history")
            .push(EditOperation::SetVolume(0.25), kind);
        if kind == MediaKind::Video {
            let derivative = root.join("derivative.wav");
            app.pending_dialog = Some(intent(ExportOutput::AudioOnly));
            app.finish_dialog(Ok(Some(derivative.clone())));
            drain_export(&mut app, &events);
            assert!(app.export_error.is_none());
            assert!(app.edits[&tab].is_dirty());
            assert_eq!(app.export_paths.get(&tab), Some(&target));
            assert!(
                towavue_runtime_windows::read_export_metadata(&derivative, MediaKind::Audio)
                    .expect("derivative tags")
                    .iter()
                    .any(|value| value.field == MetadataField::Title
                        && value.value == "日本語 exported title")
            );
        }
        let bad_target = root.join(if kind == MediaKind::Video {
            "video-bad.aac"
        } else {
            "audio-bad.aac"
        });
        std::fs::write(&bad_target, b"existing").expect("owned target");
        app.pending_dialog = Some(intent(ExportOutput::AudioOnly));
        app.finish_dialog(Ok(Some(bad_target.clone())));
        drain_export(&mut app, &events);
        assert!(app.export_error.is_some());
        assert!(app.edits[&tab].is_dirty());
        assert_eq!(std::fs::read(&bad_target).expect("target"), b"existing");
        app.handle_ui_action(UiAction::DismissExportError);
        app.pending_dialog = Some(intent(ExportOutput::Media));
        app.finish_dialog(Ok(None));
        assert_eq!(app.metadata_export_settings.get(&tab), Some(&setting()));
        app.request_guarded(GuardedAction::Exit);
        assert!(app.pending_guard.is_some());
        app.handle_ui_action(UiAction::ResolveGuard(GuardDecision::Save));
        assert_eq!(
            app.active_export
                .as_ref()
                .expect("guard save")
                .options
                .metadata,
            setting()
        );
        assert!(!app.exit_requested);
        drain_export(&mut app, &events);
        assert!(app.exit_requested && app.export_error.is_none());
        app.exit_requested = false;
        let other = app.tabs.open_new(root.join("other.png"), MediaKind::Image);
        assert_eq!(app.metadata_export_settings.get(&tab), Some(&setting()));
        assert!(!app.metadata_export_settings.contains_key(&other));
        app.tabs.activate(tab);
        app.navigate_to_unchecked(source.clone());
        assert!(!app.metadata_export_settings.contains_key(&tab));
        apply(&mut app, setting());
        app.request_guarded(GuardedAction::CloseTab(tab));
        assert!(!app.metadata_export_settings.contains_key(&tab));
        assert_eq!(std::fs::read(&source).expect("source retained"), original);
    }
}

pub(crate) fn hardware_round_trip<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
) {
    let tab = app.tabs.active().expect("tab").id;
    let previous = app
        .metadata_export_settings
        .get(&tab)
        .cloned()
        .unwrap_or_default();
    let history = app.edits.clone();
    let state = app.state;
    let generation = app.media_generation;
    app.dispatch(CommandId::MetadataExportOptions);
    app.render_frame();
    let tree = crate::video_rotation::tests::frame(app, vec![]);
    crate::video_rotation::tests::frame(app, vec![access(node(&tree, "Remove value"), None)]);
    let tree = crate::video_rotation::tests::frame(app, vec![]);
    crate::video_rotation::tests::frame(app, vec![access(node(&tree, "Apply metadata"), None)]);
    assert_eq!(
        app.metadata_export_settings
            .get(&tab)
            .expect("applied")
            .get(MetadataField::Title),
        Some("")
    );
    apply(app, previous);
    app.render_frame();
    assert_eq!(app.edits, history);
    assert_eq!(app.state, state);
    assert_eq!(app.media_generation, generation);
    assert_eq!(
        app.session
            .as_ref()
            .expect("session")
            .metrics()
            .cpu_transfer_count,
        0
    );
    eprintln!(
        "PASS hardware metadata options: UIA Apply/restore, unchanged history/transport and CPU transfers 0"
    );
}

#[test]
fn metadata_ime_and_popup_escape_do_not_discard_uncommitted_text() {
    let Some(root) = crate::tests::isolated_test_root(
        "metadata_export::tests::metadata_ime_and_popup_escape_do_not_discard_uncommitted_text",
    ) else {
        return;
    };
    let context = fonts::test_context();
    context.enable_accesskit();
    let mut app = Application::new(None, |_| {}).expect("app");
    app.ui_context = Some(context.clone());
    let source = root.join("audio.wav");
    app.tabs.open_new(source.clone(), MediaKind::Audio);
    app.path = Some(source);
    app.media_kind = Some(MediaKind::Audio);
    app.open_metadata_export_options();
    click(&mut app, "Set value");
    let size = egui::vec2(640.0, 600.0);
    let tree = frame(&mut app, size, vec![])
        .platform_output
        .accesskit_update
        .expect("tree");
    let editor = node(&tree, "Metadata value (empty removes the tag)");
    frame(
        &mut app,
        size,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Focus,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: editor,
                data: None,
            },
        )],
    );
    let escape = || egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    for text in ["に", "にほん", "日本語"] {
        let output = frame(
            &mut app,
            size,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: text.into(),
                active_range_chars: Some(0..text.chars().count()),
            })],
        );
        assert!(
            !output
                .platform_output
                .ime
                .expect("IME enabled")
                .should_interrupt_composition
        );
        assert!(
            output
                .platform_output
                .accesskit_update
                .expect("tree")
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Apply metadata") && node.is_disabled())
        );
    }
    frame(&mut app, size, vec![escape()]);
    assert!(app.metadata_dialog.is_some());
    frame(
        &mut app,
        size,
        vec![
            egui::Event::Ime(egui::ImeEvent::Preedit {
                text: String::new(),
                active_range_chars: None,
            }),
            egui::Event::Ime(egui::ImeEvent::Commit("日本語".into())),
            escape(),
        ],
    );
    assert_eq!(
        app.metadata_dialog
            .as_ref()
            .expect("commit does not close modal")
            .fields[0]
            .text,
        "日本語"
    );
    click(&mut app, "Title");
    assert!(egui::Popup::is_any_open(&context));
    frame(&mut app, size, vec![escape()]);
    assert!(app.metadata_dialog.is_some());
    assert!(!egui::Popup::is_any_open(&context));
    frame(&mut app, size, vec![escape()]);
    assert!(app.metadata_dialog.is_none());
    assert!(app.metadata_export_settings.is_empty());
    app.open_metadata_export_options();
    click(&mut app, "Set value");
    frame(
        &mut app,
        size,
        vec![access(editor, Some("stale value from the closed dialog"))],
    );
    assert!(
        app.metadata_dialog.as_ref().expect("new dialog").fields[0]
            .text
            .is_empty()
    );
    click(&mut app, "Title");
    assert!(egui::Popup::is_any_open(&context));
    app.media_generation += 1;
    frame(&mut app, size, vec![]);
    assert!(app.metadata_dialog.is_none() && !egui::Popup::is_any_open(&context));
}
