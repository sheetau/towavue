use crate::*;

#[test]
fn reading_tab_hover_joins_retained_pages_without_decoding_uploading_or_activation() {
    for density in [1.0, 1.25, 2.0] {
        let mut app = Application::new(None, |_| {}).expect("application");
        let context = fonts::test_context();
        context.global_style_mut(chrome::style);
        context.set_pixels_per_point(density);
        app.ui_context = Some(context.clone());
        let paths: Vec<_> = (0..3)
            .map(|index| PathBuf::from(format!("reading-page-{index}.png")))
            .collect();
        let tab = app.tabs.open_new(paths[1].clone(), MediaKind::Image);
        app.path = Some(paths[1].clone());
        app.displayed_tab = Some(tab);
        app.media_kind = Some(MediaKind::Image);
        app.reading_mode = true;
        app.reading_settings.page_count = 3;
        app.reading_settings.first_page_count = 3;
        app.folder_snapshot = Some(FolderSnapshot {
            folder_identity: towavue_core::ShellIdentity::new(vec![]),
            folder_path: PathBuf::new(),
            items: paths
                .iter()
                .map(|path| towavue_core::FolderMediaItem {
                    identity: towavue_core::ShellIdentity::new(vec![]),
                    path: path.clone(),
                    kind: MediaKind::Image,
                })
                .collect(),
            sort_columns: vec![],
            source: FolderSnapshotSource::LiveExplorerView,
            generation: 1,
            captured_at: std::time::SystemTime::UNIX_EPOCH,
        });
        let page = |index: usize, width, height| {
            ImagePresentation::from_decoded(
                &context,
                paths[index].as_path(),
                Arc::new(DecodedImage {
                    animation_plays: 0,
                    format: "test",
                    frames: vec![towavue_runtime_windows::DecodedImageFrame {
                        width,
                        height,
                        rgba: [index as u8 * 60, 40, 60, 255].repeat((width * height) as usize),
                        delay: Duration::ZERO,
                    }],
                }),
            )
            .expect("page")
        };
        app.image = Some(page(1, 120, 80));
        app.reading_pages = vec![Ok(page(0, 80, 120)), Ok(page(2, 60, 90))];
        let ids = [
            app.reading_pages[0].as_ref().expect("first").texture.id(),
            app.image.as_ref().expect("current").texture.id(),
            app.reading_pages[1].as_ref().expect("last").texture.id(),
        ];
        let mut time = 0.0;
        let mut frame = |app: &mut Application<_>| {
            time += 0.1;
            context.run_ui(
                egui::RawInput {
                    time: Some(time),
                    focused: false,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960.0, 576.0),
                    )),
                    events: vec![egui::Event::PointerMoved(egui::pos2(100.0, 15.0))],
                    ..Default::default()
                },
                |ui| {
                    let mut actions = Vec::new();
                    app.draw_top_bar(ui, &mut actions);
                    assert!(actions.is_empty());
                },
            )
        };
        for _ in 0..3 {
            frame(&mut app);
        }
        for background in [false, true] {
            if background {
                app.tabs.open_new("other.png".into(), MediaKind::Image);
                app.retain_image_tab();
                app.path = Some("other.png".into());
                app.displayed_tab = app.tabs.active().map(|tab| tab.id);
                frame(&mut app);
            }
            for axis in [ReadingAxis::Horizontal, ReadingAxis::Vertical] {
                for reversed in [false, true] {
                    let settings = if background {
                        &mut app
                            .retained_images
                            .get_mut(&tab)
                            .expect("saved tab")
                            .reading_settings
                    } else {
                        &mut app.reading_settings
                    };
                    settings.axis = axis;
                    settings.reversed = reversed;
                    let tabs = app.tabs.clone();
                    let _ = context.tex_manager().write().take_delta();
                    let output = frame(&mut app);
                    let rects = ids.map(|id| {
                        output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Mesh(mesh) if mesh.texture_id == id => {
                                    Some(mesh.calc_bounds())
                                }
                                egui::Shape::Rect(rect) if rect.fill_texture_id() == id => {
                                    Some(rect.rect)
                                }
                                _ => None,
                            })
                            .expect("each joined page must be in the tab preview")
                    });
                    let spread = rects
                        .iter()
                        .copied()
                        .reduce(egui::Rect::union)
                        .expect("spread");
                    assert!(spread.width() <= 240.001 && spread.height() <= 160.001);
                    let ordered = if reversed {
                        [rects[2], rects[1], rects[0]]
                    } else {
                        rects
                    };
                    for pair in ordered.windows(2) {
                        let seam = match axis {
                            ReadingAxis::Horizontal => pair[0].right() - pair[1].left(),
                            ReadingAxis::Vertical => pair[0].bottom() - pair[1].top(),
                        };
                        assert!(seam.abs() < 0.001);
                    }
                    assert!((rects[1].aspect_ratio() - 1.5).abs() < 0.001);
                    assert_eq!(app.tabs, tabs);
                    assert!(app.edits.is_empty());
                    assert!(app.tab_preview.target.is_none());
                    assert!(
                        output.textures_delta.set.is_empty(),
                        "no preview pixel updates in the rendered frame"
                    );
                    assert!(
                        context.tex_manager().write().take_delta().set.is_empty(),
                        "hover must not upload a composite image"
                    );
                }
            }
        }
        let saved = app
            .retained_images
            .get_mut(&tab)
            .expect("saved reading tab");
        saved.image = None;
        saved.reading_pages[0] = Err("unreadable page".into());
        let output = frame(&mut app);
        let painted = |output: &egui::FullOutput, id| {
            output.shapes.iter().any(
                |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == id),
            )
        };
        assert!(
            painted(&output, ids[2]),
            "remaining readable page still supplies the spread preview"
        );
        assert!(!painted(&output, ids[0]) && !painted(&output, ids[1]));
        assert!(app.tab_preview.target.is_none());
        app.retained_images
            .get_mut(&tab)
            .expect("saved reading tab")
            .graphics_epoch = app.graphics_epoch.wrapping_add(1);
        assert!(
            !painted(&frame(&mut app), ids[2]),
            "stale device textures must not be shown"
        );
        assert!(app.tab_preview.target.is_some());
        app.tab_preview.clear();
    }
}
