use super::*;

#[path = "drag_tests.rs"]
mod drag_tests;
#[path = "view_tests.rs"]
mod view_tests;

pub(crate) fn hardware_dialog_preview<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
) {
    let timeline = app.timeline_open;
    app.timeline_open = true;
    let history = app.edits.clone();
    let view = app.image_view;
    let generation = app.generation;
    app.dispatch(CommandId::FreeRotateVideo);
    let dialog = app
        .video_rotation_dialog
        .as_mut()
        .expect("hardware angle dialog");
    dialog.angle = "31.7".into();
    let token = dialog.token;
    app.render_frame();
    assert!(app.video_raster_operations.is_some());
    assert!(app.playback_error.is_none(), "{:?}", app.playback_error);
    assert_eq!(
        app.session
            .as_ref()
            .expect("hardware session")
            .metrics()
            .cpu_transfer_count,
        0
    );
    app.handle_ui_action(UiAction::FinishVideoRotation(token, None));
    app.render_frame();
    assert!(app.video_rotation_dialog.is_none() && app.video_raster_operations.is_none());
    assert_eq!(app.edits, history);
    assert_eq!(app.image_view, view);
    assert_eq!(app.generation, generation);
    drag_tests::preview_cancel(app);
    view_tests::exercise(app, false);
    video_resize::tests::exercise(app, false);
    audio_export::tests::hardware_round_trip(app);
    metadata_export::tests::hardware_round_trip(app);
    export_progress::tests::hardware_round_trip(app);
    logo_menu::tests::hardware_round_trip(app);
    tab_menu::keyboard_tests::hardware_round_trip(app);
    tab_drag::tests::hardware_round_trip(app);
    filmstrip::drag_tests::hardware_drag_cancel(app);
    let size = app.window.as_ref().expect("owned window").inner_size();
    app.renderer
        .as_mut()
        .expect("renderer")
        .resize_surface(size.width, size.height)
        .expect("restore native surface size");
    assert_eq!(
        app.session
            .as_ref()
            .expect("hardware session")
            .metrics()
            .cpu_transfer_count,
        0
    );
    app.timeline_open = timeline;
    eprintln!(
        "PASS hardware angle dialog: real app render/GPU preview/cancel, no history or generation change, CPU transfers 0"
    );
}

pub(crate) fn frame<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    events: Vec<egui::Event>,
) -> egui::accesskit::TreeUpdate {
    frame_input(app, egui::Modifiers::NONE, events)
}

fn frame_input<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    modifiers: egui::Modifiers,
    events: Vec<egui::Event>,
) -> egui::accesskit::TreeUpdate {
    let context = app.ui_context.clone().expect("context");
    context.enable_accesskit();
    // Native render_frame and synthetic pointer passes must use the same epoch;
    // RawInput's predicted time can otherwise advance past the next native pass.
    let time = match (&mut app.ui_state, &app.window) {
        (Some(state), Some(window)) => state.take_egui_input(window).time,
        _ => None,
    };
    let mut actions = Vec::new();
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            events,
            modifiers,
            time,
            ..Default::default()
        },
        |ui| app.draw_ui(ui, &mut actions),
    );
    let tree = output
        .platform_output
        .accesskit_update
        .take()
        .expect("UIA tree");
    if let Some(rect) = app.video_rect {
        let renderer = app.renderer.as_mut().expect("renderer");
        let scale = context.pixels_per_point();
        let rect = rect * scale;
        renderer
            .resize_surface((640.0 * scale) as u32, (480.0 * scale) as u32)
            .expect("surface");
        renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
        let session = app.session.as_mut().expect("session");
        if let Some(operations) = &app.video_raster_operations {
            assert!(
                session
                    .draw_current_edited(renderer, rect, app.video_uv, operations)
                    .expect("edited video")
            );
        } else {
            assert!(
                session
                    .draw_current(renderer, rect, app.video_uv)
                    .expect("direct video")
            );
        }
        renderer.render_ui(&context, output).expect("composite UI");
        renderer.present_surface().expect("present");
    }
    for action in actions {
        app.handle_ui_action(action);
    }
    tree
}

