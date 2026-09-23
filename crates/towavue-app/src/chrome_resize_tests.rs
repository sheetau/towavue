use super::*;

#[test]
fn right_edge_resize_keeps_left_aligned_text_origins_stable() {
    let Some(root) = tests::isolated_test_root(
        "chrome_resize_tests::right_edge_resize_keeps_left_aligned_text_origins_stable",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 1.5, 2.0] {
        let context = fonts::test_context();
        context.set_pixels_per_point(density);
        let mut app = Application::new(None, |_| {}).expect("headless application");
        let path = root.join("track.wav");
        app.tabs.open_new(path.clone(), MediaKind::Audio);
        app.path = Some(path.clone());
        app.media_kind = Some(MediaKind::Audio);
        app.timeline_open = true;
        app.state = PlaybackState::Paused;
        app.status_message = None;
        app.media_duration = Some(Duration::from_secs(180));
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: root.clone(),
            items: vec![towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![]),
                path,
                kind: MediaKind::Audio,
            }],
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::now(),
        });
        let mut time = 0.0;
        let mut frame = |physical_width| {
            time += 0.1;
            context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(physical_width / density, 480.0),
                    )),
                    ..Default::default()
                },
                |ui| app.draw_ui(ui, &mut Vec::new()),
            )
        };
        frame(1280.0);
        frame(1280.0);
        let origins = |output: &egui::FullOutput| {
            let mut result = BTreeMap::new();
            for shape in &output.shapes {
                if let egui::Shape::Text(text) = &shape.shape {
                    let name = text.galley.text();
                    if name == "track.wav"
                        || name == "1. track.wav"
                        || name.ends_with("\\track.wav")
                        || name == "00:00:00:000 / 00:03:00:000"
                        || name == "00:00:00:000"
                        || name == "50%"
                    {
                        result.insert(name.to_owned(), text.pos * density);
                    }
                }
            }
            result
        };
        let baseline = origins(&frame(1280.0));
        assert_eq!(
            baseline.len(),
            5,
            "tab, list, status path, time and volume: {baseline:?}"
        );
        for width in (1281..1310).chain((1270..1280).rev()).chain(1280..1290) {
            let current = origins(&frame(width as f32));
            assert_eq!(
                current, baseline,
                "right resize at density {density}, width {width}"
            );
        }
        frame(280.0 * density);
        let narrow = origins(&frame(280.0 * density));
        let clock = narrow.get("00:00:00:000").expect("compact clock");
        for pixel in 1..40 {
            let current = origins(&frame(280.0 * density + pixel as f32));
            assert_eq!(
                current.get("00:00:00:000"),
                Some(clock),
                "compact clock origin at density {density}, delta {pixel}"
            );
        }
    }
}

#[test]
fn utility_status_text_has_double_the_media_button_leading_gap() {
    let Some(_root) = tests::isolated_test_root(
        "chrome_resize_tests::utility_status_text_has_double_the_media_button_leading_gap",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    for density in [1.0, 1.25, 2.0] {
        let context = fonts::test_context();
        context.set_pixels_per_point(density);
        context.global_style_mut(chrome::style);
        for keyboard in [false, true] {
            if keyboard {
                app.tabs.open_keyboard_settings();
            } else {
                app.tabs.open_gallery();
            }
            let mut output = None;
            for _ in 0..2 {
                output = Some(context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(640.0, 400.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        app.draw_status_bar(ui, &mut Vec::new(), &mut Vec::new());
                    },
                ));
            }
            let output = output.expect("frame");
            let text = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.text().starts_with(if keyboard {
                            "Double-click a command"
                        } else {
                            "Open a file"
                        }) =>
                    {
                        Some(text)
                    }
                    _ => None,
                })
                .expect("utility status label");
            assert!(
                (text.pos.x - 2.0 * chrome::STATUS_BUTTON_GAP).abs() <= 1.0 / density,
                "utility text left gap: {:?}",
                text.pos
            );
        }
    }
}

