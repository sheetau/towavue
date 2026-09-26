use super::*;

// An authored 4x2 translucent white PGS object at (20, 40) on a 64x48
// canvas, visible from two to three seconds. Exercise real decode and upload.
fn fixture(path: &Path) {
    fn segment(bytes: &mut Vec<u8>, time: u32, kind: u8, payload: &[u8]) {
        bytes.extend_from_slice(b"PG");
        bytes.extend_from_slice(&(time * 90_000).to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.push(kind);
        bytes.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        bytes.extend_from_slice(payload);
    }
    let mut bytes = Vec::new();
    segment(
        &mut bytes,
        2,
        0x16,
        &[
            0, 64, 0, 48, 0x10, 0, 0, 0x80, 0, 0, 1, 0, 0, 0, 0, 0, 20, 0, 40,
        ],
    );
    segment(&mut bytes, 2, 0x17, &[1, 0, 0, 0, 0, 0, 0, 64, 0, 48]);
    segment(&mut bytes, 2, 0x14, &[0, 0, 1, 235, 128, 128, 128]);
    segment(
        &mut bytes,
        2,
        0x15,
        &[
            0, 0, 0, 0xc0, 0, 0, 16, 0, 4, 0, 2, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0,
        ],
    );
    segment(&mut bytes, 2, 0x80, &[]);
    segment(&mut bytes, 3, 0x16, &[0, 64, 0, 48, 0x10, 0, 1, 0, 0, 0, 0]);
    segment(&mut bytes, 3, 0x80, &[]);
    std::fs::write(path, bytes).expect("PGS fixture");
}

#[test]
#[ignore = "requires hidden hardware D3D11; reads generated subtitle pixels only"]
fn bitmap_subtitles_reach_gpu_with_alpha_canvas_timing_and_texture_retirement() {
    let Some(root) = crate::tests::isolated_test_root(
        "subtitles::paint::gpu_tests::bitmap_subtitles_reach_gpu_with_alpha_canvas_timing_and_texture_retirement",
    ) else {
        return;
    };
    let path = root.join("captions.sup");
    fixture(&path);
    let document = Arc::new(
        towavue_runtime_windows::read_subtitles(
            &MediaInput::new(path),
            None,
            &towavue_runtime_windows::Cancellation::default(),
        )
        .expect("decoded PGS"),
    );
    assert_eq!(document.cues().len(), 1);
    struct Trial(Arc<SubtitleDocument>, bool);
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = FrameRenderer::new(&window).expect("D3D11");
            for density in [1.0, 1.25, 2.0] {
                let context = fonts::test_context();
                let width = (640.0 * density) as u32;
                let height = (480.0 * density) as u32;
                renderer.resize_surface(width, height).expect("surface");
                let viewport =
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
                let mut cache = Cache::default();
                let mut previous_texture = None;
                for (time, delay, visible, reset) in [
                    (1500, 0, false, false),
                    (2500, 0, true, false),
                    (2600, 0, true, false),
                    (3000, 0, false, false),
                    (2900, -2, false, false),
                    (3100, 2, true, false),
                    (2500, 0, true, true),
                ] {
                    if reset {
                        cache = Cache::default();
                    }
                    let mut input = egui::RawInput {
                        screen_rect: Some(viewport),
                        ..Default::default()
                    };
                    input
                        .viewports
                        .get_mut(&egui::ViewportId::ROOT)
                        .expect("viewport")
                        .native_pixels_per_point = Some(density);
                    let output = context.run_ui(input, |ui| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ui, |ui| {
                                cache.show(
                                    ui,
                                    viewport,
                                    &self.0,
                                    MediaTime::from_nanoseconds(time * 1_000_000),
                                    SubtitleDelay::from_tenths(delay),
                                );
                            });
                    });
                    assert_eq!(context.pixels_per_point(), density);
                    assert_eq!(cache.images.len(), usize::from(visible));
                    let texture = cache.images.get(&0).map(|images| images[0].id());
                    if let Some(texture) = texture {
                        let uploaded = output
                            .textures_delta
                            .set
                            .iter()
                            .any(|(id, _)| *id == texture);
                        assert_eq!(
                            uploaded,
                            Some(texture) != previous_texture,
                            "upload only on activation/reset"
                        );
                    }
                    if let Some(old) = previous_texture.filter(|old| Some(*old) != texture) {
                        assert!(
                            output.textures_delta.free.contains(&old),
                            "inactive/reset texture released"
                        );
                    }
                    previous_texture = texture;
                    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
                    renderer
                        .render_ui(&context, output)
                        .expect("subtitle upload and paint");
                    let pixels = renderer.verification_surface_rgba().expect("GPU pixels");
                    renderer.present_surface().expect("present hidden surface");
                    let sample = |x: f32, y: f32| {
                        let offset = (((y * density) as usize) * width as usize
                            + (x * density) as usize)
                            * 4;
                        &pixels[offset..offset + 3]
                    };
                    // Canvas scaling is exactly 10x in logical coordinates.
                    for (x, y) in [(210.0, 405.0), (230.0, 415.0)] {
                        let expected = if visible { 128 } else { 0 };
                        assert!(
                            sample(x, y)
                                .iter()
                                .all(|channel| channel.abs_diff(expected) <= 2),
                            "alpha/canvas at {density}x ({x},{y}): {:?}",
                            sample(x, y)
                        );
                    }
                    for (x, y) in [
                        (190.0, 410.0),
                        (250.0, 410.0),
                        (220.0, 390.0),
                        (220.0, 430.0),
                    ] {
                        assert_eq!(sample(x, y), [0, 0, 0], "no pixels outside authored object");
                    }
                }
            }
            self.1 = true;
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: winit::window::WindowId,
            _: WindowEvent,
        ) {
        }
    }
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    let mut trial = Trial(document, false);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("hidden GPU trial");
    assert!(trial.1);
}