pub(crate) fn node(tree: &egui::accesskit::TreeUpdate, label: &str) -> egui::accesskit::NodeId {
    tree.nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("missing UIA node {label}"))
}

pub(crate) fn access(target_node: egui::accesskit::NodeId, value: Option<&str>) -> egui::Event {
    egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: if value.is_some() {
            egui::accesskit::Action::SetValue
        } else {
            egui::accesskit::Action::Click
        },
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node,
        data: value.map(|value| egui::accesskit::ActionData::Value(value.into())),
    })
}

#[test]
fn video_rotation_modal_previews_commits_crops_undoes_and_exports_without_changing_source_time() {
    let Some(root) = crate::tests::isolated_test_root(
        "video_rotation::tests::video_rotation_modal_previews_commits_crops_undoes_and_exports_without_changing_source_time",
    ) else {
        return;
    };
    let source = root.join("source.mkv");
    use std::os::windows::process::CommandExt;
    let result = std::process::Command::new(
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe"),
    )
    .creation_flags(0x08000000)
    .args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=120x80:rate=5:duration=1,setsar=3/2",
        "-c:v",
        "ffv1",
    ])
    .arg(&source)
    .output()
    .expect("fixture process");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    struct Trial {
        source: PathBuf,
    }
    use winit::platform::windows::EventLoopBuilderExtWindows;
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("hidden owned window"),
            );
            let renderer = match FrameRenderer::new(&window) {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("SKIP video rotation UI: D3D11 unavailable: {error}");
                    event_loop.exit();
                    return;
                }
            };
            let bytes = std::fs::read(&self.source).expect("original bytes");
            let mut app = Application::new(None, |_| {}).expect("app");
            app.window = Some(window);
            app.renderer = Some(renderer);
            let context = fonts::test_context();
            context.enable_accesskit();
            app.ui_context = Some(context.clone());
            app.shortcuts = shortcuts::defaults();
            let tab = app.tabs.open_new(self.source.clone(), MediaKind::Video);
            app.load_path(self.source.clone(), MediaKind::Video);
            let deadline = Instant::now() + Duration::from_secs(10);
            while app
                .session
                .as_ref()
                .and_then(PlaybackSession::video_geometry)
                .is_none()
            {
                app.load_next_frame();
                app.advance_media();
                assert!(Instant::now() < deadline, "frame did not arrive");
                std::thread::sleep(Duration::from_millis(2));
            }
            if app.state == PlaybackState::Playing {
                app.toggle_pause();
            }
            app.media_duration = Some(Duration::from_secs(1));
            app.timeline_open = false;
            app.process_shortcut("Ctrl+Shift+R".parse().expect("shortcut"));
            assert!(app.video_rotation_dialog.is_none());
            app.timeline_open = true;
            app.dispatch(CommandId::RotateClockwise);
            app.image_view.selection = Some(UnitRect::FULL);
            let view = app.image_view;
            let history = app.edits[&tab].clone();
            let position = app.current_position();
            let generation = app.generation;
            frame(&mut app, vec![]);
            selection::focus_first(&context, app.selection_identity());
            frame(&mut app, vec![]);
            let focus = context.memory(egui::Memory::focused);
            app.process_shortcut("Ctrl+Shift+R".parse().expect("shortcut"));
            assert!(app.video_rotation_dialog.is_some());
            frame(&mut app, vec![]);
            let tree = frame(&mut app, vec![]);
            let input = node(&tree, "Video rotation angle in degrees");
            let tree = frame(&mut app, vec![access(input, Some(" 31.74 "))]);
            let shown = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Video rotation angle"))
                .expect("slider node")
                .1
                .numeric_value();
            assert!((shown.expect("slider numeric value") - 31.7).abs() < 1e-9);
            frame(
                &mut app,
                vec![
                    egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            ctrl: true,
                            command: true,
                            ..egui::Modifiers::NONE
                        },
                    },
                    egui::Event::Text("-31.7".into()),
                ],
            );
            assert_eq!(
                app.video_rotation_dialog
                    .as_ref()
                    .expect("dialog")
                    .value()
                    .expect("typed angle")
                    .tenths(),
                -317
            );
            let slider = node(&tree, "Video rotation angle");
            frame(
                &mut app,
                vec![egui::Event::AccessKitActionRequest(
                    egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::SetValue,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: slider,
                        data: Some(egui::accesskit::ActionData::NumericValue(45.0)),
                    },
                )],
            );
            assert_eq!(
                app.video_rotation_dialog
                    .as_ref()
                    .expect("dialog")
                    .value()
                    .expect("slider angle")
                    .tenths(),
                450
            );
            for value in ["NaN", "180.1", "-", "inf"] {
                let tree = frame(&mut app, vec![access(input, Some(value))]);
                assert!(
                    tree.nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some("Apply rotation")
                            && node.is_disabled())
                );
                assert_eq!(app.edits[&tab], history);
            }
            frame(&mut app, vec![access(input, Some("31.74"))]);
            let tree = frame(&mut app, vec![]);
            assert!(app.video_raster_operations.is_some());
            assert_eq!(app.image_view, view);
            assert_eq!(app.edits[&tab], history);
            assert_eq!(
                (app.current_position(), app.generation),
                (position, generation)
            );
            app.dispatch(CommandId::RotateClockwise);
            assert_eq!(
                app.edits[&tab], history,
                "modal consumes unrelated commands"
            );
            frame(&mut app, vec![access(node(&tree, "Cancel"), None)]);
            frame(&mut app, vec![]);
            frame(&mut app, vec![]);
            assert!(app.video_rotation_dialog.is_none() && app.video_raster_operations.is_none());
            assert_eq!(context.memory(egui::Memory::focused), focus);
            assert_eq!(app.image_view, view);
            for cancel in [false, true] {
                app.dispatch(CommandId::FreeRotateVideo);
                let token = app.video_rotation_dialog.as_ref().expect("dialog").token;
                let zero = app
                    .video_rotation_dialog
                    .as_ref()
                    .expect("dialog")
                    .value()
                    .expect("zero");
                app.handle_ui_action(UiAction::FinishVideoRotation(
                    token,
                    (!cancel).then_some(zero),
                ));
                assert_eq!(app.image_view, view);
                assert_eq!(app.edits[&tab], history);
            }
            app.dispatch(CommandId::FreeRotateVideo);
            frame(&mut app, vec![]);
            let tree = frame(&mut app, vec![]);
            let input = node(&tree, "Video rotation angle in degrees");
            let tree = frame(&mut app, vec![access(input, Some("31.7"))]);
            let first = app
                .video_rotation_dialog
                .as_ref()
                .expect("dialog")
                .value()
                .expect("first");
            frame(&mut app, vec![access(node(&tree, "Apply rotation"), None)]);
            assert!(app.video_rotation_dialog.is_none());
            assert_eq!(
                app.edits[&tab].operations().last(),
                Some(&EditOperation::RotateVideo(first))
            );
            frame(&mut app, vec![]);
            assert_eq!(app.video_uv, VideoOrientation::default().source_uv());
            app.dispatch(CommandId::SelectAspectSquare);
            let selected = PixelCrop::from_selection(
                app.image_view.selection.expect("square selection"),
                first.size(),
                MediaKind::Video,
            )
            .expect("crop");
            assert_eq!(selected.width, selected.height);
            app.dispatch(CommandId::ApplyCrop);
            assert_eq!(
                app.edits[&tab].operations().last(),
                Some(&EditOperation::Crop(selected))
            );
            app.dispatch(CommandId::FreeRotateVideo);
            let dialog = app.video_rotation_dialog.as_mut().expect("second dialog");
            dialog.angle = "-12.7".into();
            let second = dialog.value().expect("second");
            let token = dialog.token;
            app.handle_ui_action(UiAction::FinishVideoRotation(token, Some(second)));
            let final_history = app.edits[&tab].clone();
            let (transform, _) = app.video_presentation((120, 80));
            assert_eq!(
                transform.size,
                (second.size().0 as f32, second.size().1 as f32)
            );
            assert_eq!(transform.pixel_aspect(1.5), 1.0);
            app.dispatch(CommandId::Undo);
            assert_eq!(
                app.visual_transform((120, 80)).size,
                (selected.width as f32, selected.height as f32)
            );
            let renderer = app.renderer.take();
            let undone = app.edits[&tab].clone();
            let undone_view = app.image_view;
            app.dispatch(CommandId::Redo);
            assert_eq!(
                app.edits[&tab], undone,
                "preflight failure preserves the redo branch"
            );
            assert_eq!(app.image_view, undone_view);
            app.renderer = renderer;
            app.dispatch(CommandId::Redo);
            assert_eq!(app.edits[&tab], final_history);
            app.timeline_open = false;
            frame(&mut app, vec![]);
            assert!(
                app.video_raster_operations.is_some(),
                "closing timeline retains raster"
            );
            app.timeline_open = true;
            assert_eq!(
                (app.current_position(), app.generation),
                (position, generation)
            );
            app.dispatch(CommandId::FreeRotateVideo);
            let stale = app.video_rotation_dialog.as_ref().expect("dialog").token;
            app.handle_ui_action(UiAction::FinishVideoRotation(stale, None));
            app.dispatch(CommandId::FreeRotateVideo);
            let current = app
                .video_rotation_dialog
                .as_ref()
                .expect("new dialog")
                .token;
            app.handle_ui_action(UiAction::FinishVideoRotation(stale, Some(second)));
            assert_eq!(
                app.video_rotation_dialog
                    .as_ref()
                    .expect("old action keeps new modal")
                    .token,
                current
            );
            app.request_guarded(GuardedAction::Exit);
            assert!(app.video_rotation_dialog.is_some() && app.pending_guard.is_none());
            app.handle_ui_action(UiAction::FinishVideoRotation(current, None));
            for changed in 0..6 {
                app.dispatch(CommandId::FreeRotateVideo);
                let dialog = app
                    .video_rotation_dialog
                    .as_mut()
                    .expect("context trial dialog");
                dialog.angle = "25".into();
                let value = dialog.value().expect("prospective rotation");
                let token = dialog.token;
                let original_media_generation = app.media_generation;
                match changed {
                    0 => app.media_generation = app.media_generation.wrapping_add(1),
                    1 => app.generation = app.generation.next(),
                    2 => app.timeline_open = false,
                    3 => app.fullscreen = true,
                    4 => app.path = Some(PathBuf::from("different-video.mkv")),
                    5 => {
                        app.edits
                            .get_mut(&tab)
                            .expect("history")
                            .push(EditOperation::FlipVertical, MediaKind::Video);
                    }
                    _ => unreachable!(),
                }
                let before = app.edits[&tab].clone();
                app.handle_ui_action(UiAction::FinishVideoRotation(token, Some(value)));
                assert!(app.video_rotation_dialog.is_none());
                assert_eq!(
                    app.edits[&tab], before,
                    "stale context {changed} adds no rotation"
                );
                app.media_generation = original_media_generation;
                app.generation = generation;
                app.timeline_open = true;
                app.fullscreen = false;
                app.path = Some(self.source.clone());
                app.edits.insert(tab, final_history.clone());
            }
            let before = app.image_view;
            app.push_visual_edit(EditOperation::RotateVideo(
                VideoRotation::new(317, (16, 16), 1.0).expect("wrong input"),
            ));
            assert_eq!(app.image_view, before);
            assert_eq!(app.edits[&tab], final_history);
            app.dispatch(CommandId::FreeRotateVideo);
            app.generation = app.generation.next();
            app.cancel_stale_video_rotation();
            assert!(app.video_rotation_dialog.is_none());
            app.generation = generation;
            let target = self.source.with_file_name("rotated.mp4");
            towavue_runtime_windows::export_media(&towavue_runtime_windows::ExportRequest {
                source: self.source.clone(),
                target: target.clone(),
                kind: MediaKind::Video,
                operations: final_history.operations().to_vec(),
                hardware_encode: false,
            })
            .expect("export composed UI edits");
            let mut count = 0;
            towavue_runtime_windows::decode_file(&target, |output| {
                if let towavue_runtime_windows::DecodeOutput::Video(frame) = output {
                    assert_eq!((frame.width, frame.height), second.size());
                    assert_eq!(frame.pixel_aspect, 1.0);
                    assert_eq!(frame.orientation, VideoOrientation::default());
                    count += 1;
                }
                true
            })
            .expect("reopen export");
            assert_eq!(count, 5);
            drag_tests::exercise(&mut app);
            view_tests::exercise(&mut app, true);
            video_resize::tests::exercise(&mut app, true);
            assert_eq!(
                std::fs::read(&self.source).expect("unchanged source"),
                bytes
            );
            eprintln!(
                "PASS video rotation UI: software GPU preview/composition, cancel/zero/focus, crop/re-rotation/Undo/Redo, unchanged source/time and exported reopen"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("loop")
        .run_app(&mut Trial { source })
        .expect("trial");
}

