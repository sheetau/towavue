use crate::*;
use towavue_core::{ImageResize, ResampleFilter};
use winit::platform::windows::EventLoopBuilderExtWindows;

fn surface<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    context: &egui::Context,
    renderer: &mut FrameRenderer,
    density: f32,
    restore: bool,
) -> Vec<u8> {
    let mut input = egui::RawInput {
        max_texture_side: Some(renderer.max_texture_side()),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(640.0, 480.0),
        )),
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .expect("viewport")
        .native_pixels_per_point = Some(density);
    let mut output = context.run_ui(input, |ui| app.draw_image(ui));
    assert_eq!(context.pixels_per_point(), density);
    if restore {
        output
            .textures_delta
            .set
            .extend(app.restored_ui_textures(context));
    }
    renderer
        .resize_surface((640.0 * density) as u32, (480.0 * density) as u32)
        .expect("surface");
    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
    renderer
        .render_ui(context, output)
        .expect("image rendering");
    let pixels = renderer.verification_surface_rgba().expect("GPU readback");
    renderer.present_surface().expect("Present");
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] != pixel[1]),
        "colored image must reach GPU"
    );
    pixels
}

#[test]
#[ignore = "requires hardware D3D11 and FFmpeg; generated small animated resize fixtures"]
fn resized_animations_recover_gpu_pixels_and_keep_undo_redo_at_three_densities() {
    let Some(root) = crate::tests::isolated_test_root(
        "resize::gpu::resized_animations_recover_gpu_pixels_and_keep_undo_redo_at_three_densities",
    ) else {
        return;
    };
    struct Trial {
        root: PathBuf,
        completed: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("owned hidden window");
            let mut renderer = FrameRenderer::new(&window).expect("hardware D3D11");
            for density in [1.0, 1.25, 2.0] {
                for filter in [
                    ResampleFilter::Nearest,
                    ResampleFilter::Bilinear,
                    ResampleFilter::Bicubic,
                    ResampleFilter::Lanczos,
                ] {
                    for nearest in [false, true] {
                        let (sender, events) = std::sync::mpsc::channel();
                        let mut app = Application::new(None, move |event| {
                            let _ = sender.send(event);
                        })
                        .expect("app");
                        let context = fonts::test_context();
                        let path = self.root.join("resize.png");
                        let tab = app.tabs.open_new(path.clone(), MediaKind::Image);
                        app.path = Some(path.clone());
                        app.displayed_tab = Some(tab);
                        app.media_kind = Some(MediaKind::Image);
                        app.state = PlaybackState::Paused;
                        app.ui_context = Some(context.clone());
                        app.nearest_images = nearest;
                        // Edit/Undo/Redo intentionally return to Fit; compare that same view.
                        app.image_view.fit();
                        let source = Arc::new(DecodedImage {
                            animation_plays: 3,
                            format: "test",
                            frames: [0, 1]
                                .map(|frame| {
                                    let rgba = (0..96_usize * 64)
                                        .flat_map(|pixel| {
                                            if (pixel % 96 / 4 + pixel / 96 / 4).is_multiple_of(2) {
                                                [220, 60 + frame * 100, 20, 255]
                                            } else {
                                                [20, 40, 160 + frame * 80, 96]
                                            }
                                        })
                                        .collect();
                                    towavue_runtime_windows::DecodedImageFrame {
                                        width: 96,
                                        height: 64,
                                        rgba,
                                        delay: Duration::from_secs(60),
                                    }
                                })
                                .into(),
                        });
                        let mut image = ImagePresentation::from_decoded_frame(
                            &context,
                            &path,
                            source.clone(),
                            1,
                            app.image_sampling(),
                        )
                        .expect("second frame");
                        image.plays_left = 2;
                        let deadline = image.next_frame_at;
                        app.image = Some(image);
                        let original = surface(&mut app, &context, &mut renderer, density, false);
                        app.dispatch(CommandId::ResizeImage);
                        assert!(app.resize_dialog.is_some());
                        app.handle_ui_action(UiAction::FinishResize(Some(
                            ImageResize::new(53, 77, filter).expect("resize"),
                        )));
                        let wait = |app: &mut Application<_>| {
                            let limit = Instant::now() + Duration::from_secs(10);
                            while app.image_edit_pending {
                                app.handle_app_event(
                                    events
                                        .recv_timeout(
                                            limit.saturating_duration_since(Instant::now()),
                                        )
                                        .expect("resampling completion"),
                                );
                            }
                            assert!(app.image_error.is_none());
                        };
                        wait(&mut app);
                        assert!(app.image_materialized);
                        let edited = surface(&mut app, &context, &mut renderer, density, false);
                        assert!(edited != original, "resize must alter displayed geometry");
                        let presentation = app.image.as_ref().expect("resized").clone();
                        assert_eq!(presentation.dimensions(), (53, 77));
                        assert_eq!(presentation.frame_index, 1);
                        assert_eq!(presentation.plays_left, 2);
                        assert_eq!(presentation.next_frame_at, deadline);
                        let history = app.edits[&tab].clone();
                        let view = app.image_view;
                        let device = renderer.graphics_device();
                        renderer.release_surface();
                        renderer = FrameRenderer::with_graphics_device(&window, device)
                            .expect("same-device renderer recreation");
                        assert!(
                            surface(&mut app, &context, &mut renderer, density, true) == edited,
                            "restored full surface differs: {filter:?}, {nearest}, {density}x"
                        );
                        let restored = app.image.as_ref().expect("restored");
                        assert!(Arc::ptr_eq(&restored.decoded, &presentation.decoded));
                        assert_eq!(restored.texture.id(), presentation.texture.id());
                        assert_eq!(restored.sampling.get(), presentation.sampling.get());
                        assert_eq!(restored.frame_index, presentation.frame_index);
                        assert_eq!(restored.next_frame_at, deadline);
                        assert_eq!(restored.plays_left, 2);
                        assert_eq!(app.edits[&tab], history);
                        assert_eq!(app.image_view, view);
                        // The GPU oracle must reject restoration of the first animation frame.
                        app.image = Some(
                            ImagePresentation::from_decoded_frame(
                                &context,
                                &path,
                                presentation.decoded.clone(),
                                0,
                                app.image_sampling(),
                            )
                            .expect("wrong-frame control"),
                        );
                        assert!(
                            surface(&mut app, &context, &mut renderer, density, false) != edited
                        );
                        app.image = Some(presentation);
                        app.undo_edit(false);
                        wait(&mut app);
                        assert!(Arc::ptr_eq(
                            &app.image.as_ref().expect("undo").decoded,
                            &source
                        ));
                        assert!(
                            surface(&mut app, &context, &mut renderer, density, false) == original,
                            "undo must restore original pixels after recovery"
                        );
                        app.undo_edit(true);
                        wait(&mut app);
                        assert!(
                            surface(&mut app, &context, &mut renderer, density, false) == edited,
                            "redo must restore resized pixels after recovery"
                        );
                        assert_eq!(app.edits[&tab].operations(), history.operations());
                    }
                }
            }
            self.completed = true;
            eprintln!(
                "PASS resized animation GPU: 24 filter/sampling/density cases; full-surface recovery, wrong-frame negative controls, Undo/Redo, retained frame/deadline/plays/history/view. Generated frames and real resampling worker; hidden hardware renderer, not injected device loss or physical DPI/input."
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial {
        root,
        completed: false,
    };
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("GPU trial");
    assert!(trial.completed, "GPU trial must not silently skip");
}
