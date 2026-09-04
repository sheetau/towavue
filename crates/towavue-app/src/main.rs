//! Application entry point for towavue.

#![forbid(unsafe_code)]

use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use towavue_core::{MediaTime, PlaybackGeneration, PlaybackState};
use towavue_runtime_windows::{
    AudioOutputEvent, FrameRenderer, PlaybackEvent, PlaybackSession, RenderError,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const AUDIO_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const VIDEO_EARLY_TOLERANCE: Duration = Duration::from_millis(5);
const KEYBOARD_SEEK_STEP: Duration = Duration::from_secs(5);
const VIDEO_LATE_TOLERANCE: Duration = Duration::from_millis(40);

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

    fn position(&self) -> MediaTime {
        let now = self.paused_at.unwrap_or_else(Instant::now);
        self.media_anchor
            .saturating_add(now.saturating_duration_since(self.wall_anchor))
    }
}

struct Application<N> {
    path: PathBuf,
    notify: Option<N>,
    window: Option<Window>,
    renderer: Option<FrameRenderer>,
    session: Option<PlaybackSession>,
    pending_time: Option<MediaTime>,
    clock: Option<PlaybackClock>,
    state: PlaybackState,
    decode_finished: bool,
    audio_drained: bool,
    metrics_recorded: bool,
    generation: PlaybackGeneration,
    pending_seek_started: Option<Instant>,
    seek_latencies: Vec<Duration>,
    drift_samples: Vec<Duration>,
}

