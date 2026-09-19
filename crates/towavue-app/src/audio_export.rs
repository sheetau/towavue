use crate::*;
use towavue_runtime_windows::{AudioChannels, AudioNormalization, LoudnessTarget};

pub(super) struct AudioExportDialog {
    token: u64,
    tab: TabId,
    source: PathBuf,
    kind: MediaKind,
    generation: u64,
    options: AudioExportOptions,
    first_frame: bool,
    focused_option: Option<egui::Id>,
}

pub(super) fn summary(options: AudioExportOptions) -> String {
    let normalization = match options.normalization {
        AudioNormalization::Off => "Normalization off".into(),
        AudioNormalization::Peak => "Peak -1 dBFS".into(),
        AudioNormalization::Loudness(target) => format!(
            "{:.1} LUFS / max {:.1} dBTP",
            f64::from(target.integrated_tenths) / 10.0,
            f64::from(target.true_peak_tenths) / 10.0
        ),
    };
    format!(
        "{normalization} / {}",
        match options.channels {
            AudioChannels::Keep => "Keep channels",
            AudioChannels::Mono => "Mono",
            AudioChannels::Stereo => "Stereo",
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
            ui.add(
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
        let modal = chrome::modal(context, "audio-export-options".into(), false).show(context, |ui| {
            chrome::modal_body(ui, 340.0, "Audio export options", &["Apply options", "Cancel"], |ui| {
                ui.label("Normalization");
                for (value, label) in [
                    (AudioNormalization::Off, "Off"),
                    (AudioNormalization::Peak, "Peak (-1 dBFS)"),
                    (AudioNormalization::Loudness(match self.options.normalization { AudioNormalization::Loudness(target) => target, _ => LoudnessTarget::default() }), "Loudness"),
                ] {
                    let response = ui.radio_value(&mut self.options.normalization, value, label).on_hover_cursor(egui::CursorIcon::PointingHand);
                    if self.first_frame { response.request_focus(); self.first_frame = false; }
                    reveal_focus(&response);
                }
                if let AudioNormalization::Loudness(target) = &mut self.options.normalization {
                    reveal_focus(&target_control(ui, &mut target.integrated_tenths, -700..=-50, "Integrated loudness (LUFS)"));
                    reveal_focus(&target_control(ui, &mut target.true_peak_tenths, -90..=0, "Maximum true peak (dBTP)"));
                    ui.label("Encoded audio is checked within 0.1 LU of the target and below the true-peak ceiling. Correction can require additional passes. Unmeasurable audio or an unmet target fails without replacing the destination.");
                }
                ui.label("Output channels");
                for (value, label) in [(AudioChannels::Keep, "Keep source channels"), (AudioChannels::Mono, "Mono"), (AudioChannels::Stereo, "Stereo")] {
                    let response = ui.radio_value(&mut self.options.channels, value, label).on_hover_cursor(egui::CursorIcon::PointingHand);
                    reveal_focus(&response);
                }
                crate::chrome::separator(ui);
                ui.label("Applies to the next Save, Save as and Export audio only for this tab's current file. Playback and edit history stay unchanged.");
                ui.label("Peak applies one common gain to reach -1 dBFS sample peak; lossy encoding may change peaks. Loudness analyzes edited audio after channel conversion, preserves dynamics when gain alone fits, and otherwise limits peaks without imposing a fixed loudness range. Normalization can override overall volume edits.");
                ui.label("Mono averages left/right; Stereo duplicates mono. Conversion requires a mono or stereo input; use Keep for multichannel audio. The source must contain audio.");
                ui.label("Settings last while this file stays in this tab. Apply does not export a file.");
            });
            ui.horizontal_wrapped(|ui| {
                crate::chrome::flat_buttons(ui);
                if ui.button("Apply options").clicked() { action = Some(Some(self.options)); }
                if ui.button("Cancel").clicked() { action = Some(None); }
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
    pub(super) fn open_audio_export_options(&mut self) {
        if self.modal_input_blocked()
            || !matches!(self.media_kind, Some(MediaKind::Audio | MediaKind::Video))
        {
            return;
        }
        if self.active_export.is_some() {
            self.set_status(
                "Wait for the current export or cancel it before changing export options.".into(),
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
            self.set_status("Audio export options cancelled because the source changed.".into());
        }
    }

    pub(super) fn show_audio_export_options(
        &mut self,
        context: &egui::Context,
        actions: &mut Vec<UiAction>,
    ) {
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
                if value == AudioExportOptions::default() {
                    self.audio_export_settings.remove(&dialog.tab);
                } else {
                    self.audio_export_settings.insert(dialog.tab, value);
                }
                self.set_status(format!(
                    "Next export: {}. Playback and edits unchanged.",
                    summary(value)
                ));
            } else {
                self.set_status(
                    "Audio export options cancelled because the source changed.".into(),
                );
            }
        }
        self.request_redraw();
    }
}

#[cfg(test)]
pub(crate) mod tests;
