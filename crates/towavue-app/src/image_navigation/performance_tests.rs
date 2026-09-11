use super::*;
use std::os::windows::process::CommandExt;

fn bitmap_fixture(path: &Path) {
    let mut bitmap = vec![0_u8; 62];
    bitmap[..2].copy_from_slice(b"BM");
    bitmap[2..6].copy_from_slice(&62_u32.to_le_bytes());
    bitmap[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bitmap[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bitmap[18..22].copy_from_slice(&2_u32.to_le_bytes());
    bitmap[22..26].copy_from_slice(&1_u32.to_le_bytes());
    bitmap[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bitmap[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bitmap[34..38].copy_from_slice(&8_u32.to_le_bytes());
    bitmap[54..].copy_from_slice(&[12, 34, 56, 12, 34, 56, 0, 0]);
    std::fs::write(path, bitmap).expect("owned bitmap");
}

#[test]
fn completed_original_is_drawn_before_its_queued_notification() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::performance_tests::completed_original_is_drawn_before_its_queued_notification",
    ) else {
        return;
    };
    let path = root.join("source.bmp");
    bitmap_fixture(&path);
    for show_preview in [false, true] {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = Application::new(None, move |event| {
            if matches!(event, AppEvent::ImagesReady) {
                let _ = tx.send(event);
            }
        })
        .expect("app");
        let context = fonts::test_context();
        app.ui_context = Some(context.clone());
        app.tabs.open_new(path.clone(), MediaKind::Image);
        // Prefetch inserts the original before its shared preview. A cache hit then
        // emits only the completion notification, so this ordering test needs no sleep guess.
        app.image_loader.prefetch(path.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        while app
            .preview_cache
            .cached_image(&path)
            .expect("preview lookup")
            .is_none()
        {
            assert!(Instant::now() < deadline, "prefetch timeout");
            std::thread::sleep(Duration::from_millis(2));
        }
        app.load_path(path.clone(), MediaKind::Image);
        let queued = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("completed original");
        assert!(app.image_loading && app.image.is_none());
        if !show_preview {
            app.ui_context = None;
            let _ = context.run_ui(Default::default(), |ui| app.draw_ui(ui, &mut Vec::new()));
            assert!(
                app.image_loading && app.image.is_none(),
                "do not drain before context setup"
            );
            app.ui_context = Some(context.clone());
        }
        if show_preview {
            app.finish_image_preview(
                path.clone(),
                app.image_preview_generation,
                towavue_runtime_windows::CachedImagePreview {
                    source_size: (2, 1),
                    image: towavue_runtime_windows::PreviewImage {
                        width: 1,
                        height: 1,
                        rgba: vec![255, 0, 0, 255],
                    },
                },
            );
        }
        let preview = app
            .image_previews
            .get(&path)
            .map(|preview| preview.texture.id());
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                ..Default::default()
            },
            |ui| app.draw_ui(ui, &mut Vec::new()),
        );
        let image = app
            .image
            .as_ref()
            .expect("ready original must precede drawing");
        let texture = image.texture.id();
        assert_eq!(image.dimensions(), (2, 1));
        assert!(!app.image_loading);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture)));
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Mesh(mesh) if Some(mesh.texture_id) == preview)));
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text().contains("Loading images"))));
        app.handle_app_event(queued);
        assert_eq!(
            app.image.as_ref().expect("original retained").texture.id(),
            texture,
            "late wakeup must not rebuild the texture"
        );
    }
}

#[test]
fn neighbor_prefetch_runs_before_current_texture_preparation() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::performance_tests::neighbor_prefetch_runs_before_current_texture_preparation",
    ) else {
        return;
    };
    let paths = [root.join("current.bmp"), root.join("next.bmp")];
    for path in &paths {
        bitmap_fixture(path);
    }
    let decoded = towavue_runtime_windows::decode_image(&paths[0]).expect("current original");
    let mut app = Application::new(None, |_| {}).expect("app");
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    app.tabs.open_new(paths[0].clone(), MediaKind::Image);
    app.path = Some(paths[0].clone());
    app.media_kind = Some(MediaKind::Image);
    app.image_loading = true;
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![]),
        folder_path: root.clone(),
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
    let previews = app.preview_cache.clone();
    assert!(previews.cached_image(&paths[1]).expect("cache").is_none());
    let next = paths[1].clone();
    let (locked, lock_ready) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        context.input_mut(|_| {
            // Texture preparation needs this lock; the independent decoder must not.
            locked.send(()).expect("context locked");
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(preview) = previews.cached_image(&next).expect("preview lookup") {
                    assert_eq!(preview.image.rgba, [56, 34, 12, 255].repeat(2));
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    });
    lock_ready
        .recv_timeout(Duration::from_secs(5))
        .expect("context lock");
    app.apply_loaded_images(towavue_runtime_windows::LoadedImages {
        generation: app.image_generation,
        first_index: 0,
        total: 1,
        images: vec![(paths[0].clone(), Ok(Arc::new(decoded)))],
    });
    assert!(
        worker.join().expect("prefetch observer"),
        "prefetch waited for texture preparation"
    );
    assert!(!app.image_loading && app.image_error.is_none());
    assert_eq!(
        app.image.as_ref().expect("current image").dimensions(),
        (2, 1)
    );
}

