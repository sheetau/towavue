use super::*;
use crate::fluent::Kind;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "requires hidden D3D11; optional help/icon GPU readbacks"]
fn help_labels_and_fluent_icons_reach_gpu() {
    let Some(_root) = crate::tests::isolated_test_root(
        "hover_help::visual_tests::help_labels_and_fluent_icons_reach_gpu",
    ) else {
        return;
    };
    struct Trial(bool);
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            let window = event_loop
                .create_window(winit::window::Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = towavue_runtime_windows::FrameRenderer::new(&window).expect("D3D11");
            for density in [1.0, 1.25, 2.0] {
                let context = crate::fonts::test_context();
                assert!(
                    crate::fonts::install(&context),
                    "installed Japanese UI font"
                );
                context.global_style_mut(crate::chrome::style);
                let width = (720.0 * density) as u32;
                let height = (360.0 * density) as u32;
                renderer.resize_surface(width, height).expect("surface");
                for pass in 0..3 {
                    let mut input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(720.0, 360.0),
                        )),
                        max_texture_side: Some(renderer.max_texture_side()),
                        hovered_files: vec![egui::HoveredFile {
                            path: Some("generated.srt".into()),
                            ..Default::default()
                        }],
                        events: vec![egui::Event::PointerMoved(egui::pos2(30.0, 290.0))],
                        ..Default::default()
                    };
                    input
                        .viewports
                        .entry(egui::ViewportId::ROOT)
                        .or_default()
                        .native_pixels_per_point = Some(density);
                    let output = context.run_ui(input, |ui| {
                        ui.painter().rect_filled(ui.max_rect(), 0.0, crate::chrome::BACKGROUND);
                        ui.horizontal(|ui| {
                            for text in ["HAMBURG", "May 2026", "September 2026", "2026年9月"] {
                                frame(ui.style()).show(ui, |ui| content(ui, |ui| ui.label(text)));
                            }
                        });
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            for kind in [Kind::Play, Kind::Pause, Kind::Previous, Kind::Next, Kind::Repeat,
                                Kind::RepeatOne, Kind::RepeatOff, Kind::Shuffle, Kind::ShuffleOff, Kind::Image, Kind::Video, Kind::Audio,
                                Kind::Move, Kind::New, Kind::Forbidden] {
                                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(28.0), egui::Sense::hover());
                                crate::fluent::paint(ui, rect, kind, crate::chrome::FOREGROUND);
                            }
                        });
                        ui.add_space(16.0);
                        ui.horizontal_top(|ui| {
                            for text in ["HAMBURG", "September 2026", "A long filename with four rows and an ellipsis at the end of the available caption space.png"] {
                                ui.vertical(|ui| egui::Frame::popup(ui.style()).inner_margin(0).show(ui, |ui| {
                                    ui.set_width(160.0);
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 70.0), egui::Sense::hover());
                                    ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(70, 115, 180));
                                    crate::media_preview::caption(ui, |ui| {
                                        let galley = filename(ui, text.into(), ui.available_width());
                                        ui.add(egui::Label::new(galley).show_tooltip_when_elided(false));
                                    });
                                }));
                            }
                        });
                        crate::file_drop::draw(&context, false, true);
                    });
                    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
                    renderer.render_ui(&context, output).expect("UI");
                    if pass == 2 {
                        let pixels = renderer.verification_surface_rgba().expect("readback");
                        assert!(
                            pixels
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .any(|p| p[..3] == [70, 115, 180])
                        );
                        if let Some(directory) = std::env::var_os("TOWAVUE_REFERENCE_RENDER_DIR") {
                            std::fs::write(
                                std::path::PathBuf::from(directory)
                                    .join(format!("help-icons-{width}x{height}.rgba")),
                                pixels,
                            )
                            .expect("artifact");
                        }
                    }
                    renderer.present_surface().expect("Present");
                }
            }
            self.0 = true;
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
    let mut trial = Trial(false);
    let mut builder = winit::event_loop::EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("trial");
    assert!(trial.0);
}
