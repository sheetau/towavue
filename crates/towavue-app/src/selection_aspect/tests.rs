use super::*;
use towavue_runtime_windows::{decode_image, export_media};

fn fixture(path: &Path, video: bool) {
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let mut command = std::process::Command::new(ffmpeg);
    command.args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=120x80:rate=5:duration=1",
    ]);
    if video {
        command.args(["-vf", "setsar=3/2", "-c:v", "ffv1"]);
    } else {
        command.args(["-frames:v", "1"]);
    }
    assert!(command.arg(path).status().expect("owned fixture").success());
}

const PRESETS: &[(CommandId, (u32, u32))] = &[
    (CommandId::SelectAspectSquare, (1, 1)),
    (CommandId::SelectAspectFourThree, (4, 3)),
    (CommandId::SelectAspectThreeFour, (3, 4)),
    (CommandId::SelectAspectThreeTwo, (3, 2)),
    (CommandId::SelectAspectTwoThree, (2, 3)),
    (CommandId::SelectAspectSixteenNine, (16, 9)),
    (CommandId::SelectAspectNineSixteen, (9, 16)),
];

#[test]
fn video_aspect_presets_follow_pixel_aspect_rotation_and_visual_context_without_editing_time() {
    let Some(root) = crate::tests::isolated_test_root(
        "selection_aspect::tests::video_aspect_presets_follow_pixel_aspect_rotation_and_visual_context_without_editing_time",
    ) else {
        return;
    };
    let source = root.join("source.mkv");
    fixture(&source, true);
    struct Trial {
        source: PathBuf,
    }
    use winit::application::ApplicationHandler;
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::window::{Window, WindowId};
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("hidden window"),
            );
            let renderer = match FrameRenderer::new(&window) {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("SKIP aspect video: D3D11 unavailable: {error}");
                    event_loop.exit();
                    return;
                }
            };
            let mut app = Application::new(None, |_| {}).expect("app");
            app.window = Some(window);
            app.renderer = Some(renderer);
            app.ui_context = Some(fonts::test_context());
            app.shortcuts = shortcuts::defaults();
            let tab = app.tabs.open_new(self.source.clone(), MediaKind::Video);
            app.load_path(self.source.clone(), MediaKind::Video);
            app.media_duration = Some(Duration::from_secs(1));
            let deadline = Instant::now() + Duration::from_secs(10);
            while app
                .session
                .as_ref()
                .and_then(PlaybackSession::video_geometry)
                .is_none()
            {
                app.load_next_frame();
                app.advance_media();
                assert!(
                    Instant::now() < deadline,
                    "video geometry: {:?}",
                    app.status_message
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            if app.state == PlaybackState::Playing {
                app.toggle_pause();
            }
            let geometry = app
                .session
                .as_ref()
                .expect("session")
                .video_geometry()
                .expect("geometry");
            assert_eq!(geometry, (120, 80, 1.5));
            app.timeline_open = false;
            app.dispatch(CommandId::SelectAspectSquare);
            assert!(
                app.image_view.selection.is_none(),
                "viewing has no selection preset"
            );
            app.timeline_open = true;
            app.time_selection = towavue_core::TimeRange::new(
                MediaTime::ZERO,
                MediaTime::from_nanoseconds(500_000_000),
            );
            let time_history = app.edits.clone();
            let position = app.current_position();
            app.process_shortcut("Ctrl+K".parse().expect("prefix"));
            app.process_shortcut("1".parse().expect("square"));
            assert!(app.time_selection.is_none());
            assert_eq!(app.edits, time_history);
            assert_eq!(app.current_position(), position);
            assert_eq!(app.state, PlaybackState::Paused);
            assert_eq!(
                PixelCrop::from_selection(
                    app.image_view.selection.expect("selection"),
                    (120, 80),
                    MediaKind::Video
                ),
                Some(PixelCrop {
                    x: 34,
                    y: 0,
                    width: 52,
                    height: 80
                })
            );
            app.dispatch(CommandId::RotateClockwise);
            let history = app.edits[&tab].clone();
            for (command, aspect) in PRESETS {
                app.dispatch(*command);
                let expected =
                    PixelCrop::centered_aspect((80, 120), MediaKind::Video, *aspect, 1.0 / 1.5)
                        .expect("aspect");
                assert_eq!(
                    app.image_view.selection,
                    Some(expected.unit_rect((80, 120)))
                );
                assert_eq!(app.edits[&tab], history);
            }
            app.dispatch(CommandId::SelectAspectSquare);
            let selected = PixelCrop::from_selection(
                app.image_view.selection.expect("selection"),
                (80, 120),
                MediaKind::Video,
            )
            .expect("pixels");
            assert_eq!(
                selected,
                PixelCrop {
                    x: 0,
                    y: 34,
                    width: 80,
                    height: 52
                }
            );
            app.process_shortcut("Ctrl+Y".parse().expect("crop"));
            assert_eq!(
                app.edits[&tab].operations().last(),
                Some(&EditOperation::Crop(selected))
            );
            app.dispatch(CommandId::Undo);
            assert_eq!(app.edits[&tab].operations(), history.operations());
            app.dispatch(CommandId::Redo);
            assert_eq!(app.visual_transform((120, 80)).size, (80.0, 52.0));
            eprintln!(
                "PASS aspect video: actual SAR, rotated presets, context, time-selection isolation, crop and Undo/Redo"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: winit::event::WindowEvent) {
        }
    }
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial { source })
        .expect("video trial");
}

