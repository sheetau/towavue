use super::*;
use crate::video_rotation::tests::{access, frame, node};

pub(crate) fn exercise<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    software: bool,
) {
    let timeline = app.timeline_open;
    let view = app.image_view;
    let history = app.edits.clone();
    let generation = app.generation;
    let position = app.current_position();
    let state = app.state;
    let tab = app.tabs.active().expect("tab").id;
    app.timeline_open = false;
    app.process_shortcut("Ctrl+R".parse().expect("shortcut"));
    assert!(app.video_resize_dialog.is_none());
    app.timeline_open = true;
    frame(app, vec![]);
    let context = app.ui_context.clone().expect("context");
    selection::focus_first(&context, app.selection_identity());
    frame(app, vec![]);
    let focus = context.memory(egui::Memory::focused);
    app.process_shortcut("Ctrl+R".parse().expect("shortcut"));
    assert!(app.video_resize_dialog.is_some());
    frame(app, vec![]);
    let tree = frame(app, vec![]);
    let width = node(&tree, "Width in pixels");
    for invalid in ["0", "17", "-2", "16386", "abc"] {
        let tree = frame(app, vec![access(width, Some(invalid))]);
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Apply resize") && node.is_disabled()),
            "{invalid}"
        );
        assert_eq!(app.edits, history);
    }
    frame(app, vec![access(width, Some("96"))]);
    let tree = frame(app, vec![]);
    frame(app, vec![access(node(&tree, "Keep aspect ratio"), None)]);
    frame(
        app,
        vec![access(node(&tree, "Height in pixels"), Some("64"))],
    );
    frame(app, vec![]);
    let dialog = app.video_resize_dialog.as_ref().expect("preview");
    let first = dialog.value().expect("valid preview");
    assert_eq!(first.size(), (96, 64));
    assert_eq!(
        app.video_raster_operations
            .as_ref()
            .expect("preview plan")
            .last(),
        Some(&EditOperation::ResizeVideo(first))
    );
    assert_eq!(app.image_view, view);
    assert_eq!(app.edits, history);
    app.dispatch(CommandId::RotateClockwise);
    assert_eq!(app.edits, history, "modal consumes unrelated edit");
    let token = app.video_resize_dialog.as_ref().expect("dialog").token;
    app.handle_ui_action(UiAction::FinishVideoResize(
        token.wrapping_sub(1),
        Some(first),
    ));
    assert!(
        app.video_resize_dialog.is_some(),
        "old action cannot close current dialog"
    );
    let tree = frame(app, vec![]);
    frame(app, vec![access(node(&tree, "Cancel"), None)]);
    frame(app, vec![]);
    frame(app, vec![]);
    assert!(app.video_resize_dialog.is_none());
    assert_eq!(app.edits, history);
    assert_eq!(app.image_view, view);
    assert_eq!(context.memory(egui::Memory::focused), focus);
    assert_eq!((app.generation, app.state), (generation, state));
    if software {
        assert_eq!(app.current_position(), position);
    }

    for overlay in [CommandId::ToggleCommandPalette, CommandId::ToggleGridMenu] {
        app.dispatch(overlay);
        app.dispatch(CommandId::ResizeVideo);
        assert!(!app.palette_open && !app.grid_open);
        let token = app
            .video_resize_dialog
            .as_ref()
            .expect("overlay resize")
            .token;
        app.handle_ui_action(UiAction::FinishVideoResize(token, None));
        frame(app, vec![]);
    }
    if !software {
        assert_eq!(
            app.session
                .as_ref()
                .expect("hardware")
                .metrics()
                .cpu_transfer_count,
            0
        );
        app.timeline_open = timeline;
        app.image_view = view;
        eprintln!(
            "PASS hardware resize UI: real preview, invalid dimensions, cancellation, overlay focus and CPU transfers 0"
        );
        return;
    }

    app.dispatch(CommandId::ResizeVideo);
    frame(app, vec![]);
    let tree = frame(app, vec![]);
    frame(app, vec![access(node(&tree, "Keep aspect ratio"), None)]);
    frame(
        app,
        vec![access(node(&tree, "Width in pixels"), Some("96"))],
    );
    let tree = frame(
        app,
        vec![access(node(&tree, "Height in pixels"), Some("64"))],
    );
    frame(app, vec![access(node(&tree, "Apply resize"), None)]);
    frame(app, vec![]);
    assert!(app.video_resize_dialog.is_none());
    assert_eq!(
        app.edits[&tab].operations().last(),
        Some(&EditOperation::ResizeVideo(first))
    );
    assert_eq!(
        app.validate_video_operations(app.video_operations())
            .expect("final"),
        (96, 64, 1.0)
    );
    let applied = app.edits[&tab].operations().to_vec();
    app.dispatch(CommandId::Undo);
    assert_eq!(app.edits[&tab].operations(), history[&tab].operations());
    app.dispatch(CommandId::Redo);
    assert_eq!(app.edits[&tab].operations(), applied);
    let view = app.image_view;
    app.dispatch(CommandId::ResizeVideo);
    let dialog = app.video_resize_dialog.as_ref().expect("identity");
    let token = dialog.token;
    let identity = dialog.value().expect("same dimensions");
    assert!(identity.is_identity());
    app.handle_ui_action(UiAction::FinishVideoResize(token, Some(identity)));
    assert_eq!(app.edits[&tab].operations(), applied);
    assert_eq!(app.image_view, view);

    for change in 0..4 {
        app.dispatch(CommandId::ResizeVideo);
        let dialog = app.video_resize_dialog.as_mut().expect("stale trial");
        match change {
            0 => {
                dialog.snapshot.media_generation = dialog.snapshot.media_generation.wrapping_add(1)
            }
            1 => dialog.snapshot.source.0 += 2,
            2 => dialog
                .snapshot
                .operations
                .push(EditOperation::FlipHorizontal),
            _ => dialog.snapshot.max_side = 1,
        }
        frame(app, vec![]);
        assert!(app.video_resize_dialog.is_none());
        assert_eq!(app.edits[&tab].operations(), applied);
    }
    let source = app.path.clone().expect("source");
    let target = source.with_file_name("ui-resize.mkv");
    towavue_runtime_windows::export_media(&towavue_runtime_windows::ExportRequest {
        source,
        target: target.clone(),
        kind: MediaKind::Video,
        operations: applied,
        hardware_encode: false,
    })
    .expect("save UI resize");
    let mut frames = 0;
    towavue_runtime_windows::decode_file(&target, |output| {
        if let towavue_runtime_windows::DecodeOutput::Video(frame) = output {
            assert_eq!(
                (frame.width, frame.height, frame.pixel_aspect),
                (96, 64, 1.0)
            );
            frames += 1;
        }
        true
    })
    .expect("reopen UI resize");
    assert_eq!(frames, 5);
    assert_eq!(
        (app.current_position(), app.generation, app.state),
        (position, generation, state)
    );
    app.timeline_open = timeline;
    eprintln!(
        "PASS video resize UI: preview/cancel/identity, focus, invalid/stale inputs, Apply/Undo/Redo, export reopen and unchanged transport"
    );
}

