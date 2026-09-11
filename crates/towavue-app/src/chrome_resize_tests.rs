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
                        || name == "00:00 / 03:00"
                        || name == "00:00"
                        || name == "100%"
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
        let clock = narrow.get("00:00").expect("compact clock");
        for pixel in 1..40 {
            let current = origins(&frame(280.0 * density + pixel as f32));
            assert_eq!(
                current.get("00:00"),
                Some(clock),
                "compact clock origin at density {density}, delta {pixel}"
            );
        }
    }
}
