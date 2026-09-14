use super::*;
use crate::image_color::{
    COLOR_IMAGE_CPU_TIME, COLOR_IMAGE_LOOKUP_TRIAL, COLOR_IMAGE_PARALLEL_TRIAL,
    COLOR_IMAGE_SCALAR_TRIAL,
};
use towavue_runtime_windows::verification_thread_cpu_time;
use winit::platform::windows::EventLoopBuilderExtWindows;

const PHASES: [&str; 8] = [
    "load_and_measurement_setup",
    "dispatch",
    "completion_events",
    "layout_and_frame_inspection",
    "gpu_submit_all_frames",
    "presentation_accounting_and_memory",
    "sequence_advance",
    "sleep",
];

#[derive(Clone, Copy, Default)]
struct PhaseCost {
    total: Duration,
    longest: Duration,
    longest_start: Duration,
    presented: usize,
    sent: usize,
}

struct PhaseTrace {
    boundary: Duration,
    costs: [PhaseCost; PHASES.len()],
}

#[derive(Debug, Default, PartialEq)]
struct TextureFootprint {
    count: usize,
    bytes: usize,
}

impl TextureFootprint {
    fn from_sizes(sizes: impl Iterator<Item = [usize; 2]>) -> Self {
        sizes.fold(Self::default(), |mut total, [width, height]| {
            total.count += 1;
            total.bytes += width * height * 4;
            total
        })
    }
}

fn texture_retirement_footprint(
    managed: &[(egui::TextureId, [usize; 2])],
    delta: &egui::TexturesDelta,
) -> TextureFootprint {
    // Report only resources already in the pool, not newly uploaded textures
    // also freed this frame. Duplicate free IDs must not inflate the estimate.
    TextureFootprint::from_sizes(
        managed
            .iter()
            .filter(|(id, _)| delta.free.contains(id))
            .map(|(_, size)| *size),
    )
}

#[test]
fn texture_footprints_distinguish_existing_retirement_from_new_uploads() {
    let managed = [
        (egui::TextureId::Managed(0), [10, 20]),
        (egui::TextureId::Managed(1), [30, 40]),
    ];
    let mut delta = egui::TexturesDelta::default();
    assert_eq!(
        texture_retirement_footprint(&managed, &delta),
        TextureFootprint::default()
    );
    delta.free = vec![
        egui::TextureId::Managed(1),
        egui::TextureId::Managed(1),
        egui::TextureId::Managed(2),
        egui::TextureId::User(0),
    ];
    assert_eq!(
        texture_retirement_footprint(&managed, &delta),
        TextureFootprint {
            count: 1,
            bytes: 4800
        }
    );
    assert_eq!(
        TextureFootprint::from_sizes(managed.iter().map(|(_, size)| *size)),
        TextureFootprint {
            count: 2,
            bytes: 5600
        }
    );
}

fn reference_order(
    snapshot: &FolderSnapshot,
    paths: &[PathBuf],
    first: &Path,
    reverse: bool,
) -> Vec<PathBuf> {
    let mut ordered: Vec<_> = snapshot
        .items_of_kind(MediaKind::Image)
        .map(|item| item.path.clone())
        .collect();
    let source: std::collections::HashSet<_> = paths.iter().collect();
    let returned: std::collections::HashSet<_> = ordered.iter().collect();
    assert!(
        ordered.len() == paths.len() && source == returned,
        "Shell image membership differs from the read-only reference set; no paths printed"
    );
    if reverse {
        ordered.reverse();
    }
    let start = ordered
        .iter()
        .position(|path| path == first)
        .expect("initial source in Shell order");
    ordered.rotate_left(start);
    ordered
}

