use crate::localization::{Language, Text, language};
use crate::*;
use towavue_core::localization::formatted;
use towavue_runtime_windows::{
    ImageMetadataFormat, MetadataExportOptions, MetadataField, MetadataSourceValue,
};

#[derive(Clone, Copy, Default, PartialEq)]
enum Mode {
    #[default]
    Keep,
    Set,
    Remove,
}

#[derive(Default)]
struct FieldDraft {
    mode: Mode,
    text: String,
}

pub(super) struct MetadataDialog {
    token: u64,
    tab: TabId,
    source: PathBuf,
    kind: MediaKind,
    generation: u64,
    fields: [FieldDraft; 10],
    selected: usize,
    current: Option<Result<Vec<MetadataSourceValue>, String>>,
    first_frame: bool,
    focused_control: Option<egui::Id>,
    ime_composing: bool,
}

impl MetadataDialog {
    fn image_format(&self) -> Option<ImageMetadataFormat> {
        (self.kind == MediaKind::Image)
            .then(|| ImageMetadataFormat::from_path(&self.source))
            .flatten()
    }

    #[cfg(test)]
    fn options(&self) -> Result<MetadataExportOptions, String> {
        self.options_in(Language::English)
    }

    fn options_in(&self, display_language: Language) -> Result<MetadataExportOptions, String> {
        if self.kind == MediaKind::Image {
            if self.image_format().is_none() {
                return Err(Text::MetadataImageInput
                    .in_language(display_language)
                    .into());
            }
            match &self.current {
                Some(Ok(_)) => {}
                Some(Err(_)) => {
                    return Err(Text::MetadataMustBeReadable
                        .in_language(display_language)
                        .into());
                }
                None => {
                    return Err(Text::WaitMetadataInspection
                        .in_language(display_language)
                        .into());
                }
            }
        }
        let mut options = MetadataExportOptions::default();
        for (field, draft) in MetadataField::ALL.into_iter().zip(&self.fields) {
            let value = match draft.mode {
                Mode::Keep => None,
                Mode::Remove => Some(String::new()),
                Mode::Set => Some(draft.text.clone()),
            };
            options.set(field, value).map_err(|error| {
                format!(
                    "{}: {}",
                    field.label_in(display_language),
                    error.message(display_language)
                )
            })?;
        }
        if let Some(format) = self.image_format() {
            format
                .validate_options(&options)
                .map_err(|error| error.message(display_language))?;
        }
        Ok(options)
    }

