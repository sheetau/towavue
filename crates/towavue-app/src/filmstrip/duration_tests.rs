use super::*;

#[test]
fn gallery_and_filmstrip_durations_are_centered_and_audio_pixels_are_tinted() {
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::duration_tests::gallery_and_filmstrip_durations_are_centered_and_audio_pixels_are_tinted",
    ) else {
        return;
    };
    let paths = [
        root.join("movie.mp4"),
        root.join("audio.flac"),
        root.join("image.png"),
    ];
    let snapshot = FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![]),
        folder_path: root.clone(),
        items: paths
            .iter()
            .map(|path| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![]),
                path: path.clone(),
                kind: MediaKind::from_path(path).expect("fixture kind"),
            })
            .collect(),
        sort_columns: vec![],
        source: towavue_core::FolderSnapshotSource::LiveExplorerView,
        generation: 1,
        captured_at: std::time::SystemTime::now(),
    };
    for density in [1.0, 1.25, 2.0] {
        for (width, gallery) in [(260.0, true), (660.0, true), (660.0, false)] {
            let context = crate::fonts::test_context();
            context.global_style_mut(crate::chrome::style);
            let mut strip =
                Filmstrip::new(PreviewCache::new(root.join("cache")).expect("cache"), || {})
                    .expect("preview worker");
            let mut textures = Vec::new();
            for (index, duration) in [
                Some(Duration::from_secs(65)),
                Some(Duration::from_secs(3601)),
                None,
            ]
            .into_iter()
            .enumerate()
            {
                let texture = context.load_texture(
                    format!("badge-fixture-{index}"),
                    egui::ColorImage::filled([3, 2], Color32::WHITE),
                    egui::TextureOptions::LINEAR,
                );
                textures.push(texture.id());
                strip
                    .previews
                    .insert(paths[index].clone(), Ok((texture, duration)));
            }
            for enabled in [true, false] {
                for _ in 0..3 {
                    let mut actions = Vec::new();
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, if gallery { 1100.0 } else { 500.0 }),
                            )),
                            viewports: [(
                                egui::ViewportId::ROOT,
                                egui::ViewportInfo {
                                    native_pixels_per_point: Some(density),
                                    ..Default::default()
                                },
                            )]
                            .into_iter()
                            .collect(),
                            ..Default::default()
                        },
                        |ui| {
                            if gallery {
                                strip.show_recent(ui, &paths, 1, enabled, &mut actions);
                            } else {
                                strip.show(
                                    ui.ctx(),
                                    ui.max_rect(),
                                    Some(&snapshot),
                                    Some(&paths[0]),
                                    enabled,
                                    &mut actions,
                                );
                            }
                        },
                    );
                    assert!(actions.is_empty());
                    assert_eq!(output.pixels_per_point, density);
                    let backgrounds = output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == Color32::from_rgba_premultiplied(26, 26, 26, 153))).count();
                    assert_eq!(backgrounds, 2, "only video/audio durations receive badges");
                    for (index, seconds) in [65, 3601].into_iter().enumerate() {
                        let label = format_time(media_time(Duration::from_secs(seconds)));
                        let (text_index, text) = output
                            .shapes
                            .iter()
                            .enumerate()
                            .find_map(|(index, shape)| match &shape.shape {
                                egui::Shape::Text(text) if text.galley.text() == label => {
                                    Some((index, text))
                                }
                                _ => None,
                            })
                            .expect("duration label");
                        let egui::Shape::Rect(background) = &output.shapes[text_index - 1].shape
                        else {
                            panic!("backdrop before duration text");
                        };
                        assert_eq!(
                            background.fill,
                            Color32::from_rgba_premultiplied(26, 26, 26, 153)
                        );
                        assert_eq!(
                            background.rect,
                            Rect::from_min_size(text.pos, text.galley.size())
                                .expand2(egui::vec2(4.0, 2.0))
                        );
                        assert_eq!(background.corner_radius, egui::CornerRadius::same(2));
                        let image_index = output.shapes.iter().position(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == textures[index])).expect("ready thumbnail");
                        assert!(image_index < text_index - 1, "badge is above its thumbnail");
                        let egui::Shape::Mesh(mesh) = &output.shapes[image_index].shape else {
                            panic!("thumbnail mesh");
                        };
                        let bounds = if gallery {
                            mesh.calc_bounds()
                        } else if let egui::Shape::Rect(card) =
                            &output.shapes[image_index - 1].shape
                        {
                            card.rect
                        } else {
                            panic!("filmstrip card background")
                        };
                        assert!((bounds.right() - background.rect.right() - 4.0).abs() < 0.01);
                        assert!((bounds.bottom() - background.rect.bottom() - 4.0).abs() < 0.01);
                        let color = if index == 1 {
                            Color32::from_gray(0x80)
                        } else {
                            Color32::WHITE
                        };
                        assert!(
                            mesh.vertices.iter().any(|v| v.color == color),
                            "audio tint is independent of video pixels"
                        );

                        assert!(
                            output.shapes[text_index]
                                .clip_rect
                                .contains_rect(background.rect)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn gallery_diagonal_and_duration_placement_balance_extreme_aspect_ratios() {
    let cell = Rect::from_min_size(egui::pos2(64.0, 40.0), Vec2::splat(176.0));
    for size in [
        Vec2::splat(100.0),
        egui::vec2(1920.0, 1080.0),
        egui::vec2(1080.0, 1920.0),
        egui::vec2(1.0, 10000.0),
        egui::vec2(10000.0, 1.0),
    ] {
        for scale in [1.0, 1.05] {
            let image = gallery_thumbnail(cell, size, scale);
            assert!((image.size().length() - 176.0 * scale).abs() < 0.001);
            assert_eq!(image.center(), cell.center());
            for badge_size in [egui::vec2(32.0, 17.0), egui::vec2(78.0, 17.0)] {
                let badge = duration_background(image, badge_size, true);
                assert!(badge.left() >= cell.center().x && badge.top() >= cell.center().y);
                assert!(cell.expand(8.0).contains_rect(badge));
            }
        }
    }
}

#[test]
fn gallery_video_geometry_excludes_transparent_padding_and_preserves_black_pixels() {
    let mut rgba = vec![0; 12 * 8 * 4];
    for y in 0..8 {
        for x in 4..7 {
            rgba[(y * 12 + x) * 4 + 3] = 255;
        }
    }
    let image = towavue_runtime_windows::PreviewImage {
        width: 12,
        height: 8,
        rgba: std::sync::Arc::new(rgba),
    };
    let uv = gallery_video_uv(&image);
    assert_eq!(
        uv,
        Rect::from_min_max(egui::pos2(4.0 / 12.0, 0.0), egui::pos2(7.0 / 12.0, 1.0))
    );
    let rect = gallery_thumbnail(
        Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(176.0)),
        egui::vec2(12.0, 8.0) * uv.size(),
        1.0,
    );
    assert!((rect.width() / rect.height() - 3.0 / 8.0).abs() < 0.001);
    assert!((rect.size().length() - 176.0).abs() < 0.001);
}

