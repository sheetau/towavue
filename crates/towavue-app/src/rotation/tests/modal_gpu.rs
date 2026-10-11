use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "requires hidden hardware D3D11; exports modal layout readbacks"]
fn editing_modals_reach_gpu_at_three_densities() {
    let Some(_root) = crate::tests::isolated_test_root(
        "rotation::tests::modal_gpu::editing_modals_reach_gpu_at_three_densities",
    ) else {
        return;
    };
    struct Trial {
        complete: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = FrameRenderer::new(&window).expect("D3D11");
            for density in [1.0, 1.25, 2.0] {
                for size in [egui::vec2(960.0, 600.0), egui::vec2(320.0, 240.0)] {
                    for phase in ["resize", "rotate", "audio", "speed"] {
                        let (mut app, _events) = application();
                        let context = app.ui_context.clone().expect("context");
                        context.global_style_mut(chrome::style);
                        if phase == "audio" || phase == "speed" {
                            let path = PathBuf::from("modal.wav");
                            let tab = app.tabs.open_new(path.clone(), MediaKind::Audio);
                            app.path = Some(path);
                            app.media_kind = Some(MediaKind::Audio);
                            app.image = None;
                            app.state = PlaybackState::Paused;
                            app.timeline_open = true;
                            app.media_duration = Some(Duration::from_secs(10));
                            app.audio_export_settings.insert(
                                tab,
                                AudioExportOptions {
                                    normalization:
                                        towavue_runtime_windows::AudioNormalization::Loudness(
                                            Default::default(),
                                        ),
                                    ..Default::default()
                                },
                            );
                        }
                        app.dispatch(match phase {
                            "resize" => CommandId::ResizeImage,
                            "rotate" => CommandId::FreeRotateImage,
                            "audio" => CommandId::AudioExportOptions,
                            _ => CommandId::EditSpeed,
                        });
                        if phase == "resize" {
                            app.resize_dialog = Some(resize::ResizeDialog::new((640, 480), 1));
                        }
                        assert!(app.modal_input_blocked(), "{phase}");
                        let width = (size.x * density) as u32;
                        let height = (size.y * density) as u32;
                        renderer.resize_surface(width, height).expect("surface");
                        for frame in 0..4 {
                            let mut input = egui::RawInput {
                                time: Some(frame as f64 * 0.25),
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    size,
                                )),
                                ..Default::default()
                            };
                            input
                                .viewports
                                .get_mut(&egui::ViewportId::ROOT)
                                .expect("viewport")
                                .native_pixels_per_point = Some(density);
                            let output =
                                context.run_ui(input, |ui| app.draw_ui(ui, &mut Vec::new()));
                            if frame == 3 {
                                crate::localization::test_ui::visible_button(
                                    &output,
                                    "Apply",
                                    size,
                                    phase != "speed",
                                );
                                crate::localization::test_ui::visible_button(
                                    &output, "Cancel", size, true,
                                );
                            }
                            renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
                            renderer.render_ui(&context, output).expect("render");
                            if frame == 3 {
                                let pixels =
                                    renderer.verification_surface_rgba().expect("readback");
                                assert_eq!(pixels.len(), (width * height * 4) as usize);
                                if let Some(directory) =
                                    std::env::var_os("TOWAVUE_REFERENCE_RENDER_DIR")
                                {
                                    std::fs::write(
                                        PathBuf::from(directory)
                                            .join(format!("modal-{phase}-{width}x{height}.rgba")),
                                        pixels,
                                    )
                                    .expect("artifact");
                                }
                            }
                            renderer.present_surface().expect("Present");
                        }
                    }
                }
            }
            self.complete = true;
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial { complete: false };
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("trial");
    assert!(trial.complete);
}
