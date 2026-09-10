use crate::*;
use towavue_runtime_windows::AudioChannels;

pub(super) struct AudioExportDialog {
    token: u64,
    tab: TabId,
    source: PathBuf,
    kind: MediaKind,
    generation: u64,
    options: AudioExportOptions,
    first_frame: bool,
}

pub(super) fn summary(options: AudioExportOptions) -> String {
    format!(
        "Peak {} / {}",
        if options.normalize_peak {
            "-1 dBFS"
        } else {
            "off"
        },
        match options.channels {
            AudioChannels::Keep => "Keep channels",
            AudioChannels::Mono => "Mono",
            AudioChannels::Stereo => "Stereo",
        }
    )
}

impl AudioExportDialog {
    fn show(&mut self, context: &egui::Context) -> Option<Option<AudioExportOptions>> {
        let mut action = None;
        let modal = egui::Modal::new("audio-export-options".into()).show(context, |ui| {
            ui.set_width((context.content_rect().width() - 48.0).clamp(1.0, 340.0));
            chrome::modal_heading(ui, "Audio export options");
            egui::ScrollArea::vertical().max_height((context.content_rect().height() - 128.0).max(20.0)).min_scrolled_height(20.0).show(ui, |ui| {
                let response = ui.checkbox(&mut self.options.normalize_peak, "Normalize peak (-1 dBFS)");
                if self.first_frame { response.request_focus(); self.first_frame = false; }
                if response.gained_focus() { response.scroll_to_me(None); }
                ui.label("Output channels");
                for (value, label) in [(AudioChannels::Keep, "Keep source channels"), (AudioChannels::Mono, "Mono"), (AudioChannels::Stereo, "Stereo")] {
                    let response = ui.radio_value(&mut self.options.channels, value, label);
                    if response.gained_focus() { response.scroll_to_me(None); }
                }
                ui.separator();
                ui.label("Applies to the next Save, Export as and Export audio only for this tab's current file. Playback and edit history stay unchanged.");
                ui.label("Peak normalization analyzes edited audio first, then applies one common gain. It can override overall volume edits, but preserves relative dynamics and silence. Not LUFS or true-peak; lossy encoding may change peaks.");
                ui.label("Mono averages left/right; Stereo duplicates mono. Conversion requires a mono or stereo input; use Keep for multichannel audio. The source must contain audio.");
                ui.label("Settings last while this file stays in this tab. Apply does not export a file.");
            });
            ui.horizontal(|ui| {
                if ui.button("Apply options").clicked() { action = Some(Some(self.options)); }
                if ui.button("Cancel").clicked() { action = Some(None); }
            });
        });
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
        self.audio_export_generation = self.audio_export_generation.wrapping_add(1);
        self.audio_export_dialog = Some(AudioExportDialog {
            token: self.audio_export_generation,
            tab: tab.id,
            source: tab.target.current_path().to_owned(),
            kind: tab.target.media_kind(),
            generation: self.media_generation,
            options: self
                .audio_export_settings
                .get(&tab.id)
                .copied()
                .unwrap_or_default(),
            first_frame: true,
        });
        self.request_redraw();
    }

    fn audio_export_dialog_is_current(&self, dialog: &AudioExportDialog) -> bool {
        self.media_generation == dialog.generation
            && self.media_kind == Some(dialog.kind)
            && self.path.as_ref() == Some(&dialog.source)
            && self.tabs.active().is_some_and(|tab| {
                tab.id == dialog.tab
                    && tab.target.current_path() == dialog.source
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