    fn show(&mut self, context: &egui::Context) -> Option<Option<MetadataExportOptions>> {
        let display_language = language(context);
        let mut action = None;
        let previous_focus = self.focused_control;
        let mut focused_control = None;
        let mut reveal_focus = |response: &egui::Response| {
            if response.has_focus() {
                focused_control = Some(response.id);
                // Arrow focus resolves after layout; gained_focus can miss it.
                // Unchanged focus must not undo manual scrolling.
                if focused_control != previous_focus {
                    response.scroll_to_me(None);
                }
            }
        };
        let image_format = self.image_format();
        let fields = image_format.map_or(&MetadataField::ALL[..], ImageMetadataFormat::fields);
        let popup_open = egui::Popup::is_any_open(context);
        let escape = context.input_mut(|input| {
            let mut ime_event = false;
            for event in &input.events {
                if let egui::Event::Ime(event) = event {
                    ime_event = true;
                    self.ime_composing = match event {
                        egui::ImeEvent::Preedit { text, .. } => !text.is_empty(),
                        _ => false,
                    };
                }
            }
            if self.ime_composing || ime_event {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                false
            } else {
                !popup_open && input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
            }
        });
        let modal = chrome::modal(
            context,
            egui::Id::new(("metadata-export-options", self.token)),
            false,
        )
        .show(context, |ui| {
            chrome::modal_body(
                ui,
                Text::CommandMetadataExportOptions.in_language(display_language),
                &[
                    Text::ApplyMetadata.in_language(display_language),
                    Text::Cancel.in_language(display_language),
                ],
                |ui| {
                    ui.label(Text::MetadataField.in_language(display_language));
                    let response = chrome::combo_box(
                        ui,
                        egui::ComboBox::from_id_salt("metadata-field").selected_text(
                            MetadataField::ALL[self.selected].label_in(display_language),
                        ),
                        |ui| {
                            for (index, field) in MetadataField::ALL.into_iter().enumerate() {
                                if !fields.contains(&field) {
                                    continue;
                                }
                                if ui
                                    .selectable_value(
                                        &mut self.selected,
                                        index,
                                        field.label_in(display_language),
                                    )
                                    .clicked()
                                {
                                    ui.close();
                                }
                            }
                        },
                    )
                    .response
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if self.first_frame {
                        response.request_focus();
                        self.first_frame = false;
                    }
                    reveal_focus(&response);
                    let field = MetadataField::ALL[self.selected];
                    if image_format == Some(ImageMetadataFormat::Png) {
                        ui.label(formatted::png_keyword(
                            display_language,
                            match field {
                                MetadataField::Artist => "Author",
                                MetadataField::AlbumArtist => "Album Artist",
                                MetadataField::Date => {
                                    Text::PngCreationTimeHelp.in_language(display_language)
                                }
                                _ => field.label(),
                            },
                        ));
                    } else if let Some(
                        format @ (ImageMetadataFormat::Jpeg | ImageMetadataFormat::Webp),
                    ) = image_format
                    {
                        ui.label(formatted::xmp_property(
                            display_language,
                            format.label(),
                            match field {
                                MetadataField::Title => {
                                    Text::XmpTitleHelp.in_language(display_language)
                                }
                                MetadataField::Artist => {
                                    Text::XmpArtistHelp.in_language(display_language)
                                }
                                MetadataField::Album => {
                                    Text::XmpAlbumHelp.in_language(display_language)
                                }
                                MetadataField::Composer => {
                                    Text::XmpComposerHelp.in_language(display_language)
                                }
                                MetadataField::Genre => {
                                    Text::XmpGenreHelp.in_language(display_language)
                                }
                                MetadataField::Date => {
                                    Text::XmpDateHelp.in_language(display_language)
                                }
                                MetadataField::Track => {
                                    Text::XmpTrackHelp.in_language(display_language)
                                }
                                MetadataField::Comment => {
                                    Text::XmpCommentHelp.in_language(display_language)
                                }
                                MetadataField::Copyright => {
                                    Text::XmpCopyrightHelp.in_language(display_language)
                                }
                                _ => unreachable!("XMP field selector is restricted"),
                            },
                        ));
                    }
                    let draft = &mut self.fields[self.selected];
                    for (mode, label) in [
                        (
                            Mode::Keep,
                            Text::KeepSourceValue.in_language(display_language),
                        ),
                        (
                            Mode::Set,
                            Text::SetMetadataValue.in_language(display_language),
                        ),
                        (
                            Mode::Remove,
                            Text::RemoveMetadataValue.in_language(display_language),
                        ),
                    ] {
                        let response = ui
                            .radio_value(&mut draft.mode, mode, label)
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        reveal_focus(&response);
                    }
                    if draft.mode == Mode::Set {
                        let response = ui
                            .push_id(self.selected, |ui| {
                                resize::multiline_text_input(
                                    ui,
                                    Text::MetadataValueInput.in_language(display_language),
                                    &mut draft.text,
                                )
                            })
                            .inner;
                        reveal_focus(&response);
                    }
                    crate::chrome::separator(ui);
                    ui.label(Text::CurrentSourceValues.in_language(display_language));
                    match &self.current {
                        None => {
                            ui.label(Text::ReadingMetadata.in_language(display_language));
                        }
                        Some(Err(error)) => {
                            ui.label(formatted::metadata_read_failed(display_language, error));
                        }
                        Some(Ok(values)) => {
                            let mut found = false;
                            for value in values.iter().filter(|value| value.field == field) {
                                found = true;
                                ui.label(format!(
                                    "{}{}: {}",
                                    value.scope.message(display_language),
                                    if value.truncated {
                                        Text::TruncatedSuffix.in_language(display_language)
                                    } else {
                                        ""
                                    },
                                    value.value
                                ));
                            }
                            if !found {
                                ui.label(if self.kind == MediaKind::Image {
                                    Text::NoImageMetadataValue.in_language(display_language)
                                } else {
                                    Text::NoStreamMetadataValue.in_language(display_language)
                                });
                            }
                        }
                    }
                    crate::chrome::separator(ui);
                    if image_format == Some(ImageMetadataFormat::Png) {
                        ui.label(Text::PngMetadataScope.in_language(display_language));
                        ui.label(Text::PngMetadataFieldsHelp.in_language(display_language));
                        ui.label(Text::PngMetadataPreservationHelp.in_language(display_language));
                        ui.label(Text::PngMetadataLimitsHelp.in_language(display_language));
                        ui.label(Text::ApngMetadataHelp.in_language(display_language));
                    } else if image_format == Some(ImageMetadataFormat::Jpeg) {
                        ui.label(Text::JpegMetadataScope.in_language(display_language));
                        ui.label(Text::JpegMetadataPreservationHelp.in_language(display_language));
                        ui.label(Text::JpegXmpFieldsHelp.in_language(display_language));
                        ui.label(Text::JpegXmpLimitsHelp.in_language(display_language));
                    } else if image_format == Some(ImageMetadataFormat::Webp) {
                        ui.label(Text::WebpMetadataScope.in_language(display_language));
                        ui.label(Text::WebpMetadataPreservationHelp.in_language(display_language));
                        ui.label(Text::WebpXmpFieldsHelp.in_language(display_language));
                        ui.label(Text::WebpXmpLimitsHelp.in_language(display_language));
                    } else if self.kind == MediaKind::Image {
                        ui.label(Text::ImageMetadataFormatsHelp.in_language(display_language));
                    } else {
                        ui.label(Text::MediaMetadataScope.in_language(display_language));
                        ui.label(Text::MediaMetadataPreservationHelp.in_language(display_language));
                    }
                    ui.label(Text::MetadataLimitsHelp.in_language(display_language));
                    if let Err(error) = self.options_in(display_language) {
                        ui.label(error);
                    }
                },
            );
            let options = self.options_in(display_language);
            ui.horizontal_wrapped(|ui| {
                crate::chrome::flat_buttons(ui);
                if ui
                    .add_enabled(
                        options.is_ok() && !self.ime_composing,
                        egui::Button::new(Text::ApplyMetadata.in_language(display_language)),
                    )
                    .clicked()
                {
                    action = Some(Some(options.expect("valid options")));
                }
                if ui
                    .button(Text::Cancel.in_language(display_language))
                    .clicked()
                {
                    action = Some(None);
                }
            });
        });
        self.focused_control = focused_control;
        if modal.is_top_modal && escape {
            action = Some(None);
        }
        action
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn open_metadata_export_options(&mut self) {
        let display_language = self.language();
        if self.modal_input_blocked() || self.media_kind.is_none() {
            return;
        }
        if self.active_export.is_some() {
            self.set_status(
                Text::WaitExportForMetadata
                    .in_language(self.language())
                    .into(),
            );
            return;
        }
        let Some(tab) = self.tabs.active() else {
            return;
        };
        if self.media_kind != Some(tab.target.media_kind())
            || self.path.as_deref() != tab.target.current_path()
        {
            return;
        }
        self.guard_return_focus = self
            .ui_context
            .as_ref()
            .and_then(|context| context.memory(egui::Memory::focused))
            .map(|focus| (tab.id, focus));
        self.metadata_generation = self.metadata_generation.wrapping_add(1);
        if let Some(context) = &self.ui_context {
            // Close background menus once; later frames retain this modal's field selector.
            egui::Popup::close_all(context);
        }
        let settings = self
            .metadata_export_settings
            .get(&tab.id)
            .cloned()
            .unwrap_or_default();
        let Some(input) = self.document_input(tab.id) else {
            return;
        };
        let source = input.logical_path().to_owned();
        let kind = tab.target.media_kind();
        let token = self.metadata_generation;
        self.metadata_dialog = Some(MetadataDialog {
            token,
            tab: tab.id,
            source: source.clone(),
            kind,
            generation: self.media_generation,
            fields: std::array::from_fn(|index| match settings.get(MetadataField::ALL[index]) {
                None => FieldDraft::default(),
                Some("") => FieldDraft {
                    mode: Mode::Remove,
                    text: String::new(),
                },
                Some(value) => FieldDraft {
                    mode: Mode::Set,
                    text: value.into(),
                },
            }),
            selected: 0,
            current: None,
            first_frame: true,
            focused_control: None,
            ime_composing: false,
        });
        if self.metadata_worker.is_none() {
            match LatestTask::new("towavue-metadata") {
                Ok(worker) => self.metadata_worker = Some(worker),
                Err(error) => self.finish_metadata_read(token, Err(error.to_string())),
            }
        }
        if let Some(worker) = &self.metadata_worker {
            let notify = Arc::clone(&self.notify);
            worker.submit(move |cancellation| {
                if cancellation.is_cancelled() {
                    return;
                }
                let result = towavue_runtime_windows::read_export_metadata(input.path(), kind)
                    .map_err(|error| error.message(display_language));
                if !cancellation.is_cancelled() {
                    notify(AppEvent::MetadataLoaded(token, result));
                }
            });
        }
        self.request_redraw();
    }

    fn metadata_dialog_is_current(&self, dialog: &MetadataDialog) -> bool {
        self.media_generation == dialog.generation
            && self.media_kind == Some(dialog.kind)
            && self.path.as_ref() == Some(&dialog.source)
            && self.tabs.active().is_some_and(|tab| {
                tab.id == dialog.tab
                    && self
                        .document_input(tab.id)
                        .is_some_and(|input| input.logical_path() == dialog.source)
                    && tab.target.media_kind() == dialog.kind
            })
    }

    pub(super) fn finish_metadata_read(
        &mut self,
        token: u64,
        result: Result<Vec<MetadataSourceValue>, String>,
    ) {
        if self
            .metadata_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.token == token && self.metadata_dialog_is_current(dialog))
        {
            self.metadata_dialog
                .as_mut()
                .expect("current dialog")
                .current = Some(result);
            self.request_redraw();
        }
    }

