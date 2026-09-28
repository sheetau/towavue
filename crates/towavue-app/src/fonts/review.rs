use super::*;
use crate::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "renders generated font review panels into an explicit local directory using hidden D3D11"]
fn render_local_font_comparison() {
    let Some(_) = crate::tests::isolated_test_root("fonts::review::render_local_font_comparison")
    else {
        return;
    };
    let directory = PathBuf::from(
        std::env::var_os("TOWAVUE_FONT_REVIEW_DIR").expect("explicit output directory"),
    );
    assert!(directory.is_absolute() && directory.is_dir());
    struct Trial {
        directory: PathBuf,
        completed: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = FrameRenderer::new(&window).expect("D3D11");
            renderer.resize_surface(960, 640).expect("surface");
            for baseline in [true, false] {
                let context = egui::Context::default();
                install(&context);
                if baseline {
                    let mut fonts = INSTALLED.get().expect("installed fonts").0.clone();
                    fonts.font_data.insert(
                        "ui-primary".into(),
                        egui::FontData::from_static(include_bytes!(
                            "../../assets/fonts/Figtree-Tabular.ttf"
                        ))
                        .into(),
                    );
                    context.set_fonts(fonts);
                }
                context.global_style_mut(chrome::style);
                let mut app = Application::new(None, |_| {}).expect("app");
                app.ui_context = Some(context.clone());
                app.tabs
                    .open_new("Landscape 0123456789.png".into(), MediaKind::Image);
                app.tabs
                    .open_new("Concert 2026.wav".into(), MediaKind::Audio);
                app.tabs
                    .open_new("Morning light.mp4".into(), MediaKind::Video);
                let first = app.tabs.tabs()[0].id;
                app.tabs.activate(first);
                app.path = Some("Landscape 0123456789.png".into());
                app.media_kind = Some(MediaKind::Image);
                app.tabs.close_gallery(app.tabs.gallery().expect("gallery"));
                for _ in 0..4 {
                    let output = context.run_ui(egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0))),
                        ..Default::default()
                    }, |ui| {
                        app.draw_top_bar(ui, &mut Vec::new());
                        let mut samples = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(egui::pos2(400.0, 80.0), egui::pos2(930.0, 600.0))));
                        samples.heading(if baseline { "Figtree" } else { "Local font trial" });
                        samples.add_space(16.0);
                        for size in [11.0, 12.0, 14.0, 20.0] {
                            samples.label(egui::RichText::new("Landscape 0123456789.png").size(size));
                            samples.label(egui::RichText::new("00:11.11 / 88:88.88   100%   2x").size(size));
                            samples.add_space(12.0);
                        }
                        samples.label("Japanese: \u{65e5}\u{672c}\u{8a9e}  \u{753b}\u{50cf} \u{52d5}\u{753b} \u{97f3}\u{58f0}");
                        samples.label("Greek: \u{391}\u{392}\u{393} \u{3b1}\u{3b2}\u{3b3}   Cyrillic: \u{410}\u{411}\u{412} \u{430}\u{431}\u{432}");
                        let opener = ui.button("File");
                        egui::Popup::menu(&opener).open(true).show(|ui| {
                            menu::show_section_with_recent(ui, app.command_context(), &app.shortcuts, Some(menu::Section::File), &mut menu::MenuData::default());
                        });
                    });
                    renderer.clear([0.11, 0.11, 0.11, 1.0]).expect("clear");
                    renderer.render_ui(&context, output).expect("render");
                }
                let pixels = renderer.verification_surface_rgba().expect("pixels");
                assert_eq!(pixels.len(), 960 * 640 * 4);
                std::fs::write(
                    self.directory.join(if baseline {
                        "figtree-960x640.rgba"
                    } else {
                        "trial-960x640.rgba"
                    }),
                    pixels,
                )
                .expect("generated review panel");
            }
            self.completed = true;
            eprintln!(
                "PASS generated font comparison: actual tab/menu widgets and multilingual samples, hidden D3D11 rendering"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial {
        directory,
        completed: false,
    };
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("font review");
    assert!(trial.completed);
}
