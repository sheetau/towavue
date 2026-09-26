use crate::localization::{Language, Text, language};
use crate::*;
use towavue_core::localization::formatted;
use towavue_core::{AudioTrack, AudioTrackRetention};
use towavue_runtime_windows::{AudioChannels, AudioNormalization, LoudnessTarget};

pub(super) struct AudioExportDialog {
    token: u64,
    tab: TabId,
    source: PathBuf,
    kind: MediaKind,
    generation: u64,
    options: AudioExportOptions,
    tracks: Option<Vec<AudioTrack>>,
    retention: AudioTrackRetention,
    first_frame: bool,
    focused_option: Option<egui::Id>,
}

#[derive(Clone, Debug)]
pub(super) struct TrackChoice {
    path: PathBuf,
    retention: AudioTrackRetention,
}

impl TrackChoice {
    pub(super) fn relocate(&mut self, source: &Path, target: &Path) {
        if self.path == source {
            self.path = target.to_owned();
        }
    }
}

pub(super) fn summary(options: AudioExportOptions, display_language: Language) -> String {
    let normalization = match options.normalization {
        AudioNormalization::Off => Text::NormalizationOff.in_language(display_language).into(),
        AudioNormalization::Peak => Text::NormalizationPeakSummary
            .in_language(display_language)
            .into(),
        AudioNormalization::Loudness(target) => formatted::loudness_summary(
            display_language,
            f64::from(target.integrated_tenths) / 10.0,
            f64::from(target.true_peak_tenths) / 10.0,
        ),
    };
    format!(
        "{normalization} / {}",
        match options.channels {
            AudioChannels::Keep => Text::KeepChannels.in_language(display_language),
            AudioChannels::Mono => Text::Mono.in_language(display_language),
            AudioChannels::Stereo => Text::Stereo.in_language(display_language),
        }
    )
}

fn target_control(
    ui: &mut egui::Ui,
    value: &mut i16,
    range: std::ops::RangeInclusive<i16>,
    label: &str,
) -> egui::Response {
    let mut number = f64::from(*value) / 10.0;
    let response = ui
        .horizontal(|ui| {
            let label = ui.label(label);
            chrome::input_style(ui);
            ui.add_sized(
                [ui.spacing().interact_size.x, chrome::INPUT_HEIGHT],
                egui::DragValue::new(&mut number)
                    .range(f64::from(*range.start()) / 10.0..=f64::from(*range.end()) / 10.0)
                    .speed(0.1)
                    .fixed_decimals(1)
                    .custom_parser(|text| {
                        text.trim()
                            .parse::<f64>()
                            .ok()
                            .filter(|value| value.is_finite())
                    }),
            )
            .labelled_by(label.id)
        })
        .inner;
    if number.is_finite() {
        *value = ((number * 10.0).round() as i16).clamp(*range.start(), *range.end());
    }
    response
}

