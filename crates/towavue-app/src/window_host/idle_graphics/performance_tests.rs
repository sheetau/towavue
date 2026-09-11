use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[derive(Debug)]
enum Phase {
    Loading(usize),
    Closing,
    Reopening,
    Done,
}

fn report(app: &WindowApplication, phase: &str) {
    let renderer = app.renderer.as_ref().expect("renderer");
    let memory = renderer.verification_memory().expect("memory counters");
    let managed = renderer.verification_managed_textures();
    let bytes: usize = managed.iter().map(|(_, [w, h])| w * h * 4).sum();
    let mib = |bytes: u64| bytes as f64 / 1048576.0;
    eprintln!(
        "HOST100 phase={phase} managed_count={} managed_rgba_mib={:.2} working_mib={:.1} private_mib={:.1} peak_commit_mib={:.1} gpu_local_nonlocal_mib={:?}",
        managed.len(),
        mib(bytes as u64),
        mib(memory.working_set),
        mib(memory.private_bytes),
        mib(memory.peak_commit),
        memory.gpu_local_nonlocal.as_ref().ok().map(|v| v.map(mib))
    );
    if let Err(error) = memory.gpu_local_nonlocal {
        eprintln!("SKIP HOST100 GPU counters: {error}");
    }
}

fn samples(app: &mut WindowApplication) -> Vec<[u8; 4]> {
    let width = app.window.as_ref().expect("window").inner_size().width as usize;
    // Flip-discard contents are not guaranteed after Present. Validate a separate
    // redraw before presenting it; navigation/reopen timing has already stopped.
    let context = app.ui_context.clone().expect("context");
    let input = context.input(|input| input.raw.clone());
    let mut actions = Vec::new();
    let output = context.run_ui(input, |ui| app.draw_ui(ui, &mut actions));
    assert!(
        actions.is_empty(),
        "validation must not dispatch UI actions"
    );
    let renderer = app.renderer.as_mut().expect("renderer");
    renderer.clear([0.0, 0.0, 0.0, 1.0]).expect("clear");
    renderer
        .render_ui(&context, output)
        .expect("validation draw");
    let pixels = renderer.verification_surface_rgba().expect("readback");
    renderer.present_surface().expect("validation Present");
    let height = pixels.len() / (width * 4);
    (0..64)
        .map(|i| {
            let x = width / 4 + (i % 8) * width / 16;
            let y = height / 4 + (i / 8) * height / 16;
            pixels[(y * width + x) * 4..(y * width + x) * 4 + 4]
                .try_into()
                .expect("RGBA")
        })
        .collect()
}

struct Trial {
    host: WindowHost,
    paths: Vec<PathBuf>,
    phase: Phase,
    started: Instant,
    expected: Vec<[u8; 4]>,
    originals: Vec<std::sync::Weak<DecodedImage>>,
    textures: Vec<egui::TextureId>,
    ready: Vec<Duration>,
    failure: Option<String>,
}

impl Trial {
    fn presented(&mut self) {
        let app = self.host.windows.values_mut().next().expect("window");
        assert!(app.renderer.is_some() && app.playback_error.is_none());
        assert!(app.image_error.is_none(), "{:?}", app.image_error);
        if app.image_loading
            || app.image.is_none()
            || app.image_handoff.is_some()
            || app.image_sequence.awaiting.is_some()
        {
            return;
        }
        let index = match self.phase {
            Phase::Loading(index) => index,
            Phase::Reopening => 99,
            _ => return,
        };
        assert_eq!(app.path.as_ref(), Some(&self.paths[index]));
        let image = app.image.as_ref().expect("original");
        assert_eq!(image.dimensions(), (4096, 2304));
        assert!(
            app.renderer
                .as_ref()
                .expect("renderer")
                .verification_managed_textures()
                .iter()
                .any(|(id, _)| *id == image.texture.id())
        );
        let ready = self.started.elapsed();
        if matches!(self.phase, Phase::Reopening) {
            eprintln!(
                "HOST100 reopen_present_ms={:.3}; excludes validation readback",
                ready.as_secs_f64() * 1000.0
            );
            assert_eq!(samples(app), self.expected, "reopened original GPU samples");
            report(app, "reopened");
            self.host.prepare_wait();
            assert!(self.host.idle_graphics.is_none());
            self.phase = Phase::Done;
        } else {
            self.ready.push(ready);
            if index % 10 == 0 {
                eprintln!("HOST100 reached original {index}");
            }
            if index == 99 {
                report(app, "before_close");
                self.expected = samples(app);
                let distinct: std::collections::HashSet<_> = self.expected.iter().collect();
                assert!(
                    distinct.len() > 4,
                    "original samples must not be a blank surface"
                );
                self.originals = app
                    .image_texture_cache
                    .entries
                    .iter()
                    .map(|image| Arc::downgrade(&image.decoded))
                    .collect();
                self.textures = app
                    .image_texture_cache
                    .entries
                    .iter()
                    .map(|image| image.texture.id())
                    .collect();
                assert!(!self.originals.is_empty());
                self.started = Instant::now();
                app.close_tab_unchecked(app.tabs.active().expect("last tab").id);
                self.phase = Phase::Closing;
            } else {
                self.phase = Phase::Loading(index + 1);
                self.started = Instant::now();
                app.navigate_to_unchecked(self.paths[index + 1].clone());
            }
        }
    }
}