#[test]
fn reference_order_tracks_shell_direction_and_initial_source() {
    let paths: Vec<_> = ["1.png", "10.png", "2.png"]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let mut snapshot = FolderSnapshot {
        folder_identity: towavue_core::ShellIdentity::new(vec![]),
        folder_path: PathBuf::new(),
        items: [0, 2, 1]
            .into_iter()
            .map(|index| towavue_core::FolderMediaItem {
                identity: towavue_core::ShellIdentity::new(vec![index as u8]),
                path: paths[index].clone(),
                kind: MediaKind::Image,
            })
            .collect(),
        sort_columns: vec![],
        source: FolderSnapshotSource::PersistedShellView,
        generation: 1,
        captured_at: std::time::SystemTime::UNIX_EPOCH,
    };
    for (reverse, indices) in [(false, [2, 1, 0]), (true, [2, 0, 1])] {
        assert_eq!(
            reference_order(&snapshot, &paths, &paths[2], reverse),
            indices.map(|index| paths[index].clone())
        );
    }
    snapshot.items.push(towavue_core::FolderMediaItem {
        identity: towavue_core::ShellIdentity::new(vec![]),
        path: "other.mp4".into(),
        kind: MediaKind::Video,
    });
    assert_eq!(
        reference_order(&snapshot, &paths, &paths[0], false),
        [0, 2, 1].map(|index| paths[index].clone())
    );
    snapshot.items[1].path = paths[0].clone();
    assert!(
        std::panic::catch_unwind(|| reference_order(&snapshot, &paths, &paths[0], false)).is_err()
    );
}

impl PhaseTrace {
    fn finish(&mut self, phase: usize, elapsed: Duration, presented: usize, sent: usize) {
        // Consecutive boundaries partition the whole measured interval, including
        // loading/held frames and measurement overhead, with no nested double count.
        let duration = elapsed - self.boundary;
        let cost = &mut self.costs[phase];
        cost.total += duration;
        if duration > cost.longest {
            cost.longest = duration;
            cost.longest_start = self.boundary;
            cost.presented = presented;
            cost.sent = sent;
        }
        self.boundary = elapsed;
    }
}