impl<N> Application<N>
where
    N: Fn(PlaybackEvent) + Send + Sync + 'static,
{
    fn new(path: PathBuf, notify: N) -> Self {
        Self {
            path,
            notify: Some(notify),
            window: None,
            renderer: None,
            session: None,
            pending_time: None,
            clock: None,
            state: PlaybackState::Loading,
            decode_finished: false,
            audio_drained: false,
            metrics_recorded: false,
            generation: PlaybackGeneration::INITIAL,
            pending_seek_started: None,
            seek_latencies: Vec::new(),
            drift_samples: Vec::new(),
        }
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let title = self.title();
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(960, 576));
        let window = event_loop.create_window(attributes)?;
        let renderer = FrameRenderer::new(&window)?;
        let graphics_device = renderer.graphics_device();
        let notify = self
            .notify
            .take()
            .ok_or("playback notification callback is unavailable")?;
        let session = PlaybackSession::open(&self.path, graphics_device, notify)?;
        self.generation = session.generation();
        self.audio_drained = !session.has_audio();
        self.state = PlaybackState::Playing;
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.session = Some(session);
        self.refresh_title();
        Ok(())
    }

    fn handle_playback_event(&mut self, event: PlaybackEvent) {
        if event.generation() != self.generation {
            return;
        }
        match event {
            PlaybackEvent::VideoReady(_) => {
                if let Some(started) = self.pending_seek_started.take() {
                    let latency = started.elapsed();
                    self.seek_latencies.push(latency);
                    eprintln!(
                        "towavue: seek_latency_ms={:.3}",
                        latency.as_secs_f64() * 1000.0
                    );
                }
                self.load_next_frame();
            }
            PlaybackEvent::DecodePathSelected(_, path) => {
                eprintln!("towavue: decode path selected: {path:?}");
            }
            PlaybackEvent::DecodeFinished(_) => {
                self.decode_finished = true;
                self.check_eof();
            }
            PlaybackEvent::DeviceRemoved(_, reason) => {
                eprintln!("towavue: recovering removed D3D11 device: {reason}");
                let position = self
                    .session
                    .as_ref()
                    .and_then(PlaybackSession::audio_position)
                    .or(self.pending_time)
                    .or_else(|| self.clock.as_ref().map(PlaybackClock::position))
                    .unwrap_or(MediaTime::ZERO);
                self.recover_graphics_device(position);
            }
            PlaybackEvent::Failed(_, error) => self.fail(error),
        }
    }

    fn load_next_frame(&mut self) {
        if self.pending_time.is_some() {
            return;
        }
        let Some(presentation_time) = self
            .session
            .as_mut()
            .and_then(PlaybackSession::pending_video_time)
        else {
            self.check_eof();
            return;
        };
        if self.clock.is_none() {
            self.clock = Some(PlaybackClock::new(presentation_time));
        }
        self.pending_time = Some(presentation_time);
    }

    fn present_pending_frame(&mut self) {
        let Some(video_time) = self.pending_time.take() else {
            return;
        };
        if let Some(audio_time) = self.audio_master_position() {
            self.drift_samples.push(Duration::from_nanos(
                video_time
                    .as_nanoseconds()
                    .abs_diff(audio_time.as_nanoseconds()),
            ));
        }
        if let (Some(session), Some(renderer)) = (self.session.as_mut(), self.renderer.as_mut())
            && let Err(error) = session.present_pending(renderer)
        {
            match error {
                RenderError::DeviceRemoved(reason) => {
                    eprintln!("towavue: recovering removed D3D11 device: {reason}");
                    self.recover_graphics_device(video_time);
                }
                other => self.fail(other.to_string()),
            }
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
        let Some(session) = self.session.as_mut() else {
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

    fn seek_relative(&mut self, forward: bool) {
        if !matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            return;
        }
        let position = self
            .session
            .as_ref()
            .and_then(PlaybackSession::audio_position)
            .or_else(|| self.clock.as_ref().map(PlaybackClock::position))
            .unwrap_or(MediaTime::ZERO);
        let target = if forward {
            position.saturating_add(KEYBOARD_SEEK_STEP)
        } else {
            position
                .saturating_sub(KEYBOARD_SEEK_STEP)
                .max(MediaTime::ZERO)
        };
        self.seek_to(target);
    }

    fn seek_to(&mut self, target: MediaTime) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match session.seek(target) {
            Ok(generation) => {
                self.generation = generation;
                self.pending_time = None;
                self.clock = None;
                self.decode_finished = false;
                self.audio_drained = !session.has_audio();
                self.metrics_recorded = false;
                self.pending_seek_started = Some(Instant::now());
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    fn recover_graphics_device(&mut self, fallback_position: MediaTime) {
        let position = self
            .session
            .as_ref()
            .and_then(PlaybackSession::audio_position)
            .unwrap_or(fallback_position);
        let Some(window) = &self.window else {
            self.fail("window was unavailable during graphics recovery".to_owned());
            return;
        };
        let renderer = match FrameRenderer::new(window) {
            Ok(renderer) => renderer,
            Err(error) => {
                self.fail(format!("D3D11 device recovery failed: {error}"));
                return;
            }
        };
        let graphics_device = renderer.graphics_device();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match session.replace_graphics_device(graphics_device, position) {
            Ok(generation) => {
                self.renderer = Some(renderer);
                self.generation = generation;
                self.pending_time = None;
                self.clock = None;
                self.decode_finished = false;
                self.audio_drained = !session.has_audio();
                self.metrics_recorded = false;
                self.pending_seek_started = Some(Instant::now());
            }
            Err(error) => self.fail(format!("D3D11 pipeline recovery failed: {error}")),
        }
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
            Some(AudioOutputEvent::EndpointChanged) => {
                let position = self
                    .session
                    .as_ref()
                    .and_then(PlaybackSession::audio_position)
                    .unwrap_or(MediaTime::ZERO);
                self.seek_to(position);
            }
            Some(AudioOutputEvent::Failed(error)) => self.fail(error),
            None => {}
        }
    }

    fn audio_master_position(&self) -> Option<MediaTime> {
        if self.audio_drained {
            None
        } else {
            self.session
                .as_ref()
                .and_then(PlaybackSession::audio_position)
        }
    }

    fn check_eof(&mut self) {
        if self.decode_finished && self.pending_time.is_none() && self.audio_drained {
            self.state = PlaybackState::Ended;
            if !self.metrics_recorded {
                if let Some(session) = &self.session {
                    let metrics = session.metrics();
                    eprintln!(
                        "towavue: adapter={:08x}:{:08x} hardware_frames={} cpu_transfers={} presented={} dropped={}",
                        metrics.adapter_luid.high_part,
                        metrics.adapter_luid.low_part,
                        metrics.hardware_frame_count,
                        metrics.cpu_transfer_count,
                        metrics.presented_frame_count,
                        metrics.dropped_frame_count
                    );
                }
                self.report_timing_metrics();
                self.metrics_recorded = true;
            }
            self.refresh_title();
        }
    }

    fn fail(&mut self, error: String) {
        eprintln!("towavue: {error}");
        self.state = PlaybackState::Faulted;
        self.refresh_title();
    }

    fn report_timing_metrics(&self) {
        if !self.seek_latencies.is_empty() {
            eprintln!(
                "towavue: seek_p95_ms={:.3}",
                percentile_95(&self.seek_latencies).as_secs_f64() * 1000.0
            );
        }
        if !self.drift_samples.is_empty() {
            eprintln!(
                "towavue: av_drift_p95_ms={:.3} av_drift_max_ms={:.3}",
                percentile_95(&self.drift_samples).as_secs_f64() * 1000.0,
                self.drift_samples
                    .iter()
                    .max()
                    .copied()
                    .unwrap_or_default()
                    .as_secs_f64()
                    * 1000.0
            );
        }
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
            if let Some(audio_position) = self.audio_master_position() {
                let cutoff = audio_position.saturating_sub(VIDEO_LATE_TOLERANCE);
                if let Some(session) = self.session.as_mut()
                    && session.drop_video_before(cutoff) > 0
                {
                    self.pending_time = session.pending_video_time();
                }
            }
            if let (Some(window), Some(presentation_time)) = (&self.window, self.pending_time) {
                let due_at = if let Some(audio_position) = self.audio_master_position() {
                    let threshold = audio_position.saturating_add(VIDEO_EARLY_TOLERANCE);
                    let wait_nanoseconds = presentation_time
                        .as_nanoseconds()
                        .saturating_sub(threshold.as_nanoseconds())
                        .max(0) as u64;
                    Instant::now()
                        + Duration::from_nanos(wait_nanoseconds).min(AUDIO_EVENT_POLL_INTERVAL)
                } else if let Some(clock) = &self.clock {
                    clock.due_at(presentation_time)
                } else {
                    Instant::now()
                };
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

fn percentile_95(samples: &[Duration]) -> Duration {
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    let index = (ordered.len() * 95).div_ceil(100).saturating_sub(1);
    ordered[index]
}

impl<N> ApplicationHandler<PlaybackEvent> for Application<N>
where
    N: Fn(PlaybackEvent) + Send + Sync + 'static,
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
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::ArrowLeft),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => self.seek_relative(false),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::ArrowRight),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => self.seek_relative(true),
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
