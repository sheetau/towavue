use crate::*;
use towavue_core::{SubtitleDelay, SubtitleTrack, SubtitleTrackId};
use towavue_runtime_windows::{MediaInput, SubtitleDocument, SubtitleError};

mod paint;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Selection {
    #[default]
    None,
    Embedded(SubtitleTrackId),
    External,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Settings {
    pub selection: Selection,
    pub visible: bool,
    pub delay: SubtitleDelay,
    pub external: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            selection: Selection::None,
            visible: false,
            delay: SubtitleDelay::default(),
            external: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    Select(Selection),
    Show(bool),
    Delay(SubtitleDelay),
}

#[derive(Clone)]
pub(super) struct Choice {
    path: PathBuf,
    settings: Settings,
    document: Option<Arc<SubtitleDocument>>,
    error: Option<String>,
}

impl Choice {
    pub(super) fn relocate(&mut self, source: &Path, target: &Path) {
        if self.path == source {
            self.path = target.to_owned();
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Owner {
    tab: TabId,
    generation: u64,
    path: PathBuf,
}

#[derive(Default)]
pub(super) struct State {
    pub choices: BTreeMap<TabId, Choice>,
    worker: Option<LatestTask>,
    serial: u64,
    pending: Option<(u64, Owner)>,
    paint: paint::Cache,
}

impl State {
    pub(super) fn take_choice(&mut self, tab: TabId) -> Option<Choice> {
        self.reset_graphics();
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, owner)| owner.tab == tab)
        {
            self.clear_read();
        }
        self.choices.remove(&tab)
    }
    pub(super) fn clear_read(&mut self) {
        self.pending = None;
        if let Some(worker) = &self.worker {
            worker.clear();
        }
        self.paint = paint::Cache::default();
    }
    pub(super) fn is_idle(&self) -> bool {
        self.worker.as_ref().is_none_or(LatestTask::is_idle)
    }
    pub(super) fn reset_graphics(&mut self) {
        self.paint = paint::Cache::default();
    }
}

pub(crate) fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["srt", "vtt", "ass", "ssa", "sub", "idx", "sup"]
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

pub(crate) fn track_label(
    language: localization::Language,
    index: usize,
    track: &SubtitleTrack,
) -> String {
    let mut label = format!(
        "{} {}",
        localization::Text::SubtitleTrack.in_language(language),
        index + 1
    );
    if let Some(title) = &track.title {
        label.push_str(" · ");
        label.push_str(&title.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    if let Some(language) = track
        .language
        .as_deref()
        .filter(|language| *language != "und")
    {
        label.push_str(" (");
        label.push_str(&language.split_whitespace().collect::<Vec<_>>().join(" "));
        label.push(')');
    }
    label
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    fn subtitle_owner(&self) -> Option<Owner> {
        if self.media_kind != Some(MediaKind::Video) {
            return None;
        }
        let tab = self.displayed_tab?;
        if self.tabs.active_id() != Some(tab) {
            return None;
        }
        Some(Owner {
            tab,
            generation: self.media_generation,
            path: self.path.clone()?,
        })
    }

    fn subtitle_choice(&self) -> Option<&Choice> {
        let owner = self.subtitle_owner()?;
        self.subtitles
            .choices
            .get(&owner.tab)
            .filter(|choice| choice.path == owner.path)
    }

    pub(super) fn subtitle_settings(&self) -> Settings {
        self.subtitle_choice()
            .map(|choice| choice.settings.clone())
            .unwrap_or_default()
    }

    fn ensure_subtitle_choice(&mut self, owner: &Owner) -> &mut Choice {
        let choice = self
            .subtitles
            .choices
            .entry(owner.tab)
            .or_insert_with(|| Choice {
                path: owner.path.clone(),
                settings: Settings::default(),
                document: None,
                error: None,
            });
        if choice.path != owner.path {
            *choice = Choice {
                path: owner.path.clone(),
                settings: Settings::default(),
                document: None,
                error: None,
            };
        }
        choice
    }

    pub(super) fn apply_subtitle_action(&mut self, mut action: Action) {
        if self.modal_input_blocked() || self.command_context().playback_blocked {
            return;
        }
        let Some(owner) = self.subtitle_owner() else {
            return;
        };
        if action == Action::Show(true) && self.subtitle_settings().selection == Selection::None {
            let selection = if self.subtitle_settings().external.is_some() {
                Some(Selection::External)
            } else {
                self.session
                    .as_ref()
                    .and_then(|session| session.subtitle_tracks().first())
                    .map(|track| Selection::Embedded(track.id))
            };
            let Some(selection) = selection else {
                self.set_status(
                    localization::Text::SubtitleChooseSource
                        .in_language(self.language())
                        .into(),
                );
                return;
            };
            action = Action::Select(selection);
        }
        if let Action::Select(Selection::Embedded(track)) = action
            && !self.session.as_ref().is_some_and(|session| {
                session
                    .subtitle_tracks()
                    .iter()
                    .any(|candidate| candidate.id == track)
            })
        {
            return;
        }
        if action == Action::Select(Selection::External)
            && self.subtitle_settings().external.is_none()
        {
            return;
        }
        let choice = self.ensure_subtitle_choice(&owner);
        match action {
            Action::Select(selection) => {
                choice.settings.selection = selection;
                choice.settings.visible = selection != Selection::None;
                choice.document = None;
                choice.error = None;
                self.subtitles.clear_read();
            }
            Action::Show(visible) => {
                choice.settings.visible = visible;
                if !visible {
                    self.subtitles.clear_read();
                }
            }
            Action::Delay(delay) => choice.settings.delay = delay,
        }
        self.update_subtitle_read();
        self.request_redraw();
    }

    pub(super) fn open_subtitle_picker(&mut self) {
        if self.modal_input_blocked() || self.command_context().playback_blocked {
            return;
        }
        if let Some(owner) = self.subtitle_owner() {
            self.begin_dialog(
                FileDialogKind::OpenSubtitle,
                DialogIntent::OpenSubtitle(owner),
            );
        }
    }

    pub(super) fn finish_subtitle_picker(
        &mut self,
        owner: Owner,
        result: Result<Option<PathBuf>, DialogError>,
    ) {
        if self.subtitle_owner().as_ref() != Some(&owner) {
            return;
        }
        match result {
            Ok(Some(path)) => self.load_external_subtitles(path),
            Ok(None) => {}
            Err(error) => self.set_status(error.message(self.language())),
        }
    }

    pub(super) fn load_external_subtitles(&mut self, path: PathBuf) {
        if self.modal_input_blocked() {
            return;
        }
        let Some(owner) = self.subtitle_owner() else {
            self.set_status(
                localization::Text::SubtitleVideoRequired
                    .in_language(self.language())
                    .into(),
            );
            return;
        };
        let choice = self.ensure_subtitle_choice(&owner);
        choice.settings.external = Some(path);
        choice.settings.selection = Selection::External;
        choice.settings.visible = true;
        choice.document = None;
        choice.error = None;
        self.subtitles.clear_read();
        self.update_subtitle_read();
        self.request_redraw();
    }

    pub(super) fn update_subtitle_read(&mut self) {
        let owner = self.subtitle_owner();
        if self
            .subtitles
            .pending
            .as_ref()
            .is_some_and(|(_, pending)| Some(pending) != owner.as_ref())
        {
            self.subtitles.clear_read();
        }
        if self.source_save.frozen || self.file_operations.busy() {
            return;
        }
        let Some(owner) = owner else {
            self.subtitles.reset_graphics();
            return;
        };
        if self
            .subtitles
            .choices
            .get(&owner.tab)
            .is_some_and(|choice| choice.path != owner.path)
        {
            self.subtitles.take_choice(owner.tab);
        }
        if self
            .subtitle_choice()
            .is_none_or(|choice| !choice.settings.visible || choice.document.is_none())
        {
            self.subtitles.reset_graphics();
        }
        let Some(choice) = self.subtitle_choice() else {
            return;
        };
        if !choice.settings.visible
            || choice.document.is_some()
            || choice.error.is_some()
            || self.subtitles.pending.is_some()
        {
            return;
        }
        let (input, track) = match choice.settings.selection {
            Selection::None => return,
            Selection::External => (
                choice
                    .settings
                    .external
                    .as_ref()
                    .map(|path| MediaInput::new(path.clone())),
                None,
            ),
            Selection::Embedded(track) => (self.document_input(owner.tab), Some(track)),
        };
        let Some(input) = input else {
            return;
        };
        if self.subtitles.worker.is_none() {
            match LatestTask::new("towavue-subtitles") {
                Ok(worker) => self.subtitles.worker = Some(worker),
                Err(error) => {
                    let error = error.to_string();
                    self.ensure_subtitle_choice(&owner).error = Some(error.clone());
                    self.set_status(error);
                    return;
                }
            }
        }
        self.subtitles.serial = self.subtitles.serial.wrapping_add(1);
        let serial = self.subtitles.serial;
        self.subtitles.pending = Some((serial, owner));
        let notify = Arc::clone(&self.notify);
        self.subtitles
            .worker
            .as_ref()
            .expect("subtitle worker")
            .submit(move |cancellation| {
                // Retain MediaInput (including an original preserved by Save/Undo)
                // until the private demuxer and decoder have closed on this worker.
                let result = towavue_runtime_windows::read_subtitles(&input, track, &cancellation)
                    .map(Arc::new);
                if !cancellation.is_cancelled() {
                    notify(AppEvent::SubtitlesLoaded(serial, result));
                }
            });
        self.set_status(
            localization::Text::SubtitlesLoading
                .in_language(self.language())
                .into(),
        );
    }

    pub(super) fn finish_subtitle_read(
        &mut self,
        serial: u64,
        result: Result<Arc<SubtitleDocument>, SubtitleError>,
    ) {
        let Some((pending, owner)) = self.subtitles.pending.clone() else {
            return;
        };
        if serial != pending || self.subtitle_owner().as_ref() != Some(&owner) {
            return;
        }
        self.subtitles.pending = None;
        match result {
            Ok(document) => {
                let empty = document.cues().is_empty();
                self.ensure_subtitle_choice(&owner).document = Some(document);
                self.set_status(
                    if empty {
                        localization::Text::SubtitlesEmpty
                    } else {
                        localization::Text::SubtitlesLoaded
                    }
                    .in_language(self.language())
                    .into(),
                );
            }
            Err(error) => {
                let error = error.message(self.language());
                self.ensure_subtitle_choice(&owner).error = Some(error.clone());
                self.set_status(error);
            }
        }
        self.request_redraw();
    }

    pub(super) fn draw_subtitles(&mut self, ui: &egui::Ui) {
        let Some(choice) = self.subtitle_choice() else {
            return;
        };
        if !choice.settings.visible {
            return;
        }
        let Some(document) = choice.document.clone() else {
            return;
        };
        let delay = choice.settings.delay;
        let position = self.current_position();
        let position = match self.session.as_ref().and_then(PlaybackSession::timeline) {
            Some(timeline) => timeline.source_time(position),
            None => Some(position),
        };
        if let Some(position) = position {
            let mut viewport = ui.max_rect().intersect(ui.clip_rect());
            if self.fullscreen {
                // Keep a stable baseline while the bottom transport bar appears
                // or disappears, including the seek strip above the status row.
                viewport.max.y = viewport
                    .max
                    .y
                    .min(ui.ctx().content_rect().bottom() - FULLSCREEN_CONTROL_BAND);
            }
            self.subtitles
                .paint
                .show(ui, viewport, &document, position, delay);
        }
    }
}
