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
    // Inspect only the 24-byte signature/IHDR prefix, before starting the clock.
    // Selecting a source by metadata avoids paths or reference pixels in output.
    let trace_index = std::env::var_os("TOWAVUE_NAV_TRACE_LARGEST_PNG").map(|_| {
        use std::io::Read;
        paths
            .iter()
            .enumerate()
            .filter_map(|(index, path)| {
                let mut prefix = [0; 24];
                std::fs::File::open(path)
                    .expect("read-only header")
                    .read_exact(&mut prefix)
                    .expect("reference header prefix");
                if &prefix[..8] != b"\x89PNG\r\n\x1a\n" || &prefix[12..16] != b"IHDR" {
                    return None;
                }
                let width = u32::from_be_bytes(prefix[16..20].try_into().expect("width"));
                let height = u32::from_be_bytes(prefix[20..24].try_into().expect("height"));
                Some((index, u64::from(width) * u64::from(height)))
            })
            .max_by_key(|&(_, area)| area)
            .expect("PNG reference required")
            .0
    });
    struct Trial<'a> {
        paths: &'a [PathBuf],
        reverse: bool,
        trace_index: Option<usize>,
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
            let first = &self.paths[if self.reverse {
                self.paths.len() - 1
            } else {
                0
            }];
            app.image_navigation_forward = !self.reverse;
            app.tabs.open_new(first.clone(), MediaKind::Image);
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
            if let Some(index) = self.trace_index {
                app.image_loader
                    .verification_trace_path(self.paths[index].clone(), started);
            }
            app.load_path(first.clone(), MediaKind::Image);
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
                    app.dispatch(if self.reverse {
                        CommandId::PreviousSameKind
                    } else {
                        CommandId::NextSameKind
                    });
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
                    if Some(index) == self.trace_index {
                        eprintln!(
                            "REFERENCE_TRACE_PRESENT at_ms={:.3} completion_events_since_previous_original_ms={:.3} layout_ms={:.3} gpu_submit_present_ms={:.3}; event preparation excludes results drained inside layout; GPU is CPU wall time including upload/render/Present, not pure upload",
                            started.elapsed().as_secs_f64() * 1000.0,
                            event_preparation.as_secs_f64() * 1000.0,
                            layout.as_secs_f64() * 1000.0,
                            gpu_time.as_secs_f64() * 1000.0,
                        );
                    }
                    let index = if self.reverse {
                        self.paths.len() - 1 - index
                    } else {
                        index
                    };
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
            let complete_order = visited.len() == self.paths.len()
                && visited.iter().enumerate().all(|(index, path)| {
                    path == &self.paths[if self.reverse {
                        self.paths.len() - 1 - index
                    } else {
                        index
                    }]
                });
            eprintln!(
                "REFERENCE_NAV images={} commands={} presented={} complete_order={complete_order} blank_frames={blanks} preview_frames={previews} max_queue={max_queue} failed={failed} elapsed_ms={:.3} ready_after_scheduled_command_median_ms={:.3} p95_ms={:.3}; reverse={}; read-only originals, hidden GPU Present, lexical synthetic order, 33ms commands without prewarming, no readback/screenshots or physical-key/IrfanView evidence",
                self.paths.len(),
                sent,
                visited.len(),
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
                self.reverse,
            );
            memory.report();
            if let Some(index) = self.trace_index {
                let command_index = if self.reverse {
                    self.paths.len() - 1 - index
                } else {
                    index
                };
                let trace = app
                    .image_loader
                    .verification_trace_snapshot()
                    .expect("selected trace");
                eprintln!(
                    "REFERENCE_TRACE index={index} command_index={command_index} scheduled_ms={:.3} dropped={}; selected largest PNG by IHDR area, trace times include scheduling; no paths or pixels",
                    (cadence * command_index as u32).as_secs_f64() * 1000.0,
                    trace.dropped
                );
                for event in trace.events {
                    eprintln!(
                        "REFERENCE_TRACE_EVENT at_ms={:.3} generation={} {:?}",
                        event.elapsed.as_secs_f64() * 1000.0,
                        event.generation,
                        event.kind
                    );
                }
                assert_eq!(trace.dropped, 0, "selected trace must not be truncated");
            }
            eprintln!(
                "REFERENCE_LOADER {:?}; idle={}; aggregate snapshot at final Present; decoder-return outcomes, not cache insertions; unfinished calls have no elapsed time; decode and wait times overlap and must not be summed; no paths or pixels",
                app.image_loader.verification_metrics(),
                app.image_loader.is_idle(),
            );
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
            self.completed = !failed && complete_order && blanks == 0 && previews == 0;
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial {
        paths: &paths,
        reverse: std::env::var_os("TOWAVUE_NAV_REFERENCE_REVERSE").is_some(),
        trace_index,
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
