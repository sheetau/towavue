use super::*;

fn controls(scale: f32, maximized: bool) -> Vec<CaptionButton> {
    let top = if maximized { 8.0 } else { 0.0 };
    [
        CaptionAction::Minimize,
        CaptionAction::ToggleMaximize,
        CaptionAction::Close,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, action)| CaptionButton {
        action,
        label: "Caption control",
        bounds: Rect::from_min_size(egui::pos2(index as f32 * 36.0, top), egui::vec2(36.0, 32.0))
            * scale,
        enabled: true,
        hovered: action == CaptionAction::Close,
        pressed: false,
        maximized,
    })
    .collect()
}

fn frame(context: &egui::Context, density: f32, maximized: bool) -> egui::FullOutput {
    let mut input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(128.0, 52.0))),
        ..Default::default()
    };
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(density);
    context.run_ui(input, |ui| {
        caption_controls(ui, &controls(density, maximized));
    })
}

#[test]
fn caption_icons_use_the_segoe_glyphs_and_fill_the_toolbar_at_each_density() {
    for density in [1.0, 1.25, 2.0] {
        let context = crate::fonts::test_context();
        crate::fonts::install(&context);
        for maximized in [false, true] {
            let _ = frame(&context, density, maximized);
            let output = frame(&context, density, maximized);
            let texts: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text),
                    _ => None,
                })
                .collect();
            assert_eq!(texts.len(), 3, "installed Windows caption family");
            let expected = if maximized {
                [0xe921, 0xe923, 0xe8bb]
            } else {
                [0xe921, 0xe922, 0xe8bb]
            };
            for ((text, codepoint), button) in
                texts.iter().zip(expected).zip(controls(density, maximized))
            {
                assert_eq!(
                    text.galley.text(),
                    char::from_u32(codepoint).expect("glyph").to_string()
                );
                let font = &text.galley.job.sections[0].format.font_id;
                assert_eq!(font.size, 10.0);
                assert_eq!(
                    font.family,
                    egui::FontFamily::Name("windows-caption".into())
                );
                assert!(
                    text.visual_bounding_rect()
                        .center()
                        .distance((button.bounds / density).center())
                        < 2.0
                );
            }
            let fill = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect) if rect.fill == Color32::from_rgb(232, 17, 32) => {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("Zed close hover");
            assert_eq!(fill, controls(density, maximized)[2].bounds / density);
        }
    }
}

#[test]
#[ignore = "requires hidden hardware D3D11; generated caption pixel readbacks"]
fn caption_controls_reach_gpu_through_the_last_toolbar_pixel() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(root) = crate::tests::isolated_test_root(
        "chrome::caption::tests::caption_controls_reach_gpu_through_the_last_toolbar_pixel",
    ) else {
        return;
    };
    struct Trial {
        root: std::path::PathBuf,
        complete: bool,
    }
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            let window = event_loop
                .create_window(winit::window::Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = towavue_runtime_windows::FrameRenderer::new(&window).expect("D3D11");
            let output_dir = std::env::var_os("TOWAVUE_REFERENCE_RENDER_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| self.root.clone());
            for density in [1.0, 1.25, 2.0] {
                let context = crate::fonts::test_context();
                crate::fonts::install(&context);
                let width = (128.0 * density) as u32;
                let height = (52.0 * density) as u32;
                renderer.resize_surface(width, height).expect("surface");
                for maximized in [false, true] {
                    for _ in 0..2 {
                        let output = frame(&context, density, maximized);
                        renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
                        renderer.render_ui(&context, output).expect("UI");
                    }
                    let pixels = renderer.verification_surface_rgba().expect("readback");
                    for button in controls(density, maximized) {
                        let mut glyph_pixels = 0;
                        for y in button.bounds.top() as usize..button.bounds.bottom() as usize {
                            for x in button.bounds.left() as usize..button.bounds.right() as usize {
                                let offset = (y * width as usize + x) * 4;
                                let rgb = &pixels[offset..offset + 3];
                                if rgb.iter().all(|value| *value > 160) {
                                    glyph_pixels += 1;
                                }
                            }
                        }
                        assert!(
                            glyph_pixels >= 2,
                            "visible caption glyph: {:?}",
                            button.action
                        );
                    }
                    let top = if maximized { 8.0 } else { 0.0 };
                    let x = (73.0 * density) as usize;
                    for y in [
                        (top * density) as usize,
                        ((top + 32.0) * density) as usize - 1,
                    ] {
                        let offset = (y * width as usize + x) * 4;
                        assert_eq!(
                            &pixels[offset..offset + 3],
                            &[232, 17, 32],
                            "full-height hover including last pixel"
                        );
                    }
                    std::fs::write(
                        output_dir.join(format!("caption-{maximized}-{width}x{height}.rgba")),
                        pixels,
                    )
                    .expect("readback file");
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
    let event_loop = winit::event_loop::EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop");
    let mut trial = Trial {
        root,
        complete: false,
    };
    event_loop.run_app(&mut trial).expect("run");
    assert!(trial.complete);
}
