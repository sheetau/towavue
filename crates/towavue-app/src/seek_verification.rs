//! Explicit reference probe, compiled only with presentation-verification.
use crate::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

type App = Application<Box<dyn Fn(AppEvent) + Send + Sync>>;

struct Sample {
    started: Instant,
    ready: Option<Duration>,
    command: Option<Duration>,
    target: MediaTime,
    pointer: Option<egui::Pos2>,
    release_pending: bool,
    latency_count: usize,
}

struct Trial {
    app: App,
    source: PathBuf,
    sample: Option<Sample>,
    index: usize,
    next_at: Instant,
    deadline: Instant,
    complete: bool,
}

impl Trial {
    fn observe(&mut self) {
        assert!(
            self.app.playback_error.is_none(),
            "{:?}",
            self.app.playback_error
        );
        if let Some(sample) = &mut self.sample {
            if self.app.pending_seek_started.is_some() {
                sample.command.get_or_insert(sample.started.elapsed());
                if self.app.pending_time.is_some() {
                    sample.ready.get_or_insert(sample.started.elapsed());
                }
            }
            if self.app.seek_latencies.len() > sample.latency_count {
                let elapsed = sample.started.elapsed();
                let session = self.app.session.as_ref().expect("session");
                assert!(!session.video_refresh_pending(), "fresh frame submitted");
                assert!(
                    session
                        .target()
                        .as_nanoseconds()
                        .abs_diff(sample.target.as_nanoseconds())
                        < 1_000_000_000
                );
                assert!(
                    session
                        .current_source_video_time()
                        .is_some_and(|time| time >= session.target())
                );
                eprintln!(
                    "APP_SEEK index={} pointer={} playing={} target_s={:.3} command_ms={:.3} ready_ms={:?} submitted_ms={:.3} production_ms={:.3} hardware={} transfers={}",
                    self.index,
                    sample.pointer.is_some(),
                    self.index >= 6,
                    session.target().as_seconds_f64(),
                    sample.command.expect("seek dispatched").as_secs_f64() * 1000.0,
                    sample.ready.map(|time| time.as_secs_f64() * 1000.0),
                    elapsed.as_secs_f64() * 1000.0,
                    self.app
                        .seek_latencies
                        .last()
                        .expect("latency")
                        .as_secs_f64()
                        * 1000.0,
                    session.metrics().hardware_frame_count,
                    session.metrics().cpu_transfer_count
                );
                self.index += 1;
                self.sample = None;
                self.next_at = Instant::now() + Duration::from_millis(100);
            }
        }
    }