#[test]
fn raster_transform_uses_final_dimensions_identity_uv_and_square_pixels() {
    let first = VideoRotation::new(317, (80, 120), 0.5).expect("first");
    let second = VideoRotation::new(-127, (48, 32), 1.0).expect("second");
    let operations = [
        EditOperation::RotateClockwise,
        EditOperation::RotateVideo(first),
        EditOperation::Crop(PixelCrop {
            x: 2,
            y: 2,
            width: 48,
            height: 32,
        }),
        EditOperation::FlipHorizontal,
        EditOperation::RotateVideo(second),
        EditOperation::RotateCounterclockwise,
    ];
    let transform = ImageTransform::new((120, 80), &operations);
    assert_eq!(
        transform.size,
        (second.size().1 as f32, second.size().0 as f32)
    );
    assert_eq!(transform.uv, VideoOrientation::default().source_uv());
    assert_eq!(transform.pixel_aspect(2.0), 1.0);
    let zero = ImageTransform::new(
        (7, 5),
        &[EditOperation::RotateVideo(
            VideoRotation::new(0, (7, 5), 2.0).expect("zero"),
        )],
    );
    assert_eq!(zero.size, (7.0, 5.0));
    assert_eq!(zero.pixel_aspect(2.0), 2.0);
}

#[test]
fn video_angle_validation_and_compact_modal_keep_budget_failures_uncommittable() {
    let path = PathBuf::from("video.mkv");
    let mut tabs = TabSet::default();
    let mut dialog = VideoRotationDialog {
        token: 1,
        snapshot: video_edit::VideoEditSnapshot {
            tab: tabs.open_new(path.clone(), MediaKind::Video),
            path,
            media_generation: 0,
            generation: PlaybackGeneration::default(),
            source: (8, 6, 1.0),
            orientation: VideoOrientation::default(),
            max_side: 16384,
            operations: vec![],
            geometry: (8, 6, 1.0),
        },
        angle: "31.74".into(),
        first_frame: true,
    };
    assert_eq!(dialog.value().expect("rounded").tenths(), 317);
    dialog.snapshot.max_side = 8;
    assert!(dialog.value().expect_err("budget").contains("budget"));
    dialog.angle = "0".into();
    assert_eq!(
        dialog
            .value()
            .expect("zero is not normalized or allocated")
            .tenths(),
        0
    );
    dialog.angle = "31.7".into();
    let context = fonts::test_context();
    context.enable_accesskit();
    let run = |dialog: &mut VideoRotationDialog, events| {
        let mut result = None;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 300.0),
                )),
                events,
                ..Default::default()
            },
            |ui| result = dialog.show(ui.ctx()),
        );
        (
            result,
            output.platform_output.accesskit_update.expect("tree"),
        )
    };
    run(&mut dialog, vec![]);
    let (_, tree) = run(&mut dialog, vec![]);
    assert!(
        tree.nodes
            .iter()
            .any(|(_, node)| node.label() == Some("Apply rotation") && node.is_disabled())
    );
    let cancel = node(&tree, "Cancel");
    assert_eq!(run(&mut dialog, vec![access(cancel, None)]).0, Some(None));
    let (result, _) = run(
        &mut dialog,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(result, Some(None));
}