impl AudioExportDialog {
    fn show(&mut self, context: &egui::Context) -> Option<Option<AudioExportOptions>> {
        let display_language = language(context);
        let mut action = None;
        let mut focused_option = None;
        let mut reveal_focus = |response: &egui::Response| {
            if response.has_focus() {
                focused_option = Some(response.id);
                // Arrow focus is resolved after layout, so gained_focus alone
                // can miss the transition observed by the following frame.
                if self.focused_option != focused_option {
                    response.scroll_to_me(None);
                }
            }
        };
        let modal =
            chrome::modal(context, "audio-export-options".into(), false).show(context, |ui| {
                chrome::modal_body(
                    ui,
                    Text::CommandAudioExportOptions.in_language(display_language),
                    &[
                        Text::ApplyOptions.in_language(display_language),
                        Text::Cancel.in_language(display_language),
                    ],
                    |ui| {
                        if self.kind == MediaKind::Video {
                            ui.label(Text::SavedAudioTracks.in_language(display_language));
                            if let Some(tracks) = &self.tracks {
                                for (index, track) in tracks.iter().enumerate() {
                                    let mut checked = match &self.retention {
                                        AudioTrackRetention::All => true,
                                        AudioTrackRetention::Selected(ids) => {
                                            ids.contains(&track.id)
                                        }
                                    };
                                    let response = ui.checkbox(
                                        &mut checked,
                                        audio_preview::track_label(display_language, index, track),
                                    );
                                    if self.first_frame {
                                        response.request_focus();
                                        self.first_frame = false;
                                    }
                                    if response.changed() {
                                        let mut ids = match &self.retention {
                                            AudioTrackRetention::All => tracks
                                                .iter()
                                                .map(|track| track.id)
                                                .collect::<Vec<_>>(),
                                            AudioTrackRetention::Selected(ids) => ids.clone(),
                                        };
                                        ids.retain(|id| *id != track.id);
                                        if checked {
                                            ids.push(track.id);
                                        }
                                        let ids = tracks
                                            .iter()
                                            .map(|track| track.id)
                                            .filter(|id| ids.contains(id))
                                            .collect::<Vec<_>>();
                                        self.retention = if ids.len() == tracks.len() {
                                            AudioTrackRetention::All
                                        } else {
                                            AudioTrackRetention::Selected(ids)
                                        };
                                    }
                                    reveal_focus(&response);
                                }
                            } else {
                                ui.label(Text::AudioTracksLoading.in_language(display_language));
                            }
                            ui.label(Text::SavedAudioTracksHelp.in_language(display_language));
                            crate::chrome::separator(ui);
                        }
                        ui.label(Text::Normalization.in_language(display_language));
                        for (value, label) in [
                            (
                                AudioNormalization::Off,
                                Text::RepeatOff.in_language(display_language),
                            ),
                            (
                                AudioNormalization::Peak,
                                Text::NormalizationPeak.in_language(display_language),
                            ),
                            (
                                AudioNormalization::Loudness(match self.options.normalization {
                                    AudioNormalization::Loudness(target) => target,
                                    _ => LoudnessTarget::default(),
                                }),
                                Text::Loudness.in_language(display_language),
                            ),
                        ] {
                            let response = ui
                                .radio_value(&mut self.options.normalization, value, label)
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if self.first_frame {
                                response.request_focus();
                                self.first_frame = false;
                            }
                            reveal_focus(&response);
                        }
                        if let AudioNormalization::Loudness(target) =
                            &mut self.options.normalization
                        {
                            reveal_focus(&target_control(
                                ui,
                                &mut target.integrated_tenths,
                                -700..=-50,
                                Text::IntegratedLoudness.in_language(display_language),
                            ));
                            reveal_focus(&target_control(
                                ui,
                                &mut target.true_peak_tenths,
                                -90..=0,
                                Text::MaximumTruePeak.in_language(display_language),
                            ));
                            ui.label(Text::LoudnessVerificationHelp.in_language(display_language));
                        }
                        ui.label(Text::OutputChannels.in_language(display_language));
                        for (value, label) in [
                            (
                                AudioChannels::Keep,
                                Text::KeepSourceChannels.in_language(display_language),
                            ),
                            (
                                AudioChannels::Mono,
                                Text::Mono.in_language(display_language),
                            ),
                            (
                                AudioChannels::Stereo,
                                Text::Stereo.in_language(display_language),
                            ),
                        ] {
                            let response = ui
                                .radio_value(&mut self.options.channels, value, label)
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            reveal_focus(&response);
                        }
                        crate::chrome::separator(ui);
                        ui.label(Text::AudioOptionsScope.in_language(display_language));
                        ui.label(Text::NormalizationHelp.in_language(display_language));
                        ui.label(Text::ChannelConversionHelp.in_language(display_language));
                        ui.label(Text::AudioOptionsLifetime.in_language(display_language));
                    },
                );
                ui.horizontal_wrapped(|ui| {
                    crate::chrome::flat_buttons(ui);
                    if ui
                        .button(Text::ApplyOptions.in_language(display_language))
                        .clicked()
                    {
                        action = Some(Some(self.options));
                    }
                    if ui
                        .button(Text::Cancel.in_language(display_language))
                        .clicked()
                    {
                        action = Some(None);
                    }
                });
            });
        self.focused_option = focused_option;
        if modal.is_top_modal
            && !modal.any_popup_open
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            action = Some(None);
        }
        action
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn audio_retention_for(&self, id: TabId, path: &Path) -> AudioTrackRetention {
        self.audio_export_tracks
            .get(&id)
            .filter(|choice| choice.path == path)
            .map_or_else(AudioTrackRetention::default, |choice| {
                choice.retention.clone()
            })
    }

