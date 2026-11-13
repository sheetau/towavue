use crate::*;
use towavue_core::{AudioTrackSelection, AudioTrackTarget};

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn toggle_audio_track_output(&mut self) {
        if self.modal_input_blocked()
            || !self.timeline_is_visible()
            || self.media_kind != Some(MediaKind::Video)
        {
            return;
        }
        let Some(catalog) = self.session.as_ref().map(PlaybackSession::audio_tracks) else {
            return;
        };
        if catalog.tracks.is_empty() {
            return;
        }
        let output = self.edit_state().audio_output;
        let target = match self.audio_selection() {
            AudioTrackSelection::All => AudioTrackTarget::All,
            AudioTrackSelection::Track(id) => AudioTrackTarget::Track(id),
            AudioTrackSelection::Default => {
                let Some(id) = catalog.preferred else { return };
                AudioTrackTarget::Track(id)
            }
        };
        let enabled = match target {
            AudioTrackTarget::All => catalog
                .tracks
                .iter()
                .all(|track| !output.is_enabled(track.id)),
            AudioTrackTarget::Track(id) => !output.is_enabled(id),
        };
        self.push_edit(EditOperation::SetAudioTrackOutput(target, enabled));
    }

    pub(super) fn waveform_track_muted(&self) -> bool {
        if self.media_kind != Some(MediaKind::Video) {
            return false;
        }
        let track = self
            .waveform_audio_track()
            .or_else(|| self.session.as_ref()?.audio_tracks().preferred);
        track.is_some_and(|id| !self.edit_state().audio_output.is_enabled(id))
    }

    pub(super) fn draw_audio_track_controls(&self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        if !self.timeline_is_visible() || self.media_kind != Some(MediaKind::Video) {
            return;
        }
        let Some(catalog) = self
            .session
            .as_ref()
            .map(PlaybackSession::audio_tracks)
            .filter(|catalog| !catalog.tracks.is_empty())
        else {
            return;
        };
        let language = self.language();
        let output = self.edit_state().audio_output;
        let selection = self.audio_selection();
        let selected = match selection {
            AudioTrackSelection::Default => catalog.preferred,
            AudioTrackSelection::Track(id) => Some(id),
            AudioTrackSelection::All => None,
        };
        let label = if selection == AudioTrackSelection::All {
            "All".to_owned()
        } else {
            format!(
                "A{}",
                catalog
                    .tracks
                    .iter()
                    .position(|track| Some(track.id) == selected)
                    .unwrap_or(0)
                    + 1
            )
        };
        let multiple = catalog.tracks.len() > 1;
        let muted = if selection == AudioTrackSelection::All {
            catalog
                .tracks
                .iter()
                .all(|track| !output.is_enabled(track.id))
        } else {
            selected.is_some_and(|id| !output.is_enabled(id))
        };
        let owner = (self.displayed_tab, self.media_generation);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = if multiple { 2.0 } else { 5.0 };
            let font = egui::FontId::proportional(chrome::TEXT_LG);
            let text_width = ui
                .painter()
                .layout_no_wrap(label.clone(), font.clone(), chrome::MUTED)
                .size()
                .x;
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(text_width + if multiple { 18.0 } else { 0.0 }, 24.0),
                egui::Sense::hover(),
            );
            if multiple {
                let response = crate::browser_controls::text_button_color(
                    ui,
                    rect,
                    ui.id().with(("audio-track", owner)),
                    &label,
                    false,
                    chrome::MUTED,
                );
                let open =
                    egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
                ui.painter().text(
                    egui::pos2(rect.right() - 8.0, rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    if open { "\u{eab7}" } else { "\u{eab4}" },
                    crate::fonts::icon_font(),
                    if response.hovered() {
                        chrome::FOREGROUND
                    } else {
                        chrome::MUTED
                    },
                );
                egui::Popup::menu(&response)
                    .align(egui::RectAlign::TOP_START)
                    .gap(2.0)
                    .style(chrome::menu_style)
                    .show(|ui| {
                        ui.set_min_width(210.0);
                        let mut rows = chrome::Dropdown::new(ui);
                        let all = localization::Text::AudioTrackAll.in_language(language);
                        let all = if catalog
                            .tracks
                            .iter()
                            .all(|track| !output.is_enabled(track.id))
                        {
                            format!("{all} (Mute)")
                        } else {
                            all.to_owned()
                        };
                        let mut chosen = selection;
                        if rows
                            .value(ui, &mut chosen, AudioTrackSelection::All, &all)
                            .clicked()
                        {
                            actions.push(UiAction::AudioTrack(
                                owner.0,
                                owner.1,
                                AudioTrackSelection::All,
                            ));
                            ui.close();
                        }
                        for (index, track) in catalog.tracks.iter().enumerate() {
                            let mut label = audio_preview::track_label(language, index, track);
                            if !output.is_enabled(track.id) {
                                label.push_str(" (Mute)");
                            }
                            let active = selected == Some(track.id);
                            if rows.row(ui, true, active, &label).clicked() {
                                actions.push(UiAction::AudioTrack(
                                    owner.0,
                                    owner.1,
                                    AudioTrackSelection::Track(track.id),
                                ));
                                ui.close();
                            }
                        }
                        rows.finish(ui);
                    });
            } else {
                let response = ui.interact(
                    rect,
                    ui.id().with("audio-track-label"),
                    egui::Sense::hover(),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &label)
                });
                ui.painter().text(
                    rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    &label,
                    font,
                    chrome::MUTED,
                );
            }
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
            let title = localization::Text::MuteAudioTrack.in_language(language);
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    response.enabled(),
                    muted,
                    title,
                )
            });
            paint_mute(ui, rect, muted, response.hovered() || response.has_focus());
            if response
                .help_text(title)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                actions.push(UiAction::Command(CommandId::MuteAudioTrack));
            }
        });
    }
}

fn paint_mute(ui: &egui::Ui, rect: egui::Rect, muted: bool, hot: bool) {
    // Geometry copied from assets/mute-track.svg, Figma node 630:7.
    let icon = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
    let ink = if muted || hot {
        chrome::FOREGROUND
    } else {
        chrome::FAINT
    };
    ui.painter().rect_filled(
        icon,
        3,
        if muted {
            chrome::KEY_ACTIVE
        } else {
            chrome::SURFACE
        },
    );
    let point = |x, y| icon.min + egui::vec2(x, y);
    ui.painter().add(egui::Shape::line(
        vec![
            point(4.28026, 12.0),
            point(4.28026, 4.0),
            point(8.28026, 8.0),
            point(12.2803, 4.0),
            point(12.2803, 12.0),
        ],
        egui::Stroke::new(1.0, ink),
    ));
}