#[test]
fn tabs_fit_until_their_actual_painted_width_reaches_the_minimum() {
    let Some(root) = tests::isolated_test_root(
        "chrome_resize_tests::tabs_fit_until_their_actual_painted_width_reaches_the_minimum",
    ) else {
        return;
    };
    for density in [1.0, 1.25, 2.0] {
        for count in [2, 4, 6] {
            let mut app = Application::new(None, |_| {}).expect("app");
            for index in 0..count {
                app.tabs
                    .open_new(root.join(format!("tab-{index}.png")), MediaKind::Image);
            }
            let context = fonts::test_context();
            context.set_pixels_per_point(density);
            context.global_style_mut(chrome::style);
            for width in [480.0, 900.0, 1400.0] {
                for tick in 0..4 {
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 400.0),
                            )),
                            time: Some(f64::from(width) + f64::from(tick)),
                            ..Default::default()
                        },
                        |ui| {
                            app.draw_top_bar(ui, &mut Vec::new());
                        },
                    );
                    if tick < 3 {
                        continue;
                    }
                    let tabs: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Rect(rect)
                                if rect.corner_radius == egui::CornerRadius::same(3)
                                    && rect.rect.width() >= 70.0
                                    && rect.rect.top() < 32.0
                                    && [chrome::BORDER, chrome::BACKGROUND]
                                        .contains(&rect.fill) =>
                            {
                                Some((rect.rect, shape.clip_rect))
                            }
                            _ => None,
                        })
                        .collect();
                    assert!(!tabs.is_empty());
                    for (rect, clip) in tabs {
                        if rect.width() > 72.0 + 1.0 / density {
                            assert!(
                                rect.left() >= clip.left() - 1.0 / density
                                    && rect.right() <= clip.right() + 1.0 / density,
                                "non-minimum tab must fit: {rect:?}, {clip:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn tab_scrollbar_fills_lower_gutter_and_keeps_title_controls_aligned() {
    let Some(root) = tests::isolated_test_root(
        "chrome_resize_tests::tab_scrollbar_fills_lower_gutter_and_keeps_title_controls_aligned",
    ) else {
        return;
    };
    for (density, inset) in [(1.0, 0.0), (1.25, 8.0), (2.0, 6.5)] {
        let mut app = Application::new(None, |_| {}).expect("app");
        for index in 0..20 {
            app.tabs
                .open_new(root.join(format!("gutter-{index}.png")), MediaKind::Image);
        }
        let context = fonts::test_context();
        context.set_pixels_per_point(density);
        context.enable_accesskit();
        context.global_style_mut(chrome::style);
        context.global_style_mut(|style| style.animation_time = 0.0);
        for tick in 0..4 {
            let output = context.run_ui(
                egui::RawInput {
                    time: Some(f64::from(tick)),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    safe_area_insets: Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
                        top: inset,
                        ..Default::default()
                    })),
                    events: vec![egui::Event::PointerMoved(egui::pos2(120.0, inset + 30.0))],
                    ..Default::default()
                },
                |ui| {
                    app.draw_top_bar(ui, &mut Vec::new());
                    assert!((ui.available_rect_before_wrap().top() - inset - 32.0).abs() < 0.01);
                },
            );
            if tick < 3 {
                continue;
            }
            let tree = output.platform_output.accesskit_update.expect("tree");
            let bar = tree
                .nodes
                .iter()
                .find(|(_, node)| node.role() == egui::accesskit::Role::ScrollBar)
                .expect("horizontal scrollbar")
                .1
                .bounds()
                .expect("bar hit region");
            assert!(((bar.y1 - bar.y0) as f32 - 3.0).abs() < 0.01, "{bar:?}");
            let border = inset + 32.0 - 1.0 / density;
            assert!(
                (bar.y1 as f32 - border).abs() <= 0.5 / density,
                "{bar:?}, border={border}"
            );
            for label in ["towavue menu", "gutter-19.png"] {
                let bounds = tree
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .expect("title control")
                    .1
                    .bounds()
                    .expect("control bounds");
                assert!(
                    ((bounds.y0 + bounds.y1) as f32 * 0.5 - inset - 16.0).abs() <= 1.0 / density,
                    "unchanged center: {label}: {bounds:?}"
                );
                assert!(
                    bounds.y1 <= bar.y0 + 0.5 / f64::from(density),
                    "separate tab and bar hit regions"
                );
            }
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.rect.width() > 5.0 && (rect.rect.height() - 3.0).abs() <= 0.5 / density
                    && (rect.rect.bottom() - border).abs() <= 0.5 / density
                    && rect.fill != chrome::BACKGROUND
            )), "painted scrollbar meets the border");
        }
    }
}
