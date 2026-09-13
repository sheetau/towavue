use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "read-only nonvisual reference-folder timing; set TOWAVUE_NAV_REFERENCE_DIR and use Release with hardware D3D11"]
#[allow(clippy::assertions_on_constants)] // Deliberately reject unoptimized manual runs.
fn reference_folder_reports_unpaced_completion_under_fixed_rate_commands() {
    assert!(!cfg!(debug_assertions), "use the Release test binary");
    let Some(root) = crate::tests::isolated_test_root(
        "image_navigation::performance_tests::reference::reference_folder_reports_unpaced_completion_under_fixed_rate_commands",
    ) else {
        return;
    };
    let source = PathBuf::from(
        std::env::var_os("TOWAVUE_NAV_REFERENCE_DIR").expect("explicit read-only reference folder"),
    );
    let mut paths: Vec<_> = std::fs::read_dir(&source)
        .expect("read reference directory")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.is_file() && MediaKind::from_path(path) == Some(MediaKind::Image))
        .collect();
    paths.sort();
    assert!(paths.len() > 1, "at least two reference images");
    let stamps = || {
        paths
            .iter()
            .map(|path| {
                let metadata = std::fs::metadata(path).expect("read source metadata");
                (metadata.len(), metadata.modified().expect("modified time"))
            })
            .collect::<Vec<_>>()
    };
    let before = stamps();
    struct Trial<'a> {
        paths: &'a [PathBuf],
        source: PathBuf,
        completed: bool,
    }
    impl ApplicationHandler for Trial<'_> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden owned window");
            let mut renderer = FrameRenderer::new(&window).expect("hardware D3D11 required");
            let (send, events) = std::sync::mpsc::channel();
            let mut app = Application::new(None, move |event| {
                let _ = send.send(event);
            })
            .expect("isolated app");
            let context = fonts::test_context();
            app.ui_context = Some(context.clone());
            app.fullscreen = true;
            app.tabs.open_new(self.paths[0].clone(), MediaKind::Image);
            app.folder_snapshot = Some(FolderSnapshot {
                folder_identity: towavue_core::ShellIdentity::new(vec![]),
                folder_path: self.source.clone(),
                items: self
                    .paths
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
            let started = Instant::now();
            app.load_path(self.paths[0].clone(), MediaKind::Image);
            let cadence = Duration::from_millis(33);
            let mut sent = 0;
            let mut visited = Vec::new();
            let mut latency = Vec::new();
            let mut blanks = 0;
            let mut previews = 0;
            let mut max_queue = 0;
            let mut memory = gpu::Memory::new(&renderer);
            let mut failed = false;
            let mut event_preparation = Duration::ZERO;
            let mut preparation = Vec::new();
            let mut original_gpu = Vec::new();
            let mut original_layout = Vec::new();
            loop {
                // Fixed schedule, independent of image completion: no initial prefetch wait.
                while sent + 1 < self.paths.len()
                    && started.elapsed() >= cadence * (sent as u32 + 1)
                {
                    app.dispatch(CommandId::NextSameKind);
                    sent += 1;
                    max_queue = max_queue.max(app.image_sequence.steps.len());
                }
                while let Ok(event) = events.try_recv() {
                    match event {
                        AppEvent::ImagesReady => {
                            let started = Instant::now();
                            app.finish_image_load();
                            event_preparation += started.elapsed();
                        }
                        AppEvent::ImagePreview(..) => app.handle_app_event(event),
                        _ => {}
                    }
                }
                let layout_started = Instant::now();
                let output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(960.0, 576.0),
                        )),
                        max_texture_side: Some(renderer.max_texture_side()),
                        time: Some(started.elapsed().as_secs_f64()),
                        ..Default::default()
                    },
                    |ui| app.draw_ui(ui, &mut Vec::new()),
                );
                let layout = layout_started.elapsed();
                let drawn = |id| {
                    output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == id)
                })
                };
                let full = app
                    .image
                    .as_ref()
                    .is_some_and(|image| drawn(image.texture.id()));
                let held = app
                    .image_handoff
                    .as_ref()
                    .is_some_and(|held| drawn(held.image.texture.id()));
                let preview = app
                    .image_previews
                    .values()
                    .any(|preview| drawn(preview.texture.id()));
                if !visited.is_empty() {
                    blanks += usize::from(!full && !held && !preview);
                    previews += usize::from(preview);
                }
                let path = full.then(|| app.path.clone().expect("displayed source"));
                let token = app.image_sequence_token(&output);
                // Never read back or emit pixels from reference media; only submit to a hidden surface.
                let gpu_time = gpu::submit(&app, &context, output, &mut renderer, false);
                if let Some(path) = path
                    && visited.last() != Some(&path)
                {
                    let index = self
                        .paths
                        .iter()
                        .position(|candidate| candidate == &path)
                        .expect("source index");
                    latency.push(started.elapsed().saturating_sub(cadence * index as u32));
                    preparation.push(std::mem::take(&mut event_preparation));
                    original_layout.push(layout);
                    original_gpu.push(gpu_time);
                    visited.push(path);
                    memory.sample(&renderer);
                }
                app.finish_image_sequence_frame(token);
                if app.image_error.is_some() || started.elapsed() > Duration::from_secs(180) {
                    failed = true;
                    break;
                }
                if sent + 1 == self.paths.len()
                    && !app.image_loading
                    && app.image_sequence.awaiting.is_none()
                    && app.image_sequence.steps.is_empty()
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            latency.sort_unstable();
            eprintln!(
                "REFERENCE_NAV images={} commands={} presented={} complete_order={} blank_frames={blanks} preview_frames={previews} max_queue={max_queue} failed={failed} elapsed_ms={:.3} ready_after_scheduled_command_median_ms={:.3} p95_ms={:.3}; read-only originals, hidden GPU Present, lexical synthetic order, 33ms commands without prewarming, no readback/screenshots or physical-key/IrfanView evidence",
                self.paths.len(),
                sent,
                visited.len(),
                visited == self.paths,
                started.elapsed().as_secs_f64() * 1000.0,
                latency
                    .get(latency.len() / 2)
                    .copied()
                    .unwrap_or_default()
                    .as_secs_f64()
                    * 1000.0,
                latency
                    .last()
                    .map_or(0.0, |_| percentile_95(&latency).as_secs_f64() * 1000.0),
            );
            memory.report();
            for (stage, samples) in [
                ("completion_events_per_original", &preparation),
                ("new_original_frame_layout", &original_layout),
                ("new_original_frame_gpu_submit_present", &original_gpu),
            ] {
                let mut sorted = samples.clone();
                sorted.sort_unstable();
                if !sorted.is_empty() {
                    eprintln!(
                        "REFERENCE_STAGE stage={stage} count={} median_ms={:.3} p95_ms={:.3} total_ms={:.3}; event preparation excludes results drained inside layout; GPU includes texture upload/render/Present, not pure upload or a GPU timestamp",
                        sorted.len(),
                        sorted[sorted.len() / 2].as_secs_f64() * 1000.0,
                        percentile_95(&sorted).as_secs_f64() * 1000.0,
                        sorted.iter().sum::<Duration>().as_secs_f64() * 1000.0
                    );
                }
            }
            self.completed = !failed && visited == self.paths && blanks == 0 && previews == 0;
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial {
        paths: &paths,
        source,
        completed: false,
    };
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("reference trial");
    assert_eq!(
        before,
        stamps(),
        "source lengths and modification times are unchanged"
    );
    assert!(
        trial.completed,
        "reference completion gate unmet; see aggregate counters (latency is reported separately)"
    );
    assert!(
        root.is_dir(),
        "isolated application root remains present during the read-only trial"
    );
}