#[test]
fn image_aspect_presets_use_edited_geometry_preserve_history_and_export_selected_pixels() {
    let Some(root) = crate::tests::isolated_test_root(
        "selection_aspect::tests::image_aspect_presets_use_edited_geometry_preserve_history_and_export_selected_pixels",
    ) else {
        return;
    };
    let source = root.join("source.png");
    fixture(&source, false);
    let source_bytes = std::fs::read(&source).expect("source");
    let decoded = Arc::new(decode_image(&source).expect("decode"));
    let mut app = Application::new(None, |_| {}).expect("app");
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    app.shortcuts = shortcuts::defaults();
    let tab = app.tabs.open_new(source.clone(), MediaKind::Image);
    app.path = Some(source.clone());
    app.media_kind = Some(MediaKind::Image);
    app.image = Some(
        ImagePresentation::from_decoded(&context, &source, Arc::clone(&decoded)).expect("image"),
    );
    app.image_view.zoom = ZoomMode::Custom(2.0);
    app.image_view.pan = (12.0, -9.0);
    for rotation in [false, true] {
        if rotation {
            app.dispatch(CommandId::RotateClockwise);
        }
        app.image_view.zoom = ZoomMode::Custom(2.0);
        app.image_view.pan = (12.0, -9.0);
        let size = if rotation { (80, 120) } else { (120, 80) };
        let history = app.edits.clone();
        for (index, (id, ratio)) in PRESETS.iter().enumerate() {
            app.process_shortcut("Ctrl+K".parse().expect("prefix"));
            app.process_shortcut((index + 1).to_string().parse().expect("preset"));
            let expected =
                PixelCrop::centered_aspect(size, MediaKind::Image, *ratio, 1.0).expect("bounds");
            assert_eq!(
                app.image_view.selection,
                Some(expected.unit_rect(size)),
                "{id:?}"
            );
            assert_eq!(app.edits, history);
            assert_eq!(app.image_view.zoom, ZoomMode::Custom(2.0));
            assert_eq!(app.image_view.pan, (12.0, -9.0));
            let _ = context.run_ui(egui::RawInput::default(), |ui| {
                app.selection_controls(
                    ui,
                    egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size.0 as f32, size.1 as f32),
                    ),
                    size,
                )
            });
            assert!(selection::has_focus(&context));
        }
    }
    app.dispatch(CommandId::SelectAspectSquare);
    let before = app.image_view.selection;
    let history = app.edits.clone();
    let image = app.image.take();
    app.dispatch(CommandId::SelectAspectSixteenNine);
    assert_eq!(
        app.image_view.selection, before,
        "loading does not replace selection"
    );
    app.image = image;
    for blocked in 0..4 {
        app.reading_mode = blocked == 0;
        app.image_edit_pending = blocked == 1;
        app.pending_guard = (blocked == 2).then_some(GuardedAction::Exit);
        app.image_error = (blocked == 3).then(|| "fixture error".into());
        app.dispatch(CommandId::SelectAspectSixteenNine);
        assert_eq!(app.image_view.selection, before);
        assert_eq!(app.edits, history);
        app.reading_mode = false;
        app.image_edit_pending = false;
        app.pending_guard = None;
        app.image_error = None;
    }
    let copied = app.image_copy_request().expect("copy snapshot");
    assert_eq!(copied.size, (80, 80));
    app.dispatch(CommandId::ApplyCrop);
    assert_eq!(
        app.edits[&tab].operations().last(),
        Some(&EditOperation::Crop(PixelCrop {
            x: 0,
            y: 20,
            width: 80,
            height: 80
        }))
    );
    let target = root.join("crop.png");
    export_media(&ExportRequest {
        source: source.clone(),
        target: target.clone(),
        kind: MediaKind::Image,
        operations: app.edits[&tab].operations().to_vec(),
        hardware_encode: false,
    })
    .expect("export");
    let exported = decode_image(&target).expect("reopen").frames.remove(0);
    assert_eq!((exported.width, exported.height), (80, 80));
    let transform = app.visual_transform((120, 80));
    for y in 0..80 {
        for x in 0..80 {
            let uv = bilinear_uv(
                transform.uv,
                (x as f32 + 0.5) / 80.0,
                (y as f32 + 0.5) / 80.0,
            );
            let offset = ((uv.y * 80.0) as usize * 120 + (uv.x * 120.0) as usize) * 4;
            let output = (y * 80 + x) * 4;
            assert_eq!(
                &exported.rgba[output..output + 4],
                &decoded.frames[0].rgba[offset..offset + 4]
            );
        }
    }
    app.dispatch(CommandId::Undo);
    assert_eq!(
        app.edits[&tab].operations(),
        &[EditOperation::RotateClockwise]
    );
    app.dispatch(CommandId::Redo);
    assert_eq!(app.visual_transform((120, 80)).size, (80.0, 80.0));
    assert_eq!(
        std::fs::read(source).expect("source preserved"),
        source_bytes
    );
    // A materialized image already includes the preceding rotation/crop/resize history.
    app.image = Some(
        ImagePresentation::from_decoded(
            &context,
            &target,
            Arc::new(DecodedImage {
                format: "png",
                frames: vec![exported],
            }),
        )
        .expect("materialized image"),
    );
    app.image_materialized = true;
    let history = app.edits.clone();
    app.dispatch(CommandId::SelectAspectFourThree);
    assert_eq!(
        app.image_view.selection,
        Some(
            PixelCrop {
                x: 0,
                y: 10,
                width: 80,
                height: 60
            }
            .unit_rect((80, 80))
        )
    );
    assert_eq!(app.edits, history);
}
