use super::*;
use crate::{CommandId, Duration, EditOperation, FrameRenderer, Instant, PathBuf, shortcuts};
use std::sync::mpsc;
use towavue_core::{TimeRange, TimelineEdit};
use towavue_runtime_windows::{DecodeOutput, decode_file};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

fn time(ns: i64) -> MediaTime {
    MediaTime::from_nanoseconds(ns)
}

fn settle<N: Fn(AppEvent) + Send + Sync + 'static>(
    app: &mut Application<N>,
    rx: &mpsc::Receiver<AppEvent>,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        for event in rx.try_iter() {
            if let AppEvent::FrameStep(_, _) = event {
                app.handle_app_event(event);
            }
        }
        app.load_next_frame();
        app.advance_media();
        if app.frame_steps.pending.is_none()
            && app.frame_steps.queued.is_empty()
            && !app
                .session
                .as_ref()
                .expect("session")
                .video_refresh_pending()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "frame presentation timeout: {:?}",
            app.status_message
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(app.state, PlaybackState::Paused);
}

#[test]
fn asynchronous_steps_present_actual_vfr_pts_and_reject_obsolete_results() {
    let Some(root) = crate::tests::isolated_test_root(
        "frame_step::tests::asynchronous_steps_present_actual_vfr_pts_and_reject_obsolete_results",
    ) else {
        return;
    };
    let path = root.join("frame-step.mp4");
    let ffmpeg =
        PathBuf::from(std::env::var_os("FFMPEG_DIR").expect("fixed FFmpeg")).join("bin/ffmpeg.exe");
    assert!(
        std::process::Command::new(ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x96:rate=25:duration=2",
                "-vf",
                "select='not(eq(mod(n,3),1))'",
                "-fps_mode",
                "vfr",
                "-c:v",
                "mpeg4",
                "-bf",
                "2",
                "-g",
                "8",
            ])
            .arg(&path)
            .status()
            .expect("VFR fixture")
            .success()
    );
    let mut reference = Vec::new();
    decode_file(&path, |output| {
        if let DecodeOutput::Video(frame) = output {
            reference.push(frame.presentation_time);
        }
        true
    })
    .expect("reference decode");
    assert!(
        reference
            .windows(3)
            .any(|w| w[1].as_nanoseconds() - w[0].as_nanoseconds()
                != w[2].as_nanoseconds() - w[1].as_nanoseconds())
    );
    struct Trial {
        path: PathBuf,
        reference: Vec<MediaTime>,
    }
    impl ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .expect("hidden window"),
            );
            let renderer = match FrameRenderer::new(&window) {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("SKIP frame stepping: D3D11 unavailable: {error}");
                    event_loop.exit();
                    return;
                }
            };
            let (tx, rx) = mpsc::channel();
            let mut app = Application::new(None, move |event| {
                let _ = tx.send(event);
            })
            .expect("app");
            app.window = Some(window);
            app.renderer = Some(renderer);
            let tab = app.tabs.open_new(self.path.clone(), MediaKind::Video);
            app.load_path(self.path.clone(), MediaKind::Video);
            app.media_duration = Some(Duration::from_secs(2));
            app.shortcuts = shortcuts::defaults();
            app.toggle_pause();
            settle(&mut app, &rx);
            let displayed = |app: &Application<_>| {
                app.session
                    .as_ref()
                    .expect("session")
                    .current_video_time()
                    .expect("frame")
            };
            assert_eq!(displayed(&app), self.reference[0]);
            app.toggle_pause();
            app.process_shortcut(".".parse().expect("next"));
            assert_eq!(app.state, PlaybackState::Paused, "stepping pauses playback");
            settle(&mut app, &rx);
            assert_eq!(displayed(&app), self.reference[1]);
            let pending = app.pending_time.replace(self.reference[2]);
            let clock = app
                .clock
                .replace(crate::PlaybackClock::new(self.reference[10], 1.0));
            assert!(
                !app.frame_is_due(),
                "paused stepping holds the exact frame even if a master deadline is ahead"
            );
            app.pending_time = pending;
            app.clock = clock;
            // Inputs arrive before any result is handled; preserve order, not only the latest request.
            for key in [".", ".", ",", "."] {
                app.process_shortcut(key.parse().expect("step key"));
            }
            settle(&mut app, &rx);
            assert_eq!(displayed(&app), self.reference[3]);
            app.process_shortcut(",".parse().expect("previous"));
            settle(&mut app, &rx);
            assert_eq!(displayed(&app), self.reference[2]);
            assert!(!app.edits.contains_key(&tab));
            app.seek_to(MediaTime::ZERO);
            settle(&mut app, &rx);
            let generation = app.generation;
            app.dispatch(CommandId::PreviousVideoFrame);
            settle(&mut app, &rx);
            assert_eq!(app.generation, generation, "boundary does not seek");
            app.seek_to(time(2_000_000_000));
            settle(&mut app, &rx);
            assert_eq!(displayed(&app), *self.reference.last().expect("last"));
            app.dispatch(CommandId::PreviousVideoFrame);
            settle(&mut app, &rx);
            assert_eq!(
                displayed(&app),
                self.reference[self.reference.len() - 2],
                "step from the terminal displayed PTS, not duration"
            );
            app.seek_to(MediaTime::ZERO);
            settle(&mut app, &rx);
            for _ in 0..40 {
                app.dispatch(CommandId::NextVideoFrame);
            }
            assert_eq!(app.frame_steps.queued.len(), 31);
            settle(&mut app, &rx);
            assert_eq!(displayed(&app), self.reference[32]);

            // Both proactive invalidation and callback-side guards leave position/history untouched.
            for mode in 0..9 {
                app.seek_to(self.reference[3]);
                settle(&mut app, &rx);
                app.dispatch(CommandId::NextVideoFrame);
                let request = app.frame_steps.pending.expect("request");
                app.frame_steps.worker.clear();
                match mode {
                    0 => app.seek_to(self.reference[1]),
                    1 => app.cancel_frame_steps(),
                    2 => app.media_generation = app.media_generation.wrapping_add(1),
                    3 => app.generation = app.generation.next(),
                    4 => app.state = PlaybackState::Playing,
                    5 => app.export_error = Some("modal test".into()),
                    6 => app.palette_open = true,
                    7 => app.media_kind = Some(MediaKind::Audio),
                    8 => {
                        app.tabs
                            .open_new(PathBuf::from("other.png"), MediaKind::Image);
                    }
                    _ => unreachable!(),
                }
                let generation = app.generation;
                let target = app.session.as_ref().expect("session").target();
                app.finish_frame_step(request.serial, Ok(Some(self.reference[4])));
                assert_eq!(app.generation, generation);
                assert_eq!(app.session.as_ref().expect("session").target(), target);
                assert!(app.frame_steps.pending.is_none());
                assert!(app.frame_steps.queued.is_empty());
                app.tabs.activate(tab);
                app.state = PlaybackState::Paused;
                app.export_error = None;
                app.palette_open = false;
                app.media_kind = Some(MediaKind::Video);
            }
            app.seek_to(self.reference[2]);
            settle(&mut app, &rx);
            app.dispatch(CommandId::NextVideoFrame);
            let request = app.frame_steps.pending.expect("request");
            app.frame_steps.worker.clear();
            app.finish_frame_step(request.serial.wrapping_sub(1), Ok(Some(self.reference[9])));
            assert_eq!(
                app.frame_steps
                    .pending
                    .expect("new request preserved")
                    .serial,
                request.serial
            );
            app.finish_frame_step(request.serial, Err("synthetic query failure".into()));
            assert_eq!(app.state, PlaybackState::Paused);
            assert!(app.frame_steps.pending.is_none());
            assert!(!app.edits.contains_key(&tab));

            app.push_edit(EditOperation::Timeline(TimelineEdit::Delete(
                TimeRange::new(time(350_000_000), time(910_000_000)).expect("delete"),
            )));
            app.push_edit(EditOperation::Timeline(TimelineEdit::Stretch(
                TimeRange::new(time(200_000_000), time(700_000_000)).expect("stretch"),
                time(800_000_000),
            )));
            let history = app.edits[&tab].clone();
            let plan = app
                .history_timeline()
                .expect("timeline")
                .expect("edited plan");
            let edited: Vec<_> = self
                .reference
                .iter()
                .filter_map(|pts| plan.edited_time(*pts))
                .collect();
            app.seek_to(edited[0]);
            settle(&mut app, &rx);
            for expected in edited.iter().skip(1) {
                app.dispatch(CommandId::NextVideoFrame);
                settle(&mut app, &rx);
                assert_eq!(
                    displayed(&app),
                    *expected,
                    "forward across edited spans: target {:?}, source {:?}",
                    app.session.as_ref().expect("session").target(),
                    plan.source_time(*expected)
                );
            }
            for expected in edited.iter().rev().skip(1) {
                app.dispatch(CommandId::PreviousVideoFrame);
                settle(&mut app, &rx);
                assert_eq!(displayed(&app), *expected, "backward across edited spans");
            }
            assert_eq!(
                app.edits[&tab], history,
                "stepping never changes edit history"
            );
            eprintln!(
                "Frame step PASS: VFR presentation, ordered bounded input, terminal PTS, stale guards, delete/stretch and unchanged history"
            );
            event_loop.exit();
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("event loop")
        .run_app(&mut Trial { path, reference })
        .expect("frame trial");
}