impl ApplicationHandler<Event> for Trial {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.host.start_pending(event_loop, true);
        if self.host.windows.values().any(|app| app.renderer.is_none()) {
            eprintln!("SKIP HOST100: hardware D3D11 surface unavailable");
            event_loop.set_control_flow(ControlFlow::Poll);
            event_loop.exit();
            return;
        }
        self.host
            .windows
            .values_mut()
            .next()
            .expect("window")
            .fullscreen = true;
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        self.host.user_event(event_loop, event);
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let redraw = matches!(event, WindowEvent::RedrawRequested);
        self.host.window_event(event_loop, id, event);
        if redraw {
            self.presented();
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.host.about_to_wait(event_loop);
        if self.started.elapsed() >= Duration::from_secs(30) {
            let app = self.host.windows.values().next().expect("window");
            self.failure = Some(format!(
                "HOST100 timeout {:?}: loading={} image={} handoff={} awaiting={:?} path={:?}",
                self.phase,
                app.image_loading,
                app.image.is_some(),
                app.image_handoff.is_some(),
                app.image_sequence.awaiting,
                app.path
            ));
            // Unwind outside the Windows callback, after the event loop exits.
            event_loop.set_control_flow(ControlFlow::Poll);
            event_loop.exit();
            return;
        }
        if matches!(self.phase, Phase::Closing)
            && self
                .host
                .idle_graphics
                .as_ref()
                .is_some_and(|idle| idle.attempted)
        {
            assert!(self.started.elapsed() >= Duration::from_secs(1));
            eprintln!(
                "HOST100 close_to_idle_ms={:.3}; real event-loop timer, no diagnostic trim call",
                self.started.elapsed().as_secs_f64() * 1000.0
            );
            let app = self.host.windows.values_mut().next().expect("window");
            assert!(self.originals.iter().all(|image| image.upgrade().is_none()));
            let owned = app
                .renderer
                .as_ref()
                .expect("renderer")
                .verification_managed_textures();
            assert!(
                self.textures
                    .iter()
                    .all(|id| !owned.iter().any(|(owned_id, _)| id == owned_id))
            );
            report(app, "after_production_idle");
            self.phase = Phase::Reopening;
            self.started = Instant::now();
            app.open_external(self.paths[99].clone(), false);
        }
        if matches!(self.phase, Phase::Done) {
            let mut ready = self.ready[1..].to_vec();
            ready.sort_unstable();
            eprintln!(
                "HOST100 originals=100 subsequent_ready_median_ms={:.3}; serial explicit navigation, warm generated JPEGs, visible hardware window; not burst/physical input or a trim-vs-no-trim latency comparison",
                ready[ready.len() / 2].as_secs_f64() * 1000.0
            );
            event_loop.set_control_flow(ControlFlow::Poll);
            event_loop.exit();
        } else {
            event_loop.set_control_flow(earliest_wait(
                event_loop.control_flow(),
                ControlFlow::WaitUntil(self.started + Duration::from_secs(30)),
            ));
        }
    }
}

#[test]
#[ignore = "opens a visible window and generates 100 large JPEGs; requires hardware D3D11 and FFMPEG_DIR; performance needs --release"]
fn hundred_originals_close_and_reopen_through_production_host() {
    if cfg!(debug_assertions) {
        eprintln!("HOST100 DEBUG: correctness only; timings are not performance evidence");
    }
    let Some(root) = crate::tests::isolated_test_root(
        "window_host::idle_graphics::performance_tests::hundred_originals_close_and_reopen_through_production_host",
    ) else {
        return;
    };
    let paths = crate::image_navigation::performance_tests::large_jpeg_fixture(&root);
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    let event_loop = builder.build().expect("event loop");
    let host =
        WindowHost::new(Some(paths[0].clone()), Some(event_loop.create_proxy())).expect("host");
    let mut trial = Trial {
        host,
        paths,
        phase: Phase::Loading(0),
        started: Instant::now(),
        expected: Vec::new(),
        originals: Vec::new(),
        textures: Vec::new(),
        ready: Vec::new(),
        failure: None,
    };
    event_loop.run_app(&mut trial).expect("host measurement");
    assert_eq!(trial.failure, None);
}
