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

pub(super) fn progress(
    ui: &egui::Ui,
    thumbnail: egui::Rect,
    position: MediaTime,
    duration: Option<MediaTime>,
) {
    let pixel = 1.0 / ui.ctx().pixels_per_point();
    let bottom = (thumbnail.bottom() / pixel).floor() * pixel;
    let track = egui::Rect::from_min_max(
        egui::pos2(thumbnail.left(), bottom - pixel),
        egui::pos2(thumbnail.right(), bottom),
    );
    ui.painter().rect_filled(track, 0.0, chrome::MUTED);
    if let Some(duration) = duration.filter(|duration| *duration > MediaTime::ZERO) {
        let fraction = (position.as_nanoseconds() as f64 / duration.as_nanoseconds() as f64)
            .clamp(0.0, 1.0) as f32;
        ui.painter().rect_filled(
            track.with_max_x(track.left() + track.width() * fraction),
            0.0,
            chrome::FOREGROUND,
        );
    }
}

impl Transport {
    pub fn show(&self, ui: &mut egui::Ui, thumbnail: egui::Rect) -> Option<CommandId> {
        progress(ui, thumbnail, self.position, self.duration);
        if self.state == PlaybackState::Playing {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if !ui.rect_contains_pointer(thumbnail) {
            return None;
        }
        let audio = self.kind == MediaKind::Audio;
        let rect = egui::Rect::from_center_size(
            thumbnail.center(),
            egui::vec2(if audio { 84.0 } else { 28.0 }, 24.0),
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
        action
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
        if self.modal_input_blocked()
            || self.palette_open
            || self.grid_open
            || self.filmstrip_open
            || self.incoming_tab_pointer.is_some()
            || self.ui_context.as_ref().is_some_and(|context| {
                egui::Popup::is_any_open(context)
                    || context.input(|input| !input.raw.hovered_files.is_empty())
            })
        {
            return;
        }
        let Some(transport) = self
            .preview_transport(tab, &path)
            .filter(|transport| transport.instance == instance)
        else {
            return;
        };
        if self
            .tabs
            .tabs()
            .iter()
            .find(|item| item.id == tab)
            .is_none_or(|tab| tab.target.current_path() != path)
        {
            return;
        }
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
}