    fn begin(&mut self) {
        let playing = self.index >= 6;
        if (self.app.state == PlaybackState::Playing) != playing {
            self.app.toggle_pause();
        }
        let fraction = [0.5, 0.75, 0.25][self.index % 3];
        let duration = self.app.playback_duration().expect("duration");
        let target = media_time(duration.mul_f64(fraction));
        let pointer = (self.index % 6 >= 3).then(|| {
            let context = self.app.ui_context.as_ref().expect("context");
            let screen = context.content_rect();
            egui::pos2(
                screen.left() + 5.0 + (screen.width() - 10.0) * fraction as f32,
                screen.bottom() - chrome::STATUS_HEIGHT,
            )
        });
        self.sample = Some(Sample {
            started: Instant::now(),
            ready: None,
            command: None,
            target,
            pointer,
            release_pending: pointer.is_some(),
            latency_count: self.app.seek_latencies.len(),
        });
        if let Some(position) = pointer {
            let input = self
                .app
                .ui_state
                .as_mut()
                .expect("UI state")
                .egui_input_mut();
            // Inject focused UI input without changing the desktop foreground window.
            input.focused = true;
            input.events.extend([
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            self.app.request_redraw();
        } else {
            self.app.handle_ui_action(UiAction::Seek(target));
            self.observe();
        }
    }
}

impl ApplicationHandler<window_host::Event> for Trial {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.app
            .start_on_device(event_loop, None, true)
            .expect("nonactivating production window");
        let tab = self
            .app
            .tabs
            .open_new(self.source.clone(), MediaKind::Video);
        self.app.edits.insert(tab, EditHistory::default());
        self.app.media_kind = Some(MediaKind::Video);
        self.app.set_playback_volume(0.0);
        self.app.load_path(self.source.clone(), MediaKind::Video);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: window_host::Event) {
        match event {
            window_host::Event::Window(_, event) => self.app.user_event(event_loop, event),
            window_host::Event::Accessibility(event) => {
                self.app.handle_app_event(AppEvent::Accessibility(event))
            }
            _ => panic!("unexpected host event"),
        }
        self.observe();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let redraw = matches!(event, WindowEvent::RedrawRequested);
        self.observe();
        self.app.window_event(event_loop, id, event);
        self.observe();
        if redraw
            && let Some(sample) = &mut self.sample
            && sample.release_pending
        {
            sample.release_pending = false;
            let position = sample.pointer.expect("pointer sample");
            self.app
                .ui_state
                .as_mut()
                .expect("UI state")
                .egui_input_mut()
                .events
                .push(egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
            self.app.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        assert!(
            Instant::now() < self.deadline,
            "bounded application trial, sample {}",
            self.index
        );
        self.observe();
        if self.index == 12 {
            self.complete = true;
            event_loop.exit();
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
        if self.sample.is_none()
            && Instant::now() >= self.next_at
            && self.app.media_duration.is_some()
            && self
                .app
                .session
                .as_ref()
                .is_some_and(|session| session.metrics().presented_frame_count > 0)
        {
            self.begin();
        }
        self.app.about_to_wait(event_loop);
        // Only the test's next command/deadline adds a wakeup; rendering and native
        // worker events retain the production scheduler without a busy frame loop.
        let deadline = if self.sample.is_none() {
            self.next_at.max(Instant::now() + Duration::from_millis(10))
        } else {
            self.deadline
        };
        match event_loop.control_flow() {
            ControlFlow::Poll => {}
            ControlFlow::Wait => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            ControlFlow::WaitUntil(at) => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(at.min(deadline)))
            }
        }
    }
}

pub(crate) fn run() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        panic!("use Release for timing");
    }
    let root = PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_VERIFICATION_ROOT").expect("explicit isolated root"),
    );
    assert!(root.is_absolute() && root.is_dir());
    assert!(
        root.file_name()
            .expect("root name")
            .to_string_lossy()
            .starts_with("towavue-seek-app-")
    );
    for (name, directory) in [("APPDATA", "config"), ("LOCALAPPDATA", "local")] {
        assert_eq!(
            std::env::var_os(name).map(PathBuf::from),
            Some(root.join(directory)),
            "isolated preferences required"
        );
    }
    let source = PathBuf::from(
        std::env::var_os("TOWAVUE_SEEK_REFERENCE_SOURCE").expect("explicit reference"),
    );
    let stamp = || {
        let m = std::fs::metadata(&source).expect("source");
        (m.len(), m.modified().expect("mtime"))
    };
    let before = stamp();
    let event_loop = EventLoop::<window_host::Event>::with_user_event()
        .with_any_thread(true)
        .build()
        .expect("native event loop");
    let proxy = event_loop.create_proxy();
    let notify = proxy.clone();
    let mut app: App = Application::new_with_preview_cache(
        None,
        Box::new(move |event| {
            let _ = notify.send_event(window_host::Event::Window(super::WindowKey(0), event));
        }) as Box<dyn Fn(AppEvent) + Send + Sync>,
        PreviewCache::local().expect("isolated preview cache"),
    )
    .expect("isolated app");
    app.event_loop_proxy = Some(proxy);
    let mut trial = Trial {
        app,
        source: source.clone(),
        sample: None,
        index: 0,
        next_at: Instant::now(),
        deadline: Instant::now() + Duration::from_secs(180),
        complete: false,
    };
    event_loop
        .run_app(&mut trial)
        .expect("native application trial");
    assert!(
        trial.complete,
        "all command/pointer paused/playing samples completed"
    );
    drop(trial);
    assert_eq!(stamp(), before);
    eprintln!("APP_SEEK_CHECKS samples=12 fresh_frames=true source_stamps=true complete=true");
    Ok(())
}
