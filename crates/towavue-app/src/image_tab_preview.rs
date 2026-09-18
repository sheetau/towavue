use crate::*;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Preparation {
    provider: Option<FolderOrderProvider>,
    pending: Option<(TabId, u64, PathBuf, u64)>,
    failed: bool,
}

impl Preparation {
    pub(super) fn cancel_for(&mut self, tab: TabId) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.0 == tab)
        {
            self.clear();
        }
    }

    fn clear(&mut self) {
        if self.pending.take().is_some()
            && let Some(provider) = &self.provider
        {
            provider.request(None);
        }
    }
}

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
    pub(super) fn prepare_image_tab(&mut self, tab: Option<TabId>) {
        let target = tab.and_then(|id| {
            self.tabs
                .tabs()
                .iter()
                .find(|tab| tab.id == id)
                .filter(|tab| {
                    tab.target.media_kind() == MediaKind::Image && self.displayed_tab != Some(id)
                })
                .map(|tab| (id, tab.target.current_path().to_owned()))
        });
        let Some((id, path)) = target else {
            self.image_tab_preparation.clear();
            return;
        };
        if !self.retained_images.contains_key(&id) {
            self.media_sequence = self
                .media_sequence
                .max(self.media_generation)
                .wrapping_add(1);
            // Only view identity and folder metadata are prepared here. Original
            // image loading stays with explicit activation; the card has its own
            // bounded thumbnail worker and never borrows the foreground loader.
            self.retained_images.insert(
                id,
                RetainedImageTab {
                    prepared_only: true,
                    path: path.clone(),
                    instance: self.media_sequence,
                    view: ImageViewState::default(),
                    reading_mode: false,
                    reading_settings: ReadingSettings::default(),
                    filmstrip_open: false,
                    filmstrip_view: filmstrip::View::default(),
                    timeline_open: false,
                    image: None,
                    reading_pages: Vec::new(),
                    previews: BTreeMap::new(),
                    folder_snapshot: self
                        .folder_snapshot
                        .as_ref()
                        .filter(|snapshot| {
                            Some(snapshot.folder_path.as_path()) == path.parent()
                                && snapshot.items.iter().any(|item| item.path == path)
                        })
                        .cloned(),
                    source: None,
                    operations: None,
                    comparison: None,
                    materialized: false,
                    error: None,
                    resume_loading: true,
                    resume_editing: false,
                    state: PlaybackState::Loading,
                    graphics_epoch: self.graphics_epoch,
                    status_message: None,
                    export_notice: None,
                },
            );
            self.request_redraw();
        }
        let saved = &self.retained_images[&id];
        if saved.path != path || saved.folder_snapshot.is_some() {
            self.image_tab_preparation.clear();
            return;
        }
        let instance = saved.instance;
        let preparation = &mut self.image_tab_preparation;
        if preparation
            .pending
            .as_ref()
            .is_some_and(|pending| pending.0 == id && pending.1 == instance && pending.2 == path)
            || preparation.failed
        {
            return;
        }
        let Some(folder) = path.parent() else {
            return;
        };
        if preparation.provider.is_none() {
            let notify = Arc::clone(&self.notify);
            match FolderOrderProvider::with_notify(move || notify(AppEvent::FolderReady)) {
                Ok(provider) => preparation.provider = Some(provider),
                Err(error) => {
                    eprintln!("towavue: image tab folder preparation unavailable: {error}");
                    preparation.failed = true;
                    return;
                }
            }
        }
        let generation = preparation
            .provider
            .as_ref()
            .expect("provider")
            .request(Some(folder.to_owned()));
        preparation.pending = Some((id, instance, path, generation));
    }

    pub(super) fn finish_image_tab_preparation(&mut self) {
        let preparation = &mut self.image_tab_preparation;
        let Some(snapshot) = preparation
            .provider
            .as_ref()
            .and_then(FolderOrderProvider::take_completed)
        else {
            return;
        };
        let Some((id, instance, path, generation)) = &preparation.pending else {
            return;
        };
        if snapshot.generation != *generation {
            return;
        }
        if let Some(saved) = self.retained_images.get_mut(id)
            && saved.instance == *instance
            && saved.path == *path
            && Some(snapshot.folder_path.as_path()) == path.parent()
            && self
                .tabs
                .tabs()
                .iter()
                .any(|tab| tab.id == *id && tab.target.current_path() == path)
        {
            saved.folder_snapshot = Some(snapshot);
        }
        preparation.pending = None;
        self.request_redraw();
    }

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
        for item in
            snapshot.reading_sequence(reading.is_some_and(|settings| settings.folder_reversed))
        {
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
                .reading_sequence(reading.is_some_and(|settings| settings.folder_reversed))
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
        let (_, snapshot, reading) = self.preview_image_snapshot(id, source)?;
        snapshot
            .reading_sequence(reading.is_some_and(|settings| settings.folder_reversed))
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
