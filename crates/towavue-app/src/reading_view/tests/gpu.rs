use super::*;
use std::os::windows::process::CommandExt;
use winit::platform::windows::EventLoopBuilderExtWindows;

fn color(page: usize, frame: usize) -> [u8; 4] {
    [
        20 + page as u8 * 23,
        40 + frame as u8 * 3,
        220 - page as u8 * 19,
        255,
    ]
}

fn image_surface<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    context: &egui::Context,
    renderer: &mut FrameRenderer,
    density: f32,
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
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(density);
    let output = context.run_ui(input, |ui| app.draw_image(ui));
    renderer
        .resize_surface((640.0 * density) as u32, (480.0 * density) as u32)
        .expect("surface size");
    renderer
        .clear([0.0, 0.0, 0.0, 1.0])
        .expect("clear complete surface");
    renderer
        .render_ui(context, output)
        .expect("draw navigation phase");
    let pixels = renderer
        .verification_surface_rgba()
        .expect("complete surface readback");
    if app.image.is_some() || app.image_handoff.is_some() {
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] != pixel[1] || pixel[1] != pixel[2]),
            "colored generated pages, not only the clear surface or grayscale chrome, must reach the GPU"
        );
    }
    renderer
        .present_surface()
        .expect("Present navigation phase");
    pixels
}

fn equal_surface(before: &[u8], after: &[u8], density: f32, step: usize, phase: &str) {
    assert_eq!(before.len(), after.len());
    if before == after {
        return;
    }
    let width = (640.0 * density) as usize;
    let mut bounds = [usize::MAX, usize::MAX, 0, 0];
    let mut count = 0;
    for (index, (before, after)) in before
        .as_chunks::<4>()
        .0
        .iter()
        .zip(after.as_chunks::<4>().0)
        .enumerate()
    {
        if before != after {
            let (x, y) = (index % width, index / width);
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x);
            bounds[3] = bounds[3].max(y);
            count += 1;
        }
    }
    panic!("{phase}: {density}x, step {step}, {count} differing pixels in {bounds:?}");
}

#[test]
#[ignore = "large reading textures require hardware D3D11 and substantial GPU/CPU memory"]
fn reading_gpu_pressure_preserves_pages_animation_and_texture_lifetimes() {
    let Some(root) = crate::tests::isolated_test_root(
        "reading_view::tests::gpu::reading_gpu_pressure_preserves_pages_animation_and_texture_lifetimes",
    ) else {
        return;
    };
    run(root, false);
}

#[test]
#[ignore = "generates large PNG/APNG files and requires hardware D3D11 plus FFMPEG_DIR"]
fn reading_files_reach_gpu_through_navigation_and_async_completion() {
    let Some(root) = crate::tests::isolated_test_root(
        "reading_view::tests::gpu::reading_files_reach_gpu_through_navigation_and_async_completion",
    ) else {
        return;
    };
    run(root, true);
}

