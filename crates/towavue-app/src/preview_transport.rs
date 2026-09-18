use crate::*;

#[cfg(test)]
mod tests;

pub(super) struct Transport {
    pub instance: u64,
    pub kind: MediaKind,
    pub state: PlaybackState,
    pub position: MediaTime,
    pub duration: Option<MediaTime>,
    pub enabled: bool,
    pub previous: bool,
    pub next: bool,
}

#[derive(Debug, PartialEq)]
pub(super) enum Action {
    Command(CommandId),
    Seek(MediaTime),
    ImageSeek(usize),
}

impl Transport {
    pub fn show(&self, ui: &mut egui::Ui, thumbnail: egui::Rect) -> Option<Action> {
        // Register the background first so seek and previous/next controls keep
        // their own hit regions. Media replacement changes the click owner.
        let surface = ui
            .add_enabled_ui(self.enabled, |ui| {
                ui.interact(
                    thumbnail,
                    ui.id().with(("preview-toggle", self.instance)),
                    egui::Sense::click(),
                )
            })
            .inner;
        surface.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                surface.enabled(),
                "Toggle preview playback",
            )
        });
        crate::tab_focus::release_pointer_focus(&surface);
        let duration = self.duration.filter(|duration| *duration > MediaTime::ZERO);
        let seconds = duration.map_or(0.0, MediaTime::as_seconds_f64);
        let progress = if seconds > 0.0 {
            (self.position.as_seconds_f64() / seconds).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let pixel = 1.0 / ui.ctx().pixels_per_point();
        let rect = egui::Rect::from_center_size(
            egui::pos2(
                thumbnail.center().x,
                (thumbnail.bottom() / pixel).floor() * pixel - pixel * 0.5,
            ),
            egui::vec2(thumbnail.width(), 12.0),
        );
        let seek = ui
            .add_enabled_ui(self.enabled && duration.is_some(), |ui| {
                let (response, drag) = seekbar::inline(
                    ui,
                    rect,
                    ui.id().with(("preview-seek", self.instance)),
                    progress,
                );
                let value = seekbar::value_input(
                    &response,
                    "Preview playback position (seconds)",
                    self.position.as_seconds_f64(),
                    0.0..=seconds,
                    KEYBOARD_SEEK_STEP.as_secs_f64(),
                    true,
                );
                value
                    .or_else(|| {
                        drag.released
                            .then_some(drag.position)
                            .flatten()
                            .map(|point| seconds * f64::from(seekbar::compact_ratio(rect, point.x)))
                    })
                    .map(|seconds| Action::Seek(media_time(Duration::from_secs_f64(seconds))))
            })
            .inner;
        if self.state == PlaybackState::Playing {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if !ui.rect_contains_pointer(thumbnail) {
            return seek;
        }
        let audio = self.kind == MediaKind::Audio;
        let rect = egui::Rect::from_center_size(
            thumbnail.center(),
            egui::vec2(if audio { 72.0 } else { 24.0 }, 24.0),
        );
        ui.painter()
            .rect_filled(rect, 4.0, Color32::from_black_alpha(160));
        let mut action = None;
        let mut buttons = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("preview-transport", self.instance))
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        {
            let ui = &mut buttons;
            ui.spacing_mut().item_spacing.x = 0.0;
            if audio
                && ui
                    .add_enabled_ui(self.previous, |ui| {
                        chrome::button(ui, chrome::Icon::PreviousTrack, "Previous track")
                    })
                    .inner
                    .clicked()
            {
                action = Some(CommandId::PreviousMedia);
            }
            let playing = self.state == PlaybackState::Playing;
            if ui
                .add_enabled_ui(self.enabled, |ui| {
                    chrome::button(
                        ui,
                        if playing {
                            chrome::Icon::Pause
                        } else {
                            chrome::Icon::Play
                        },
                        if playing { "Pause" } else { "Play" },
                    )
                })
                .inner
                .clicked()
            {
                action = Some(CommandId::TogglePause);
            }
            if audio
                && ui
                    .add_enabled_ui(self.next, |ui| {
                        chrome::button(ui, chrome::Icon::NextTrack, "Next track")
                    })
                    .inner
                    .clicked()
            {
                action = Some(CommandId::NextMedia);
            }
        }
        action.map(Action::Command).or(seek).or_else(|| {
            surface
                .clicked()
                .then_some(Action::Command(CommandId::TogglePause))
        })
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn preview_transport(&self, tab: TabId, path: &Path) -> Option<Transport> {
        let (instance, kind, state, position, duration, session, recovering) =
            if self.displayed_tab == Some(tab) && self.path.as_deref() == Some(path) {
                (
                    self.media_generation,
                    self.media_kind?,
                    self.state,
                    self.current_position(),
                    self.playback_duration().map(media_time),
                    self.session.is_some(),
                    self.queued_recovery.is_some(),
                )
            } else {
                let saved = self
                    .retained_playback
                    .get(&tab)
                    .filter(|saved| saved.path == path)?;
                (
                    saved.instance,
                    saved.kind,
                    saved.state,
                    saved.position(),
                    saved
                        .session
                        .as_ref()
                        .and_then(PlaybackSession::timeline)
                        .map(|plan| plan.duration())
                        .or(saved.duration.map(media_time)),
                    saved.session.is_some(),
                    saved.recovery_position.is_some(),
                )
            };
        if !matches!(kind, MediaKind::Audio | MediaKind::Video) {
            return None;
        }
        let enabled = session
            && !recovering
            && state.after_play_pause().is_some()
            && duration != Some(MediaTime::ZERO);
        let queue = self.audio_queues.get(&tab);
        Some(Transport {
            instance,
            kind,
            state,
            position,
            duration,
            enabled,
            previous: session
                && !recovering
                && queue.is_some_and(|queue| queue.order.previous(path).is_some()),
            next: session
                && !recovering
                && queue.is_some_and(|queue| queue.order.next(path, false).is_some()),
        })
    }

    pub(super) fn handle_preview_transport(
        &mut self,
        tab: TabId,
        instance: u64,
        path: PathBuf,
        command: CommandId,
    ) {
        let Some(transport) = self.validated_preview_transport(tab, instance, &path) else {
            return;
        };
        match command {
            CommandId::TogglePause if transport.enabled => {
                if self.displayed_tab == Some(tab) {
                    self.toggle_pause();
                } else if let Some(saved) = self.retained_playback.get_mut(&tab) {
                    saved.toggle_pause();
                    if saved.state == PlaybackState::Playing {
                        self.arm_audio_queue(tab);
                    }
                }
            }
            CommandId::PreviousMedia | CommandId::NextMedia
                if transport.kind == MediaKind::Audio =>
            {
                let forward = command == CommandId::NextMedia;
                if !(if forward {
                    transport.next
                } else {
                    transport.previous
                }) {
                    return;
                }
                let queue = self.audio_queues.get(&tab).expect("audio queue");
                let next = if forward {
                    queue.order.next(&path, false)
                } else {
                    queue.order.previous(&path)
                };
                if let Some(next) = next {
                    if next == path {
                        if self.displayed_tab == Some(tab) {
                            self.navigate_audio(forward);
                        } else if let Some(saved) = self.retained_playback.get_mut(&tab) {
                            saved.restart();
                        }
                    } else {
                        self.request_guarded(GuardedAction::NavigateAudioTab(tab, path, next));
                    }
                }
            }
            _ => return,
        }
        self.request_redraw();
    }

    pub(super) fn handle_preview_seek(
        &mut self,
        tab: TabId,
        instance: u64,
        path: &Path,
        target: MediaTime,
    ) {
        let Some(transport) = self
            .validated_preview_transport(tab, instance, path)
            .filter(|transport| transport.enabled)
        else {
            return;
        };
        let Some(duration) = transport
            .duration
            .filter(|duration| *duration > MediaTime::ZERO)
        else {
            return;
        };
        let target = target.max(MediaTime::ZERO).min(duration);
        if self.displayed_tab == Some(tab) {
            self.seek_to(target);
        } else {
            let edit = self
                .edits
                .get(&tab)
                .map(EditHistory::state)
                .unwrap_or_default();
            let saved = self
                .retained_playback
                .get_mut(&tab)
                .expect("validated playback");
            saved.seek_to(target, edit);
            if saved.state == PlaybackState::Playing {
                self.arm_audio_queue(tab);
            }
        }
        self.request_redraw();
    }

    fn validated_preview_transport(
        &self,
        tab: TabId,
        instance: u64,
        path: &Path,
    ) -> Option<Transport> {
        if self.preview_input_blocked() {
            return None;
        }
        let transport = self
            .preview_transport(tab, path)
            .filter(|transport| transport.instance == instance)?;
        if self
            .tabs
            .tabs()
            .iter()
            .find(|item| item.id == tab)
            .is_none_or(|tab| tab.target.current_path() != path)
        {
            return None;
        }
        Some(transport)
    }

    pub(super) fn preview_input_blocked(&self) -> bool {
        self.modal_input_blocked()
            || self.palette_open
            || self.grid_open
            || self.incoming_tab_pointer.is_some()
            || self.ui_context.as_ref().is_some_and(|context| {
                egui::Popup::is_any_open(context)
                    || context.input(|input| !input.raw.hovered_files.is_empty())
            })
    }
}
