use crate::*;

#[cfg(test)]
mod tests;

pub(super) struct FolderPosition {
    pub instance: u64,
    pub count: usize,
    pages: Option<Vec<PathBuf>>,
    pub index: usize,
    pub revision: u64,
    pub reading: Option<ReadingSettings>,
}

impl FolderPosition {
    pub fn show(
        &self,
        ui: &mut egui::Ui,
        thumbnail: egui::Rect,
    ) -> Option<preview_transport::Action> {
        let pixel = 1.0 / ui.ctx().pixels_per_point();
        let rect = egui::Rect::from_center_size(
            egui::pos2(
                thumbnail.center().x,
                (thumbnail.bottom() / pixel).floor() * pixel - pixel * 0.5,
            ),
            egui::vec2(thumbnail.width(), 12.0),
        );
        let count = self.count;
        let reversed = self.reading.is_some_and(|settings| settings.reversed);
        let progress = self.index as f32 / count.saturating_sub(1).max(1) as f32;
        ui.add_enabled_ui(count > 1, |ui| {
            let (response, drag) = seekbar::inline_directed(
                ui,
                rect,
                ui.id()
                    .with(("preview-image-position", self.instance, self.revision)),
                progress,
                reversed,
            );
            let value = seekbar::directed_value_input(
                &response,
                "Preview image position",
                (self.index + 1) as f64,
                1.0..=count as f64,
                1.0,
                true,
                reversed,
            );
            value
                .map(|value| value.round() as usize - 1)
                .or_else(|| {
                    drag.released
                        .then_some(drag.position)
                        .flatten()
                        .map(|point| {
                            seekbar::item_index(
                                seekbar::directed_ratio(rect, point.x, reversed),
                                count,
                            )
                        })
                })
                .filter(|index| *index != self.index)
                .map(preview_transport::Action::ImageSeek)
        })
        .inner
    }

    pub fn reading_paths(&self) -> Option<Vec<PathBuf>> {
        self.pages.clone()
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    fn preview_image_snapshot(
        &self,
        id: TabId,
        path: &Path,
    ) -> Option<(u64, &FolderSnapshot, Option<ReadingSettings>)> {
        let tab = self.tabs.tabs().iter().find(|tab| tab.id == id)?;
        if tab.target.media_kind() != MediaKind::Image || tab.target.current_path() != path {
            return None;
        }
        let (instance, snapshot, reading, settings) =
            if self.displayed_tab == Some(id) && self.path.as_deref() == Some(path) {
                (
                    self.media_generation,
                    self.folder_snapshot.as_ref(),
                    self.reading_mode,
                    self.reading_settings,
                )
            } else {
                let saved = self
                    .retained_images
                    .get(&id)
                    .filter(|saved| saved.path == path)?;
                (
                    saved.instance,
                    saved.folder_snapshot.as_ref(),
                    saved.reading_mode,
                    saved.reading_settings,
                )
            };
        Some((instance, snapshot?, reading.then_some(settings)))
    }

    pub(super) fn preview_folder(&self, id: TabId, path: &Path) -> Option<FolderPosition> {
        let (instance, snapshot, reading) = self.preview_image_snapshot(id, path)?;
        let mut index = None;
        let mut count = 0;
        for item in snapshot.items_of_kind(MediaKind::Image) {
            if item.path == path {
                index = Some(count);
            }
            count += 1;
        }
        let index = index?;
        // Only the at-most-ten reading paths are copied. Large folders do not
        // allocate a full path list on each hover redraw.
        let pages = reading.map(|settings| {
            let range = settings.spread(index, count);
            snapshot
                .items_of_kind(MediaKind::Image)
                .skip(range.start)
                .take(range.len())
                .map(|item| item.path.clone())
                .collect()
        });
        Some(FolderPosition {
            instance,
            count,
            pages,
            index,
            revision: snapshot.generation,
            reading,
        })
    }

    pub(super) fn preview_image_path(
        &self,
        id: TabId,
        source: &Path,
        index: usize,
    ) -> Option<PathBuf> {
        let (_, snapshot, _) = self.preview_image_snapshot(id, source)?;
        snapshot
            .items_of_kind(MediaKind::Image)
            .nth(index)
            .map(|item| item.path.clone())
    }

    pub(super) fn handle_preview_image_seek(
        &mut self,
        id: TabId,
        instance: u64,
        source: PathBuf,
        path: PathBuf,
    ) {
        if self.preview_input_blocked()
            || !self.valid_preview_image_destination(id, instance, &source, &path)
        {
            return;
        }
        self.request_guarded(GuardedAction::NavigateImageTab(id, instance, source, path));
    }

    fn valid_preview_image_destination(
        &self,
        id: TabId,
        instance: u64,
        source: &Path,
        path: &Path,
    ) -> bool {
        source != path
            && self
                .preview_image_snapshot(id, source)
                .is_some_and(|(owner, snapshot, _)| {
                    owner == instance
                        && snapshot
                            .items_of_kind(MediaKind::Image)
                            .any(|item| item.path == path)
                })
    }

    pub(super) fn navigate_image_tab(
        &mut self,
        id: TabId,
        instance: u64,
        source: PathBuf,
        path: PathBuf,
    ) {
        // A guard/save may outlive the source, folder contents, or tab owner.
        // Revalidate the selected path rather than applying an obsolete row index.
        if !self.valid_preview_image_destination(id, instance, &source, &path) {
            return;
        }
        if self.displayed_tab == Some(id) {
            self.navigate_to_unchecked(path);
            return;
        }
        let saved = self
            .retained_images
            .get_mut(&id)
            .expect("validated image tab");
        self.media_sequence = self
            .media_sequence
            .max(self.media_generation)
            .wrapping_add(1);
        saved.instance = self.media_sequence;
        saved.path = path.clone();
        saved.view = ImageViewState::default();
        saved.image = None;
        saved.reading_pages.clear();
        saved.previews.clear();
        saved.source = None;
        saved.operations = None;
        saved.comparison = None;
        saved.materialized = false;
        saved.error = None;
        saved.resume_loading = true;
        saved.resume_editing = false;
        saved.state = PlaybackState::Loading;
        saved.status_message = None;
        saved.export_notice = None;
        saved.filmstrip_view = filmstrip::View::default();
        // Hidden image tabs load originals on activation, like unopened tabs.
        // Their card uses bounded shared previews; never borrow the foreground loader.
        self.tabs
            .get_mut(id)
            .expect("validated tab")
            .target
            .set_current_path(path.clone(), MediaKind::Image);
        self.edits.insert(id, EditHistory::default());
        self.export_paths.remove(&id);
        self.audio_export_settings.remove(&id);
        self.metadata_export_settings.remove(&id);
        if let Some(context) = &self.ui_context {
            tab_focus::forget(context, id);
        }
        if let Some(recent) = &self.recent_files {
            recent.record(path);
        }
        self.request_redraw();
    }
}
