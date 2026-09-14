use super::*;
use crate::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "requires hardware D3D11; reads only generated status text in an owned hidden surface"]
fn right_status_resize_bounds_gpu_color_drift_and_detects_pixel_displacement() {
    let Some(root) = crate::tests::isolated_test_root(
        "status_file_details::gpu_tests::right_status_resize_bounds_gpu_color_drift_and_detects_pixel_displacement",
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
            let mut comparisons = 0;
            let mut changed_frames_total = 0;
            for kind in [MediaKind::Image, MediaKind::Video, MediaKind::Audio] {
                for density in [1.0, 1.25, 1.5, 2.0] {
                    let context = fonts::test_context();
                    context.global_style_mut(chrome::style);
                    let mut app = Application::new(None, |_| {}).expect("app");
                    let path = self.root.join("resize.png");
                    app.tabs.open_new(path.clone(), kind);
                    app.path = Some(path);
                    app.media_kind = Some(kind);
                    app.state = PlaybackState::Paused;
                    app.refresh_status_file_details();
                    assert!(app.status_file_details.finish(
                        app.status_file_details.ticket,
                        Some(FileDetails {
                            bytes: 4096,
                            modified_local: Some("2024-02-29 12:34:56".into()),
                        }),
                    ));
                    let height = (320.0 * density) as u32;
                    let mut baseline: Option<Vec<u8>> = None;
                    let mut region = None;
                    let mut changed_frames = 0;
                    let mut max_changed = 0;
                    let mut max_delta = 0;
                    for delta in (0..40).chain((0..40).rev()) {
                        let width = (960.0 * density) as u32 + delta;
                        let mut input = egui::RawInput {
                            max_texture_side: Some(renderer.max_texture_side()),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width as f32 / density, 320.0),
                            )),
                            ..Default::default()
                        };
                        input
                            .viewports
                            .get_mut(&egui::ViewportId::ROOT)
                            .expect("viewport")
                            .native_pixels_per_point = Some(density);
                        let output = context.run_ui(input, |ui| {
                            app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
                        });
                        assert_eq!(context.pixels_per_point(), density);
                        let bounds = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Text(text)
                                    if text.galley.text().contains("Modified (local)") =>
                                {
                                    assert!(
                                        !text.galley.elided,
                                        "compare fixed glyphs, not ellipsis changes"
                                    );
                                    Some(
                                        text.visual_bounding_rect().intersect(shape.clip_rect)
                                            * density,
                                    )
                                }
                                _ => None,
                            })
                            .expect("right status text");
                        // Fix the crop to the initial right-edge offset, not each frame's
                        // glyph position: moving glyphs must fail this comparison.
                        let (right_offset, top, crop_width, crop_height) = *region
                            .get_or_insert_with(|| {
                                let left = (bounds.left().floor() as u32).saturating_sub(2);
                                let top = (bounds.top().floor() as u32).saturating_sub(2);
                                let right = (bounds.right().ceil() as u32 + 2).min(width);
                                let bottom = (bounds.bottom().ceil() as u32 + 2).min(height);
                                (width - left, top, right - left, bottom - top)
                            });
                        renderer
                            .resize_surface(width, height)
                            .expect("surface resize");
                        renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
                        renderer
                            .render_ui(&context, output)
                            .expect("status rendering");
                        let pixels = renderer.verification_surface_rgba().expect("GPU readback");
                        renderer.present_surface().expect("Present");
                        assert_eq!(pixels.len(), (width * height * 4) as usize);
                        let left = width - right_offset;
                        assert!(crop_width > 0 && crop_height > 0 && left + crop_width < width);
                        let crop = |left: u32| {
                            (top..top + crop_height)
                                .flat_map(|y| {
                                    let start = ((y * width + left) * 4) as usize;
                                    pixels[start..start + (crop_width * 4) as usize]
                                        .iter()
                                        .copied()
                                })
                                .collect::<Vec<_>>()
                        };
                        let current = crop(left);
                        assert!(
                            current
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .all(|pixel| pixel[3] == 255),
                            "opaque status surface"
                        );
                        if let Some(baseline) = &baseline {
                            assert_eq!(current.len(), baseline.len());
                            let changed =
                                current.iter().zip(baseline).filter(|(a, b)| a != b).count();
                            changed_frames += usize::from(changed != 0);
                            max_changed = max_changed.max(changed);
                            max_delta = max_delta.max(
                                current
                                    .iter()
                                    .zip(baseline)
                                    .map(|(a, b)| a.abs_diff(*b))
                                    .max()
                                    .unwrap_or(0),
                            );
                            comparisons += 1;
                        } else {
                            assert!(
                                current.as_chunks::<4>().0.iter().any(|pixel| pixel[0] > 64),
                                "rendered glyphs required"
                            );
                            let shifted = crop(left + 1);
                            let shift_delta = current
                                .iter()
                                .zip(&shifted)
                                .map(|(a, b)| a.abs_diff(*b))
                                .max()
                                .expect("pixels");
                            assert!(
                                shift_delta > 1,
                                "a one-pixel displacement must fail the color-drift bound"
                            );
                            baseline = Some(current);
                        }
                    }
                    eprintln!(
                        "STATUS_GPU kind={kind:?}, density={density}, changed_frames={changed_frames}/79, max_changed_bytes={max_changed}, max_channel_delta={max_delta}"
                    );
                    // Exact byte equality fails with one-code color differences as
                    // target width changes. Keep that observation visible; this is a
                    // bounded color-drift check, not byte-exact or physical-input proof.
                    assert!(
                        max_delta <= 1,
                        "kind={kind:?}, density={density}, max_channel_delta={max_delta}"
                    );
                    changed_frames_total += changed_frames;
                }
            }
            assert_eq!(comparisons, 948);
            eprintln!(
                "PASS right status GPU resize: 948 glyph-region comparisons within one 8-bit color code ({changed_frames_total} differ bytewise), opaque alpha, and 12 one-pixel-shift controls exceeding that bound; three media kinds, four densities, forward/back one-pixel surface resizing. Hidden hardware readback before Present; not byte-exact, physical resizing or DWM composition."
            );
            self.completed = true;
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
