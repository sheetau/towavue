use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

fn color(page: usize, frame: usize) -> [u8; 4] {
    [
        20 + page as u8 * 23,
        40 + frame as u8 * 3,
        220 - page as u8 * 19,
        255,
    ]
}

#[test]
#[ignore = "large reading textures require hardware D3D11 and substantial GPU/CPU memory"]
fn reading_gpu_pressure_preserves_pages_animation_and_texture_lifetimes() {
    let Some(root) = crate::tests::isolated_test_root(
        "reading_view::tests::gpu::reading_gpu_pressure_preserves_pages_animation_and_texture_lifetimes",
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
            let mut renderer =
                FrameRenderer::new(&window).expect("hardware D3D11 required for this opt-in test");
            let originals: Vec<_> = (0..9)
                .map(|page| {
                    let animated = page % 3 == 1;
                    let (width, height, count) = if animated {
                        (512, 384, 64)
                    } else {
                        (4096, 3072, 1)
                    };
                    Arc::new(DecodedImage {
                        format: "generated",
                        animation_plays: if animated { 3 } else { 1 },
                        frames: (0..count)
                            .map(|frame| towavue_runtime_windows::DecodedImageFrame {
                                width,
                                height,
                                rgba: color(page, frame).repeat(width as usize * height as usize),
                                delay: Duration::from_millis(10),
                            })
                            .collect(),
                    })
                })
                .collect();
            let mut checks = 0;
            let mut cache_hits = 0;
            let mut evictions = 0;
            for density in [1.0, 1.25, 2.0] {
                let context = fonts::test_context();
                context.global_style_mut(chrome::style);
                context.input_mut(|input| input.max_texture_side = renderer.max_texture_side());
                let mut app = Application::new(None, |_| {}).expect("app");
                assert_eq!(app.image_texture_cache.byte_limit, 256 * 1024 * 1024);
                app.ui_context = Some(context.clone());
                app.media_kind = Some(MediaKind::Image);
                app.state = PlaybackState::Paused;
                app.reading_mode = true;
                app.reading_settings.page_count = 3;
                let mut ids = Vec::new();
                for (step, spread) in [0, 1, 0, 2, 1, 2, 0, 1, 2, 1, 0, 2].into_iter().enumerate() {
                    let pages: Vec<_> = (spread * 3..spread * 3 + 3)
                        .map(|page| {
                            let before: Vec<_> = app
                                .image_texture_cache
                                .entries
                                .iter()
                                .map(|entry| entry.texture.id())
                                .collect();
                            let hit = app
                                .image_texture_cache
                                .entries
                                .iter()
                                .find(|entry| {
                                    entry.decoded.as_ptr() == Arc::as_ptr(&originals[page])
                                })
                                .map(|entry| entry.texture.id());
                            let image = app
                                .image_texture_cache
                                .load(
                                    &context,
                                    &self.root.join(format!("{page}.png")),
                                    Arc::clone(&originals[page]),
                                    TextureOptions::NEAREST,
                                )
                                .expect("page texture");
                            if let Some(id) = hit {
                                assert_eq!(image.texture.id(), id, "reuse the cached GPU handle");
                                cache_hits += 1;
                            }
                            evictions += before
                                .iter()
                                .filter(|id| {
                                    !app.image_texture_cache
                                        .entries
                                        .iter()
                                        .any(|entry| entry.texture.id() == **id)
                                })
                                .count();
                            ids.push(image.texture.id());
                            image
                        })
                        .collect();
                    let mut pages = pages.into_iter();
                    app.image = pages.next();
                    app.reading_pages = pages.map(Ok).collect();
                    app.reading_settings.axis = if step % 2 == 0 {
                        ReadingAxis::Horizontal
                    } else {
                        ReadingAxis::Vertical
                    };
                    app.reading_settings.reversed = step % 3 == 0;
                    app.image_view = ImageViewState::default();
                    let animation_start = app.reading_pages[0]
                        .as_ref()
                        .expect("animation")
                        .next_frame_at
                        .expect("animation deadline")
                        - Duration::from_millis(10);
                    for elapsed_frames in [0, 1, 31, 63, 64, 191, 192] {
                        let animation = app.reading_pages[0].as_mut().expect("animation");
                        animation.advance_animation(
                            animation_start + Duration::from_millis(elapsed_frames * 10),
                        );
                        let expected_frame = if elapsed_frames >= 192 {
                            63
                        } else {
                            elapsed_frames as usize % 64
                        };
                        assert_eq!(animation.frame_index, expected_frame);
                        assert_eq!(animation.next_frame_at.is_none(), elapsed_frames >= 192);
                        if elapsed_frames == 31 {
                            app.zoom_reading(1.5);
                        }
                        if elapsed_frames == 64 {
                            app.image_view = ImageViewState::default();
                        }
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
                            .entry(egui::ViewportId::ROOT)
                            .or_default()
                            .native_pixels_per_point = Some(density);
                        let mut output = context.run_ui(input, |ui| app.draw_reading_pages(ui));
                        let images = [
                            app.image.as_ref().expect("first"),
                            app.reading_pages[0].as_ref().expect("middle"),
                            app.reading_pages[1].as_ref().expect("last"),
                        ];
                        let bounds: Vec<_> = images
                            .iter()
                            .enumerate()
                            .map(|(index, image)| {
                                let rect = output
                                    .shapes
                                    .iter()
                                    .find_map(|shape| match &shape.shape {
                                        egui::Shape::Mesh(mesh)
                                            if mesh.texture_id == image.texture.id() =>
                                        {
                                            Some(mesh.calc_bounds().intersect(shape.clip_rect))
                                        }
                                        _ => None,
                                    })
                                    .expect("page mesh");
                                (
                                    rect,
                                    color(
                                        spread * 3 + index,
                                        if index == 1 { expected_frame } else { 0 },
                                    ),
                                )
                            })
                            .collect();
                        if step == 5 && elapsed_frames == 31 {
                            let device = renderer.graphics_device();
                            renderer.release_surface();
                            renderer = FrameRenderer::with_graphics_device(&window, device)
                                .expect("same-device renderer recreation");
                            output
                                .textures_delta
                                .set
                                .extend(app.restored_ui_textures(&context));
                        }
                        let width = (640.0 * density) as u32;
                        let height = (480.0 * density) as u32;
                        renderer
                            .resize_surface(width, height)
                            .expect("surface dimensions");
                        renderer.clear([1.0, 0.0, 1.0, 1.0]).expect("clear");
                        renderer
                            .render_ui(&context, output)
                            .expect("render reading spread");
                        let pixels = renderer.verification_surface_rgba().expect("GPU readback");
                        let mut sampled = 0;
                        for (rect, expected) in bounds {
                            let rect = rect.shrink(4.0);
                            for y in (rect.top() * density).ceil().max(0.0) as usize
                                ..(rect.bottom() * density).floor().max(0.0) as usize
                            {
                                for x in (rect.left() * density).ceil().max(0.0) as usize
                                    ..(rect.right() * density).floor().max(0.0) as usize
                                {
                                    assert_eq!(
                                        &pixels[(y * width as usize + x) * 4..][..4],
                                        expected.as_slice(),
                                        "page pixels: {density}x, step {step}, phase {elapsed_frames}"
                                    );
                                    sampled += 1;
                                }
                            }
                        }
                        assert!(sampled > 1000, "verify displayed image interiors");
                        renderer.present_surface().expect("Present");
                        assert!(
                            app.image_texture_cache
                                .entries
                                .iter()
                                .map(|entry| entry.bytes)
                                .sum::<usize>()
                                <= 256 * 1024 * 1024
                        );
                        assert!(app.image_texture_cache.entries.iter().all(|entry| {
                            !entry
                                .decoded
                                .upgrade()
                                .expect("owned fixture")
                                .is_animated()
                        }));
                        checks += 1;
                    }
                }
                app.image = None;
                app.reading_pages.clear();
                app.image_texture_cache.entries.clear();
                assert!(
                    ids.iter()
                        .all(|id| context.tex_manager().read().meta(*id).is_none()),
                    "release every page handle"
                );
                let output = context.run_ui(Default::default(), |_| {});
                renderer
                    .render_ui(&context, output)
                    .expect("deliver texture frees");
                assert!(
                    renderer
                        .verification_managed_textures()
                        .iter()
                        .all(|(id, _)| !ids.contains(id)),
                    "release native page textures"
                );
                drop(app);
                let device = renderer.graphics_device();
                renderer.release_surface();
                renderer =
                    FrameRenderer::with_graphics_device(&window, device).expect("next density");
            }
            assert!(cache_hits > 0 && evictions > 0);
            assert!(
                originals.iter().all(|image| Arc::strong_count(image) == 1),
                "presentations release original pixels"
            );
            eprintln!(
                "READING_GPU checks={checks} cache_hits={cache_hits} evictions={evictions}; hidden hardware surfaces, no physical input or peak-memory measurement"
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
