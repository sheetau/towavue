use super::*;
use std::os::windows::process::CommandExt;

mod gpu;

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
    measure(root, None);
}

fn measure(root: PathBuf, mut renderer: Option<FrameRenderer>) {
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
    let draw = |app: &mut Application<_>, renderer: &mut Option<FrameRenderer>, verify| {
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
        let token = app.image_sequence_token(&output);
        let presented = full.then(|| app.path.clone().expect("original source"));
        let gpu_calls = if let Some(renderer) = renderer {
            gpu::submit(app, &context, output, renderer, verify)
        } else {
            Duration::ZERO
        };
        // GPU submission above includes Present; the CPU-only measurement uses a simulated ack.
        app.finish_image_sequence_frame(token);
        (full, preview, held, gpu_calls, presented)
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
    draw(&mut app, &mut renderer, false);
    app.load_path(paths[0].clone(), MediaKind::Image);
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.image_loading {
        assert!(Instant::now() < deadline, "initial image timeout");
        receive(&mut app, Duration::from_millis(10));
        draw(&mut app, &mut renderer, false);
    }
    assert!(draw(&mut app, &mut renderer, false).0);
    // A deliberate initial look allows the existing bounded neighbor worker to run.
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        receive(&mut app, Duration::from_millis(5));
    }
    let cases = [
        (Duration::ZERO, false),
        (Duration::from_millis(33), false),
        (Duration::ZERO, true),
    ];
    for (cadence, verify) in cases {
        if verify && renderer.is_none() {
            continue;
        }
        app.nearest_images = verify;
        draw(&mut app, &mut renderer, false);
        let mut memory = renderer.as_ref().map(gpu::Memory::new);
        let mut ready = Vec::new();
        let mut prepare = Vec::new();
        let mut draw_time = Vec::new();
        let mut gpu_time = Vec::new();
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
            let mut gpu_calls = Duration::ZERO;
            loop {
                let draw_started = Instant::now();
                let (full, low_resolution, held, gpu_elapsed, _) =
                    draw(&mut app, &mut renderer, verify);
                drawing += draw_started.elapsed();
                gpu_calls += gpu_elapsed;
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
            gpu_time.push(gpu_calls);
            if let (Some(memory), Some(renderer)) = (&mut memory, &renderer) {
                memory.sample(renderer);
            }
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
            "NAV100 gpu={} readback={} cadence_ms={} files_mib={:.1} full=100 blank_targets={} preview_targets={} handoff_targets={} ready_median_ms={:.3} ready_p95_ms={:.3} event_completion_median_ms={:.3} draw_total_median_ms={:.3} gpu_calls_total_median_ms={:.3} dispatch_median_ms={:.3}",
            renderer.is_some(),
            verify,
            cadence.as_millis(),
            bytes as f64 / 1048576.0,
            blank_targets,
            preview_targets,
            handoff_targets,
            median(&ready),
            percentile_95(&ready).as_secs_f64() * 1000.0,
            median(&prepare),
            median(&draw_time),
            median(&gpu_time),
            median(&dispatch)
        );
        if let Some(memory) = memory {
            memory.report();
        }
    }
    if renderer.is_some() {
        app.nearest_images = false;
        draw(&mut app, &mut renderer, false);
        assert_eq!(app.path.as_ref(), Some(&paths[0]));
        let started = Instant::now();
        for _ in 0..100 {
            app.dispatch(CommandId::NextSameKind);
        }
        assert_eq!(
            app.path.as_ref(),
            Some(&paths[1]),
            "burst cannot supersede the first unseen original"
        );
        let mut visited = Vec::new();
        let mut blank = 0;
        let mut previews = 0;
        while visited.len() < 100 {
            let (full, preview, held, _, presented) = draw(&mut app, &mut renderer, false);
            blank += usize::from(!full && !preview && !held);
            previews += usize::from(preview);
            if let Some(path) = presented {
                visited.push(path);
            }
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "GPU burst timeout"
            );
            if visited.len() < 100 {
                receive(&mut app, Duration::from_millis(1));
            }
        }
        assert!(
            visited
                .iter()
                .enumerate()
                .all(|(index, path)| path == &paths[(index + 1) % 100])
        );
        assert!(app.image_sequence.awaiting.is_none() && app.image_sequence.steps.is_empty());
        assert_eq!((blank, previews), (0, 0));
        eprintln!(
            "NAV100_BURST accepted=100 presented=100 blank=0 preview=0 elapsed_ms={:.3}; direct command burst, real GPU Present, no physical key delivery proof",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    eprintln!(
        "Scope: warm filesystem, synthetic Shell snapshot, serialized commands with all originals visited. GPU runs use a hidden 960x576 window and include upload/render/Present in readiness; CPU runs only prepare meshes/textures. The separate readback run uses Nearest with 64 pixel checks per displayed original and is NOT a throughput comparison. Memory: process-lifetime OS peaks, GPU sampled maxima after image arrivals, not transient GPU peaks. No physical keys, dropped-key burst, cold data, other formats or IrfanView comparison. Any blank/preview target leaves the seamless-navigation gate unmet."
    );
}
