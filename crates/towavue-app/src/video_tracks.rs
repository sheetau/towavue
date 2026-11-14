use crate::*;
use towavue_core::{VideoTrack, VideoTrackId};

#[derive(Clone, Debug)]
pub(super) struct Choice {
    path: PathBuf,
    track: VideoTrackId,
}

impl Choice {
    pub(super) fn relocate(&mut self, source: &Path, target: &Path) {
        if self.path == source {
            self.path = target.to_owned();
        }
    }
}

pub(crate) fn track_label(
    language: localization::Language,
    index: usize,
    track: &VideoTrack,
) -> String {
    let mut label = format!(
        "{} {}",
        localization::Text::VideoTrack.in_language(language),
        index + 1
    );
    if let Some(title) = &track.title {
        label.push_str(" · ");
        label.push_str(&title.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    if let Some(language) = track.language.as_deref().filter(|value| *value != "und") {
        label.push_str(" (");
        label.push_str(&language.split_whitespace().collect::<Vec<_>>().join(" "));
        label.push(')');
    }
    label
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn video_selection_for(
        &self,
        tab: Option<TabId>,
        path: &Path,
    ) -> Option<VideoTrackId> {
        tab.and_then(|id| self.video_choices.get(&id))
            .filter(|choice| choice.path == path)
            .map(|choice| choice.track)
    }

    pub(super) fn video_selection(&self) -> Option<VideoTrackId> {
        self.path
            .as_deref()
            .and_then(|path| self.video_selection_for(self.displayed_tab, path))
    }

    pub(super) fn video_preview_track(&self) -> Option<VideoTrackId> {
        self.video_selection()
            .or_else(|| self.session.as_ref()?.video_tracks().preferred)
    }

    pub(super) fn cycle_video_track(&mut self) {
        if let Some(track) = self
            .session
            .as_ref()
            .and_then(|session| session.video_tracks().next_track(self.video_selection()))
        {
            self.select_video_track(track);
        }
    }

    pub(super) fn select_video_track(&mut self, track: VideoTrackId) {
        if self.media_kind != Some(MediaKind::Video)
            || self.modal_input_blocked()
            || self.command_context().playback_blocked
        {
            return;
        }
        let (Some(id), Some(path), Some(session)) =
            (self.displayed_tab, self.path.clone(), self.session.as_ref())
        else {
            return;
        };
        if self.tabs.active_id() != Some(id)
            || self.video_selection().or(session.video_tracks().preferred) == Some(track)
            || !session
                .video_tracks()
                .tracks
                .iter()
                .any(|entry| entry.id == track)
        {
            return;
        }
        self.request_guarded(GuardedAction::VideoTrack(
            id,
            self.media_generation,
            path,
            track,
        ));
    }

    /// Both Discard and successful Save arrive here through the ordinary guard.
    /// A saved document retains its original MediaInput, including all streams.
    pub(super) fn switch_video_track(
        &mut self,
        id: TabId,
        instance: u64,
        path: PathBuf,
        track: VideoTrackId,
    ) {
        if self.displayed_tab != Some(id)
            || self.tabs.active_id() != Some(id)
            || self.media_generation != instance
            || self.path.as_ref() != Some(&path)
        {
            return;
        }
        let position = self.current_position();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let target = session
            .timeline()
            .and_then(|plan| plan.source_time(position))
            .unwrap_or(position);
        if let Err(error) = session.configure_video_track(Some(track)) {
            self.set_status(error.message(self.language()));
            return;
        }
        self.video_choices.insert(id, Choice { path, track });
        // Edits have one owner. Switching cannot resurrect a previous track's
        // crop, timeline, audio edits or redo stack on a different picture.
        self.edits.insert(id, EditHistory::default());
        self.time_selection = None;
        self.playback_selection = None;
        self.image_view.selection = None;
        self.thumbnail_worker.clear();
        self.thumbnail_loading = None;
        self.hover_thumbnail = None;
        self.failed_thumbnails.clear();
        let volume = self.playback_volume();
        if let Some(session) = &mut self.session {
            session.set_volume(volume);
        }
        self.video_sheets.clear();
        self.tab_preview.clear();
        self.seek_to_target(target);
        self.refresh_title();
        self.request_redraw();
    }

    pub(super) fn video_track_controls_width(&self, ui: &egui::Ui) -> f32 {
        if !self.timeline_is_visible() || self.media_kind != Some(MediaKind::Video) {
            return 0.0;
        }
        let Some(catalog) = self
            .session
            .as_ref()
            .map(PlaybackSession::video_tracks)
            .filter(|catalog| catalog.tracks.len() > 1)
        else {
            return 0.0;
        };
        let selected = self.video_preview_track();
        let label = format!(
            "V{}",
            catalog
                .tracks
                .iter()
                .position(|track| Some(track.id) == selected)
                .unwrap_or(0)
                + 1
        );
        ui.painter()
            .layout_no_wrap(
                label,
                egui::FontId::proportional(chrome::TEXT_LG),
                chrome::MUTED,
            )
            .size()
            .x
            + 28.0
            + chrome::STATUS_BUTTON_GAP
    }

    pub(super) fn draw_video_track_controls(&self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        if !self.timeline_is_visible() || self.media_kind != Some(MediaKind::Video) {
            return;
        }
        let Some(catalog) = self
            .session
            .as_ref()
            .map(PlaybackSession::video_tracks)
            .filter(|catalog| catalog.tracks.len() > 1)
        else {
            return;
        };
        let selected = self.video_selection().or(catalog.preferred);
        let owner = (self.displayed_tab, self.media_generation);
        let label = format!(
            "V{}",
            catalog
                .tracks
                .iter()
                .position(|track| Some(track.id) == selected)
                .unwrap_or(0)
                + 1
        );
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(5, 0))
            .show(ui, |ui| {
                let width = ui
                    .painter()
                    .layout_no_wrap(
                        label.clone(),
                        egui::FontId::proportional(chrome::TEXT_LG),
                        chrome::MUTED,
                    )
                    .size()
                    .x
                    + 18.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
                let popup = ui.id().with(("video-track-popup", owner));
                let open = egui::Popup::is_id_open(ui.ctx(), popup);
                let response = browser_controls::text_button_color(
                    ui,
                    rect,
                    ui.id().with(("video-track", owner)),
                    &label,
                    false,
                    if open {
                        chrome::FOREGROUND
                    } else {
                        chrome::MUTED
                    },
                );
                ui.painter().text(
                    egui::pos2(rect.right() - 8.0, rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    if open { "\u{eab7}" } else { "\u{eab4}" },
                    fonts::icon_font(),
                    if open || response.hovered() || response.has_focus() {
                        chrome::FOREGROUND
                    } else {
                        chrome::MUTED
                    },
                );
                chrome::menu_popup(&response, popup)
                    .align(egui::RectAlign::TOP_START)
                    .gap(2.0)
                    .style(chrome::menu_style)
                    .show(|ui| {
                        let choices: Vec<_> = catalog
                            .tracks
                            .iter()
                            .enumerate()
                            .map(|(index, track)| {
                                (track.id, track_label(self.language(), index, track))
                            })
                            .collect();
                        let font = egui::TextStyle::Button.resolve(ui.style());
                        let width = choices
                            .iter()
                            .map(|(_, label)| {
                                ui.painter()
                                    .layout_no_wrap(label.clone(), font.clone(), chrome::FOREGROUND)
                                    .size()
                                    .x
                            })
                            .fold(0.0, f32::max)
                            + ui.spacing().interact_size.y
                            + 2.0 * ui.spacing().button_padding.x;
                        ui.set_width(width.ceil().min(ui.available_width()));
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        egui::ScrollArea::vertical()
                            .max_height(
                                (rect.top() - ui.ctx().content_rect().top() - 12.0).max(22.0),
                            )
                            .show(ui, |ui| {
                                for (id, label) in choices {
                                    let mut checked = Some(id) == selected;
                                    if chrome::menu_checkbox(ui, &mut checked, label).clicked() {
                                        actions.push(UiAction::VideoTrack(owner.0, owner.1, id));
                                        ui.close();
                                    }
                                }
                            });
                    });
            });
    }
}

#[cfg(test)]
mod tests;
