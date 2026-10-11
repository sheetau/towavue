use super::*;

fn cascade(ui: &mut Ui, level: usize, layers: &mut Vec<(egui::LayerId, Rect)>) {
    ui.set_width(110.0);
    let button = ui.button(format!("Level {level}"));
    if level < 3 {
        let id = egui::containers::menu::SubMenu::id_from_widget_id(button.id);
        egui::containers::menu::MenuState::mark_shown(ui.ctx(), id);
        egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item = Some(id));
        if let Some(popup) = submenu(ui, &button, None, |ui| cascade(ui, level + 1, layers)) {
            layers.push((popup.response.layer_id, popup.response.rect));
        }
    }
    remember_menu_bounds(ui);
}

#[test]
fn cascades_stay_above_their_parents_on_both_sides_after_parent_promotion() {
    for density in [1.0, 1.25, 2.0] {
        for x in [16.0, 340.0] {
            let context = crate::fonts::test_context();
            context.global_style_mut(|visuals| {
                style(visuals);
                visuals.animation_time = 0.0;
            });
            context.set_pixels_per_point(density);
            let mut layers = Vec::new();
            for pass in 0..8 {
                layers.clear();
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(480.0, 300.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        let opener = ui.button("Open");
                        let popup = egui::Popup::menu(&opener)
                            .style(menu_style)
                            .at_position(egui::pos2(x, 80.0))
                            .open(true)
                            .show(|ui| {
                                if pass >= 4 {
                                    ui.ctx().move_to_top(ui.layer_id());
                                }
                                cascade(ui, 0, &mut layers);
                            })
                            .expect("test fixture");
                        layers.push((popup.response.layer_id, popup.response.rect));
                    },
                );
                if pass >= 5 {
                    assert_eq!(layers.len(), 4);
                    // Descendants are collected first. At every overlap, the
                    // deepest visible popup must own both painting and input.
                    for pair in layers.windows(2) {
                        let overlap = pair[0].1.intersect(pair[1].1);
                        assert!(overlap.is_positive(), "shared border: {layers:?}");
                        let point = overlap.center();
                        let expected = layers
                            .iter()
                            .find(|(_, rect)| rect.contains(point))
                            .expect("test fixture")
                            .0;
                        assert_eq!(context.layer_id_at(point), Some(expected), "{layers:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn menu_rows_have_symmetric_padding_unclipped_corners_and_square_check_slots() {
    for density in [1.0, 1.25, 2.0] {
        for checkbox in [false, true] {
            let context = crate::fonts::test_context();
            context.global_style_mut(|visuals| {
                style(visuals);
                visuals.animation_time = 0.0;
            });
            context.set_pixels_per_point(density);
            let mut row = Rect::NOTHING;
            let mut frame = Rect::NOTHING;
            let mut output = egui::FullOutput::default();
            for _ in 0..5 {
                output = context.run_ui(egui::RawInput::default(), |ui| {
                    let opener = ui.button("Open");
                    frame = egui::Popup::menu(&opener)
                        .style(menu_style)
                        .open(true)
                        .show(|ui| {
                            let rows = MenuRows::new(ui);
                            let response = if checkbox {
                                menu_checkbox(ui, &mut true, "English")
                            } else {
                                ui.button("English")
                            };
                            response.request_focus();
                            row = response.rect;
                            rows.finish(ui, &[response.id]);
                        })
                        .expect("test fixture")
                        .response
                        .rect;
                });
            }
            assert!(
                ((row.left() - frame.left()) - (frame.right() - row.right())).abs()
                    <= 1.0 / density
            );
            let highlight = output.shapes.iter().find(|shape| {
                matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.rect == row && rect.fill == HOVER)
            });
            if checkbox {
                assert!(highlight.is_none());
            } else {
                assert!(
                    highlight
                        .expect("row highlight")
                        .clip_rect
                        .contains_rect(row)
                );
            }
            if checkbox {
                let check = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Path(path) if path.points.len() == 3 => Some(path),
                        _ => None,
                    })
                    .expect("test fixture");
                assert!((check.points[1].x - row.left() - row.height() * 0.5).abs() < 0.1);
                let text = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == "English" => Some(text),
                        _ => None,
                    })
                    .expect("test fixture");
                assert!((text.pos.x - row.left() - row.height() - 6.0).abs() <= 1.0 / density);
                assert!(
                    (row.right() - text.pos.x - text.galley.size().x - 6.0).abs() <= 1.0 / density
                );
            }
        }
    }
}

#[test]
#[ignore = "requires hidden hardware D3D11; optional menu readbacks"]
fn menus_reach_gpu_at_supported_densities() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let Some(_root) = crate::tests::isolated_test_root(
        "chrome::menu_tests::menus_reach_gpu_at_supported_densities",
    ) else {
        return;
    };
    struct Trial(bool);
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            let window = event_loop
                .create_window(winit::window::Window::default_attributes().with_visible(false))
                .expect("test fixture");
            let mut renderer =
                towavue_runtime_windows::FrameRenderer::new(&window).expect("test fixture");
            for density in [1.0, 1.25, 2.0] {
                let width = (480.0 * density) as u32;
                let height = (300.0 * density) as u32;
                renderer
                    .resize_surface(width, height)
                    .expect("test fixture");
                for mode in 0..4 {
                    let context = crate::fonts::test_context();
                    context.set_pixels_per_point(density);
                    context.global_style_mut(style);
                    for pass in 0..8 {
                        let output = context.run_ui(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(480.0, 300.0),
                                )),
                                time: Some(f64::from(pass) * 0.25),
                                ..Default::default()
                            },
                            |ui| {
                                let opener = ui.button("Open");
                                egui::Popup::menu(&opener)
                                    .style(menu_style)
                                    .open(true)
                                    .at_position(egui::pos2(
                                        if mode == 1 { 340.0 } else { 16.0 },
                                        50.0,
                                    ))
                                    .show(|ui| match mode {
                                        0 | 1 => cascade(ui, 0, &mut Vec::new()),
                                        2 => {
                                            let rows = MenuRows::new(ui);
                                            let first = menu_checkbox(ui, &mut true, "English");
                                            first.request_focus();
                                            let second = menu_checkbox(ui, &mut false, "Japanese");
                                            rows.finish(ui, &[first.id, second.id]);
                                        }
                                        _ => {
                                            let tabs = towavue_core::TabSet::default();
                                            crate::tab_menu::show(
                                                ui,
                                                &tabs,
                                                tabs.gallery().expect("test fixture"),
                                                true,
                                                &crate::shortcuts::defaults(),
                                                None,
                                            );
                                        }
                                    });
                            },
                        );
                        renderer
                            .clear([0.18, 0.25, 0.32, 1.0])
                            .expect("test fixture");
                        renderer.render_ui(&context, output).expect("test fixture");
                        if pass == 7 {
                            let pixels =
                                renderer.verification_surface_rgba().expect("test fixture");
                            assert_eq!(pixels.len(), (width * height * 4) as usize);
                            if let Some(directory) =
                                std::env::var_os("TOWAVUE_REFERENCE_RENDER_DIR")
                            {
                                std::fs::write(
                                    std::path::PathBuf::from(directory)
                                        .join(format!("menu-{mode}-{width}x{height}.rgba")),
                                    pixels,
                                )
                                .expect("test fixture");
                            }
                        }
                        renderer.present_surface().expect("test fixture");
                    }
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
        .expect("test fixture")
        .run_app(&mut trial)
        .expect("test fixture");
    assert!(trial.0);
}