    pub(super) fn open_audio_export_options(&mut self) {
        if self.modal_input_blocked()
            || !matches!(self.media_kind, Some(MediaKind::Audio | MediaKind::Video))
        {
            return;
        }
        if self.active_export.is_some() {
            self.set_status(
                Text::WaitExportForAudioOptions
                    .in_language(self.language())
                    .into(),
            );
            return;
        }
        let Some(tab) = self.tabs.active() else {
            return;
        };
        self.guard_return_focus = self
            .ui_context
            .as_ref()
            .and_then(|context| context.memory(egui::Memory::focused))
            .map(|focus| (tab.id, focus));
        let Some(source) = tab.target.current_path().map(Path::to_owned) else {
            return;
        };
        self.audio_export_generation = self.audio_export_generation.wrapping_add(1);
        self.audio_export_dialog = Some(AudioExportDialog {
            token: self.audio_export_generation,
            tab: tab.id,
            retention: self.audio_retention_for(tab.id, &source),
            tracks: self
                .session
                .as_ref()
                .map(|session| session.audio_tracks().tracks.clone()),
            source,
            kind: tab.target.media_kind(),
            generation: self.media_generation,
            options: self
                .audio_export_settings
                .get(&tab.id)
                .copied()
                .unwrap_or_default(),
            first_frame: true,
            focused_option: None,
        });
        self.request_redraw();
    }

    fn audio_export_dialog_is_current(&self, dialog: &AudioExportDialog) -> bool {
        self.media_generation == dialog.generation
            && self.media_kind == Some(dialog.kind)
            && self.path.as_ref() == Some(&dialog.source)
            && self.tabs.active().is_some_and(|tab| {
                tab.id == dialog.tab
                    && tab.target.current_path() == Some(dialog.source.as_ref())
                    && tab.target.media_kind() == dialog.kind
            })
    }

    pub(super) fn cancel_stale_audio_export_options(&mut self) {
        if self
            .audio_export_dialog
            .as_ref()
            .is_some_and(|dialog| !self.audio_export_dialog_is_current(dialog))
        {
            self.audio_export_dialog = None;
            self.set_status(
                Text::AudioOptionsSourceChanged
                    .in_language(self.language())
                    .into(),
            );
        }
    }

    pub(super) fn show_audio_export_options(
        &mut self,
        context: &egui::Context,
        actions: &mut Vec<UiAction>,
    ) {
        if let Some(dialog) = &mut self.audio_export_dialog
            && dialog.tracks.is_none()
            && let Some(session) = &self.session
        {
            dialog.tracks = Some(session.audio_tracks().tracks.clone());
        }
        if let Some(dialog) = &mut self.audio_export_dialog
            && let Some(action) = dialog.show(context)
        {
            actions.push(UiAction::FinishAudioExportOptions(dialog.token, action));
        }
    }

    pub(super) fn finish_audio_export_options(
        &mut self,
        token: u64,
        value: Option<AudioExportOptions>,
    ) {
        if self
            .audio_export_dialog
            .as_ref()
            .is_none_or(|dialog| dialog.token != token)
        {
            return;
        }
        let dialog = self
            .audio_export_dialog
            .take()
            .expect("matching options dialog");
        if let Some(value) = value {
            if self.audio_export_dialog_is_current(&dialog) {
                if dialog.kind == MediaKind::Video {
                    if dialog.retention == AudioTrackRetention::All {
                        self.audio_export_tracks.remove(&dialog.tab);
                    } else {
                        self.audio_export_tracks.insert(
                            dialog.tab,
                            TrackChoice {
                                path: dialog.source.clone(),
                                retention: dialog.retention,
                            },
                        );
                    }
                }
                if value == AudioExportOptions::default() {
                    self.audio_export_settings.remove(&dialog.tab);
                } else {
                    self.audio_export_settings.insert(dialog.tab, value);
                }
                self.set_status(formatted::next_audio_export(
                    self.language(),
                    &summary(value, self.language()),
                ));
            } else {
                self.set_status(
                    Text::AudioOptionsSourceChanged
                        .in_language(self.language())
                        .into(),
                );
            }
        }
        self.request_redraw();
    }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
pub(crate) mod track_tests;