    pub(super) fn cancel_metadata_dialog(&mut self) {
        if self.metadata_dialog.take().is_some()
            && let Some(context) = &self.ui_context
        {
            egui::Popup::close_all(context);
        }
        if let Some(worker) = &self.metadata_worker {
            worker.clear();
        }
    }

    pub(super) fn cancel_stale_metadata_dialog(&mut self) {
        if self
            .metadata_dialog
            .as_ref()
            .is_some_and(|dialog| !self.metadata_dialog_is_current(dialog))
        {
            self.cancel_metadata_dialog();
            self.set_status(
                Text::MetadataOptionsSourceChanged
                    .in_language(self.language())
                    .into(),
            );
        }
    }

    pub(super) fn show_metadata_options(
        &mut self,
        context: &egui::Context,
        actions: &mut Vec<UiAction>,
    ) {
        if let Some(dialog) = &mut self.metadata_dialog
            && let Some(action) = dialog.show(context)
        {
            actions.push(UiAction::FinishMetadataOptions(dialog.token, action));
        }
    }

    pub(super) fn finish_metadata_options(
        &mut self,
        token: u64,
        value: Option<MetadataExportOptions>,
    ) {
        if self
            .metadata_dialog
            .as_ref()
            .is_none_or(|dialog| dialog.token != token)
        {
            return;
        }
        let dialog = self.metadata_dialog.as_ref().expect("matching dialog");
        let tab = dialog.tab;
        let current = self.metadata_dialog_is_current(dialog);
        // UIA or queued actions must not bypass image capability/read validation.
        if let Some(value) = &value
            && current
            && dialog.kind == MediaKind::Image
            && (dialog.options_in(self.language()).is_err()
                || dialog
                    .image_format()
                    .is_none_or(|format| format.validate_options(value).is_err()))
        {
            return;
        }
        self.cancel_metadata_dialog();
        if let Some(value) = value {
            if current {
                if value.is_empty() {
                    self.metadata_export_settings.remove(&tab);
                } else {
                    self.metadata_export_settings.insert(tab, value);
                }
                self.set_status(
                    Text::MetadataOptionsApplied
                        .in_language(self.language())
                        .into(),
                );
            } else {
                self.set_status(
                    Text::MetadataOptionsSourceChanged
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
