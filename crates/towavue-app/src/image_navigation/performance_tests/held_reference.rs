use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

struct FrameCost {
    start: Instant,
    phases: [Duration; 4],
    final_original: bool,
}

// This opt-in submits private originals only to a hidden surface. It emits no
// paths, pixels or screenshots, and never injects keyboard input into Windows.
#[test]
#[ignore = "read-only held navigation; explicit TOWAVUE_NAV_REFERENCE_DIR, Release and hardware D3D11 required"]
#[allow(clippy::assertions_on_constants)]
fn held_reference_reaches_release_destination_from_middle_and_late_positions() {
    assert!(!cfg!(debug_assertions), "use Release");
    let Some(_root) = crate::tests::isolated_test_root(
        "image_navigation::performance_tests::held_reference::held_reference_reaches_release_destination_from_middle_and_late_positions",
    ) else {
        return;
    };
    let folder = PathBuf::from(
        std::env::var_os("TOWAVUE_NAV_REFERENCE_DIR").expect("explicit read-only folder"),
    );
    let mut provider = towavue_runtime_windows::FolderOrderProvider::new().expect("Shell provider");
    let snapshot = provider.snapshot(&folder).expect("native Shell snapshot");
    assert_ne!(snapshot.source, FolderSnapshotSource::NaturalNameFallback);
    let paths: Vec<_> = snapshot
        .items_of_kind(MediaKind::Image)
        .map(|item| item.path.clone())
        .collect();
    assert!(
        paths.len() > 400,
        "reference needs enough middle/late images"
    );
    let stamps = || {
        paths
            .iter()
            .map(|path| {
                let metadata = std::fs::metadata(path).expect("source metadata");
                (metadata.len(), metadata.modified().expect("source time"))
            })
            .collect::<Vec<_>>()
    };
    let before = stamps();
    struct Trial<'a> {
        snapshot: FolderSnapshot,
        paths: &'a [PathBuf],
        completed: bool,
    }
    impl ApplicationHandler for Trial<'_> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden window");
            let mut renderer = FrameRenderer::new(&window).expect("hardware D3D11");
            for (numerator, reverse) in [(2, false), (3, true)] {
                let start = self.paths.len() * numerator / 4;
                let final_index = if reverse { start - 91 } else { start + 91 };
                let (send, events) = std::sync::mpsc::channel();
                let mut app = Application::new(None, move |event| {
                    let _ = send.send(event);
                })
                .expect("isolated app");
                let context = fonts::test_context();
                app.ui_context = Some(context.clone());
                app.fullscreen = true;
                app.folder_navigation_loop = false;
                app.image_navigation_forward = !reverse;
                app.folder_snapshot = Some(self.snapshot.clone());
                app.tabs
                    .open_new(self.paths[start].clone(), MediaKind::Image);
                let setup = Instant::now();
                app.image_loader
                    .verification_trace_path(self.paths[final_index].clone(), setup);
                app.load_path(self.paths[start].clone(), MediaKind::Image);
                let mut visited = vec![];
                let mut folder_completions = 0;
                let mut frame_costs = Vec::with_capacity(8192);
                let mut dropped_costs = 0;
                let mut tick = |app: &mut Application<_>, visited: &mut Vec<usize>| {
                    let frame_started = Instant::now();
                    while let Ok(event) = events.try_recv() {
                        let previous = app
                            .folder_snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.generation);
                        app.handle_app_event(event);
                        if let Some(snapshot) = &app.folder_snapshot
                            && Some(snapshot.generation) != previous
                        {
                            assert!(
                                snapshot
                                    .items_of_kind(MediaKind::Image)
                                    .map(|item| &item.path)
                                    .eq(self.paths.iter()),
                                "Shell order changed; no private paths printed"
                            );
                            folder_completions += 1;
                        }
                    }
                    assert!(app.image_error.is_none(), "reference decoding failed");
                    let events_done = Instant::now();
                    let output = context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(960.0, 576.0),
                            )),
                            max_texture_side: Some(renderer.max_texture_side()),
                            time: Some(setup.elapsed().as_secs_f64()),
                            ..Default::default()
                        },
                        |ui| app.draw_ui(ui, &mut Vec::new()),
                    );
                    let drawn = |id| {
                        output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == id))
                    };
                    let full = app
                        .image
                        .as_ref()
                        .is_some_and(|image| drawn(image.texture.id()));
                    let held = app
                        .image_handoff
                        .as_ref()
                        .is_some_and(|held| drawn(held.image.texture.id()));
                    if !visited.is_empty() {
                        assert!(full || held, "blank frame after initial original");
                        assert!(
                            !app.image_previews
                                .values()
                                .any(|preview| drawn(preview.texture.id())),
                            "preview substituted for original"
                        );
                    }
                    let current = full.then(|| {
                        self.paths
                            .iter()
                            .position(|path| app.path.as_ref() == Some(path))
                            .expect("reference membership")
                    });
                    let token = app.image_sequence_token(&output);
                    let layout_done = Instant::now();
                    gpu::submit(app, &context, output, &mut renderer, false);
                    let gpu_done = Instant::now();
                    if let Some(index) = current
                        && visited.last() != Some(&index)
                    {
                        visited.push(index);
                    }
                    app.finish_image_sequence_frame(token);
                    if frame_costs.len() < 8192 {
                        frame_costs.push(FrameCost {
                            start: frame_started,
                            phases: [
                                events_done - frame_started,
                                layout_done - events_done,
                                gpu_done - layout_done,
                                gpu_done.elapsed(),
                            ],
                            final_original: current == Some(final_index),
                        });
                    } else {
                        dropped_costs += 1;
                    }
                };
                while visited.is_empty()
                    || app.image_loading
                    || app.image_sequence.awaiting.is_some()
                {
                    assert!(
                        setup.elapsed() < Duration::from_secs(60),
                        "initial reference timeout"
                    );
                    tick(&mut app, &mut visited);
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(visited, [start]);
                let key: KeyStroke = if reverse { "Left" } else { "Right" }
                    .parse()
                    .expect("navigation key");
                let identity = if reverse {
                    Key::ArrowLeft
                } else {
                    Key::ArrowRight
                };
                let started = Instant::now();
                let mut sent = 1;
                app.process_shortcut(key.clone());
                let mut max_queue = 0;
                while sent < 91 {
                    while sent < 91 && started.elapsed() >= Duration::from_millis(33 * sent) {
                        app.repeat_media_shortcut(key.clone());
                        sent += 1;
                    }
                    max_queue = max_queue.max(app.image_sequence.steps.len());
                    tick(&mut app, &mut visited);
                    assert!(
                        started.elapsed() < Duration::from_secs(60),
                        "held reference timeout"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                let released = Instant::now();
                app.release_image_repeats(&identity, false);
                let release_dispatch = released.elapsed();
                let mut settled = None;
                loop {
                    tick(&mut app, &mut visited);
                    assert!(
                        released.elapsed() < Duration::from_secs(60),
                        "release destination timeout"
                    );
                    if !app.image_loading
                        && app.image_sequence.awaiting.is_none()
                        && app.image_sequence.steps.is_empty()
                    {
                        assert!(
                            app.path.as_ref() == Some(&self.paths[final_index]),
                            "wrong release destination"
                        );
                        let done = settled.get_or_insert_with(Instant::now);
                        if done.elapsed() >= Duration::from_millis(500) {
                            break;
                        }
                    } else {
                        assert!(settled.is_none(), "navigation restarted after settling");
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(visited.last(), Some(&final_index));
                assert!(
                    visited.windows(2).all(|pair| if reverse {
                        pair[1] < pair[0]
                    } else {
                        pair[1] > pair[0]
                    }),
                    "original presentations must progress monotonically"
                );
                let settled_at = settled.expect("settled");
                let tail: Vec<_> = frame_costs
                    .iter()
                    .filter(|cost| cost.start >= released && cost.start < settled_at)
                    .collect();
                let milliseconds = |duration: Duration| duration.as_secs_f64() * 1000.0;
                eprintln!(
                    "HELD_TAIL_CLOCK reverse={reverse} input_started_ms={:.3} released_ms={:.3} settled_ms={:.3} release_dispatch_ms={:.3} measured_frames={} dropped_costs={dropped_costs}; same setup origin as loader trace; dispatch precedes frame phases; sleep/observer work excluded from phases",
                    milliseconds(started - setup),
                    milliseconds(released - setup),
                    milliseconds(settled_at - setup),
                    milliseconds(release_dispatch),
                    tail.len(),
                );
                for (index, label) in [
                    "events",
                    "layout_and_inspection",
                    "gpu_submit_present",
                    "sequence_advance",
                ]
                .into_iter()
                .enumerate()
                {
                    eprintln!(
                        "HELD_TAIL_PHASE reverse={reverse} phase={label} total_ms={:.3} maximum_ms={:.3}",
                        milliseconds(tail.iter().map(|cost| cost.phases[index]).sum()),
                        milliseconds(
                            tail.iter()
                                .map(|cost| cost.phases[index])
                                .max()
                                .unwrap_or_default()
                        ),
                    );
                }
                if let Some(final_frame) = tail.iter().find(|cost| cost.final_original) {
                    eprintln!(
                        "HELD_TAIL_FINAL_FRAME reverse={reverse} start_ms={:.3} phase_ms={:?}",
                        milliseconds(final_frame.start - setup),
                        final_frame.phases.map(milliseconds)
                    );
                }
                let trace = app
                    .image_loader
                    .verification_trace_snapshot()
                    .expect("selected final trace");
                for event in trace.events {
                    eprintln!(
                        "HELD_TAIL_LOADER reverse={reverse} at_ms={:.3} generation={} {:?}",
                        milliseconds(event.elapsed),
                        event.generation,
                        event.kind
                    );
                }
                assert_eq!(trace.dropped, 0, "final-source trace overflow");
                assert_eq!(dropped_costs, 0, "frame phase trace overflow");
                let decoded = &app.image.as_ref().expect("final original").decoded;
                let encoded_bytes = std::fs::metadata(&self.paths[final_index])
                    .expect("final metadata")
                    .len();
                eprintln!(
                    "HELD_TAIL_SOURCE reverse={reverse} encoded_bytes={encoded_bytes} dimensions={}x{} frames={}; no source path or pixel output",
                    decoded.frames[0].width,
                    decoded.frames[0].height,
                    decoded.frames.len()
                );
                eprintln!(
                    "HELD_TAIL_TOTAL_LOADER reverse={reverse} {:?}; aggregate complete calls, overlaps are not additive",
                    app.image_loader.verification_metrics()
                );
                // Full decode after timing verifies retained original pixels/dimensions,
                // without prewarming the run or reading back the private GPU surface.
                let original = towavue_runtime_windows::decode_image(&self.paths[final_index])
                    .expect("independent final original decode");
                let displayed = &app.image.as_ref().expect("final original").decoded;
                assert_eq!(displayed.frames.len(), original.frames.len());
                assert!(
                    displayed
                        .frames
                        .iter()
                        .zip(&original.frames)
                        .all(|(left, right)| left.width == right.width
                            && left.height == right.height
                            && left.rgba == right.rgba),
                    "final original mismatch; pixels not printed"
                );
                eprintln!(
                    "HELD_REFERENCE start_fraction={numerator}/4 reverse={reverse} folder_images={} commands={sent} originals={} intentional_skips={} max_queue={max_queue} release_to_settled_ms={:.3} folder_completions={folder_completions} order_source={:?}; hidden GPU Present, 33ms scripted repeats, retained originals, no late replay over 500ms, no screenshots/readback/physical-input/IrfanView claim",
                    self.paths.len(),
                    visited.len(),
                    sent as usize + 1 - visited.len(),
                    settled
                        .expect("settled")
                        .duration_since(released)
                        .as_secs_f64()
                        * 1000.0,
                    self.snapshot.source
                );
            }
            self.completed = true;
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial {
        snapshot,
        paths: &paths,
        completed: false,
    };
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut trial)
        .expect("reference trial");
    assert!(trial.completed, "trial did not complete");
    assert_eq!(before, stamps(), "reference source lengths/times changed");
}
