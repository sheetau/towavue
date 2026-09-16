use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

const TEST: &str = "window_host::resume_tests::hosted_resume_process_restart_and_transfer";
const PHASE: &str = "TOWAVUE_RESUME_PROCESS_PHASE";

#[test]
#[ignore = "three visible owned-window processes; generated video, isolated settings and hardware D3D11"]
fn hosted_resume_process_restart_and_transfer() {
    let Some(root) = crate::tests::isolated_test_root(TEST) else {
        return;
    };
    let path = root.join("resume-process.mp4");
    if let Ok(phase) = std::env::var(PHASE) {
        run_process(path, phase.parse().expect("phase"));
        return;
    }
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("FFmpeg")).join("bin/ffmpeg.exe");
    let result = std::process::Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=25:duration=4",
            "-an",
            "-c:v",
            "mpeg4",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&path)
        .output()
        .expect("generated video");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = std::fs::read(&path).expect("original bytes");
    let modified = std::fs::metadata(&path)
        .expect("metadata")
        .modified()
        .expect("mtime");
    // Each child creates the production WindowHost with the same isolated APPDATA.
    // The normal window close must drain its writes before the next process opens.
    for phase in 0..3 {
        let result = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                TEST,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(PHASE, phase.to_string())
            .output()
            .expect("resume process");
        eprint!("{}", String::from_utf8_lossy(&result.stderr));
        assert!(
            result.status.success(),
            "phase {phase}: {}\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout)
        );
    }
    assert_eq!(std::fs::read(&path).expect("source bytes"), bytes);
    assert_eq!(
        std::fs::metadata(path)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        modified
    );
    eprintln!(
        "PASS visible resume: pending/live host transfer, normal close, three process launches and source preservation"
    );
}

struct Trial {
    host: WindowHost,
    phase: u32,
    source: WindowKey,
    active: WindowKey,
    opened: bool,
    moved: bool,
    complete: bool,
    deadline: Instant,
    failure: Option<String>,
}

impl Trial {
    fn target(&self) -> MediaTime {
        MediaTime::from_nanoseconds(if self.phase == 0 {
            1_000_000_000
        } else {
            2_000_000_000
        })
    }

    fn step(&mut self, event_loop: &ActiveEventLoop) -> Result<bool, String> {
        if Instant::now() >= self.deadline {
            return Err(format!("resume phase {} timed out", self.phase));
        }
        let target = self.target();
        let app = self
            .host
            .windows
            .get_mut(&self.active)
            .ok_or("active window closed")?;
        if let Some(error) = &app.playback_error {
            return Err(error.clone());
        }
        let Some(session) = &app.session else {
            return Ok(false);
        };
        if !self.opened {
            let expected = MediaTime::from_nanoseconds(i64::from(self.phase) * 1_000_000_000);
            if session.target() != expected || app.resume_owner.is_none() {
                return Err(format!(
                    "phase {} restored {:?}, expected {expected:?}",
                    self.phase,
                    session.target()
                ));
            }
            app.toggle_pause();
            app.seek_to(target);
            self.opened = true;
            return Ok(false);
        }
        if session.current_video_time() != Some(target)
            || session.metrics().presented_frame_count == 0
        {
            return Ok(false);
        }
        if app.current_position() != target || app.state != PlaybackState::Paused {
            return Err(format!(
                "paused resume position changed: phase={} moved={} state={:?} position={:?} target={target:?} session_target={:?} clock={:?}",
                self.phase,
                self.moved,
                app.state,
                app.current_position(),
                session.target(),
                app.clock.as_ref().map(PlaybackClock::position)
            ));
        }
        if self.phase == 0 && !self.moved {
            let generation = app.generation;
            let request = app.tab_detach_request(app.tabs.active().ok_or("active tab")?.id)?;
            self.host.move_tab(self.active, self.source, &request, 0)?;
            self.active = self.source;
            let app = self
                .host
                .windows
                .get_mut(&self.active)
                .ok_or("destination")?;
            if app.resume_owner.is_none()
                || app.generation != generation
                || app.current_position() != target
            {
                return Err("live transfer lost resume ownership or reopened playback".into());
            }
            app.render_frame();
            if app.playback_error.is_some()
                || app
                    .session
                    .as_ref()
                    .and_then(PlaybackSession::current_video_time)
                    != Some(target)
            {
                return Err("transferred frame did not render".into());
            }
            self.moved = true;
            return Ok(false);
        }
        let ids: Vec<_> = self
            .host
            .windows
            .values()
            .filter_map(|app| app.window.as_ref().map(|window| window.id()))
            .collect();
        for id in ids {
            self.host
                .window_event(event_loop, id, WindowEvent::CloseRequested);
        }
        self.host.prepare_wait();
        if !self.host.windows.is_empty() {
            return Err("normal close did not remove every owned window".into());
        }
        eprintln!(
            "PASS resume process phase={} pid={} restored={}s closed={}s",
            self.phase,
            std::process::id(),
            self.phase,
            target.as_seconds_f64()
        );
        Ok(true)
    }
}

impl ApplicationHandler<Event> for Trial {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.phase == 0 {
            self.active = self.host.add_application(None).expect("destination window");
        }
        self.host.start_pending(event_loop, true);
        if self.host.windows.values().any(|app| {
            app.renderer.is_none()
                || app.window.as_ref().and_then(|window| window.is_visible()) != Some(true)
        }) {
            self.failure = Some("visible D3D11 startup failed".into());
            event_loop.exit();
            return;
        }
        if self.phase == 0 {
            // Transfer before any lookup callback is delivered. The old window's
            // late result must not open a second session or strand the new tab.
            let app = &self.host.windows[&self.source];
            assert_eq!(app.state, PlaybackState::Loading);
            assert!(app.session.is_none());
            let request = app
                .tab_detach_request(app.tabs.active().expect("initial tab").id)
                .expect("pending request");
            self.host
                .move_tab(self.source, self.active, &request, 0)
                .expect("pending transfer");
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        self.host.user_event(event_loop, event);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.host.window_event(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.complete && self.failure.is_none() {
            match self.step(event_loop) {
                Ok(true) => self.complete = true,
                Ok(false) => {}
                Err(error) => self.failure = Some(error),
            }
        }
        if self.complete || self.failure.is_some() {
            event_loop.set_control_flow(ControlFlow::Poll);
            event_loop.exit();
        } else {
            event_loop.set_control_flow(earliest_wait(
                self.host.prepare_wait(),
                ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16)),
            ));
        }
    }
}

fn run_process(path: PathBuf, phase: u32) {
    assert!(phase < 3);
    let mut builder = EventLoop::<Event>::with_user_event();
    builder.with_any_thread(true);
    let event_loop = builder.build().expect("event loop");
    let host = WindowHost::new(Some(path), Some(event_loop.create_proxy())).expect("host");
    let source = *host.windows.keys().next().expect("source");
    let mut trial = Trial {
        host,
        phase,
        source,
        active: source,
        opened: false,
        moved: false,
        complete: false,
        deadline: Instant::now() + Duration::from_secs(30),
        failure: None,
    };
    event_loop.run_app(&mut trial).expect("window host loop");
    assert!(
        trial.complete && trial.failure.is_none(),
        "resume failure: {:?}",
        trial.failure
    );
}