#[test]
fn phase_boundaries_account_for_loading_frames_and_measurement_gaps() {
    let mut trace = PhaseTrace {
        boundary: Duration::ZERO,
        costs: [PhaseCost::default(); PHASES.len()],
    };
    for (phase, milliseconds, presented, sent) in [
        (0, 10, 0, 0),
        (1, 12, 0, 1),
        (4, 7012, 0, 1),
        (7, 7013, 0, 1),
        (1, 7015, 1, 210),
        (4, 7020, 1, 210),
    ] {
        trace.finish(phase, Duration::from_millis(milliseconds), presented, sent);
    }
    assert_eq!(
        trace.costs.iter().map(|cost| cost.total).sum::<Duration>(),
        trace.boundary
    );
    let gpu = trace.costs[4];
    assert_eq!(gpu.total, Duration::from_millis(7005));
    assert_eq!(gpu.longest, Duration::from_millis(7000));
    assert_eq!(gpu.longest_start, Duration::from_millis(12));
    assert_eq!((gpu.presented, gpu.sent), (0, 1));
}

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
    let largest_png = std::env::var_os("TOWAVUE_NAV_TRACE_LARGEST_PNG").is_some();
    let explicit_index = std::env::var("TOWAVUE_NAV_TRACE_INDEX")
        .ok()
        .map(|value| value.parse::<usize>().expect("trace source index"));
    assert!(
        !(largest_png && explicit_index.is_some()),
        "choose one trace selector"
    );
    assert!(
        explicit_index.is_none_or(|index| index < paths.len()),
        "trace index outside source list"
    );
    let trace_index = explicit_index.or_else(|| {
        largest_png.then(|| {
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
        })
    });
    struct Trial<'a> {
        paths: &'a [PathBuf],
        reverse: bool,
        trace_index: Option<usize>,
        idle_frame_interval: Duration,
        cache_mib: usize,
        directional_prefetch: bool,
        parallel_prefetch: bool,
        parallel_color: bool,
        lookup_color: bool,
        scalar_color: bool,
        native_order: bool,
        source: PathBuf,
        completed: bool,
    }
    impl ApplicationHandler for Trial<'_> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            COLOR_IMAGE_PARALLEL_TRIAL.set(self.parallel_color);
            COLOR_IMAGE_LOOKUP_TRIAL.set(self.lookup_color);
            COLOR_IMAGE_SCALAR_TRIAL.set(self.scalar_color);
            let window = event_loop
                .create_window(Window::default_attributes().with_visible(false))
                .expect("hidden owned window");
            let mut renderer = FrameRenderer::new(&window).expect("hardware D3D11 required");
            let (send, events) = std::sync::mpsc::channel();
            let mut app = Application::new(None, move |event| {
                let _ = send.send(event);
            })
            .expect("isolated app");
            app.verification_directional_prefetch = self.directional_prefetch;
            app.image_loader
                .verification_set_cache_byte_limit(self.cache_mib * 1024 * 1024)
                .expect("unused reference loader");
            app.image_loader
                .verification_set_parallel_prefetch(self.parallel_prefetch)
                .expect("unused reference loader");
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
            let mut expected: Vec<_> = if self.native_order {
                app.folder_snapshot = None;
                Vec::new()
            } else if self.reverse {
                self.paths.iter().rev().cloned().collect()
            } else {
                self.paths.to_vec()
            };
            let mut folder_completions = 0;
            let mut order_source = None;
            let started = Instant::now();
            COLOR_IMAGE_CPU_TIME.set(self.trace_index.map(|_| Duration::ZERO));
            let mut phases = PhaseTrace {
                boundary: Duration::ZERO,
                costs: [PhaseCost::default(); PHASES.len()],
            };
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
            let mut failed;
            let mut event_preparation = Duration::ZERO;
            let mut preparation = Vec::new();
            let mut original_gpu = Vec::new();
            let mut original_layout = Vec::new();
            let mut previous_conversion_time = COLOR_IMAGE_CONVERSION_TIME.get();
            let mut previous_conversion_cpu = Duration::ZERO;
            let mut next_idle_frame = Instant::now();
            let mut presentations = 0;
            let mut first_present = None;
            phases.finish(0, started.elapsed(), 0, 0);
            loop {
                let mut changed = false;
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
                    changed = true;
                    max_queue = max_queue.max(app.image_sequence.steps.len());
                }
                phases.finish(1, started.elapsed(), visited.len(), sent);
                while let Ok(event) = events.try_recv() {
                    if self.native_order {
                        let previous = app
                            .folder_snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.generation);
                        let event_started = Instant::now();
                        app.handle_app_event(event);
                        event_preparation += event_started.elapsed();
                        changed = true;
                        if let Some(snapshot) = &app.folder_snapshot
                            && Some(snapshot.generation) != previous
                        {
                            let ordered =
                                reference_order(snapshot, self.paths, first, self.reverse);
                            if expected.is_empty() {
                                expected = ordered;
                                order_source = Some(snapshot.source);
                            } else {
                                assert!(
                                    expected == ordered,
                                    "reference Shell order changed during measurement; no paths printed"
                                );
                            }
                            folder_completions += 1;
                        }
                        continue;
                    }
                    match event {
                        AppEvent::ImagesReady => {
                            changed = true;
                            let started = Instant::now();
                            app.finish_image_load();
                            event_preparation += started.elapsed();
                        }
                        AppEvent::ImagePreview(..) => {
                            app.handle_app_event(event);
                            changed = true;
                        }
                        _ => {}
                    }
                }
                phases.finish(2, started.elapsed(), visited.len(), sent);
                // Isolate repeated held-frame GPU work without slowing commands
                // or fresh completions. This is not a native redraw scheduler.
                if !changed && Instant::now() < next_idle_frame {
                    std::thread::sleep(Duration::from_millis(1));
                    phases.finish(7, started.elapsed(), visited.len(), sent);
                    continue;
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
                let upload_before = renderer.verification_upload_times();
                let selected = self.trace_index.is_some_and(|index| {
                    path.as_ref() == Some(&self.paths[index]) && visited.last() != path.as_ref()
                });
                let textures = selected.then(|| {
                    let managed = renderer.verification_managed_textures();
                    (
                        TextureFootprint::from_sizes(managed.iter().map(|(_, size)| *size)),
                        texture_retirement_footprint(&managed, &output.textures_delta),
                        TextureFootprint::from_sizes(
                            output
                                .textures_delta
                                .set
                                .iter()
                                .filter(|(id, delta)| {
                                    matches!(id, egui::TextureId::Managed(_)) && delta.pos.is_none()
                                })
                                .map(|(_, delta)| delta.image.size()),
                        ),
                    )
                });
                phases.finish(3, started.elapsed(), visited.len(), sent);
                let cpu_started = selected.then(|| {
                    verification_thread_cpu_time().expect("GPU submission CPU accounting")
                });
                let gpu_time = gpu::submit(&app, &context, output, &mut renderer, false);
                let gpu_cpu = cpu_started.map(|before| {
                    verification_thread_cpu_time().expect("GPU submission CPU accounting") - before
                });
                phases.finish(4, started.elapsed(), visited.len(), sent);
                presentations += 1;
                next_idle_frame = Instant::now() + self.idle_frame_interval;
                if let Some(path) = path
                    && visited.last() != Some(&path)
                {
                    first_present.get_or_insert_with(|| started.elapsed());
                    let index = self
                        .paths
                        .iter()
                        .position(|candidate| candidate == &path)
                        .expect("source index");
                    if Some(index) == self.trace_index {
                        let (before, retiring, uploading) =
                            textures.expect("selected texture trace");
                        let after = TextureFootprint::from_sizes(
                            renderer
                                .verification_managed_textures()
                                .into_iter()
                                .map(|(_, size)| size),
                        );
                        eprintln!(
                            "REFERENCE_TRACE_TEXTURES before_count={} before_bytes={} retiring_existing_count={} retiring_existing_bytes={} whole_upload_count={} whole_upload_bytes={} after_count={} after_bytes={}; RGBA8 dimension estimates, not physical VRAM; retirement matches pre-submit resources only; full replacements counted as uploads; snapshots outside GPU timing",
                            before.count,
                            before.bytes,
                            retiring.count,
                            retiring.bytes,
                            uploading.count,
                            uploading.bytes,
                            after.count,
                            after.bytes,
                        );
                        eprintln!(
                            "REFERENCE_TRACE_PRESENT at_ms={:.3} completion_events_since_previous_original_ms={:.3} layout_ms={:.3} gpu_submit_present_ms={:.3}; event preparation excludes results drained inside layout; GPU is CPU wall time including upload/render/Present, not pure upload",
                            started.elapsed().as_secs_f64() * 1000.0,
                            event_preparation.as_secs_f64() * 1000.0,
                            layout.as_secs_f64() * 1000.0,
                            gpu_time.as_secs_f64() * 1000.0,
                        );
                        let stages = gpu::SUBMISSION_STAGES.get();
                        let upload_after = renderer.verification_upload_times();
                        let upload: [f64; 3] = std::array::from_fn(|i| {
                            (upload_after[i] - upload_before[i]).as_secs_f64() * 1000.0
                        });
                        eprintln!(
                            "REFERENCE_TRACE_PREPARATION color_conversion_ms={:.3} surface_clear_ms={:.3} render_ui_ms={:.3} present_ms={:.3} native_texture_creation_ms={:.3} source_release_ms={:.3} texture_pool_update_ms={:.3}; conversion is since previous original; native creation/release are within pool update, pool update within render_ui; CPU wall times, no GPU timestamps",
                            (COLOR_IMAGE_CONVERSION_TIME.get() - previous_conversion_time)
                                .as_secs_f64()
                                * 1000.0,
                            stages[0].as_secs_f64() * 1000.0,
                            stages[1].as_secs_f64() * 1000.0,
                            stages[2].as_secs_f64() * 1000.0,
                            upload[0],
                            upload[1],
                            upload[2],
                        );
                        eprintln!(
                            "REFERENCE_TRACE_CPU color_cpu_ms={:.3} gpu_submit_cpu_ms={:.3}; calling-thread kernel+user accounting, no worker CPU or GPU execution time; accounting granularity prevents exact wait-time subtraction",
                            (COLOR_IMAGE_CPU_TIME.get().expect("enabled accounting")
                                - previous_conversion_cpu)
                                .as_secs_f64()
                                * 1000.0,
                            gpu_cpu.expect("selected original accounting").as_secs_f64() * 1000.0,
                        );
                    }
                    previous_conversion_time = COLOR_IMAGE_CONVERSION_TIME.get();
                    previous_conversion_cpu = COLOR_IMAGE_CPU_TIME.get().unwrap_or_default();
                    // Scheduled command position follows presentation order, not a
                    // lexical filename index. The final gate checks the entire order.
                    let index = visited.len();
                    latency.push(started.elapsed().saturating_sub(cadence * index as u32));
                    preparation.push(std::mem::take(&mut event_preparation));
                    original_layout.push(layout);
                    original_gpu.push(gpu_time);
                    visited.push(path);
                    memory.sample(&renderer);
                }
                phases.finish(5, started.elapsed(), visited.len(), sent);
                app.finish_image_sequence_frame(token);
                failed = app.image_error.is_some() || started.elapsed() > Duration::from_secs(180);
                let finished = sent + 1 == self.paths.len()
                    && !app.image_loading
                    && app.image_sequence.awaiting.is_none()
                    && app.image_sequence.steps.is_empty();
                phases.finish(6, started.elapsed(), visited.len(), sent);
                if failed || finished {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
                phases.finish(7, started.elapsed(), visited.len(), sent);
            }
            assert_eq!(
                phases.costs.iter().map(|cost| cost.total).sum::<Duration>(),
                phases.boundary
            );
            eprintln!(
                "REFERENCE_FIRST_PRESENT at_ms={:?}; first original, excludes pre-clock window/device/app construction",
                first_present.map(|time| time.as_secs_f64() * 1000.0)
            );
            for (phase, cost) in PHASES.into_iter().zip(phases.costs) {
                eprintln!(
                    "REFERENCE_PHASE phase={phase} total_ms={:.3} longest_ms={:.3} longest_start_ms={:.3} presented_after={} commands_after={}; exclusive wall-time intervals, all frames including loading/held frames; not CPU-active or GPU timestamp time",
                    cost.total.as_secs_f64() * 1000.0,
                    cost.longest.as_secs_f64() * 1000.0,
                    cost.longest_start.as_secs_f64() * 1000.0,
                    cost.presented,
                    cost.sent,
                );
            }
            latency.sort_unstable();
            let complete_order = visited.len() == self.paths.len() && visited == expected;
            eprintln!(
                "REFERENCE_NAV images={} commands={} presented={} complete_order={complete_order} blank_frames={blanks} preview_frames={previews} max_queue={max_queue} failed={failed} elapsed_ms={:.3} ready_after_scheduled_command_median_ms={:.3} p95_ms={:.3}; reverse={} native_order={} folder_completions={folder_completions} order_source={order_source:?}; read-only originals, hidden GPU Present, 33ms commands without prewarming, no readback/screenshots or physical-key/IrfanView evidence",
                self.paths.len(),
                sent,
                visited.len(),
                phases.boundary.as_secs_f64() * 1000.0,
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
                self.native_order,
            );
            eprintln!(
                "REFERENCE_CACHE decoded_mib={} directional_prefetch={} parallel_prefetch={}; verification-only policy controls; entry count, texture and per-canvas limits unchanged",
                self.cache_mib, self.directional_prefetch, self.parallel_prefetch
            );
            memory.report();
            eprintln!(
                "REFERENCE_COLOR parallel={} lookup={} scalar={}; all false uses production packed conversion; lookup and scalar retain historical rows; parallel includes output initialization and worker creation/join, excluded from calling-thread CPU",
                self.parallel_color, self.lookup_color, self.scalar_color,
            );
            eprintln!(
                "REFERENCE_REDRAW presentations={presentations} idle_frame_interval_ms={:.3}; command/completion draws are immediate, interval only applies without those events; no physical redraw-cadence evidence",
                self.idle_frame_interval.as_secs_f64() * 1000.0,
            );
            if let Some(index) = self.trace_index {
                let command_index = expected
                    .iter()
                    .position(|path| path == &self.paths[index])
                    .expect("trace source in navigation order");
                let trace = app
                    .image_loader
                    .verification_trace_snapshot()
                    .expect("selected trace");
                eprintln!(
                    "REFERENCE_TRACE index={index} command_index={command_index} scheduled_ms={:.3} dropped={}; explicit source index or largest PNG by IHDR area, trace times include scheduling; no paths or pixels",
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
            COLOR_IMAGE_CPU_TIME.set(None);
            COLOR_IMAGE_PARALLEL_TRIAL.set(false);
            COLOR_IMAGE_LOOKUP_TRIAL.set(false);
            COLOR_IMAGE_SCALAR_TRIAL.set(false);
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut trial = Trial {
        paths: &paths,
        reverse: std::env::var_os("TOWAVUE_NAV_REFERENCE_REVERSE").is_some(),
        trace_index,
        idle_frame_interval: std::env::var("TOWAVUE_NAV_IDLE_FRAME_MS")
            .map(|value| Duration::from_millis(value.parse().expect("idle-frame milliseconds")))
            .unwrap_or(Duration::ZERO),
        cache_mib: std::env::var("TOWAVUE_NAV_DECODE_CACHE_MIB")
            .map(|value| {
                let mib = value.parse::<usize>().expect("decoded-cache MiB");
                assert!(
                    (1..=512).contains(&mib),
                    "decoded-cache MiB must be 1..=512"
                );
                mib
            })
            .unwrap_or(384),
        directional_prefetch: std::env::var("TOWAVUE_NAV_DIRECTIONAL_PREFETCH")
            .map(|value| match value.as_str() {
                "0" => false,
                "1" => true,
                _ => panic!("directional-prefetch must be 0 or 1"),
            })
            .unwrap_or(true),
        parallel_prefetch: std::env::var("TOWAVUE_NAV_PARALLEL_PREFETCH")
            .map(|value| match value.as_str() {
                "0" => false,
                "1" => true,
                _ => panic!("parallel-prefetch must be 0 or 1"),
            })
            .unwrap_or(true),
        source,
        parallel_color: std::env::var("TOWAVUE_NAV_PARALLEL_COLOR")
            .map(|value| match value.as_str() {
                "0" => false,
                "1" => true,
                _ => panic!("parallel-color must be 0 or 1"),
            })
            .unwrap_or(false),
        completed: false,
        native_order: std::env::var("TOWAVUE_NAV_NATIVE_ORDER")
            .map(|value| match value.as_str() {
                "0" => false,
                "1" => true,
                _ => panic!("native-order must be 0 or 1"),
            })
            .unwrap_or(false),
        lookup_color: std::env::var("TOWAVUE_NAV_LOOKUP_COLOR")
            .map(|value| match value.as_str() {
                "0" => false,
                "1" => true,
                _ => panic!("lookup-color must be 0 or 1"),
            })
            .unwrap_or(false),
        scalar_color: std::env::var("TOWAVUE_NAV_SCALAR_COLOR")
            .map(|value| match value.as_str() {
                "0" => false,
                "1" => true,
                _ => panic!("scalar-color must be 0 or 1"),
            })
            .unwrap_or(false),
    };
    assert!(
        [trial.parallel_color, trial.lookup_color, trial.scalar_color]
            .into_iter()
            .filter(|enabled| *enabled)
            .count()
            <= 1,
        "choose one color control"
    );
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
