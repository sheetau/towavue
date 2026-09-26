//! Held selection input through the hidden application's ordinary renderer.
use super::*;

const HELD_FRAMES: usize = 120;

pub(super) struct Probe {
    case: usize,
    case_count: usize,
    prepared: bool,
    active: Option<Gesture>,
    settled_waveform: bool,
}

struct Gesture {
    frame: usize,
    from: egui::Pos2,
    to: egui::Pos2,
    expected: towavue_core::TimeRange,
    tolerance: f64,
    generation: Option<PlaybackGeneration>,
    held_ms: Vec<f64>,
    interval_ms: Vec<f64>,
    last_frame_at: Option<Instant>,
    press_ms: f64,
    release_ms: f64,
    overview_pending: usize,
    detail_pending: usize,
    refined_frames: usize,
    release_at: Option<Instant>,
    latency_count: usize,
}

impl Probe {
    pub(super) fn from_environment() -> Option<Self> {
        let mode = std::env::var("TOWAVUE_SEEK_DRAG").ok()?;
        assert!(matches!(mode.as_str(), "live" | "settled"));
        Some(Self {
            case: 0,
            case_count: if std::env::var_os("TOWAVUE_SEEK_PAUSED_ONLY").is_some() {
                2
            } else {
                4
            },
            prepared: false,
            active: None,
            settled_waveform: mode == "settled",
        })
    }

    pub(super) fn complete(&self) -> bool {
        self.case == self.case_count
    }

