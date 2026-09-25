use super::*;
use crate::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "requires hardware D3D11; generated HUD and timeline pixels in a hidden owned window"]
fn native_hud_caps_and_cti_slopes_are_smooth_and_symmetric() {
    let Some(_root) = crate::tests::isolated_test_root(
        "volume_hud::pixel_tests::native_hud_caps_and_cti_slopes_are_smooth_and_symmetric",
    ) else {
        return;
    };
    struct Trial {
        completed: bool,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = FrameRenderer::new(&window).expect("D3D11");
            let mut old_asymmetry = 0;
            let mut old_cap_rise = 0;
            for density in [1.0, 1.25, 1.5, 2.0] {
                for scene in ["old-shadow", "new-shadow", "new-hud", "old-cti", "new-cti"] {
                    for horizontal in [false, true] {
                        if scene.ends_with("cti") && horizontal {
                            continue;
                        }
                        let context = fonts::test_context();
                        context.global_style_mut(chrome::style);
                        let side = (320.0 * density) as u32;
                        renderer.resize_surface(side, side).expect("surface");
                        let viewport = Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 320.0));
                        let media = horizontal.then(|| viewport.with_min_y(80.0));
                        let (track, _) = geometry(viewport, media, 0.0);
                        let mut tabs = towavue_core::TabSet::default();
                        let tab = tabs.open_new("generated.mp4".into(), MediaKind::Video);
                        let start = Instant::now();
                        let mut hud = Hud::default();
                        hud.changed(tab, 1, start);
                        let output = context.run_ui(
                            egui::RawInput {
                                time: Some(1.0),
                                screen_rect: Some(viewport),
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
                            |ui| match scene {
                                "old-shadow" => {
                                    ui.painter().add(
                                        egui::epaint::Shadow {
                                            offset: [0, 0],
                                            blur: 12,
                                            spread: 1,
                                            color: Color32::from_black_alpha(80),
                                        }
                                        .as_shape(track, 2),
                                    );
                                }
                                "new-shadow" => paint_shadow(ui.painter(), track),
                                "new-hud" => {
                                    hud.show(ui, Some((tab, 1)), media, 0.0, start + FADE);
                                }
                                "old-cti" => {
                                    let x = (100.0 * density).floor() / density + 0.5 / density;
                                    let mut mesh = egui::Mesh::default();
                                    for point in [
                                        pos2(x - 4.5, 40.0),
                                        pos2(x + 4.5, 40.0),
                                        pos2(x + 4.5, 46.0),
                                        pos2(x + 0.5, 50.0),
                                        pos2(x - 0.5, 50.0),
                                        pos2(x - 4.5, 46.0),
                                    ] {
                                        mesh.colored_vertex(point, chrome::FOREGROUND);
                                    }
                                    for i in 1..5 {
                                        mesh.add_triangle(0, i, i + 1);
                                    }
                                    ui.painter().add(mesh);
                                }
                                "new-cti" => {
                                    let response = ui.interact(
                                        Rect::from_min_size(pos2(20.0, 40.0), vec2(160.0, 100.0)),
                                        "cti-pixels".into(),
                                        egui::Sense::click_and_drag(),
                                    );
                                    let result = crate::time_selection::show(
                                        ui,
                                        &response,
                                        MediaTime::from_nanoseconds(10_000_000_000),
                                        MediaTime::from_nanoseconds(5_000_000_000),
                                        None,
                                        None,
                                        true,
                                    );
                                    assert!(
                                        result.seek.is_none()
                                            && result.selection.is_none()
                                            && result.edit.is_none()
                                    );
                                }
                                _ => unreachable!(),
                            },
                        );
                        renderer
                            .clear(if scene.ends_with("cti") {
                                [0.0, 0.0, 0.0, 1.0]
                            } else {
                                [0.7, 0.7, 0.7, 1.0]
                            })
                            .expect("clear");
                        renderer.render_ui(&context, output).expect("render");
                        let pixels = renderer.verification_surface_rgba().expect("pixels");
                        renderer.present_surface().expect("Present");
                        if let Some(folder) = std::env::var_os("TOWAVUE_CHROME_REFERENCE_DIR") {
                            std::fs::write(
                                PathBuf::from(folder)
                                    .join(format!("{scene}-{horizontal}-{side}.rgba")),
                                &pixels,
                            )
                            .expect("generated reference");
                        }
                        let sample = |x: usize, y: usize| pixels[(y * side as usize + x) * 4];
                        if scene.ends_with("cti") {
                            let center = (100.0 * density).floor() as usize;
                            let mut difference = 0;
                            let mut lit = 0;
                            for y in (40.0 * density) as usize..(50.0 * density).ceil() as usize {
                                for dx in 1..=(5.0 * density).ceil() as usize {
                                    let left = sample(center - dx, y);
                                    let right = sample(center + dx, y);
                                    difference = difference.max(left.abs_diff(right));
                                    lit += usize::from(left > 0 || right > 0);
                                }
                            }
                            assert!(lit > 10, "visible head");
                            if scene == "new-cti" {
                                assert!(difference <= 2, "CTI symmetry at {density}: {difference}");
                            } else {
                                old_asymmetry = old_asymmetry.max(difference);
                            }
                        } else if scene.ends_with("shadow") {
                            let axis = usize::from(!horizontal);
                            let mut maximum_rise = 0;
                            for end in [false, true] {
                                let low = (track.min[axis] - 7.0) * density;
                                let high = (track.max[axis] + 7.0) * density;
                                let middle = track.center()[axis] * density;
                                let transverse =
                                    (track.center()[1 - axis] * density).floor() as usize;
                                let count = ((high - low) * 0.5).floor() as usize;
                                let mut darkest = u8::MAX;
                                for step in 0..count {
                                    let along = if end {
                                        high.floor() as usize - 1 - step
                                    } else {
                                        low.floor() as usize + step
                                    };
                                    if (along as f32 - middle).abs() < 2.0 {
                                        break;
                                    }
                                    let value = if horizontal {
                                        sample(along, transverse)
                                    } else {
                                        sample(transverse, along)
                                    };
                                    maximum_rise = maximum_rise.max(value.saturating_sub(darkest));
                                    darkest = darkest.min(value);
                                }
                            }
                            if scene == "new-shadow" {
                                assert!(
                                    maximum_rise <= 2,
                                    "shadow cap at {density}, horizontal={horizontal}: {maximum_rise}"
                                );
                            } else {
                                old_cap_rise = old_cap_rise.max(maximum_rise);
                            }
                        }
                    }
                }
            }
            assert!(
                old_asymmetry > 2,
                "unfeathered control must detect asymmetric diagonals"
            );
            assert!(
                old_cap_rise > 2,
                "generic thin-rectangle blur must detect the dark cap artifacts"
            );
            eprintln!(
                "PASS generated GPU caps/slopes at four densities; old CTI asymmetry={old_asymmetry}, old shadow cap rise={old_cap_rise}. Hidden readback, not physical appearance approval."
            );
            self.completed = true;
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial { completed: false };
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("GPU trial");
    assert!(trial.completed);
}