#[test]
fn video_resize_default_dimensions_preserve_display_aspect_and_validate_even_edges() {
    let mut tabs = TabSet::default();
    for (size, aspect, expected) in [
        ((64, 48), 1.0, (64, 48)),
        ((64, 48), 2.0, (128, 48)),
        ((64, 48), 0.5, (64, 96)),
        ((65, 49), 1.0, (66, 50)),
    ] {
        let dialog = VideoResizeDialog {
            token: 1,
            snapshot: video_edit::VideoEditSnapshot {
                tab: tabs.open_new(PathBuf::from("video.mkv"), MediaKind::Video),
                path: PathBuf::from("video.mkv"),
                media_generation: 0,
                generation: PlaybackGeneration::default(),
                source: (size.0, size.1, aspect),
                orientation: towavue_runtime_windows::VideoOrientation::default(),
                max_side: 16384,
                operations: vec![],
                geometry: (size.0, size.1, aspect),
            },
            inputs: resize::ResizeDialog::for_video(size, aspect),
        };
        assert_eq!(dialog.value().expect("default").size(), expected);
    }
}

#[test]
fn video_resize_modal_filters_ratio_budget_and_compact_escape() {
    let mut tabs = TabSet::default();
    let mut dialog = VideoResizeDialog {
        token: 1,
        snapshot: video_edit::VideoEditSnapshot {
            tab: tabs.open_new(PathBuf::from("video.mkv"), MediaKind::Video),
            path: PathBuf::from("video.mkv"),
            media_generation: 0,
            generation: PlaybackGeneration::default(),
            source: (64, 48, 1.5),
            orientation: towavue_runtime_windows::VideoOrientation::default(),
            max_side: 16384,
            operations: vec![],
            geometry: (64, 48, 1.5),
        },
        inputs: resize::ResizeDialog::for_video((64, 48), 1.5),
    };
    let context = fonts::test_context();
    context.enable_accesskit();
    let draw = |dialog: &mut VideoResizeDialog, compact, events| {
        let mut action = None;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    if compact {
                        egui::vec2(320.0, 300.0)
                    } else {
                        egui::vec2(640.0, 480.0)
                    },
                )),
                events,
                ..Default::default()
            },
            |ui| action = dialog.show(ui.ctx()),
        );
        (
            action,
            output.platform_output.accesskit_update.expect("tree"),
        )
    };
    draw(&mut dialog, false, vec![]);
    let (_, tree) = draw(&mut dialog, false, vec![]);
    draw(
        &mut dialog,
        false,
        vec![access(node(&tree, "Width in pixels"), Some("100"))],
    );
    assert_eq!(dialog.value().expect("display ratio").size(), (100, 50));
    draw(
        &mut dialog,
        false,
        vec![access(node(&tree, "Height in pixels"), Some("34"))],
    );
    assert_eq!(dialog.value().expect("height ratio").size(), (68, 34));
    for (label, filter) in [
        ("Nearest", towavue_core::ResampleFilter::Nearest),
        ("Bilinear", towavue_core::ResampleFilter::Bilinear),
        ("Bicubic", towavue_core::ResampleFilter::Bicubic),
        ("Lanczos", towavue_core::ResampleFilter::Lanczos),
    ] {
        let (_, tree) = draw(&mut dialog, false, vec![]);
        let combo = tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == egui::accesskit::Role::ComboBox)
            .expect("filter selector")
            .0;
        let (_, tree) = draw(&mut dialog, false, vec![access(combo, None)]);
        draw(&mut dialog, false, vec![access(node(&tree, label), None)]);
        assert_eq!(dialog.value().expect("filter selection").filter(), filter);
        draw(&mut dialog, false, vec![]);
        draw(&mut dialog, false, vec![]);
    }
    dialog.snapshot.max_side = 32;
    let (_, tree) = draw(&mut dialog, false, vec![]);
    assert!(
        tree.nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Apply resize") && node.is_disabled())
    );
    dialog.snapshot.max_side = 16384;
    let (_, tree) = draw(&mut dialog, true, vec![]);
    assert!(
        tree.nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Width in pixels"))
    );
    let (action, _) = draw(
        &mut dialog,
        true,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(action, Some(None));
}
