//! Application entry point for towavue.

#![forbid(unsafe_code)]

use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use towavue_core::{MediaTime, PlaybackState};
use towavue_runtime_windows::{
    AudioOutputEvent, PlaybackEvent, PlaybackSession, SoftwareFrameRenderer, VideoFrame,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const AUDIO_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(20);

fn main() -> Result<(), Box<dyn Error>> {
    let path = parse_media_path()?;
    let event_loop = EventLoop::<PlaybackEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut application = Application::new(path, move |event| {
        let _ = proxy.send_event(event);
    });
    event_loop.run_app(&mut application)?;
    Ok(())
}

fn parse_media_path() -> Result<PathBuf, Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let path = arguments.next().ok_or("usage: towavue-app <media-file>")?;
    if arguments.next().is_some() {
        return Err("towavue accepts exactly one media file".into());
    }
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err(format!("media file does not exist: {}", path.display()).into());
    }
    Ok(path)
}

struct PlaybackClock {
    media_anchor: MediaTime,
    wall_anchor: Instant,
    paused_at: Option<Instant>,
}

impl PlaybackClock {
    fn new(media_anchor: MediaTime) -> Self {
        Self {
            media_anchor,
            wall_anchor: Instant::now(),
            paused_at: None,
        }
    }

    fn set_paused(&mut self, paused: bool) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(Instant::now()),
            (false, Some(paused_at)) => {
                self.wall_anchor += Instant::now().saturating_duration_since(paused_at);
                self.paused_at = None;
            }
            _ => {}
        }
    }

    fn due_at(&self, media_time: MediaTime) -> Instant {
        let delta = media_time
            .as_nanoseconds()
            .saturating_sub(self.media_anchor.as_nanoseconds());
        self.wall_anchor + Duration::from_nanos(delta.max(0) as u64)
    }
}

struct Application<N> {
    path: PathBuf,
    notify: Option<N>,
    window: Option<Window>,
    renderer: Option<SoftwareFrameRenderer>,
    session: Option<PlaybackSession>,
    pending_frame: Option<VideoFrame>,
    clock: Option<PlaybackClock>,
    state: PlaybackState,
    decode_finished: bool,
    audio_drained: bool,
}

impl<N> Application<N>
where
    N: Fn(PlaybackEvent) + Send + 'static,
{
    fn new(path: PathBuf, notify: N) -> Self {
        Self {
            path,
            notify: Some(notify),
            window: None,
            renderer: None,
            session: None,
            pending_frame: None,
            clock: None,
            state: PlaybackState::Loading,
            decode_finished: false,
            audio_drained: false,
        }
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let title = self.title();
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(960, 576));
        let window = event_loop.create_window(attributes)?;
        let renderer = SoftwareFrameRenderer::new(&window)?;
        let notify = self
            .notify
            .take()
            .ok_or("playback notification callback is unavailable")?;
        let session = PlaybackSession::open(&self.path, notify)?;
        self.audio_drained = !session.has_audio();
        self.state = PlaybackState::Playing;
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.session = Some(session);
        self.refresh_title();
        Ok(())
    }

    fn handle_playback_event(&mut self, event: PlaybackEvent) {
        match event {
            PlaybackEvent::VideoReady => self.load_next_frame(),
            PlaybackEvent::DecodeFinished => {
                self.decode_finished = true;
                self.check_eof();
            }
            PlaybackEvent::Failed(error) => self.fail(error),
        }
    }

    fn load_next_frame(&mut self) {
        if self.pending_frame.is_some() {
            return;
        }
        let Some(frame) = self
            .session
            .as_ref()
            .and_then(PlaybackSession::try_video_frame)
        else {
            self.check_eof();
            return;
        };
        if self.clock.is_none() {
            self.clock = Some(PlaybackClock::new(frame.presentation_time));
        }
        self.pending_frame = Some(frame);
    }

    fn present_pending_frame(&mut self) {
        let Some(frame) = self.pending_frame.take() else {
            return;
        };
        if let Some(renderer) = self.renderer.as_mut()
            && let Err(error) = renderer.present(&frame)
        {
            self.fail(error.to_string());
            return;
        }
        self.load_next_frame();
        self.check_eof();
    }

    fn toggle_pause(&mut self) {
        let paused = match self.state {
            PlaybackState::Playing => true,
            PlaybackState::Paused => false,
            _ => return,
        };
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if let Err(error) = session.set_paused(paused) {
            self.fail(error.to_string());
            return;
        }
        if let Some(clock) = self.clock.as_mut() {
            clock.set_paused(paused);
        }
        self.state = if paused {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        };
        self.refresh_title();
    }

    fn poll_audio(&mut self) {
        let event = self
            .session
            .as_ref()
            .and_then(PlaybackSession::try_audio_event);
        match event {
            Some(AudioOutputEvent::Drained) => {
                self.audio_drained = true;
                self.check_eof();
            }
            Some(AudioOutputEvent::Failed(error)) => self.fail(error),
            None => {}
        }
    }

    fn check_eof(&mut self) {
        if self.decode_finished && self.pending_frame.is_none() && self.audio_drained {
            self.state = PlaybackState::Ended;
            self.refresh_title();
        }
    }

    fn fail(&mut self, error: String) {
        eprintln!("towavue: {error}");
        self.state = PlaybackState::Faulted;
        self.refresh_title();
    }

    fn refresh_title(&self) {
        if let Some(window) = &self.window {
            window.set_title(&self.title());
        }
    }

    fn title(&self) -> String {
        let name = self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into(),
        );
        format!("{name} — towavue ({:?})", self.state)
    }

    fn schedule(&mut self, event_loop: &ActiveEventLoop) {
        self.poll_audio();
        if self.state == PlaybackState::Playing {
            if let (Some(window), Some(frame), Some(clock)) =
                (&self.window, &self.pending_frame, &self.clock)
            {
                let due_at = clock.due_at(frame.presentation_time);
                if due_at <= Instant::now() {
                    window.request_redraw();
                    event_loop.set_control_flow(ControlFlow::Wait);
                    return;
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(due_at));
                return;
            }
            if !self.audio_drained {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + AUDIO_EVENT_POLL_INTERVAL,
                ));
                return;
            }
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

impl<N> ApplicationHandler<PlaybackEvent> for Application<N>
where
    N: Fn(PlaybackEvent) + Send + 'static,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none()
            && let Err(error) = self.start(event_loop)
        {
            eprintln!("towavue: {error}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: PlaybackEvent) {
        self.handle_playback_event(event);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(Window::id) != Some(window_id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::Space),
                        state: ElementState::Pressed,
                        repeat: false,
                        ..
                    },
                ..
            } => self.toggle_pause(),
            WindowEvent::RedrawRequested if self.state == PlaybackState::Playing => {
                self.present_pending_frame();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule(event_loop);
    }
}