#[test]
#[ignore = "generates 100 large JPEGs and measures real decoding; requires FFMPEG_DIR and a Release test build"]
fn hundred_large_images_report_navigation_gaps_and_preparation_cost() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::performance_tests::hundred_large_images_report_navigation_gaps_and_preparation_cost",
    ) else {
        return;
    };
    let output = std::process::Command::new(
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe"),
    )
    .creation_flags(0x08000000)
    .args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=4096x2304:rate=30",
        "-frames:v",
        "100",
        "-c:v",
        "mjpeg",
        "-q:v",
        "3",
        "-threads",
        "2",
    ])
    .arg(root.join("image-%03d.jpg"))
    .output()
    .expect("generate owned JPEG sequence");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let paths: Vec<_> = (1..=100)
        .map(|n| root.join(format!("image-{n:03}.jpg")))
        .collect();
    let bytes: u64 = paths
        .iter()
        .map(|path| std::fs::metadata(path).expect("fixture").len())
        .sum();
    let (notify, events) = std::sync::mpsc::channel();
    let mut app = Application::new(None, move |event| {
        let _ = notify.send(event);
    })
    .expect("app");
    let context = fonts::test_context();
    app.ui_context = Some(context.clone());
    app.fullscreen = true;
    app.tabs.open_new(paths[0].clone(), MediaKind::Image);
    app.folder_snapshot = Some(FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![]),
        folder_path: root.clone(),
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
    let epoch = Instant::now();
    let draw = |app: &mut Application<_>| {
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 576.0),
                )),
                max_texture_side: Some(8192),
                time: Some(epoch.elapsed().as_secs_f64()),
                ..Default::default()
            },
            |ui| app.draw_ui(ui, &mut Vec::new()),
        );
        let target = app.image.as_ref().map(|image| image.texture.id());
        let full = target.is_some_and(|target| {
            output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == target)
        })
        });
        let preview = app
            .path
            .as_ref()
            .and_then(|path| app.image_previews.get(path));
        let preview = preview.is_some_and(|preview| output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == preview.texture.id())
        }));
        let held = app.image_handoff.as_ref().is_some_and(|held| {
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape,
                egui::Shape::Mesh(mesh) if mesh.texture_id == held.image.texture.id())
            })
        });
        (full, preview, held)
    };
    let receive = |app: &mut Application<_>, timeout| {
        let mut prepare = Duration::ZERO;
        if let Ok(event) = events.recv_timeout(timeout) {
            if matches!(event, AppEvent::ImagesReady) {
                let started = Instant::now();
                app.finish_image_load();
                prepare = started.elapsed();
            } else if matches!(event, AppEvent::ImagePreview(..)) {
                app.handle_app_event(event);
            }
        }
        prepare
    };
    draw(&mut app);
    app.load_path(paths[0].clone(), MediaKind::Image);
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.image_loading {
        assert!(Instant::now() < deadline, "initial image timeout");
        receive(&mut app, Duration::from_millis(10));
        draw(&mut app);
    }
    assert!(draw(&mut app).0);
    // A deliberate initial look allows the existing bounded neighbor worker to run.
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        receive(&mut app, Duration::from_millis(5));
    }
    for cadence in [Duration::ZERO, Duration::from_millis(33)] {
        let mut ready = Vec::new();
        let mut prepare = Vec::new();
        let mut draw_time = Vec::new();
        let mut dispatch = Vec::new();
        let mut blank_targets = 0;
        let mut preview_targets = 0;
        let mut handoff_targets = 0;
        for index in 1..=100 {
            let start = Instant::now();
            app.dispatch(CommandId::NextSameKind);
            dispatch.push(start.elapsed());
            let mut blank = false;
            let mut preview = false;
            let mut handoff = false;
            let mut preparation = Duration::ZERO;
            let mut drawing = Duration::ZERO;
            loop {
                let draw_started = Instant::now();
                let (full, low_resolution, held) = draw(&mut app);
                drawing += draw_started.elapsed();
                blank |= !full && !low_resolution && !held;
                preview |= low_resolution;
                handoff |= held;
                if full {
                    break;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(30),
                    "image {index} timeout"
                );
                preparation += receive(&mut app, Duration::from_millis(1));
            }
            ready.push(start.elapsed());
            prepare.push(preparation);
            draw_time.push(drawing);
            blank_targets += usize::from(blank);
            preview_targets += usize::from(preview);
            handoff_targets += usize::from(handoff);
            assert!(!app.image_loading && app.image_error.is_none());
            assert_eq!(app.path.as_ref(), Some(&paths[index % 100]));
            assert_eq!(
                app.image.as_ref().expect("original").dimensions(),
                (4096, 2304)
            );
            while start.elapsed() < cadence {
                receive(&mut app, Duration::from_millis(1));
            }
        }
        let median = |values: &[Duration]| {
            let mut values = values.to_vec();
            values.sort_unstable();
            values[values.len() / 2].as_secs_f64() * 1000.0
        };
        eprintln!(
            "NAV100 cadence_ms={} files_mib={:.1} full=100 blank_targets={} preview_targets={} handoff_targets={} ready_median_ms={:.3} ready_p95_ms={:.3} event_completion_median_ms={:.3} draw_total_median_ms={:.3} dispatch_median_ms={:.3}",
            cadence.as_millis(),
            bytes as f64 / 1048576.0,
            blank_targets,
            preview_targets,
            handoff_targets,
            median(&ready),
            percentile_95(&ready).as_secs_f64() * 1000.0,
            median(&prepare),
            median(&draw_time),
            median(&dispatch)
        );
    }
    eprintln!(
        "Scope: warm filesystem, synthetic Shell snapshot, serialized commands with all originals visited, CPU egui mesh/texture preparation. Event completion and total draw calls can both prepare textures; neither isolates conversion time. No GPU presentation, physical keys, dropped-key burst, peak memory or IrfanView comparison. Any blank/preview target leaves the seamless-navigation gate unmet."
    );
}
