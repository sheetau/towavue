use super::*;
use crate::*;
use towavue_core::{TimeRange, TimelineEdit};

fn time(ms: i64) -> MediaTime {
    MediaTime::from_nanoseconds(ms * 1_000_000)
}

#[test]
fn audio_steps_ignore_unloaded_or_wrong_media_without_pausing() {
    let Some(_root) = crate::tests::isolated_test_root(
        "frame_step::audio_tests::audio_steps_ignore_unloaded_or_wrong_media_without_pausing",
    ) else {
        return;
    };
    let mut app = Application::new(None, |_| {}).expect("app");
    for kind in [
        None,
        Some(MediaKind::Audio),
        Some(MediaKind::Video),
        Some(MediaKind::Image),
    ] {
        app.media_kind = kind;
        app.state = PlaybackState::Playing;
        app.step_audio(true);
        assert_eq!(app.state, PlaybackState::Playing);
        assert!(app.edits.is_empty());
    }
}

#[test]
#[ignore = "requires a Windows D3D11 device and shared WASAPI endpoint; only owned silence is played"]
fn audio_steps_pause_accumulate_clamp_and_follow_the_edited_time_axis() {
    let Some(root) = crate::tests::isolated_test_root(
        "frame_step::audio_tests::audio_steps_pause_accumulate_clamp_and_follow_the_edited_time_axis",
    ) else {
        return;
    };
    let path = root.join("steps.wav");
    use std::os::windows::process::CommandExt;
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
        "anullsrc=r=48000:cl=stereo",
        "-t",
        "2",
        "-c:a",
        "pcm_s16le",
    ])
    .arg(&path)
    .output()
    .expect("silent fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    struct Trial {
        path: PathBuf,
    }
    impl winit::application::ApplicationHandler for Trial {
        fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(winit::window::Window::default_attributes().with_visible(false))
                    .expect("owned hidden window"),
            );
            let renderer = match FrameRenderer::new(&window) {
                Ok(renderer) => renderer,
                Err(error) => {
                    eprintln!("SKIP audio steps: D3D11 unavailable: {error}");
                    event_loop.exit();
                    return;
                }
            };
            let original = std::fs::read(&self.path).expect("source");
            let mut app = Application::new(None, |_| {}).expect("app");
            app.window = Some(window);
            app.renderer = Some(renderer);
            app.ui_context = Some(fonts::test_context());
            app.shortcuts = shortcuts::defaults();
            let tab = app.tabs.open_new(self.path.clone(), MediaKind::Audio);
            app.load_path(self.path.clone(), MediaKind::Audio);
            if app.state == PlaybackState::Faulted
                && app.playback_error.as_deref().is_some_and(|error| {
                    error.starts_with("audio output failed: WASAPI output failed:")
                })
            {
                eprintln!(
                    "SKIP audio steps: shared WASAPI unavailable: {:?}",
                    app.playback_error
                );
                event_loop.exit();
                return;
            }
            assert_eq!(
                app.state,
                PlaybackState::Playing,
                "{:?}",
                app.playback_error
            );
            app.media_duration = Some(Duration::from_secs(2));
            app.push_edit(EditOperation::SetVolume(0.0));
            app.toggle_pause();
            app.seek_to(time(400));
            let selection = TimeRange::new(time(100), time(200));
            app.time_selection = selection;
            let history = app.edits.clone();
            for index in 1..=10 {
                app.process_shortcut(".".parse().expect("key"));
                assert_eq!(app.current_position(), time(400 + index * 10));
                assert_eq!(app.state, PlaybackState::Paused);
            }
            for index in 1..=10 {
                app.process_shortcut(",".parse().expect("key"));
                assert_eq!(app.current_position(), time(500 - index * 10));
            }
            assert_eq!(app.time_selection, selection);
            assert_eq!(app.edits, history);
            app.toggle_pause();
            let before = app.current_position();
            app.dispatch(CommandId::StepAudioForward);
            assert_eq!(app.state, PlaybackState::Paused);
            assert!(app.current_position() >= before.saturating_add(Duration::from_millis(10)));
            assert!(app.current_position() < before.saturating_add(Duration::from_millis(100)));
            app.seek_to(time(5));
            app.dispatch(CommandId::StepAudioBackward);
            assert_eq!(app.current_position(), MediaTime::ZERO);
            app.seek_to(time(1995));
            app.dispatch(CommandId::StepAudioForward);
            assert_eq!(app.current_position(), time(2000));
            app.seek_to(time(1990));
            app.toggle_pause();
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.state != PlaybackState::Ended && Instant::now() < deadline {
                app.poll_audio();
                app.advance_media();
                app.decode_finished = app.session.as_ref().expect("session").decode_finished();
                app.check_eof();
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(app.state, PlaybackState::Ended, "real silent EOF");
            app.dispatch(CommandId::StepAudioBackward);
            assert_eq!(
                (app.current_position(), app.state),
                (time(1990), PlaybackState::Paused)
            );
            app.resize_dialog = Some(resize::ResizeDialog::new((10, 10)));
            let generation = app.generation;
            app.dispatch(CommandId::StepAudioBackward);
            assert_eq!(app.generation, generation);
            app.resize_dialog = None;

            app.push_edit(EditOperation::SetRate(2.0));
            app.push_edit(EditOperation::Timeline(TimelineEdit::Delete(
                TimeRange::new(time(500), time(1000)).expect("delete"),
            )));
            app.push_edit(EditOperation::Timeline(TimelineEdit::Stretch(
                TimeRange::new(time(0), time(200)).expect("stretch"),
                time(400),
            )));
            let plan = app.history_timeline().expect("plan").expect("edited time");
            let history = app.edits.clone();
            app.seek_to(time(695));
            app.dispatch(CommandId::StepAudioForward);
            assert_eq!(app.current_position(), time(705));
            assert_eq!(app.playback_rate(), 2.0);
            app.seek_to(plan.duration().saturating_sub(Duration::from_millis(5)));
            app.dispatch(CommandId::StepAudioForward);
            assert_eq!(app.current_position(), plan.duration());
            let range = TimeRange::new(time(100), time(200)).expect("range");
            app.playback_selection = Some(range);
            app.seek_to(time(195));
            app.dispatch(CommandId::StepAudioForward);
            assert_eq!(app.current_position(), time(205));
            assert!(app.playback_selection.is_none());
            assert_eq!(app.edits, history);
            assert_eq!(app.tabs.active().expect("tab").id, tab);
            let paused = app.current_position();
            std::thread::sleep(Duration::from_millis(30));
            app.poll_audio();
            assert_eq!(
                (app.current_position(), app.state),
                (paused, PlaybackState::Paused)
            );
            assert_eq!(
                std::fs::read(&self.path).expect("unchanged source"),
                original
            );
            eprintln!(
                "PASS audio steps: 10 ms accumulated paused Seek, playing/EOF, bounds, modal, edited Delete/Stretch/rate, selection exit, unchanged source/history"
            );
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &winit::event_loop::ActiveEventLoop,
            _: winit::window::WindowId,
            _: winit::event::WindowEvent,
        ) {
        }
    }
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let mut builder = winit::event_loop::EventLoop::builder();
    builder.with_any_thread(true);
    builder
        .build()
        .expect("loop")
        .run_app(&mut Trial { path })
        .expect("trial");
}
