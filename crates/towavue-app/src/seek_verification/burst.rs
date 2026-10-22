//! Rapid seek-bar clicks through the hidden production event loop; no readback.
use super::*;

pub(super) struct Probe {
    count: usize,
    random: u32,
    release: Option<egui::Pos2>,
    next_at: Instant,
    finished: bool,
    initialized: bool,
    last_generation: Option<PlaybackGeneration>,
    mixed: bool,
    drag_frames: usize,
}

impl Probe {
    pub(super) fn from_environment() -> Option<Self> {
        std::env::var_os("TOWAVUE_SEEK_CLICK_BURST").map(|_| Self {
            count: 0,
            random: 0x51a7_2943,
            release: None,
            next_at: Instant::now(),
            finished: false,
            initialized: false,
            last_generation: None,
            mixed: std::env::var_os("TOWAVUE_SEEK_MIXED_GESTURES").is_some(),
            drag_frames: 0,
        })
    }
    pub(super) fn complete(&self) -> bool {
        self.finished
    }

    pub(super) fn before_frame(&mut self, app: &mut App) {
        if self.finished || app.media_duration.is_none() || app.session.is_none() {
            return;
        }
        if !self.initialized {
            if app
                .session
                .as_ref()
                .expect("session")
                .metrics()
                .presented_frame_count
                == 0
            {
                return;
            }
            app.timeline_open = false;
            if app.state == PlaybackState::Playing {
                app.toggle_pause();
            }
            self.initialized = true;
        }
        if self.release.is_some() || Instant::now() < self.next_at {
            return;
        }
        if let Some(previous) = self.last_generation.take() {
            assert_ne!(
                app.generation, previous,
                "each pointer click dispatches a seek"
            );
        }
        if self.count == 480 {
            assert!(app.pending_seek_started.is_none(), "final click settles");
            assert!(
                !app.session
                    .as_ref()
                    .expect("session")
                    .video_refresh_pending()
            );
            self.finished = true;
            eprintln!(
                "APP_SEEK_CLICK_BURST gestures=480 mixed={} hidden=true muted=true fresh_final_frame=true",
                self.mixed
            );
            return;
        }
        if self.count == 240 && app.state != PlaybackState::Playing {
            app.toggle_pause();
        }
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        let fraction = 0.1 + f64::from(self.random % 1001) / 10_000.0;
        let screen = app.ui_context.as_ref().expect("UI context").content_rect();
        let point = egui::pos2(
            screen.left() + 5.0 + (screen.width() - 10.0) * fraction as f32,
            screen.bottom() - chrome::STATUS_HEIGHT,
        );
        self.last_generation = Some(app.generation);
        let input = app.ui_state.as_mut().expect("UI state").egui_input_mut();
        input.focused = true;
        input.events.extend([
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        self.release = Some(point);
        if self.mixed && !self.count.is_multiple_of(3) {
            self.drag_frames = 3 + self.count % 9;
        }
        self.next_at =
            Instant::now() + Duration::from_millis([40, 80, 150, 300, 500][self.count % 5]);
        self.count += 1;
        if self.count == 480 {
            self.next_at = Instant::now() + Duration::from_secs(2);
        }
        if self.count.is_multiple_of(24) {
            eprintln!("APP_SEEK_CLICK count={} fraction={fraction:.4}", self.count);
        }
    }

    pub(super) fn after_frame(&mut self, app: &mut App) {
        if self.drag_frames > 0 {
            let point = self.release.as_mut().expect("held pointer");
            point.x += if self.drag_frames % 4 < 2 { 3.0 } else { -3.0 };
            app.ui_state
                .as_mut()
                .expect("UI state")
                .egui_input_mut()
                .events
                .push(egui::Event::PointerMoved(*point));
            self.drag_frames -= 1;
            return;
        }
        if let Some(point) = self.release.take() {
            // Leave a frame for the queued release to commit before checking its
            // generation or injecting the next press after a longer held drag.
            self.next_at = self.next_at.max(Instant::now() + Duration::from_millis(40));
            app.ui_state
                .as_mut()
                .expect("UI state")
                .egui_input_mut()
                .events
                .push(egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
        }
    }
}
