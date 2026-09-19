use crate::*;
use towavue_runtime_windows::{ImagePasteJob, PastedImage};

#[derive(Default)]
pub(super) struct State {
    pub serial: u64,
    pub pending: Option<ImagePasteJob>,
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn current_document_untitled(&self) -> bool {
        self.tabs
            .active()
            .is_some_and(|tab| matches!(tab.target, TabTarget::UntitledImage))
    }

    pub(super) fn paste_image(&mut self) {
        if self.modal_input_blocked() || self.active_export.is_some() {
            return;
        }
        self.image_paste.serial = self.image_paste.serial.wrapping_add(1);
        let serial = self.image_paste.serial;
        let notify = Arc::clone(&self.notify);
        match ImagePasteJob::start(move |result| notify(AppEvent::ImagePasted(serial, result))) {
            Ok(job) => {
                self.image_paste.pending = Some(job);
                self.set_status("Pasting image...".into());
            }
            Err(error) => self.set_status(format!("Could not paste image: {error}")),
        }
        self.request_redraw();
    }

    pub(super) fn finish_image_paste(&mut self, serial: u64, result: Result<PastedImage, String>) {
        if serial != self.image_paste.serial || self.image_paste.pending.is_none() {
            return;
        }
        self.image_paste.pending = None;
        if self.exit_requested {
            return;
        }
        match result.and_then(|pasted| self.open_pasted_image(pasted)) {
            Ok(()) => {}
            Err(error) => self.set_status(format!("Could not paste image: {error}")),
        }
        self.request_redraw();
    }

    pub(super) fn open_pasted_image(&mut self, pasted: PastedImage) -> Result<(), String> {
        let context = self
            .ui_context
            .as_ref()
            .ok_or("Image view is unavailable")?;
        // Validate the destination texture before changing the selected tab.
        let image = ImagePresentation::from_named_frame(
            context,
            "Untitled",
            Arc::clone(pasted.image()),
            0,
            self.image_sampling(),
        )?;
        let id = self.tabs.open_untitled_image();
        self.source_backings.insert(id, pasted.original().clone());
        self.edits.entry(id).or_default().invalidate_saved_source();
        self.load_document(None, MediaKind::Image, false);
        self.image = Some(image);
        self.image_error = None;
        self.refresh_title();
        Ok(())
    }

    pub(super) fn load_document(
        &mut self,
        path: Option<PathBuf>,
        kind: MediaKind,
        transferred: bool,
    ) {
        if let Some(path) = path {
            self.load_path_inner(path, kind, transferred, None);
            return;
        }
        let Some(id) = self.tabs.active().map(|tab| tab.id) else {
            return;
        };
        self.cancel_hold_speed();
        self.retain_image_tab();
        self.retain_playback_tab();
        self.clear_active_media();
        self.image_generation = self.image_loader.request(Vec::new());
        self.displayed_tab = Some(id);
        self.media_kind = Some(MediaKind::Image);
        if let Some(saved) = self
            .retained_images
            .remove(&id)
            .filter(|saved| saved.path.is_none())
        {
            self.restore_image_tab(saved);
        }
        self.refresh_title();
        self.request_redraw();
    }

    pub(super) fn owns_paste_shortcut(&self, stroke: &KeyStroke) -> bool {
        if self.modal_input_blocked()
            || self.palette_open
            || self.grid_open
            || self.native_ime_composing
            || self.ui_context.as_ref().is_none_or(|context| {
                context.text_edit_focused() || egui::Popup::is_any_open(context)
            })
        {
            return false;
        }
        let mut entered = self.entered_shortcut.clone();
        entered.push(stroke.clone());
        self.shortcuts
            .all(CommandId::PasteImage)
            .iter()
            .any(|binding| {
                binding.strokes().starts_with(&entered)
                    || binding.strokes().starts_with(std::slice::from_ref(stroke))
            })
    }
}

#[cfg(test)]
pub(crate) mod tests;
