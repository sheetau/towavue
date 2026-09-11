use super::*;
use std::os::windows::process::CommandExt;

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
        (full, preview)
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
        let mut dispatch = Vec::new();
        let mut blank_targets = 0;
        let mut preview_targets = 0;
        for index in 1..=100 {
            let start = Instant::now();
            app.dispatch(CommandId::NextSameKind);
            dispatch.push(start.elapsed());
            let mut blank = false;
            let mut preview = false;
            let mut preparation = Duration::ZERO;
            loop {
                let (full, low_resolution) = draw(&mut app);
                blank |= !full && !low_resolution;
                preview |= low_resolution;
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
            blank_targets += usize::from(blank);
            preview_targets += usize::from(preview);
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
            "NAV100 cadence_ms={} files_mib={:.1} full=100 blank_targets={} preview_targets={} ready_median_ms={:.3} ready_p95_ms={:.3} prepare_median_ms={:.3} dispatch_median_ms={:.3}",
            cadence.as_millis(),
            bytes as f64 / 1048576.0,
            blank_targets,
            preview_targets,
            median(&ready),
            percentile_95(&ready).as_secs_f64() * 1000.0,
            median(&prepare),
            median(&dispatch)
        );
    }
    eprintln!(
        "Scope: warm filesystem, synthetic Shell snapshot, serialized commands with all originals visited, CPU egui mesh/texture preparation; no GPU presentation, physical keys, dropped-key burst, peak memory or IrfanView comparison. Any blank/preview target leaves the seamless-navigation gate unmet."
    );
}