fn run(root: PathBuf, real_files: bool) {
    struct Trial {
        root: PathBuf,
        real_files: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden owned window");
            let mut renderer =
                FrameRenderer::new(&window).expect("hardware D3D11 required for this opt-in test");
            let mut originals: Vec<_> = (0..9)
                .map(|page| {
                    let animated = page % 3 == 1;
                    let (width, height, count) = if animated {
                        (512, 384, 64)
                    } else if self.real_files && page % 2 == 1 {
                        (3072, 4096, 1)
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
            let paths: Vec<_> = (0..9)
                .map(|page| self.root.join(format!("{page}.png")))
                .collect();
            if self.real_files {
                use std::io::Write;
                let ffmpeg = PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg"))
                    .join("bin/ffmpeg.exe");
                for (path, image) in paths.iter().zip(&originals) {
                    let (width, height) = image.dimensions();
                    let mut command = std::process::Command::new(&ffmpeg);
                    command.creation_flags(0x0800_0000);
                    command
                        .args([
                            "-v",
                            "error",
                            "-f",
                            "rawvideo",
                            "-pixel_format",
                            "rgba",
                            "-video_size",
                        ])
                        .arg(format!("{width}x{height}"))
                        .args([
                            "-framerate",
                            "100",
                            "-i",
                            "pipe:0",
                            "-threads",
                            "1",
                            "-frames:v",
                        ])
                        .arg(image.frames.len().to_string());
                    if image.is_animated() {
                        command.args(["-f", "apng", "-plays", "3"]);
                    } else {
                        command.args(["-c:v", "png", "-f", "image2"]);
                    }
                    let mut child = command
                        .arg(path)
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::piped())
                        .spawn()
                        .expect("encode owned PNG");
                    let mut input = child.stdin.take().expect("raw frames pipe");
                    for frame in &image.frames {
                        input.write_all(&frame.rgba).expect("encode frame");
                    }
                    drop(input);
                    let output = child.wait_with_output().expect("PNG encoder exit");
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                originals.clear(); // Only the real loader may supply the integration trial's pixels.
            }
            let source_stamps: Vec<_> = if self.real_files {
                paths
                    .iter()
                    .map(|path| {
                        let metadata = std::fs::metadata(path).expect("generated source");
                        (metadata.len(), metadata.modified().expect("source date"))
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let mut checks = 0;
            let mut pending_frames = 0;
            let mut partial_frames = 0;
            let mut completed_frames = 0;
            let mut foreground_decodes = 0;
            let mut foreground_hits = 0;
            let mut cache_hits = 0;
            let mut evictions = 0;
            for density in [1.0, 1.25, 2.0] {
                let context = fonts::test_context();
                context.global_style_mut(chrome::style);
                context.input_mut(|input| input.max_texture_side = renderer.max_texture_side());
                let (notify, events) = std::sync::mpsc::channel();
                let mut app = Application::new(None, move |event| {
                    let _ = notify.send(event);
                })
                .expect("app");
                assert_eq!(app.image_texture_cache.byte_limit, 256 * 1024 * 1024);
                app.ui_context = Some(context.clone());
                app.media_kind = Some(MediaKind::Image);
                app.state = PlaybackState::Paused;
                app.reading_mode = true;
                app.reading_settings.page_count = 3;
                app.reading_settings.first_page_count = 3;
                if self.real_files {
                    app.tabs.open_new(paths[0].clone(), MediaKind::Image);
                    app.folder_snapshot = Some(FolderSnapshot {
                        folder_identity: towavue_core::ShellIdentity::new(vec![0]),
                        folder_path: self.root.clone(),
                        items: paths
                            .iter()
                            .enumerate()
                            .map(|(index, path)| towavue_core::FolderMediaItem {
                                identity: towavue_core::ShellIdentity::new(vec![index as u8]),
                                path: path.clone(),
                                kind: MediaKind::Image,
                            })
                            .collect(),
                        sort_columns: vec![],
                        source: FolderSnapshotSource::NaturalNameFallback,
                        generation: 1,
                        captured_at: std::time::SystemTime::UNIX_EPOCH,
                    });
                }
                let mut ids = Vec::new();
                let mut decoded_owners = Vec::new();
                for (step, spread) in [0, 1, 0, 2, 1, 2, 0, 1, 2, 1, 0, 2].into_iter().enumerate() {
                    if self.real_files {
                        let previous = (step != 0)
                            .then(|| image_surface(&mut app, &context, &mut renderer, density));
                        if step == 0 {
                            app.load_path(paths[spread * 3].clone(), MediaKind::Image);
                        } else {
                            app.navigate_to_unchecked(paths[spread * 3].clone());
                            assert!(
                                app.image_handoff
                                    .as_ref()
                                    .is_some_and(|held| held.reading.is_some()),
                                "retain the prior spread while loading"
                            );
                            equal_surface(
                                previous.as_ref().expect("previous spread"),
                                &image_surface(&mut app, &context, &mut renderer, density),
                                density,
                                step,
                                "request changed the displayed spread",
                            );
                            pending_frames += 1;
                        }
                        let deadline = Instant::now() + Duration::from_secs(60);
                        while app.image_loading {
                            let event = events
                                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                                .expect("async original completion");
                            if matches!(event, AppEvent::ImagesReady) {
                                app.finish_image_load();
                                let pixels =
                                    image_surface(&mut app, &context, &mut renderer, density);
                                if app.image_loading {
                                    if let Some(previous) = &previous {
                                        equal_surface(
                                            previous,
                                            &pixels,
                                            density,
                                            step,
                                            "partial completion changed the held spread",
                                        );
                                        pending_frames += 1;
                                        partial_frames += 1;
                                    }
                                } else {
                                    equal_surface(
                                        &pixels,
                                        &image_surface(&mut app, &context, &mut renderer, density),
                                        density,
                                        step,
                                        "completed spread changed on the next frame",
                                    );
                                    completed_frames += 1;
                                }
                            }
                        }
                        assert_eq!(app.path.as_ref(), Some(&paths[spread * 3]));
                        assert_eq!(app.reading_pages.len(), 2);
                        assert!(app.image_error.is_none() && app.image_handoff.is_none());
                        decoded_owners.push(Arc::downgrade(
                            &app.image.as_ref().expect("first loaded page").decoded,
                        ));
                        decoded_owners.extend(app.reading_pages.iter().map(|page| {
                            Arc::downgrade(&page.as_ref().expect("loaded page").decoded)
                        }));
                        ids.push(app.image.as_ref().expect("first loaded page").texture.id());
                        ids.extend(
                            app.reading_pages
                                .iter()
                                .map(|page| page.as_ref().expect("loaded page").texture.id()),
                        );
                    } else {
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
                                    assert_eq!(
                                        image.texture.id(),
                                        id,
                                        "reuse the cached GPU handle"
                                    );
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
                    }
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
                            entry
                                .decoded
                                .upgrade()
                                .is_none_or(|image| !image.is_animated())
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
                let metrics = app.image_loader.verification_metrics();
                foreground_decodes += metrics.foreground.calls;
                foreground_hits += metrics.initial_cache_hits + metrics.late_cache_hits;
                drop(app);
                let deadline = Instant::now() + Duration::from_secs(10);
                while decoded_owners.iter().any(|owner| owner.upgrade().is_some()) {
                    assert!(
                        Instant::now() < deadline,
                        "closed loader releases displayed originals"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                let device = renderer.graphics_device();
                renderer.release_surface();
                renderer =
                    FrameRenderer::with_graphics_device(&window, device).expect("next density");
            }
            if !self.real_files {
                assert!(cache_hits > 0 && evictions > 0);
            }
            assert!(
                originals.iter().all(|image| Arc::strong_count(image) == 1),
                "presentations release original pixels"
            );
            for (path, stamp) in paths.iter().zip(source_stamps) {
                let metadata = std::fs::metadata(path).expect("source remains present");
                assert_eq!(
                    (metadata.len(), metadata.modified().expect("source date")),
                    stamp
                );
            }
            if self.real_files {
                assert!(foreground_decodes > 0 && foreground_hits > 0);
                assert!(
                    partial_frames > 0,
                    "observe real partial completions, not only warm batches"
                );
                assert_eq!(completed_frames, 36);
                eprintln!(
                    "READING_FILES checks={checks} pending_frames={pending_frames} partial_frames={partial_frames} completed_frames={completed_frames} foreground_decodes={foreground_decodes} foreground_hits={foreground_hits}; hidden hardware surfaces and async loader, fixed folder order, no physical input or peak-memory measurement"
                );
            } else {
                eprintln!(
                    "READING_GPU checks={checks} cache_hits={cache_hits} evictions={evictions}; hidden hardware surfaces, no physical input or peak-memory measurement"
                );
            }
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial { root, real_files })
        .expect("trial");
}
