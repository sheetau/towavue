use crate::*;
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
    ime_composing: bool,
}

impl MetadataDialog {
    fn image_format(&self) -> Option<ImageMetadataFormat> {
        (self.kind == MediaKind::Image)
            .then(|| ImageMetadataFormat::from_path(&self.source))
            .flatten()
    }

    fn options(&self) -> Result<MetadataExportOptions, String> {
        if self.kind == MediaKind::Image {
            if self.image_format().is_none() {
                return Err("Image metadata requires PNG, JPEG or WebP input.".into());
            }
            match &self.current {
                Some(Ok(_)) => {}
                Some(Err(_)) => {
                    return Err("Image metadata must be readable before applying options.".into());
                }
                None => {
                    return Err(
                        "Wait for image metadata inspection before applying options.".into(),
                    );
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
            options
                .set(field, value)
                .map_err(|error| format!("{}: {error}", field.label()))?;
        }
        if let Some(format) = self.image_format() {
            format
                .validate_options(&options)
                .map_err(|error| error.to_string())?;
        }
        Ok(options)
    }

    fn show(&mut self, context: &egui::Context) -> Option<Option<MetadataExportOptions>> {
        let mut action = None;
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
        let modal = egui::Modal::new(egui::Id::new(("metadata-export-options", self.token))).show(context, |ui| {
            ui.set_width((context.content_rect().width() - 48.0).clamp(1.0, 420.0));
            chrome::modal_heading(ui, "Metadata export options");
            egui::ScrollArea::vertical().max_height((context.content_rect().height() - 128.0).max(20.0)).min_scrolled_height(20.0).show(ui, |ui| {
                ui.label("Metadata field");
                let response = egui::ComboBox::from_id_salt("metadata-field")
                    .selected_text(MetadataField::ALL[self.selected].label()).show_ui(ui, |ui| {
                        for (index, field) in MetadataField::ALL.into_iter().enumerate() {
                            if !fields.contains(&field) { continue; }
                            if ui.selectable_value(&mut self.selected, index, field.label()).clicked() { ui.close(); }
                        }
                    }).response;
                if self.first_frame { response.request_focus(); self.first_frame = false; }
                if response.gained_focus() { response.scroll_to_me(None); }
                let field = MetadataField::ALL[self.selected];
                if image_format == Some(ImageMetadataFormat::Png) {
                    ui.label(format!("PNG keyword: {}", match field {
                        MetadataField::Artist => "Author",
                        MetadataField::AlbumArtist => "Album Artist",
                        MetadataField::Date => "Creation Time (text, no date conversion)",
                        _ => field.label(),
                    }));
                } else if let Some(format @ (ImageMetadataFormat::Jpeg | ImageMetadataFormat::Webp)) = image_format {
                    ui.label(format!("{} XMP property: {}", format.label(), match field {
                        MetadataField::Title => "dc:title (language alternatives)",
                        MetadataField::Artist => "dc:creator (ordered authors)",
                        MetadataField::Album => "xmpDM:album (text)",
                        MetadataField::Composer => "xmpDM:composer (text)",
                        MetadataField::Genre => "xmpDM:genre (text)",
                        MetadataField::Date => "xmpDM:releaseDate (release date, not capture time; YYYY, YYYY-MM, YYYY-MM-DD or date/time with optional timezone)",
                        MetadataField::Track => "xmpDM:trackNumber (decimal integer with optional sign, not track/total)",
                        MetadataField::Comment => "dc:description (language alternatives)",
                        MetadataField::Copyright => "dc:rights (language alternatives)",
                        _ => unreachable!("XMP field selector is restricted"),
                    }));
                }
                let draft = &mut self.fields[self.selected];
                for (mode, label) in [(Mode::Keep, "Keep source value"), (Mode::Set, "Set value"), (Mode::Remove, "Remove value")] {
                    let response = ui.radio_value(&mut draft.mode, mode, label);
                    if response.gained_focus() { response.scroll_to_me(None); }
                }
                if draft.mode == Mode::Set {
                    let response = ui.push_id(self.selected, |ui| resize::multiline_text_input(ui, "Metadata value (empty removes the tag)", &mut draft.text)).inner;
                    if response.gained_focus() { response.scroll_to_me(None); }
                }
                ui.separator();
                ui.label("Current source values");
                match &self.current {
                    None => { ui.label("Reading metadata…"); }
                    Some(Err(error)) => { ui.label(format!("Could not read metadata: {error}")); }
                    Some(Ok(values)) => {
                        let mut found = false;
                        for value in values.iter().filter(|value| value.field == field) {
                            found = true;
                            ui.label(format!("{}{}: {}", value.scope, if value.truncated { " (truncated)" } else { "" }, value.value));
                        }
                        if !found { ui.label(if self.kind == MediaKind::Image { "No matching image text value." } else { "No value in the file or selected streams." }); }
                    }
                }
                ui.separator();
                if image_format == Some(ImageMetadataFormat::Png) {
                    ui.label("PNG input and PNG output only. Applies to the next Save or Export as for this tab's current file. Original file, displayed pixels and edit history stay unchanged.");
                    ui.label("Only these 10 PNG text fields are edited; EXIF, XMP and technical metadata are not edited. Keep preserves matching source text chunks in PNG output, including when all fields are Keep. Other formats do not guarantee preservation.");
                    ui.label("Set/Remove replaces all matching text variants. Choose a .png or .apng export path; other output formats fail without replacing the target. Reading rejects corrupt text or more than 128 text chunks / 1 MiB stored or expanded text.");
                    ui.label("Supported APNG saves retain all frames, delays and loop count (1 to 65536 frames), including PREVIOUS disposal. A separate default poster receives the same edits and stays outside the animation. Frame compositing is shared with display. Other animation formats are separate capabilities.");
                } else if image_format == Some(ImageMetadataFormat::Jpeg) {
                    ui.label("JPEG input supports JPEG or WebP output. Applies to the next Save or Export as for this tab's current file. Original file, displayed pixels and edit history stay unchanged.");
                    ui.label("JPEG output with no image edits preserves compressed pixels and all non-XMP markers (including EXIF, ICC, IPTC and comments). Image edits or format conversion still re-encode. Remove is not a privacy scrub: copies in other metadata remain.");
                    ui.label("Only these 9 XMP fields are edited. EXIF, IPTC and JPEG comments (COM) are not synchronized; unknown or technical source XMP is not copied. Keep preserves supported source values as written, languages and author order, including when all fields are Keep. Existing noncanonical Date/Track values are retained; new values must match the displayed types.");
                    ui.label("Set replaces all values of the field with one (x-default for language alternatives). Remove deletes all values. Choose a .jpg, .jpeg or .webp export path; other output formats fail without replacing the target. Extended XMP, corrupt or oversized metadata is rejected (one packet, 65502 bytes, 128 text values).");
                } else if image_format == Some(ImageMetadataFormat::Webp) {
                    ui.label("WebP input supports WebP output, or JPEG output for static images. Applies to the next Save or Export as for this tab's current file. Original file, displayed pixels and edit history stay unchanged. Animated WebP retains all frames, exact timing and loops using lossless full-canvas snapshots; compositing and edits are shared with display.");
                    ui.label("Only these 9 XMP fields are edited. Source EXIF, ICC and unknown or technical XMP are not copied or synchronized. Keep preserves supported source values as written, languages and author order, including when all fields are Keep. Existing noncanonical Date/Track values are retained; new values must match the displayed types.");
                    ui.label("Set replaces all values of the field with one (x-default for language alternatives). Remove deletes all values. Choose a .webp export path, or .jpg/.jpeg for a static image; animated JPEG conversion and other output formats fail without replacing the target. JPEG cannot retain transparency. Corrupt or oversized metadata is rejected (one XMP packet, 65502 bytes, 128 text values).");
                } else if self.kind == MediaKind::Image {
                    ui.label("Image metadata supports PNG to PNG and JPEG/WebP output from JPEG or static WebP. Animated WebP metadata requires WebP output. Other image formats cannot apply metadata options.");
                } else {
                    ui.label("Applies to the next Save, Export as and Export audio only for this tab's current file. Playback, original file and edit history stay unchanged.");
                    ui.label("Set/Remove affects the file and output streams. Unsupported tags or changed values fail before replacing the target. Keep is not a guarantee of complete metadata preservation across formats.");
                }
                ui.label("Up to 1024 UTF-8 bytes per field / 4096 total; no NUL. Settings reset on reload, another file or closing the tab. Apply does not export.");
                if let Err(error) = self.options() { ui.label(error); }
            });
            let options = self.options();
            ui.horizontal(|ui| {
                if ui.add_enabled(options.is_ok() && !self.ime_composing, egui::Button::new("Apply metadata")).clicked() { action = Some(Some(options.expect("valid options"))); }
                if ui.button("Cancel").clicked() { action = Some(None); }
            });
        });
        if modal.is_top_modal && escape {
            action = Some(None);
        }
        action
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn open_metadata_export_options(&mut self) {
        if self.modal_input_blocked() || self.media_kind.is_none() {
            return;
        }
        if self.active_export.is_some() {
            self.set_status(
                "Wait for the current export or cancel it before changing metadata options.".into(),
            );
            return;
        }
        let Some(tab) = self.tabs.active() else {
            return;
        };
        if self.media_kind != Some(tab.target.media_kind())
            || self.path.as_deref() != Some(tab.target.current_path())
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
        let source = tab.target.current_path().to_owned();
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
                let result = towavue_runtime_windows::read_export_metadata(&source, kind)
                    .map_err(|error| error.to_string());
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
                    && tab.target.current_path() == dialog.source
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
            self.set_status("Metadata options cancelled because the source changed.".into());
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
            && (dialog.options().is_err()
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
                    "Metadata options applied to the next export. Playback and edits unchanged."
                        .into(),
                );
            } else {
                self.set_status("Metadata options cancelled because the source changed.".into());
            }
        }
        self.request_redraw();
    }
}

#[cfg(test)]
pub(crate) mod tests;
