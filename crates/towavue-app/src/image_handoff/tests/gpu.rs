use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

fn draw(
    app: &mut App,
    context: &egui::Context,
    renderer: &mut FrameRenderer,
    restore: bool,
) -> Vec<u8> {
    let mut output = frame(app, context);
    if restore {
        output
            .textures_delta
            .set
            .extend(app.restored_ui_textures(context));
    }
    let density = context.pixels_per_point();
    let width = (640.0 * density) as u32;
    let height = (480.0 * density) as u32;
    renderer
        .resize_surface(width, height)
        .expect("surface size");
    renderer.clear([1.0, 0.0, 1.0, 1.0]).expect("clear");
    renderer.render_ui(context, output).expect("UI submission");
    let pixels = renderer.verification_surface_rgba().expect("GPU readback");
    assert_eq!(pixels.len(), (width * height * 4) as usize);
    renderer.present_surface().expect("Present");
    // Exclude chrome and letterboxing, but compare every pixel in the central image area.
    let mut content = Vec::new();
    for row in (150.0 * density) as usize..(330.0 * density) as usize {
        let start = (row * width as usize + (160.0 * density) as usize) * 4;
        let end = (row * width as usize + (480.0 * density) as usize) * 4;
        content.extend_from_slice(&pixels[start..end]);
    }
    content
}

#[test]
fn gpu_handoff_preserves_pixels_through_supersession_and_renderer_recreation() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_handoff::tests::gpu::gpu_handoff_preserves_pixels_through_supersession_and_renderer_recreation",
    ) else {
        return;
    };
    struct Trial {
        root: PathBuf,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden owned window");
            let mut renderer = match FrameRenderer::new(&window) {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("SKIP image handoff GPU: hardware D3D11 unavailable: {error}");
                    event_loop.exit();
                    return;
                }
            };
            for density in [1.0, 1.25, 2.0] {
                let (mut app, context, _) = fixture(&self.root);
                app.fullscreen = true;
                context.set_pixels_per_point(density);
                let mut source = decoded(160, 90, [20, 40, 60, 255]);
                for (index, pixel) in Arc::get_mut(&mut source).expect("unique fixture").frames[0]
                    .rgba
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .enumerate()
                {
                    if (index % 160 / 4 + index / 160 / 4) % 2 == 0 {
                        pixel.copy_from_slice(&[220, 160, 80, 255]);
                    }
                }
                let old_decoded = Arc::downgrade(&source);
                let rgba_pointer = source.frames[0].rgba.as_ptr();
                app.image = Some(
                    ImagePresentation::from_decoded(&context, &self.root.join("old.png"), source)
                        .expect("pattern original"),
                );
                let old_texture = app.image.as_ref().expect("original").texture.id();
                for _ in 0..3 {
                    draw(&mut app, &context, &mut renderer, false);
                }
                assert_eq!(context.pixels_per_point(), density);
                let original = draw(&mut app, &context, &mut renderer, false);
                assert!(
                    original
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|pixel| pixel.as_slice() != &original[..4]),
                    "pattern must reach the GPU, not just the clear color"
                );
                for index in 0..12 {
                    let path = self.root.join(format!("next-{index}.png"));
                    navigate_pending(&mut app, path.clone());
                    app.finish_image_preview(
                        path,
                        app.image_preview_generation,
                        towavue_runtime_windows::CachedImagePreview {
                            source_size: (160, 90),
                            image: towavue_runtime_windows::PreviewImage {
                                width: 1,
                                height: 1,
                                rgba: vec![255, 0, 0, 255],
                            },
                        },
                    );
                    let held = app.image_handoff.as_ref().expect("held original");
                    assert_eq!(held.image.decoded.frames[0].rgba.as_ptr(), rgba_pointer);
                    assert_eq!(
                        old_decoded.strong_count(),
                        1,
                        "one shared original, not one per request"
                    );
                    if index == 0 {
                        // Negative controls: the same GPU oracle must distinguish a preview
                        // and an empty loading surface when the display holder is absent.
                        let held = app.image_handoff.take();
                        let preview = draw(&mut app, &context, &mut renderer, false);
                        assert!(
                            preview
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .all(|pixel| *pixel == [255, 0, 0, 255])
                        );
                        assert!(preview != original);
                        app.image_previews.clear();
                        assert!(draw(&mut app, &context, &mut renderer, false) != original);
                        app.image_handoff = held;
                    }
                    assert!(
                        draw(&mut app, &context, &mut renderer, false) == original,
                        "held original changed at {density}x, request {index}"
                    );
                    if index == 5 {
                        let device = renderer.graphics_device();
                        renderer.release_surface();
                        renderer = FrameRenderer::with_graphics_device(&window, device)
                            .expect("recreate on the same device");
                        assert!(
                            draw(&mut app, &context, &mut renderer, true) == original,
                            "restore held texture after renderer recreation"
                        );
                    }
                }
                app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
                    generation: app.image_generation,
                    first_index: 0,
                    total: 1,
                    images: vec![(
                        app.path.clone().expect("latest target"),
                        Ok(decoded(160, 90, [0, 255, 0, 255])),
                    )],
                });
                assert!(
                    old_decoded.upgrade().is_none(),
                    "release the old decoded allocation"
                );
                assert!(context.tex_manager().read().meta(old_texture).is_none());
                for _ in 0..2 {
                    let current = draw(&mut app, &context, &mut renderer, false);
                    assert!(
                        current
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|pixel| *pixel == [0, 255, 0, 255]),
                        "replace all sampled pixels with the latest original"
                    );
                }
                // Context texture IDs restart per fixture; release this renderer's texture table.
                let device = renderer.graphics_device();
                renderer.release_surface();
                renderer =
                    FrameRenderer::with_graphics_device(&window, device).expect("next fixture");
            }
            eprintln!(
                "PASS image handoff GPU: 3 densities, 12 supersessions each, full central pixel equality, preview suppression, shared-device renderer recreation, new-original replacement and old decoded release. Hidden window; not physical input or process/GPU peak memory."
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial { root })
        .expect("trial");
}