    pub(super) fn before_frame(&mut self, app: &mut App) {
        if self.complete() {
            return;
        }
        let playing = self.case >= 2;
        if !self.prepared {
            app.set_time_selection(None);
            if (app.state == PlaybackState::Playing) != playing {
                app.toggle_pause();
            }
            self.prepared = true;
            return;
        }
        if self.active.is_none() {
            if app.pending_seek_started.is_some()
                || app
                    .session
                    .as_ref()
                    .expect("session")
                    .video_refresh_pending()
                || (self.settled_waveform
                    && (app.waveform_loading
                        || app.waveform_detail.is_pending()
                        || app.verification_waveform_shape().1 == 0))
            {
                return;
            }
            let context = app.ui_context.as_ref().expect("context");
            let outer = egui::containers::panel::PanelState::load(context, app.timeline_panel_id())
                .expect("timeline panel")
                .outer_rect;
            let rect = egui::Rect::from_min_max(
                outer.min + egui::vec2(8.0, 8.0),
                outer.max - egui::vec2(8.0, 0.0),
            );
            let duration = app.playback_duration().expect("duration");
            let range = app.playback_range();
            let start = range.start.as_seconds_f64();
            let end = range.end.unwrap_or(media_time(duration)).as_seconds_f64();
            let at = |fraction| {
                let seconds = start + (end - start) * fraction;
                let point = egui::pos2(
                    rect.left() + rect.width() * (seconds / duration.as_secs_f64()) as f32,
                    rect.top() + rect.height() * 0.72,
                );
                (point, media_time(Duration::from_secs_f64(seconds)))
            };
            let (left, a) = at(0.35);
            let (right, b) = at(0.65);
            // The UI captures the pre-press CTI as a four-point snap magnet.
            // Playing reverse drags may end near that retained position.
            let head = app.current_position();
            let head_x = rect.left()
                + rect.width() * (head.as_seconds_f64() / duration.as_secs_f64()) as f32;
            let snap = |point: egui::Pos2, time| {
                if (point.x - head_x).abs() <= 4.0 {
                    head
                } else {
                    time
                }
            };
            let a = snap(left, a);
            let b = snap(right, b);
            let reverse = self.case % 2 == 1;
            eprintln!(
                "APP_DRAG_BEGIN case={} playing={} reverse={} settled_waveform={} overview_pending={} detail_pending={}",
                self.case,
                playing,
                reverse,
                self.settled_waveform,
                app.waveform_loading,
                app.waveform_detail.is_pending()
            );
            self.active = Some(Gesture {
                frame: 0,
                from: if reverse { right } else { left },
                to: if reverse { left } else { right },
                expected: towavue_core::TimeRange::new(a, b).expect("selection"),
                tolerance: duration.as_secs_f64() / f64::from(rect.width()) + 0.001,
                generation: None,
                held_ms: Vec::with_capacity(HELD_FRAMES),
                interval_ms: Vec::with_capacity(HELD_FRAMES),
                last_frame_at: None,
                press_ms: 0.0,
                release_ms: 0.0,
                overview_pending: 0,
                detail_pending: 0,
                refined_frames: 0,
                release_at: None,
                latency_count: app.seek_latencies.len(),
            });
        }
        let gesture = self.active.as_mut().expect("gesture");
        towavue_runtime_windows::record_burst(
            towavue_runtime_windows::BurstEvent::ProbeGestureFrame,
            self.case as u64,
            app.path.as_deref(),
            [
                gesture.frame as u64,
                u64::from(playing),
                app.verification_waveform_shape().1 as u64,
            ],
        );
        let session = app.session.as_ref().expect("session");
        let metrics = session.metrics();
        towavue_runtime_windows::record_burst(
            towavue_runtime_windows::BurstEvent::ProbePlaybackState,
            self.case as u64,
            app.path.as_deref(),
            [
                u64::from(session.video_refresh_pending()),
                metrics.hardware_frame_count,
                metrics.cpu_transfer_count,
            ],
        );
        let input = app.ui_state.as_mut().expect("UI state").egui_input_mut();
        input.focused = true;
        match gesture.frame {
            0 => input.events.extend([
                egui::Event::PointerMoved(gesture.from),
                egui::Event::PointerButton {
                    pos: gesture.from,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]),
            1..=HELD_FRAMES => input.events.push(egui::Event::PointerMoved(
                gesture
                    .from
                    .lerp(gesture.to, gesture.frame as f32 / HELD_FRAMES as f32),
            )),
            frame if frame == HELD_FRAMES + 1 => {
                gesture.release_at = Some(Instant::now());
                input.events.push(egui::Event::PointerButton {
                    pos: gesture.to,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            _ => {}
        }
    }

    pub(super) fn after_frame(&mut self, app: &App, elapsed: Duration) {
        let Some(gesture) = &mut self.active else {
            return;
        };
        let session = app.session.as_ref().expect("session");
        let context = app.ui_context.as_ref().expect("context");
        let ms = elapsed.as_secs_f64() * 1000.0;
        let now = Instant::now();
        assert_eq!(app.state == PlaybackState::Playing, self.case >= 2);
        if gesture.frame <= HELD_FRAMES {
            assert!(timeline_input::is_active(context), "owned held input");
            assert!(app.time_selection.is_none(), "selection commits on release");
            if gesture.frame == 0 {
                gesture.press_ms = ms;
                gesture.generation = Some(session.generation());
            } else {
                assert_eq!(
                    Some(session.generation()),
                    gesture.generation,
                    "no held-drag seek"
                );
                gesture.held_ms.push(ms);
                gesture.interval_ms.push(
                    now.duration_since(gesture.last_frame_at.expect("previous frame"))
                        .as_secs_f64()
                        * 1000.0,
                );
                gesture.overview_pending += usize::from(app.waveform_loading);
                gesture.detail_pending += usize::from(app.waveform_detail.is_pending());
                let refined = app.verification_waveform_shape().1 > 0;
                gesture.refined_frames += usize::from(refined);
                assert!(
                    !self.settled_waveform || refined,
                    "retain settled refinement"
                );
            }
            gesture.last_frame_at = Some(now);
        } else {
            assert!(
                !timeline_input::is_active(context),
                "release ends owned input"
            );
            let selection = app.time_selection.expect("released selection");
            for (actual, expected) in [
                (selection.start(), gesture.expected.start()),
                (selection.end(), gesture.expected.end()),
            ] {
                assert!(
                    (actual.as_seconds_f64() - expected.as_seconds_f64()).abs() < gesture.tolerance,
                    "selection endpoint {actual:?}, expected {expected:?}, tolerance {} seconds",
                    gesture.tolerance
                );
            }
            if gesture.frame == HELD_FRAMES + 1 {
                gesture.release_ms = ms;
            }
            let since_release = gesture.release_at.expect("released").elapsed();
            assert!(
                since_release < Duration::from_secs(15),
                "bounded final seek"
            );
            if !session.video_refresh_pending() && app.pending_seek_started.is_none() {
                assert!(
                    app.seek_latencies.len() > gesture.latency_count,
                    "press seek submitted"
                );
                verify_decode_path(session);
                gesture.held_ms.sort_by(f64::total_cmp);
                gesture.interval_ms.sort_by(f64::total_cmp);
                assert_eq!(gesture.held_ms.len(), HELD_FRAMES);
                eprintln!(
                    "APP_DRAG case={} held_frames={} press_ms={:.3} held_median_ms={:.3} held_p95_ms={:.3} held_max_ms={:.3} release_ms={:.3} release_ready_ms={:.3} overview_pending_frames={} detail_pending_frames={} refined_frames={} seeks={} stages_ms={:?}",
                    self.case,
                    HELD_FRAMES,
                    gesture.press_ms,
                    gesture.held_ms[HELD_FRAMES / 2],
                    gesture.held_ms[HELD_FRAMES * 95 / 100],
                    gesture.held_ms.last().expect("held frames"),
                    gesture.release_ms,
                    since_release.as_secs_f64() * 1000.0,
                    gesture.overview_pending,
                    gesture.detail_pending,
                    gesture.refined_frames,
                    app.seek_latencies.len() - gesture.latency_count,
                    session.verification_seek_stages()
                );
                eprintln!(
                    "APP_DRAG_INTERVAL case={} frames={} median_ms={:.3} p95_ms={:.3} max_ms={:.3}",
                    self.case,
                    gesture.interval_ms.len(),
                    gesture.interval_ms[HELD_FRAMES / 2],
                    gesture.interval_ms[HELD_FRAMES * 95 / 100],
                    gesture.interval_ms.last().expect("frame intervals")
                );
                self.case += 1;
                self.prepared = false;
                self.active = None;
                if self.complete() {
                    eprintln!(
                        "APP_DRAG_CHECKS cases={} held_frames={} selection=true no_held_seeks=true complete=true",
                        self.case_count,
                        self.case_count * HELD_FRAMES
                    );
                }
                return;
            }
        }
        gesture.frame += 1;
    }
}