#[test]
#[ignore = "requires hidden hardware D3D11; optional generated Gallery readbacks"]
fn gallery_grid_reaches_gpu_without_card_backgrounds() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(root) = crate::tests::isolated_test_root(
        "filmstrip::duration_tests::gallery_grid_reaches_gpu_without_card_backgrounds",
    ) else {
        return;
    };
    struct Trial {
        root: PathBuf,
        complete: bool,
    }
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            let window = event_loop
                .create_window(winit::window::Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = towavue_runtime_windows::FrameRenderer::new(&window).expect("D3D11");
            for density in [1.0, 1.25, 2.0] {
                let context = crate::fonts::test_context();
                context.global_style_mut(crate::chrome::style);
                let mut strip = Filmstrip::new(
                    PreviewCache::new(self.root.join("cache")).expect("cache"),
                    || {},
                )
                .expect("worker");
                let mut paths = Vec::new();
                let mut textures = Vec::new();
                for (index, size) in [
                    [160, 90],
                    [90, 160],
                    [128, 128],
                    [1, 320],
                    [320, 1],
                    [240, 100],
                    [100, 240],
                    [128, 128],
                    [160, 90],
                    [90, 160],
                    [240, 100],
                    [128, 128],
                ]
                .into_iter()
                .enumerate()
                {
                    let path = self.root.join(format!("generated-{index}.mp4"));
                    let color = Color32::from_rgb(70 + index as u8 * 10, 115, 180);
                    let texture = context.load_texture(
                        format!("fixture-{index}"),
                        egui::ColorImage::filled(size, color),
                        egui::TextureOptions::LINEAR,
                    );
                    textures.push(texture.id());
                    strip.previews.insert(
                        path.clone(),
                        Ok((texture, Some(Duration::from_secs(65 + index as u64 * 360)))),
                    );
                    paths.push(path);
                }
                let width = (1160.0 * density) as u32;
                let height = (740.0 * density) as u32;
                renderer.resize_surface(width, height).expect("surface");
                let mut idle_bounds = None;
                for frame in 0..8 {
                    let mut input = egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1160.0, 740.0),
                        )),
                        max_texture_side: Some(renderer.max_texture_side()),
                        time: Some(frame as f64 * 0.1),
                        focused: true,
                        ..Default::default()
                    };
                    input
                        .viewports
                        .entry(egui::ViewportId::ROOT)
                        .or_default()
                        .native_pixels_per_point = Some(density);
                    if frame >= 3 {
                        input
                            .events
                            .push(egui::Event::PointerMoved(egui::pos2(170.0, 150.0)));
                    }
                    let output = context.run_ui(input, |ui| {
                        ui.painter()
                            .rect_filled(ui.max_rect(), 0.0, crate::chrome::BACKGROUND);
                        crate::welcome::show(
                            ui,
                            &Default::default(),
                            &mut String::new(),
                            &mut None,
                            &paths,
                            true,
                            |ui, _, _| {
                                strip.show_recent_with_policy(
                                    ui,
                                    &paths,
                                    1,
                                    true,
                                    &mut vec![],
                                    PreparationPolicy::Paused,
                                );
                                vec![crate::gallery_rail::Month {
                                    date: Some((2026, 9)),
                                    offset: 0.0,
                                }]
                            },
                        );
                    });
                    let bounds = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh) if mesh.texture_id == textures[0] => {
                                Some(mesh.calc_bounds())
                            }
                            _ => None,
                        })
                        .expect("first image");
                    assert_eq!(output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if textures.contains(&mesh.texture_id))).count(), paths.len(), "all cards remain painted at frame {frame}");
                    if frame == 2 {
                        idle_bounds = Some(bounds);
                    }
                    if frame == 7 {
                        let idle = idle_bounds.expect("idle frame");
                        assert_eq!(bounds.center(), idle.center());
                        assert!((bounds.width() / idle.width() - 1.05).abs() < 0.001);
                    }
                    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
                    renderer.render_ui(&context, output).expect("UI");
                    if frame == 2 || frame == 7 {
                        let pixels = renderer.verification_surface_rgba().expect("readback");
                        // A square cell corner lies outside its inscribed thumbnail.
                        let at = |x: f32, y: f32| {
                            let start = (((y * density) as usize * width as usize)
                                + (x * density) as usize)
                                * 4;
                            &pixels[start..start + 4]
                        };
                        assert_eq!(at(66.0, 44.0), &crate::chrome::BACKGROUND.to_array());
                        assert_eq!(at(170.0, 150.0), &[70, 115, 180, 255]);
                        assert_eq!(
                            at(580.0, 132.0),
                            &[90, 115, 180, 255],
                            "unhovered card remains visible at frame {frame}"
                        );
                        if let Some(directory) = std::env::var_os("TOWAVUE_REFERENCE_RENDER_DIR") {
                            std::fs::write(
                                PathBuf::from(directory)
                                    .join(format!("gallery-{frame}-{width}x{height}.rgba")),
                                pixels,
                            )
                            .expect("readback file");
                        }
                    }
                    renderer.present_surface().expect("Present");
                }
            }
            self.complete = true;
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &winit::event_loop::ActiveEventLoop,
            _: winit::window::WindowId,
            _: winit::event::WindowEvent,
        ) {
        }
    }
    let mut trial = Trial {
        root,
        complete: false,
    };
    let mut builder = winit::event_loop::EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("trial");
    assert!(trial.complete);
}
