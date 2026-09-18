use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "requires hidden hardware D3D11; exports generated settings UI readbacks"]
fn keyboard_settings_ui_reaches_gpu_at_three_densities() {
    let Some(root) = crate::tests::isolated_test_root(
        "keyboard_settings::tests::gpu::keyboard_settings_ui_reaches_gpu_at_three_densities",
    ) else {
        return;
    };
    struct Trial {
        root: PathBuf,
        complete: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = FrameRenderer::new(&window).expect("hardware D3D11");
            let output = std::env::var_os("TOWAVUE_KEYBOARD_RENDER_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| self.root.clone());
            std::fs::create_dir_all(&output).expect("settings GPU fixture");
            for density in [1.0, 1.25, 2.0] {
                let context = fonts::test_context();
                context.global_style_mut(chrome::style);
                let mut app = Application::new(None, |_| {}).expect("settings GPU fixture");
                app.ui_context = Some(context.clone());
                app.dispatch(CommandId::OpenKeyboardSettings);
                for (phase_index, phase) in
                    ["list", "search", "commands", "files", "folders", "edit"]
                        .into_iter()
                        .enumerate()
                {
                    if phase == "search" {
                        app.keyboard_settings.query = "reading".into();
                    }
                    if phase == "commands" {
                        app.recent_commands = vec![CommandId::OpenFile, CommandId::OpenFolder];
                        app.dispatch(CommandId::ToggleCommandPalette);
                    }
                    if phase == "files" || phase == "folders" {
                        app.recent_paths = vec![
                            self.root.join("Example media.png"),
                            self.root.join("Another image.png"),
                        ];
                        app.recent_folders = vec![
                            self.root.join("Example media folder"),
                            self.root.join("Another folder"),
                        ];
                        app.palette.open_files(phase == "folders");
                        app.palette_open = true;
                    }
                    if phase == "edit" {
                        app.cancel_command_overlay();
                        app.keyboard_settings.begin_edit(
                            CommandId::OpenFile,
                            Some(0),
                            &app.shortcuts,
                        );
                        app.keyboard_settings
                            .capture("Ctrl+K".parse().expect("valid test key"));
                        app.keyboard_settings
                            .capture("Ctrl+S".parse().expect("valid test key"));
                    }
                    let width = (900.0 * density) as u32;
                    let height = (600.0 * density) as u32;
                    renderer
                        .resize_surface(width, height)
                        .expect("settings GPU fixture");
                    let mut pixels = Vec::new();
                    for frame_index in 0..3 {
                        let mut input = egui::RawInput {
                            time: Some((phase_index * 3 + frame_index) as f64 * 0.25),
                            max_texture_side: Some(renderer.max_texture_side()),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(900.0, 600.0),
                            )),
                            ..Default::default()
                        };
                        input
                            .viewports
                            .get_mut(&egui::ViewportId::ROOT)
                            .expect("settings GPU fixture")
                            .native_pixels_per_point = Some(density);
                        let mut actions = Vec::new();
                        let frame = context.run_ui(input, |ui| app.draw_ui(ui, &mut actions));
                        assert!(actions.is_empty());
                        renderer
                            .clear([0.0, 0.0, 0.0, 1.0])
                            .expect("settings GPU fixture");
                        renderer
                            .render_ui(&context, frame)
                            .expect("settings GPU fixture");
                        pixels = renderer
                            .verification_surface_rgba()
                            .expect("settings GPU fixture");
                        renderer.present_surface().expect("settings GPU fixture");
                    }
                    assert_eq!(pixels.len(), width as usize * height as usize * 4);
                    std::fs::write(
                        output.join(format!("keyboard-{phase}-{width}x{height}.rgba")),
                        &pixels,
                    )
                    .expect("settings GPU fixture");
                    assert!(
                        pixels
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .filter(|pixel| pixel[0] > 100)
                            .count()
                            > 1000,
                        "text reaches the hardware surface: {phase}, density {density}"
                    );
                }
            }
            self.complete = true;
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial {
        root,
        complete: false,
    };
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("settings GPU fixture")
        .run_app(&mut trial)
        .expect("settings GPU fixture");
    assert!(trial.complete, "native UI readback must not silently skip");
}
